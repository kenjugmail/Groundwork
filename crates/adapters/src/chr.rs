//! County Health Rankings & Roadmaps (University of Wisconsin Population
//! Health Institute) — national county-level slow baselines across multiple
//! need categories. Public CSV, no API key, annual.
//!
//! The analytic CSV has two header rows (human names, then machine names like
//! `v139_rawvalue`) followed by a US row, state rows (county FIPS 000), and
//! county rows. We index by the human header names.

use crate::{Fetcher, ParseError};
use store::raw_store::Capture;

pub const CHR_URL: &str = "https://www.countyhealthrankings.org/sites/default/files/media/document/analytic_data2025_v2.csv";
pub const CHR_PROVENANCE: &str = "https://www.countyhealthrankings.org/health-data";
pub const CHR_YEAR: i32 = 2025;

/// (human header, our namespaced metric name, is_fraction)
pub const MEASURES: [(&str, &str, bool); 7] = [
    // Population lets the UI turn rates into people (and gap estimates into
    // dollars) without a second data source.
    ("Population raw value", "chr_population", false),
    ("Food Insecurity raw value", "chr_food_insecurity_rate", true),
    ("Children in Poverty raw value", "chr_child_poverty_rate", true),
    ("Unemployment raw value", "chr_unemployment_rate", true),
    ("Uninsured raw value", "chr_uninsured_rate", true),
    ("Median Household Income raw value", "chr_median_household_income", false),
    // % of households with overcrowding, high cost burden, or inadequate
    // kitchen/plumbing — the closest national county-level proxy for
    // housing precarity short of homelessness counts (which are CoC-level).
    ("Severe Housing Problems raw value", "chr_severe_housing_rate", true),
];

#[derive(Debug, Clone)]
pub struct GeoBaselines {
    /// 5-digit county GEOID, or 2-digit state FIPS for state rows.
    pub geoid: String,
    pub name: String,
    pub state_abbr: String,
    /// (metric, value) pairs present in this row.
    pub values: Vec<(String, f64)>,
}

pub async fn fetch(fetcher: &dyn Fetcher) -> anyhow::Result<Capture> {
    fetcher.fetch("chr", CHR_URL).await
}

pub fn parse(capture: &Capture) -> Result<Vec<GeoBaselines>, ParseError> {
    parse_with_min(capture, 1000)
}

pub fn parse_with_min(capture: &Capture, min_rows: usize) -> Result<Vec<GeoBaselines>, ParseError> {
    let text = String::from_utf8_lossy(&capture.bytes);
    let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(text.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| ParseError::Other(e.to_string()))?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    let col = |name: &str| headers.iter().position(|h| h == name);
    let (Some(c_fips), Some(c_name), Some(c_state)) = (
        col("5-digit FIPS Code"),
        col("Name"),
        col("State Abbreviation"),
    ) else {
        return Err(ParseError::Drift(format!(
            "CHR header row unrecognized (first cols: {:?})",
            &headers[..headers.len().min(6)]
        )));
    };
    let measure_cols: Vec<(usize, &str, bool)> = MEASURES
        .iter()
        .filter_map(|(h, m, frac)| col(h).map(|i| (i, *m, *frac)))
        .collect();
    if measure_cols.len() < MEASURES.len() {
        return Err(ParseError::Drift(format!(
            "CHR measure columns missing: found {}/{}",
            measure_cols.len(),
            MEASURES.len()
        )));
    }

    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| ParseError::Other(e.to_string()))?;
        let field = |i: usize| rec.get(i).unwrap_or("").trim().to_string();
        let fips = field(c_fips);
        // Skip the machine-name header row and the national row.
        if !fips.chars().all(|c| c.is_ascii_digit()) || fips == "00000" || fips.is_empty() {
            continue;
        }
        let geoid = if fips.ends_with("000") && fips.len() == 5 {
            fips[..2].to_string() // state row
        } else {
            fips
        };
        let mut values = Vec::new();
        for (i, metric, is_frac) in &measure_cols {
            let raw = field(*i);
            if raw.is_empty() {
                continue;
            }
            let v: f64 = match raw.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            if *is_frac && !(0.0..=1.0).contains(&v) {
                return Err(ParseError::Drift(format!(
                    "CHR {metric} value {v} for {geoid} outside [0,1] — column moved?"
                )));
            }
            values.push((metric.to_string(), v));
        }
        if values.is_empty() {
            continue;
        }
        out.push(GeoBaselines {
            geoid,
            name: field(c_name),
            state_abbr: field(c_state),
            values,
        });
    }
    if out.len() < min_rows {
        return Err(ParseError::Drift(format!(
            "CHR returned only {} geo rows (expected ~3,200 counties + states)",
            out.len()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture_capture;

    #[test]
    fn parses_fixture() {
        let bytes = include_bytes!("../../../fixtures/chr_sample.csv").to_vec();
        let cap = fixture_capture("chr", CHR_URL, bytes);
        let rows = parse_with_min(&cap, 1).unwrap();
        // National row excluded; states collapse to 2-digit FIPS.
        assert!(rows.iter().all(|r| r.geoid != "00000"));
        assert!(rows.iter().any(|r| r.geoid.len() == 2));
        assert!(rows.iter().any(|r| r.geoid.len() == 5));
        let county = rows.iter().find(|r| r.geoid.len() == 5).unwrap();
        assert!(county.values.iter().any(|(m, v)| m == "chr_food_insecurity_rate" && *v > 0.0));
    }

    #[test]
    fn tiny_capture_trips_row_count_drift() {
        let bytes = include_bytes!("../../../fixtures/chr_sample.csv").to_vec();
        let cap = fixture_capture("chr", CHR_URL, bytes);
        assert!(matches!(parse(&cap), Err(ParseError::Drift(_))));
    }
}
