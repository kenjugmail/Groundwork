//! Baseline loaders: ACS (live API) and Map the Meal Gap (manual CSV drop).

use adapters::{acs, chr, meal_gap, Fetcher};
use store::Db;

/// National multi-category baselines from County Health Rankings: food
/// insecurity, child poverty, unemployment, uninsured, median income —
/// per county and state. Slow clock, annual, recorded like every fetch.
pub async fn ingest_chr(db: &Db, fetcher: &dyn Fetcher) -> anyhow::Result<usize> {
    let capture = chr::fetch(fetcher).await?;
    let rows = match chr::parse(&capture) {
        Ok(rows) => rows,
        Err(e) => {
            db.mark_source_ingest("chr", true).await?;
            anyhow::bail!("CHR parse drift (source marked degraded): {e}");
        }
    };
    let mut upserted = 0usize;
    let mut skipped = 0usize;
    for r in &rows {
        if !db.geo_unit_exists(&r.geoid).await? {
            skipped += 1; // geometry not loaded (run load-us-counties) or territory
            continue;
        }
        for (metric, value) in &r.values {
            db.upsert_baseline(&r.geoid, metric, chr::CHR_YEAR, *value, "chr").await?;
            upserted += 1;
        }
    }
    if skipped > 0 {
        tracing::warn!(skipped, "CHR rows without a matching geo_unit (run load-us-counties first for full coverage)");
    }
    db.mark_source_ingest("chr", false).await?;
    Ok(upserted)
}

pub async fn ingest_acs(db: &Db, fetcher: &dyn Fetcher) -> anyhow::Result<usize> {
    let api_key = std::env::var("CENSUS_API_KEY").ok().filter(|k| !k.is_empty());
    if api_key.is_none() {
        tracing::warn!("CENSUS_API_KEY is empty — the Census API now requires a key; the response will be an error page and the source will be marked degraded");
    }
    let capture = acs::fetch(fetcher, api_key.as_deref()).await?;
    let rows = match acs::parse(&capture) {
        Ok(rows) => rows,
        Err(e) => {
            db.mark_source_ingest("acs", true).await?;
            anyhow::bail!("ACS parse drift (source marked degraded): {e}");
        }
    };
    let mut n = 0;
    for r in &rows {
        if !db.geo_unit_exists(&r.geoid).await? {
            continue; // tract vintage mismatch or out-of-scope row
        }
        if let Some(p) = r.poverty_rate {
            db.upsert_baseline(&r.geoid, "acs_poverty_rate", acs::ACS_YEAR, p, "acs").await?;
            n += 1;
        }
        if let Some(s) = r.snap_rate {
            db.upsert_baseline(&r.geoid, "acs_snap_rate", acs::ACS_YEAR, s, "acs").await?;
        }
    }
    db.mark_source_ingest("acs", false).await?;
    Ok(n)
}

pub async fn ingest_meal_gap(db: &Db, file: &str) -> anyhow::Result<usize> {
    let bytes = tokio::fs::read(file).await?;
    let rows = meal_gap::parse(&bytes).map_err(|e| anyhow::anyhow!("MMG parse: {e}"))?;
    let mut n = 0;
    for r in &rows {
        if !db.geo_unit_exists(&r.county_geoid).await? {
            tracing::warn!(county = %r.county_geoid, "MMG county not in geo_units; skipped (run load-tracts first)");
            continue;
        }
        db.upsert_baseline(
            &r.county_geoid,
            "mmg_food_insecurity_rate",
            r.year,
            r.food_insecurity_rate,
            "meal_gap",
        )
        .await?;
        n += 1;
    }
    db.mark_source_ingest("meal_gap", false).await?;
    Ok(n)
}
