//! NYC Open Data (Socrata) — SNAP recipients by borough, monthly (HRA).
//! Dataset 5awp-wfkt, aggregated by borough via SoQL. We emit a
//! `snap_enrollment_change` signal per borough for each month-over-month
//! change: lagged administrative confirmation of food stress.

use crate::{geo, DriftGate, Fetcher, GateResult, ParseError, SourceAdapter};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use groundwork_types::{NewSignal, ResolutionLevel, SignalType};
use serde::Deserialize;
use std::collections::BTreeMap;
use store::raw_store::Capture;

pub const SNAP_URL: &str = "https://data.cityofnewyork.us/resource/5awp-wfkt.json?$select=month,borough,sum(bc_snap_recipients)%20as%20snap&$group=month,borough&$order=month%20DESC&$limit=60";
const DATASET_PAGE: &str = "https://data.cityofnewyork.us/Social-Services/Cash-Assistance-Medicaid-and-SNAP-Borough-Communit/5awp-wfkt";

#[derive(Debug, Deserialize)]
struct Row {
    month: String,
    borough: String,
    snap: String,
}

pub struct SocrataSnapAdapter;

#[async_trait::async_trait]
impl SourceAdapter for SocrataSnapAdapter {
    fn source_id(&self) -> &'static str {
        "socrata_snap"
    }

    async fn fetch(&self, fetcher: &dyn Fetcher) -> anyhow::Result<Vec<Capture>> {
        Ok(vec![fetcher.fetch(self.source_id(), SNAP_URL).await?])
    }

    fn parse(&self, capture: &Capture) -> Result<Vec<NewSignal>, ParseError> {
        let bytes = capture.bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&capture.bytes);
        let rows: Vec<Row> = serde_json::from_slice(bytes)
            .map_err(|e| ParseError::Drift(format!("Socrata response shape changed: {e}")))?;

        // borough -> month -> recipients
        let mut by_borough: BTreeMap<String, BTreeMap<NaiveDate, f64>> = BTreeMap::new();
        for r in &rows {
            let month = r
                .month
                .get(..10)
                .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                .ok_or_else(|| ParseError::Drift(format!("bad month '{}'", r.month)))?;
            let snap: f64 = r
                .snap
                .parse()
                .map_err(|_| ParseError::Drift(format!("bad snap count '{}'", r.snap)))?;
            by_borough.entry(r.borough.clone()).or_default().insert(month, snap);
        }

        let mut signals = Vec::new();
        for (borough, months) in &by_borough {
            let Some(county_geoid) = geo::ny_county_geoid(borough) else {
                continue;
            };
            let series: Vec<(&NaiveDate, &f64)> = months.iter().collect();
            for pair in series.windows(2) {
                let ((_prev_m, prev_v), (cur_m, cur_v)) = (pair[0], pair[1]);
                if *prev_v <= 0.0 {
                    continue;
                }
                let pct_change = (cur_v - prev_v) / prev_v * 100.0;
                if pct_change == 0.0 {
                    continue;
                }
                let observed_at: DateTime<Utc> =
                    Utc.from_utc_datetime(&cur_m.and_hms_opt(12, 0, 0).unwrap());
                signals.push(NewSignal {
                    source_id: "socrata_snap".into(),
                    geo_unit_id: county_geoid.clone(),
                    signal_type: SignalType::SnapEnrollmentChange,
                    observed_at,
                    magnitude: pct_change.abs(),
                    direction: if pct_change > 0.0 { 1 } else { -1 },
                    payload: serde_json::json!({
                        "borough": borough,
                        "month": cur_m.to_string(),
                        "recipients": cur_v,
                        "pct_change": pct_change,
                    }),
                    provenance_url: DATASET_PAGE.into(),
                    raw_excerpt: format!(
                        "{borough}: SNAP recipients {} in {} ({:+.2}% vs prior month, {} recipients)",
                        if pct_change > 0.0 { "rose" } else { "fell" },
                        cur_m.format("%B %Y"),
                        pct_change,
                        *cur_v as i64
                    ),
                    raw_capture_id: Some(capture.meta.capture_id.clone()),
                    resolution_level: ResolutionLevel::County,
                    dedupe_key: format!("socrata_snap:{county_geoid}:{cur_m}"),
                });
            }
        }
        Ok(signals)
    }

    fn gates(&self) -> Vec<Box<dyn DriftGate>> {
        vec![Box::new(BoroughCoverageGate)]
    }
}

/// All five boroughs must appear; a missing borough means the dataset's
/// grouping or vocabulary changed.
struct BoroughCoverageGate;
impl DriftGate for BoroughCoverageGate {
    fn name(&self) -> &str {
        "socrata_all_boroughs_present"
    }
    fn check(&self, _capture: &Capture, parsed: &[NewSignal]) -> GateResult {
        let expected = ["36005", "36047", "36061", "36081", "36085"];
        for geoid in expected {
            if !parsed.iter().any(|s| s.geo_unit_id == geoid) {
                return GateResult::Fail(format!("no signals for county {geoid}"));
            }
        }
        GateResult::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixture_capture, run_gates};

    #[test]
    fn parses_fixture_and_gates_pass() {
        let cap = fixture_capture(
            "socrata_snap",
            SNAP_URL,
            include_bytes!("../../../fixtures/socrata_snap_sample.json").to_vec(),
        );
        let adapter = SocrataSnapAdapter;
        let signals = adapter.parse(&cap).unwrap();
        assert!(!signals.is_empty());
        assert_eq!(run_gates(&adapter.gates(), &cap, &signals), GateResult::Pass);
        for s in &signals {
            assert_eq!(s.signal_type, SignalType::SnapEnrollmentChange);
            assert!(s.magnitude >= 0.0);
        }
    }
}
