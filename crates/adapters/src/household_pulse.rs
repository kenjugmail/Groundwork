//! Census Household Pulse Survey — food insufficiency, state level.
//!
//! Pulse distribution churns (per-wave URLs; the 2024–25 HTOPS transition),
//! so the source URL is config-driven (`PULSE_CSV_URL`, defaults to the
//! committed fixture) and the expected shape is a simple normalized CSV:
//! `week_start,week_end,geography,food_insufficient_pct`. A drift gate
//! quarantines anything else. Coarse geography (state) — this is a
//! calibration signal, apportioned thinly across tracts at fusion time.

use crate::{DriftGate, Fetcher, GateResult, ParseError, SourceAdapter};
use chrono::{NaiveDate, TimeZone, Utc};
use groundwork_types::{NewSignal, ResolutionLevel, SignalType};
use store::raw_store::Capture;

pub const DEFAULT_URL: &str = "fixture://pulse_sample.csv";
/// NY state GEOID.
const NY_GEOID: &str = "36";

pub struct HouseholdPulseAdapter {
    pub url: String,
}

impl Default for HouseholdPulseAdapter {
    fn default() -> Self {
        Self { url: std::env::var("PULSE_CSV_URL").unwrap_or_else(|_| DEFAULT_URL.into()) }
    }
}

#[async_trait::async_trait]
impl SourceAdapter for HouseholdPulseAdapter {
    fn source_id(&self) -> &'static str {
        "household_pulse"
    }

    async fn fetch(&self, fetcher: &dyn Fetcher) -> anyhow::Result<Vec<Capture>> {
        Ok(vec![fetcher.fetch(self.source_id(), &self.url).await?])
    }

    fn parse(&self, capture: &Capture) -> Result<Vec<NewSignal>, ParseError> {
        let mut rdr = csv::Reader::from_reader(capture.bytes.as_slice());
        let headers: Vec<String> = rdr
            .headers()
            .map_err(|e| ParseError::Other(e.to_string()))?
            .iter()
            .map(|h| h.trim().to_lowercase())
            .collect();
        let col = |needle: &str| headers.iter().position(|h| h.contains(needle));
        let (Some(c_start), Some(c_geo), Some(c_pct)) =
            (col("week_start"), col("geography"), col("insufficien"))
        else {
            return Err(ParseError::Drift(format!(
                "Pulse CSV headers unrecognized: {headers:?}"
            )));
        };

        let mut signals = Vec::new();
        for rec in rdr.records() {
            let rec = rec.map_err(|e| ParseError::Other(e.to_string()))?;
            let field = |i: usize| rec.get(i).unwrap_or("").trim().to_string();
            if !field(c_geo).eq_ignore_ascii_case("new york") {
                continue;
            }
            let week_start = NaiveDate::parse_from_str(&field(c_start), "%Y-%m-%d")
                .map_err(|e| ParseError::Drift(format!("bad week_start: {e}")))?;
            let pct: f64 = field(c_pct)
                .trim_end_matches('%')
                .parse()
                .map_err(|_| ParseError::Drift(format!("bad pct '{}'", field(c_pct))))?;
            signals.push(NewSignal {
                source_id: "household_pulse".into(),
                geo_unit_id: NY_GEOID.into(),
                signal_type: SignalType::SurveyFoodInsufficiency,
                observed_at: Utc.from_utc_datetime(&week_start.and_hms_opt(12, 0, 0).unwrap()),
                magnitude: pct,
                direction: 1,
                payload: serde_json::json!({ "geography": "New York", "pct": pct }),
                provenance_url: "https://www.census.gov/data/experimental-data-products/household-pulse-survey.html".into(),
                raw_excerpt: format!(
                    "Household Pulse: {pct}% of New York adults reported food insufficiency, collection period starting {week_start}"
                ),
                raw_capture_id: Some(capture.meta.capture_id.clone()),
                resolution_level: ResolutionLevel::State,
                dedupe_key: format!("household_pulse:NY:{week_start}"),
            });
        }
        Ok(signals)
    }

    fn gates(&self) -> Vec<Box<dyn DriftGate>> {
        vec![Box::new(PctRangeGate)]
    }
}

/// Food insufficiency outside [0, 60]% means the column we grabbed isn't a
/// percentage anymore.
struct PctRangeGate;
impl DriftGate for PctRangeGate {
    fn name(&self) -> &str {
        "pulse_pct_in_range"
    }
    fn check(&self, _capture: &Capture, parsed: &[NewSignal]) -> GateResult {
        for s in parsed {
            if !(0.0..=60.0).contains(&s.magnitude) {
                return GateResult::Fail(format!("magnitude {} out of range", s.magnitude));
            }
        }
        GateResult::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture_capture;

    #[test]
    fn parses_fixture() {
        let cap = fixture_capture(
            "household_pulse",
            DEFAULT_URL,
            include_bytes!("../../../fixtures/pulse_sample.csv").to_vec(),
        );
        let adapter = HouseholdPulseAdapter { url: DEFAULT_URL.into() };
        let signals = adapter.parse(&cap).unwrap();
        assert_eq!(signals.len(), 3);
        assert!(signals.iter().all(|s| s.resolution_level == ResolutionLevel::State));
    }
}
