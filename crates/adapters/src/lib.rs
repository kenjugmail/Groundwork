//! Source adapters and the contracts that make them recordable and replayable.
//!
//! Adapters never call the network directly: the orchestrator injects a
//! [`Fetcher`], whose recording implementation captures raw bytes before any
//! parsing, and whose replay implementation serves historical captures.

pub mod acs;
pub mod geo;
pub mod household_pulse;
pub mod meal_gap;
pub mod socrata_snap;
pub mod two11;
pub mod warn_ny;

use groundwork_types::NewSignal;
use std::collections::BTreeMap;
use store::raw_store::Capture;

/// Network access seam. Implementations: `RecordingFetcher` (live, records
/// every response to the raw store first) and `ReplayFetcher` (serves
/// historical captures / fixtures).
#[async_trait::async_trait]
pub trait Fetcher: Send + Sync {
    async fn fetch(&self, source_id: &str, url: &str) -> anyhow::Result<Capture>;
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("document structure drifted: {0}")]
    Drift(String),
    #[error("parse failure: {0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum GateResult {
    Pass,
    Fail(String),
}

/// A drift gate asserts a source invariant on a capture + its parsed output.
/// Any failure quarantines the whole batch (coverage_degraded) instead of
/// feeding the nowcast — a source going dark must lower coverage, never need.
pub trait DriftGate: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, capture: &Capture, parsed: &[NewSignal]) -> GateResult;
}

/// One structured source. `fetch` records; `parse` is pure (replayable).
#[async_trait::async_trait]
pub trait SourceAdapter: Send + Sync {
    fn source_id(&self) -> &'static str;
    async fn fetch(&self, fetcher: &dyn Fetcher) -> anyhow::Result<Vec<Capture>>;
    fn parse(&self, capture: &Capture) -> Result<Vec<NewSignal>, ParseError>;
    fn gates(&self) -> Vec<Box<dyn DriftGate>>;
}

/// Run every gate; first failure wins.
pub fn run_gates(
    gates: &[Box<dyn DriftGate>],
    capture: &Capture,
    parsed: &[NewSignal],
) -> GateResult {
    for g in gates {
        if let GateResult::Fail(reason) = g.check(capture, parsed) {
            return GateResult::Fail(format!("{}: {}", g.name(), reason));
        }
    }
    GateResult::Pass
}

/// Build an in-memory capture from fixture bytes (tests / replay).
pub fn fixture_capture(source_id: &str, url: &str, bytes: Vec<u8>) -> Capture {
    use sha2::Digest;
    let sha256 = hex_lower(&sha2::Sha256::digest(&bytes));
    Capture {
        meta: store::raw_store::CaptureMeta {
            capture_id: format!("fixture/{source_id}"),
            source_id: source_id.to_string(),
            url: url.to_string(),
            status: 200,
            headers: BTreeMap::new(),
            fetched_at: chrono::Utc::now(),
            sha256,
        },
        bytes,
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
