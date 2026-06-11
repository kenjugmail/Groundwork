//! Ingest a source: fetch (recorded) → parse → drift gates → transactional
//! write, quarantining the whole batch on gate failure or parse drift.

use adapters::{run_gates, Fetcher, GateResult, ParseError, SourceAdapter};
use store::Db;

pub struct IngestOutcome {
    pub source_id: String,
    pub inserted: u64,
    pub quarantined: bool,
    pub detail: String,
}

pub async fn ingest_source(
    db: &Db,
    adapter: &dyn SourceAdapter,
    fetcher: &dyn Fetcher,
) -> anyhow::Result<IngestOutcome> {
    let source_id = adapter.source_id().to_string();
    let captures = adapter.fetch(fetcher).await?;
    let mut inserted = 0u64;
    let mut quarantined = false;
    let mut details = Vec::new();

    for capture in &captures {
        match adapter.parse(capture) {
            Ok(signals) => match run_gates(&adapter.gates(), capture, &signals) {
                GateResult::Pass => {
                    let n = db.insert_signals(&signals, false).await?;
                    inserted += n;
                    details.push(format!("{n} signals from capture {}", capture.meta.capture_id));
                }
                GateResult::Fail(reason) => {
                    // Garbage must lower coverage, never feed the nowcast.
                    quarantined = true;
                    let n = db.insert_signals(&signals, true).await?;
                    inserted += n;
                    tracing::warn!(source = %source_id, %reason, "drift gate failed; batch quarantined");
                    details.push(format!("QUARANTINED ({reason})"));
                }
            },
            Err(ParseError::Drift(reason)) => {
                quarantined = true;
                tracing::warn!(source = %source_id, %reason, "parse drift; nothing ingested, source marked degraded");
                details.push(format!("PARSE DRIFT ({reason})"));
            }
            Err(ParseError::Other(e)) => anyhow::bail!("parse failure for {source_id}: {e}"),
        }
    }

    db.mark_source_ingest(&source_id, quarantined).await?;
    Ok(IngestOutcome { source_id, inserted, quarantined, detail: details.join("; ") })
}
