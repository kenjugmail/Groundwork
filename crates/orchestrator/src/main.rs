//! Groundwork orchestrator: schedules source pulls on their own cadences,
//! records every fetch, runs drift gates, writes signals transactionally,
//! and recomputes the nowcast after ingests.

mod baselines;
mod fetcher;
mod ingest;
mod nowcast;
mod tracts;

use adapters::{
    household_pulse::HouseholdPulseAdapter, socrata_snap::SocrataSnapAdapter,
    warn_ny::WarnNyAdapter, SourceAdapter,
};
use clap::{Parser, Subcommand};
use fetcher::RecordingFetcher;
use store::raw_store::FsRawStore;
use store::Db;

#[derive(Parser)]
#[command(name = "orchestrator", about = "Groundwork ingestion coordinator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run pending migrations and exit.
    Migrate,
    /// Download TIGER/Line tract geometries for NYC + Westchester and load geo_units.
    LoadTracts {
        /// Use an already-downloaded tl_*_36_tract.zip instead of fetching.
        #[arg(long)]
        zip: Option<String>,
    },
    /// Download cartographic boundaries for all ~3,100 US counties and load geo_units.
    LoadUsCounties {
        #[arg(long)]
        zip: Option<String>,
    },
    /// Ingest one source now: warn-ny | acs | meal-gap | household-pulse | socrata-snap
    Ingest {
        source: String,
        /// CSV path (meal-gap only).
        #[arg(long)]
        file: Option<String>,
    },
    /// Recompute the nowcast for every tract.
    Nowcast,
    /// Run all enabled sources on their cadences (long-running).
    Run,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL not set (copy .env.example to .env)"))?;
    let raw_dir = std::env::var("RAW_STORE_DIR").unwrap_or_else(|_| "./raw_store".into());

    let db = Db::connect(&database_url).await?;
    db.migrate().await?;
    let fetcher = RecordingFetcher::new(FsRawStore::new(&raw_dir));

    match cli.command {
        Command::Migrate => {
            println!("migrations up to date");
        }
        Command::LoadTracts { zip } => {
            let n = tracts::load_tracts(&db, zip.as_deref()).await?;
            println!("loaded {n} tracts into geo_units");
        }
        Command::LoadUsCounties { zip } => {
            let n = tracts::load_us_counties(&db, zip.as_deref()).await?;
            println!("loaded {n} US counties into geo_units");
        }
        Command::Ingest { source, file } => {
            match source.as_str() {
                "warn-ny" => report(ingest::ingest_source(&db, &WarnNyAdapter, &fetcher).await?),
                "household-pulse" => report(
                    ingest::ingest_source(&db, &HouseholdPulseAdapter::default(), &fetcher).await?,
                ),
                "socrata-snap" => {
                    report(ingest::ingest_source(&db, &SocrataSnapAdapter, &fetcher).await?)
                }
                "acs" => {
                    let n = baselines::ingest_acs(&db, &fetcher).await?;
                    println!("acs: upserted baselines for {n} tracts");
                }
                "chr" => {
                    let n = baselines::ingest_chr(&db, &fetcher).await?;
                    println!("chr: upserted {n} baseline values across US counties + states");
                }
                "meal-gap" => {
                    let f = file.ok_or_else(|| {
                        anyhow::anyhow!("meal-gap needs --file <csv> (manual annual drop)")
                    })?;
                    let n = baselines::ingest_meal_gap(&db, &f).await?;
                    println!("meal-gap: upserted {n} county baselines");
                }
                other => anyhow::bail!("unknown source '{other}'"),
            }
            let n = nowcast::recompute(&db).await?;
            println!("nowcast recomputed for {n} tracts");
        }
        Command::Nowcast => {
            let n = nowcast::recompute(&db).await?;
            println!("nowcast recomputed for {n} tracts");
        }
        Command::Run => run_loop(&db, &fetcher).await?,
    }
    Ok(())
}

fn report(outcome: ingest::IngestOutcome) {
    println!(
        "{}: {} signals inserted{} — {}",
        outcome.source_id,
        outcome.inserted,
        if outcome.quarantined { " [QUARANTINED]" } else { "" },
        outcome.detail
    );
}

/// Long-running scheduler: each signal source on its own cadence with
/// exponential backoff on failure; nowcast recomputed after each cycle.
/// No slow source blocks the others.
async fn run_loop(db: &Db, _f: &RecordingFetcher<FsRawStore>) -> anyhow::Result<()> {
    let sources = db.sources().await?;
    let raw_dir = std::env::var("RAW_STORE_DIR").unwrap_or_else(|_| "./raw_store".into());
    let database_url = std::env::var("DATABASE_URL")?;

    let mut handles = Vec::new();
    for src in sources.into_iter().filter(|s| s.enabled) {
        let adapter: Option<Box<dyn SourceAdapter>> = match src.id.as_str() {
            "warn_ny" => Some(Box::new(WarnNyAdapter)),
            "household_pulse" => Some(Box::new(HouseholdPulseAdapter::default())),
            "socrata_snap" => Some(Box::new(SocrataSnapAdapter)),
            _ => None, // baselines are manual/annual; 211 awaits its agreement
        };
        let Some(adapter) = adapter else { continue };
        let cadence = std::time::Duration::from_secs(src.cadence_seconds.max(60) as u64);
        let db = Db::connect(&database_url).await?;
        let fetcher = RecordingFetcher::new(FsRawStore::new(&raw_dir));
        handles.push(tokio::spawn(async move {
            let mut backoff = std::time::Duration::from_secs(60);
            loop {
                match ingest::ingest_source(&db, adapter.as_ref(), &fetcher).await {
                    Ok(outcome) => {
                        tracing::info!(
                            source = %outcome.source_id,
                            inserted = outcome.inserted,
                            quarantined = outcome.quarantined,
                            "ingest cycle complete"
                        );
                        if let Err(e) = nowcast::recompute(&db).await {
                            tracing::error!("nowcast recompute failed: {e:#}");
                        }
                        backoff = std::time::Duration::from_secs(60);
                        tokio::time::sleep(cadence).await;
                    }
                    Err(e) => {
                        tracing::error!(source = %adapter.source_id(), "ingest failed: {e:#}; retrying in {backoff:?}");
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(std::time::Duration::from_secs(6 * 3600));
                    }
                }
            }
        }));
    }
    if handles.is_empty() {
        anyhow::bail!("no enabled scheduled sources");
    }
    println!("orchestrator running ({} sources); Ctrl-C to stop", handles.len());
    for h in handles {
        h.await?;
    }
    Ok(())
}
