//! Census ACS 5-year baseline: poverty rate (B17001) and SNAP rate (B22003)
//! per tract for the six in-scope counties. Baseline source — produces
//! `baselines` rows, not signals.

use crate::{Fetcher, ParseError};
use store::raw_store::Capture;

pub const ACS_YEAR: i32 = 2023;

pub fn acs_url(api_key: Option<&str>) -> String {
    let mut url = format!(
        "https://api.census.gov/data/{ACS_YEAR}/acs/acs5?get=NAME,B17001_002E,B17001_001E,B22003_002E,B22003_001E&for=tract:*&in=state:36&in=county:005,047,061,081,085,119"
    );
    if let Some(k) = api_key {
        if !k.is_empty() {
            url.push_str(&format!("&key={k}"));
        }
    }
    url
}

#[derive(Debug, Clone)]
pub struct TractBaseline {
    pub geoid: String,
    pub poverty_rate: Option<f64>,
    pub snap_rate: Option<f64>,
    pub population: Option<f64>,
}

pub async fn fetch(fetcher: &dyn Fetcher, api_key: Option<&str>) -> anyhow::Result<Capture> {
    fetcher.fetch("acs", &acs_url(api_key)).await
}

/// ACS returns a JSON array-of-arrays; row 0 is the header.
pub fn parse(capture: &Capture) -> Result<Vec<TractBaseline>, ParseError> {
    let bytes = capture.bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&capture.bytes);
    let rows: Vec<Vec<Option<String>>> = serde_json::from_slice(bytes)
        .map_err(|e| ParseError::Drift(format!("ACS response not JSON array-of-arrays: {e}")))?;
    let mut iter = rows.into_iter();
    let header = iter.next().ok_or(ParseError::Drift("empty ACS response".into()))?;
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h.as_deref() == Some(name))
            .ok_or_else(|| ParseError::Drift(format!("ACS column {name} missing")))
    };
    let (c_pov_n, c_pov_d, c_snap_n, c_snap_d) = (
        col("B17001_002E")?,
        col("B17001_001E")?,
        col("B22003_002E")?,
        col("B22003_001E")?,
    );
    let (c_state, c_county, c_tract) = (col("state")?, col("county")?, col("tract")?);

    let num = |v: Option<&String>| -> Option<f64> {
        v.and_then(|s| s.parse::<f64>().ok()).filter(|x| *x >= 0.0)
    };
    let mut out = Vec::new();
    for row in iter {
        let get = |i: usize| row.get(i).and_then(|v| v.as_ref());
        let (Some(state), Some(county), Some(tract)) = (get(c_state), get(c_county), get(c_tract))
        else {
            continue;
        };
        let geoid = format!("{state}{county}{tract}");
        let pov_d = num(get(c_pov_d));
        let snap_d = num(get(c_snap_d));
        out.push(TractBaseline {
            geoid,
            poverty_rate: match (num(get(c_pov_n)), pov_d) {
                (Some(n), Some(d)) if d > 0.0 => Some(n / d),
                _ => None,
            },
            snap_rate: match (num(get(c_snap_n)), snap_d) {
                (Some(n), Some(d)) if d > 0.0 => Some(n / d),
                _ => None,
            },
            population: pov_d,
        });
    }
    if out.is_empty() {
        return Err(ParseError::Drift("ACS returned zero tract rows".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture_capture;

    #[test]
    fn parses_fixture() {
        let bytes = include_bytes!("../../../fixtures/acs_sample.json").to_vec();
        let cap = fixture_capture("acs", &acs_url(None), bytes);
        let rows = parse(&cap).unwrap();
        assert!(!rows.is_empty());
        let r = &rows[0];
        assert_eq!(r.geoid.len(), 11);
        assert!(r.poverty_rate.unwrap() >= 0.0 && r.poverty_rate.unwrap() <= 1.0);
    }
}
