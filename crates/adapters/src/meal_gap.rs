//! Map the Meal Gap (Feeding America) — the slow baseline anchor.
//!
//! MMG is registration-gated and not wholesale redistributable, so this is a
//! manual annual drop: export the county sheet to CSV and run
//! `orchestrator ingest meal-gap --file <csv>`. Only a tiny derived sample is
//! committed as a fixture.
//!
//! Expected CSV columns (case/space tolerant): `FIPS`, `County, State`
//! (or `County`), `Year`, `Food Insecurity Rate` (fraction or percent).

use crate::ParseError;

#[derive(Debug, Clone)]
pub struct CountyMealGap {
    pub county_geoid: String,
    pub year: i32,
    pub food_insecurity_rate: f64,
}

pub fn parse(bytes: &[u8]) -> Result<Vec<CountyMealGap>, ParseError> {
    let mut rdr = csv::ReaderBuilder::new().flexible(true).from_reader(bytes);
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| ParseError::Other(e.to_string()))?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let col = |name: &str| headers.iter().position(|h| h.contains(name));
    let (Some(c_fips), Some(c_year), Some(c_rate)) =
        (col("fips"), col("year"), col("food insecurity rate"))
    else {
        return Err(ParseError::Drift(format!(
            "MMG CSV headers unrecognized: {headers:?}"
        )));
    };

    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| ParseError::Other(e.to_string()))?;
        let field = |i: usize| rec.get(i).unwrap_or("").trim().to_string();
        let fips_raw = field(c_fips);
        if fips_raw.is_empty() {
            continue;
        }
        // County FIPS may arrive as 4 digits (leading zero stripped by Excel).
        let county_geoid = format!("{:0>5}", fips_raw);
        let year: i32 = field(c_year)
            .parse()
            .map_err(|_| ParseError::Drift(format!("unparseable MMG year '{}'", field(c_year))))?;
        let mut rate: f64 = field(c_rate)
            .trim_end_matches('%')
            .parse()
            .map_err(|_| ParseError::Drift(format!("unparseable MMG rate '{}'", field(c_rate))))?;
        if rate > 1.0 {
            rate /= 100.0; // percent → fraction
        }
        out.push(CountyMealGap { county_geoid, year, food_insecurity_rate: rate });
    }
    if out.is_empty() {
        return Err(ParseError::Drift("MMG CSV contained no county rows".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let rows = parse(include_bytes!("../../../fixtures/mmg_sample.csv")).unwrap();
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|r| r.food_insecurity_rate > 0.0 && r.food_insecurity_rate < 1.0));
        assert!(rows.iter().any(|r| r.county_geoid == "36119"));
    }
}
