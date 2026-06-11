//! NY DOL WARN Act layoff notices.
//!
//! The DOL's HTML listing was retired on 2025-04-01; current notices live in a
//! Tableau Public dashboard whose CSV export endpoint we pull. That migration
//! is itself a worked example of source drift — the gates below are tuned to
//! catch the next one.
//!
//! Geo-resolution: the feed names the impacted site's county. We resolve
//! county name → county GEOID and emit signals at `resolution_level=county`;
//! apportionment to tracts happens in fusion, never here.

use crate::{geo, DriftGate, Fetcher, GateResult, ParseError, SourceAdapter};
use chrono::{NaiveDate, TimeZone, Utc};
use groundwork_types::{NewSignal, ResolutionLevel, SignalType};
use store::raw_store::Capture;

pub const WARN_CSV_URL: &str = "https://public.tableau.com/views/WorkerAdjustmentRetrainingNotificationWARN/WARN?:showVizHome=no&:format=csv";
/// Human-facing dashboard, used as provenance (the CSV URL is an export of it).
pub const WARN_DASHBOARD_URL: &str = "https://dol.ny.gov/warn-dashboard";

const REQUIRED_HEADERS: [&str; 6] = [
    "Business Legal Name",
    "Date of WARN Notice",
    "Impacted Site County",
    "Number of Affected Workers",
    "Layoff or Closure?",
    "Date Posted",
];

pub struct WarnNyAdapter;

#[async_trait::async_trait]
impl SourceAdapter for WarnNyAdapter {
    fn source_id(&self) -> &'static str {
        "warn_ny"
    }

    async fn fetch(&self, fetcher: &dyn Fetcher) -> anyhow::Result<Vec<Capture>> {
        Ok(vec![fetcher.fetch(self.source_id(), WARN_CSV_URL).await?])
    }

    fn parse(&self, capture: &Capture) -> Result<Vec<NewSignal>, ParseError> {
        let text = String::from_utf8_lossy(&capture.bytes);
        let mut rdr = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(text.as_bytes());

        // Headers in the export carry trailing spaces; index by trimmed name.
        let headers: Vec<String> = rdr
            .headers()
            .map_err(|e| ParseError::Other(e.to_string()))?
            .iter()
            .map(|h| h.trim().to_string())
            .collect();
        let col = |name: &str| headers.iter().position(|h| h == name);
        let (Some(c_name), Some(c_notice), Some(c_county), Some(c_workers), Some(c_kind), Some(c_posted)) = (
            col("Business Legal Name"),
            col("Date of WARN Notice"),
            col("Impacted Site County"),
            col("Number of Affected Workers"),
            col("Layoff or Closure?"),
            col("Date Posted"),
        ) else {
            return Err(ParseError::Drift(format!(
                "expected headers missing; got: {headers:?}"
            )));
        };

        let mut signals = Vec::new();
        for rec in rdr.records() {
            let rec = rec.map_err(|e| ParseError::Other(e.to_string()))?;
            let field = |i: usize| rec.get(i).unwrap_or("").trim().to_string();
            let company = field(c_name);
            let county_name = field(c_county);
            if company.is_empty() || county_name.is_empty() {
                continue;
            }
            let Some(county_geoid) = geo::ny_county_geoid(&county_name) else {
                // Unknown county names are a drift symptom; the gate below
                // measures the resolution rate. Skip the row here.
                continue;
            };
            let workers: f64 = field(c_workers).replace(',', "").parse().unwrap_or(0.0);
            if workers <= 0.0 {
                continue;
            }
            let notice_date = parse_date(&field(c_notice))
                .or_else(|| parse_date(&field(c_posted)))
                .ok_or_else(|| ParseError::Drift(format!("unparseable date in row for {company}")))?;
            let observed_at = Utc
                .from_utc_datetime(&notice_date.and_hms_opt(12, 0, 0).unwrap());

            let kind = field(c_kind);
            let excerpt = format!(
                "{company} — {kind}, {workers} affected workers, {county_name} County, WARN notice dated {notice_date}"
            );
            signals.push(NewSignal {
                source_id: "warn_ny".into(),
                geo_unit_id: county_geoid.clone(),
                signal_type: SignalType::LayoffWarn,
                observed_at,
                magnitude: workers,
                direction: 1,
                payload: serde_json::json!({
                    "company": company,
                    "county": county_name,
                    "kind": kind,
                    "affected_workers": workers,
                }),
                provenance_url: WARN_DASHBOARD_URL.into(),
                raw_excerpt: excerpt,
                raw_capture_id: Some(capture.meta.capture_id.clone()),
                resolution_level: ResolutionLevel::County,
                dedupe_key: format!("warn_ny:{company}:{county_geoid}:{notice_date}"),
            });
        }
        Ok(signals)
    }

    fn gates(&self) -> Vec<Box<dyn DriftGate>> {
        vec![
            Box::new(HeaderGate),
            Box::new(RowCountGate { min: 1, max: 5000 }),
            Box::new(CountyResolutionGate { min_ratio: 0.8 }),
        ]
    }
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(s, "%m/%d/%Y"))
        .ok()
}

