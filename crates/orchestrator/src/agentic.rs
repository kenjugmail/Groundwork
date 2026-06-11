//! The agentic ingest path: feeds → articles (recorded) → extraction agent
//! (output recorded as its own capture) → verbatim-span guard → geo-resolve
//! → typed signals → gates → transactional write.
//!
//! Separate from `ingest::ingest_source` because extraction is an async
//! model call and the agentic gates need per-batch drop statistics the
//! DriftGate trait can't see.

use adapters::Fetcher;
use chrono::Utc;
use groundwork_types::{NewSignal, ResolutionLevel};
use harness::config::FeedsConfig;
use harness::extract::{extract_validated, ExtractionModel, PromptPrefix};
use harness::gates::{check, AgenticGateResult, ExtractionStats};
use harness::{geo_places, rss, verify};
use sha2::Digest;
use store::raw_store::RawDocStore;
use store::Db;

pub struct AgenticOutcome {
    pub stats: ExtractionStats,
    pub inserted: u64,
    pub quarantined: bool,
    pub detail: String,
}

pub async fn ingest_local_news(
    db: &Db,
    fetcher: &dyn Fetcher,
    raw_store: &dyn RawDocStore,
    model: &dyn ExtractionModel,
) -> anyhow::Result<AgenticOutcome> {
    let cfg = FeedsConfig::load()?;
    let prefix = PromptPrefix::build()?;
    let mut stats = ExtractionStats::default();
    let mut new_signals: Vec<NewSignal> = Vec::new();
    let mut budget = cfg.harness.max_docs_per_day;

    for feed in &cfg.feeds {
        if budget == 0 {
            break;
        }
        let feed_capture = match fetcher.fetch("local_news", &feed.url).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(feed = %feed.name, "feed fetch failed (continuing): {e}");
                continue;
            }
        };
        let items = match rss::parse_feed(&feed_capture.bytes) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(feed = %feed.name, "feed parse failed (continuing): {e}");
                continue;
            }
        };

        for item in items {
            if budget == 0 {
                break;
            }
            // At-most-once per article URL.
            if !db.mark_url_seen("local_news", &item.url).await? {
                continue;
            }
            let article = match fetcher.fetch("local_news", &item.url).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(url = %item.url, "article fetch failed: {e}");
                    continue;
                }
            };
            budget -= 1;
            let text = rss::article_text(&article.bytes);
            if text.len() < 200 {
                continue; // paywall stub / empty page
            }

            stats.docs_processed += 1;
            let (extracted, raw_output) = match extract_validated(model, &prefix, &text).await {
                Ok(x) => x,
                Err(e) => {
                    tracing::warn!(url = %item.url, "extraction failed: {e}");
                    stats.docs_schema_failed += 1;
                    continue;
                }
            };
            // The model output is itself a recorded artifact (record/replay).
            let extraction_capture = raw_store
                .put(
                    "local_news_extractions",
                    &item.url,
                    200,
                    std::collections::BTreeMap::from([(
                        "x-groundwork-model".to_string(),
                        model.model_id(),
                    )]),
                    raw_output.into_bytes(),
                )
                .await?;

            for sig in extracted.signals {
                stats.signals_extracted += 1;
                // Hallucination guard: no quotable span, no signal.
                if !verify::excerpt_in_document(&sig.raw_excerpt, &text) {
                    stats.signals_dropped_no_span += 1;
                    tracing::warn!(url = %item.url, "dropped signal: raw_excerpt not found in document");
                    continue;
                }
                let Some(geo_unit_id) = geo_places::resolve(&sig.geo_text) else {
                    stats.signals_dropped_no_geo += 1;
                    tracing::info!(geo = %sig.geo_text, "dropped signal: unresolvable geography");
                    continue;
                };
                let observed_at = item.published.unwrap_or_else(Utc::now);
                let excerpt_hash = hex::encode(sha2::Sha256::digest(sig.raw_excerpt.as_bytes()));
                let url_hash = hex::encode(sha2::Sha256::digest(item.url.as_bytes()));
                stats.signals_kept += 1;
                new_signals.push(NewSignal {
                    source_id: "local_news".into(),
                    geo_unit_id,
                    signal_type: sig.signal_type,
                    observed_at,
                    magnitude: sig.magnitude,
                    direction: sig.direction,
                    payload: serde_json::json!({
                        "confidence": sig.confidence,
                        "geo_text": sig.geo_text,
                        "feed": feed.name,
                        "title": item.title,
                        "model": model.model_id(),
                        "extraction_capture_id": extraction_capture.meta.capture_id,
                        "observed_date_text": sig.observed_date_text,
                    }),
                    provenance_url: item.url.clone(),
                    raw_excerpt: sig.raw_excerpt,
                    raw_capture_id: Some(article.meta.capture_id.clone()),
                    resolution_level: ResolutionLevel::County,
                    dedupe_key: format!(
                        "local_news:{}:{}:{}",
                        &url_hash[..16],
                        sig.signal_type.as_str(),
                        &excerpt_hash[..16]
                    ),
                });
            }
        }
    }

    let gate = check(&stats);
    let quarantined = matches!(gate, AgenticGateResult::Fail(_));
    let inserted = db.insert_signals(&new_signals, quarantined).await?;
    db.mark_source_ingest("local_news", quarantined).await?;
    let detail = match gate {
        AgenticGateResult::Pass => format!(
            "{} docs, {} signals kept ({} no-span, {} no-geo)",
            stats.docs_processed, stats.signals_kept,
            stats.signals_dropped_no_span, stats.signals_dropped_no_geo
        ),
        AgenticGateResult::Fail(ref reason) => {
            tracing::warn!(%reason, "agentic gate failed; batch quarantined");
            format!("QUARANTINED ({reason})")
        }
    };
    Ok(AgenticOutcome { stats, inserted, quarantined, detail })
}
