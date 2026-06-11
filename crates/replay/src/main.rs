//! Replay harness: run every recorded capture for a source back through its
//! adapter's parse + drift gates, and (optionally) diff the signal output
//! against a committed golden file.
//!
//! Use this whenever extraction logic or fusion weights change: "did this
//! change move the map, and was that justified?"

use adapters::{run_gates, GateResult, SourceAdapter};
use clap::Parser;
use store::raw_store::{FsRawStore, RawDocStore};

#[derive(Parser)]
#[command(name = "replay", about = "Replay recorded captures through adapters + gates")]
struct Cli {
    /// Source id: warn_ny | household_pulse | socrata_snap
    #[arg(long)]
    source: String,
    /// Raw capture store directory.
    #[arg(long, default_value = "./raw_store")]
    captures: String,
    /// Write parsed signals to this golden file instead of comparing.
    #[arg(long)]
    update_golden: Option<String>,
    /// Compare output against this golden file (JSON array of signals).
    #[arg(long)]
    golden: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    let cli = Cli::parse();

    // Agentic source: replay stored extractions (deterministic, free) —
    // re-runs the verbatim-span guard + geo-resolution over the saved model
    // outputs against the saved article captures.
    if cli.source == "local_news" {
        return replay_local_news(&cli.captures).await;
    }

    let adapter: Box<dyn SourceAdapter> = match cli.source.as_str() {
        "warn_ny" => Box::new(adapters::warn_ny::WarnNyAdapter),
        "household_pulse" => Box::new(adapters::household_pulse::HouseholdPulseAdapter::default()),
        "socrata_snap" => Box::new(adapters::socrata_snap::SocrataSnapAdapter),
        other => anyhow::bail!("no adapter for source '{other}'"),
    };

    let raw = FsRawStore::new(&cli.captures);
    let metas = raw.list(&cli.source).await?;
    if metas.is_empty() {
        anyhow::bail!("no captures for '{}' under {}", cli.source, cli.captures);
    }

    let mut all_signals = Vec::new();
    let mut failures = 0;
    for meta in &metas {
        let capture = raw.get(&meta.capture_id).await?;
        match adapter.parse(&capture) {
            Ok(signals) => {
                let gate = run_gates(&adapter.gates(), &capture, &signals);
                match gate {
                    GateResult::Pass => {
                        println!("OK   {}  {} signals", meta.capture_id, signals.len());
                        all_signals.extend(signals);
                    }
                    GateResult::Fail(reason) => {
                        println!("GATE {}  would quarantine: {reason}", meta.capture_id);
                        failures += 1;
                    }
                }
            }
            Err(e) => {
                println!("DRIFT {}  parse failed: {e}", meta.capture_id);
                failures += 1;
            }
        }
    }

    // Stable ordering for diffable goldens; capture ids are machine-local
    // so they're stripped from the golden representation.
    all_signals.sort_by(|a, b| a.dedupe_key.cmp(&b.dedupe_key));
    all_signals.dedup_by(|a, b| a.dedupe_key == b.dedupe_key); // store-level idempotency, mirrored
    for s in &mut all_signals {
        s.raw_capture_id = None;
    }
    let rendered = serde_json::to_string_pretty(&all_signals)?;

    if let Some(path) = cli.update_golden {
        tokio::fs::write(&path, &rendered).await?;
        println!("golden updated: {path} ({} signals)", all_signals.len());
    } else if let Some(path) = cli.golden {
        let expected = tokio::fs::read_to_string(&path).await?;
        let expected_norm: serde_json::Value = serde_json::from_str(&expected)?;
        let actual_norm: serde_json::Value = serde_json::from_str(&rendered)?;
        if expected_norm == actual_norm {
            println!("golden match: {path}");
        } else {
            anyhow::bail!("replay output diverges from golden {path} — review whether the change to extraction logic is justified");
        }
    }

    println!(
        "replayed {} captures: {} clean, {failures} drift/gate",
        metas.len(),
        metas.len() - failures
    );
    if failures > 0 {
        std::process::exit(2);
    }
    Ok(())
}

/// Re-validate every stored extraction: schema, verbatim span against the
/// matching article capture, geo-resolution. Catches regressions in the
/// guard/resolver logic without paying for model calls.
async fn replay_local_news(captures_dir: &str) -> anyhow::Result<()> {
    let raw = FsRawStore::new(captures_dir);
    let extractions = raw.list("local_news_extractions").await?;
    if extractions.is_empty() {
        anyhow::bail!("no stored extractions under {captures_dir}");
    }
    let articles = raw.list("local_news").await?;
    let mut ok = 0usize;
    let mut problems = 0usize;
    for meta in &extractions {
        let extraction = raw.get(&meta.capture_id).await?;
        let parsed: Result<harness::ExtractedSignals, _> =
            serde_json::from_slice(&extraction.bytes);
        let Ok(parsed) = parsed else {
            println!("DRIFT {}  stored output no longer parses", meta.capture_id);
            problems += 1;
            continue;
        };
        // Find the article capture for the same URL.
        let Some(article_meta) = articles.iter().rev().find(|a| a.url == meta.url) else {
            println!("MISS {}  no article capture for {}", meta.capture_id, meta.url);
            problems += 1;
            continue;
        };
        let article = raw.get(&article_meta.capture_id).await?;
        let text = harness::rss::article_text(&article.bytes);
        for sig in &parsed.signals {
            if !harness::verify::excerpt_in_document(&sig.raw_excerpt, &text) {
                println!("SPAN {}  excerpt no longer verifies", meta.capture_id);
                problems += 1;
            } else if harness::geo_places::resolve(&sig.geo_text).is_none() {
                println!("GEO  {}  '{}' no longer resolves", meta.capture_id, sig.geo_text);
                problems += 1;
            } else {
                ok += 1;
            }
        }
    }
    println!("replayed {} extractions: {ok} signals verified, {problems} problems", extractions.len());
    if problems > 0 {
        std::process::exit(2);
    }
    Ok(())
}