struct HeaderGate;
impl DriftGate for HeaderGate {
    fn name(&self) -> &str {
        "warn_headers_present"
    }
    fn check(&self, capture: &Capture, _parsed: &[NewSignal]) -> GateResult {
        let text = String::from_utf8_lossy(&capture.bytes);
        let first_line = text.lines().next().unwrap_or("");
        for h in REQUIRED_HEADERS {
            if !first_line.contains(h) {
                return GateResult::Fail(format!("missing expected header '{h}'"));
            }
        }
        GateResult::Pass
    }
}

struct RowCountGate {
    min: usize,
    max: usize,
}
impl DriftGate for RowCountGate {
    fn name(&self) -> &str {
        "warn_row_count_in_band"
    }
    fn check(&self, capture: &Capture, _parsed: &[NewSignal]) -> GateResult {
        let rows = String::from_utf8_lossy(&capture.bytes)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
            .saturating_sub(1);
        if rows < self.min || rows > self.max {
            GateResult::Fail(format!("{rows} rows outside [{}, {}]", self.min, self.max))
        } else {
            GateResult::Pass
        }
    }
}

/// If too few raw rows resolved to a known county, the county column has
/// probably moved or changed vocabulary — quarantine rather than under-count.
struct CountyResolutionGate {
    min_ratio: f64,
}
impl DriftGate for CountyResolutionGate {
    fn name(&self) -> &str {
        "warn_county_resolution_rate"
    }
    fn check(&self, capture: &Capture, parsed: &[NewSignal]) -> GateResult {
        let raw_rows = String::from_utf8_lossy(&capture.bytes)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
            .saturating_sub(1);
        if raw_rows == 0 {
            return GateResult::Fail("no data rows".into());
        }
        let ratio = parsed.len() as f64 / raw_rows as f64;
        if ratio < self.min_ratio {
            GateResult::Fail(format!(
                "only {:.0}% of rows resolved to known NY counties (min {:.0}%)",
                ratio * 100.0,
                self.min_ratio * 100.0
            ))
        } else {
            GateResult::Pass
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fixture_capture, run_gates};

    fn fixture() -> Capture {
        let bytes = include_bytes!("../../../fixtures/warn_sample.csv").to_vec();
        fixture_capture("warn_ny", WARN_CSV_URL, bytes)
    }

    #[test]
    fn parses_fixture_with_provenance() {
        let adapter = WarnNyAdapter;
        let signals = adapter.parse(&fixture()).unwrap();
        assert!(!signals.is_empty());
        for s in &signals {
            assert!(!s.raw_excerpt.is_empty());
            assert!(!s.provenance_url.is_empty());
            assert!(s.magnitude > 0.0);
            assert_eq!(s.direction, 1);
            assert_eq!(s.geo_unit_id.len(), 5); // county GEOID
        }
    }

    #[test]
    fn gates_pass_on_fixture() {
        let adapter = WarnNyAdapter;
        let cap = fixture();
        let parsed = adapter.parse(&cap).unwrap();
        assert_eq!(run_gates(&adapter.gates(), &cap, &parsed), GateResult::Pass);
    }

    #[test]
    fn header_drift_fails_gate() {
        let adapter = WarnNyAdapter;
        let mangled = String::from_utf8_lossy(&fixture().bytes)
            .replace("Impacted Site County", "Site County (new!)");
        let cap = fixture_capture("warn_ny", WARN_CSV_URL, mangled.into_bytes());
        let parsed = adapter.parse(&cap).unwrap_or_default();
        match run_gates(&adapter.gates(), &cap, &parsed) {
            GateResult::Fail(reason) => assert!(reason.contains("warn_headers_present")),
            GateResult::Pass => panic!("expected gate failure on header drift"),
        }
    }
}
