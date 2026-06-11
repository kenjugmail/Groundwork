//! The nowcast job: apportion active signals to tracts, compute coverage,
//! run the fusion model per tract, and write gap_nowcasts.

use chrono::Utc;
use fusion::{ApportionedSignal, TractContext, Weights};
use groundwork_types::Signal;
use std::collections::HashMap;
use store::Db;

pub async fn recompute(db: &Db) -> anyhow::Result<usize> {
    let weights = Weights::v1();
    let as_of = Utc::now();

    let tracts = db.all_tract_ids().await?;
    if tracts.is_empty() {
        anyhow::bail!("no tracts loaded; run `orchestrator load-tracts` first");
    }

    // Baselines.
    let mmg: HashMap<String, f64> =
        db.latest_baselines("mmg_food_insecurity_rate").await?.into_iter().collect();
    let poverty: HashMap<String, f64> =
        db.latest_baselines("acs_poverty_rate").await?.into_iter().collect();
    // County mean poverty (over tracts with data) for the baseline adjustment.
    let mut county_pov_sum: HashMap<String, (f64, f64)> = HashMap::new();
    for (geoid, p) in &poverty {
        if geoid.len() == 11 {
            let e = county_pov_sum.entry(geoid[..5].to_string()).or_insert((0.0, 0.0));
            e.0 += p;
            e.1 += 1.0;
        }
    }
    let county_poverty: HashMap<String, f64> = county_pov_sum
        .into_iter()
        .map(|(c, (sum, n))| (c, sum / n))
        .collect();

    // Apportion active signals to tracts: tract-level pass through; county and
    // state-level signals are split uniformly across member tracts (v1 — see
    // METHODOLOGY.md; population-weighted is a planned, PR-able change).
    let signals = db.active_signals().await?;
    let mut per_tract: HashMap<String, Vec<ApportionedSignal>> = HashMap::new();
    let mut tracts_by_parent: HashMap<String, Vec<String>> = HashMap::new();
    for s in &signals {
        let targets: Vec<String> = match s.geo_unit_id.len() {
            11 => vec![s.geo_unit_id.clone()],
            _ => {
                if !tracts_by_parent.contains_key(&s.geo_unit_id) {
                    let t = db.tracts_under(&s.geo_unit_id).await?;
                    tracts_by_parent.insert(s.geo_unit_id.clone(), t);
                }
                tracts_by_parent[&s.geo_unit_id].clone()
            }
        };
        if targets.is_empty() {
            continue;
        }
        let share = apportioned_magnitude(s, targets.len());
        for t in targets {
            per_tract.entry(t).or_default().push(ApportionedSignal {
                signal_id: s.id,
                signal_type: s.signal_type,
                observed_at: s.observed_at,
                magnitude: share,
                direction: s.direction,
            });
        }
    }

    // Coverage: fraction of enabled signal-bearing sources that are fresh
    // (last ok ingest within 2x cadence and not quarantined more recently).
    let coverage = global_coverage(db).await?;

    let mut written = 0usize;
    for tract in &tracts {
        let county = &tract[..5];
        let county_mmg = mmg.get(county).copied().unwrap_or(0.0);
        let baseline_gap = fusion::tract_baseline_gap(
            county_mmg,
            poverty.get(tract).copied(),
            county_poverty.get(county).copied(),
        );
        let ctx = TractContext {
            geo_unit_id: tract.clone(),
            baseline_gap,
            coverage_score: coverage,
        };
        let empty = Vec::new();
        let sigs = per_tract.get(tract).unwrap_or(&empty);
        let nc = fusion::nowcast(&ctx, sigs, &weights, as_of);
        db.upsert_nowcast(
            &nc.geo_unit_id,
            nc.as_of,
            nc.baseline_gap,
            nc.nowcast_gap,
            nc.uncertainty,
            nc.coverage_score,
            &nc.news_decomposition,
            &nc.model_version,
            &nc.weights_version,
        )
        .await?;
        written += 1;
    }
    Ok(written)
}

/// How a coarse signal's magnitude lands on one member tract.
/// Survey percentages are intensities (do not divide); count-like magnitudes
/// (workers, % change in a county aggregate) are diluted across tracts.
fn apportioned_magnitude(s: &Signal, n_targets: usize) -> f64 {
    use groundwork_types::SignalType::*;
    match s.signal_type {
        SurveyFoodInsufficiency => s.magnitude,
        LayoffWarn => s.magnitude / n_targets as f64 * 100.0, // workers per ~1% of county
        SnapEnrollmentChange | PantryCapacity => s.magnitude,
    }
}

async fn global_coverage(db: &Db) -> anyhow::Result<f64> {
    let sources = db.sources().await?;
    let now = Utc::now();
    let signal_sources: Vec<_> = sources
        .iter()
        .filter(|s| s.kind == "structured" || s.kind == "survey")
        .collect();
    if signal_sources.is_empty() {
        return Ok(0.0);
    }
    let enabled = signal_sources.iter().filter(|s| s.enabled).count();
    let mut fresh = 0usize;
    for s in &signal_sources {
        if !s.enabled {
            continue; // disabled (e.g. 211 pending agreement) counts against coverage
        }
        let ok = match s.last_ok_ingest {
            Some(t) => {
                let fresh_enough =
                    (now - t).num_seconds() <= 2 * s.cadence_seconds;
                let not_quarantined_since =
                    s.last_quarantine.map(|q| q < t).unwrap_or(true);
                fresh_enough && not_quarantined_since
            }
            None => false,
        };
        if ok {
            fresh += 1;
        }
    }
    let _ = enabled;
    Ok(fresh as f64 / signal_sources.len() as f64)
}
