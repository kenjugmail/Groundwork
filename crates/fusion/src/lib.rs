//! Additive nowcast model (v1). Pure functions, no I/O, no DB — separable so
//! the DFM/Kalman upgrade (M3) can replace it without touching the pipeline.
//!
//! nowcast_gap = baseline_gap + Σ w(type)·g(type)·recency·normalized_magnitude·direction
//!
//! Every term is recorded as a NewsDecompositionEntry: the attribution
//! sentence is the product. Zero signals ⇒ nowcast == baseline.

use chrono::{DateTime, Utc};
use groundwork_types::{GapNowcast, NewsDecompositionEntry, SignalType};
use serde::Deserialize;
use std::collections::HashMap;

pub const WEIGHTS_V1: &str = include_str!("../weights.v1.toml");

#[derive(Debug, Clone, Deserialize)]
pub struct Weights {
    pub version: String,
    pub model_version: String,
    pub uncertainty: UncertaintyParams,
    pub signal_types: HashMap<String, TypeWeights>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UncertaintyParams {
    pub sigma_baseline: f64,
    pub k_signal: f64,
    pub k_coverage: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TypeWeights {
    pub weight: f64,
    pub gameability_discount: f64,
    pub half_life_days: f64,
    pub normalizer: f64,
}

impl Weights {
    pub fn v1() -> Self {
        toml::from_str(WEIGHTS_V1).expect("bundled weights.v1.toml must parse")
    }

    pub fn for_type(&self, t: SignalType) -> Option<&TypeWeights> {
        self.signal_types.get(t.as_str())
    }
}

/// A signal after geo-apportionment to one tract: `magnitude` already divided
/// across the member tracts of a coarser unit when applicable.
#[derive(Debug, Clone)]
pub struct ApportionedSignal {
    pub signal_id: uuid::Uuid,
    pub signal_type: SignalType,
    pub observed_at: DateTime<Utc>,
    pub magnitude: f64,
    pub direction: i16,
}

/// Inputs that are about the tract, not the signals.
#[derive(Debug, Clone)]
pub struct TractContext {
    pub geo_unit_id: String,
    pub baseline_gap: f64,
    pub coverage_score: f64,
}

pub fn recency_factor(observed_at: DateTime<Utc>, as_of: DateTime<Utc>, half_life_days: f64) -> f64 {
    let age_days = (as_of - observed_at).num_seconds() as f64 / 86_400.0;
    if age_days < 0.0 {
        return 1.0; // future-dated observation: no decay, no amplification
    }
    (-(std::f64::consts::LN_2) * age_days / half_life_days).exp()
}

pub fn nowcast(
    ctx: &TractContext,
    signals: &[ApportionedSignal],
    weights: &Weights,
    as_of: DateTime<Utc>,
) -> GapNowcast {
    let mut decomposition = Vec::new();
    let mut delta = 0.0;
    for s in signals {
        let Some(tw) = weights.for_type(s.signal_type) else {
            tracing::warn!(signal_type = s.signal_type.as_str(), "no weight entry; signal ignored");
            continue;
        };
        let recency = recency_factor(s.observed_at, as_of, tw.half_life_days);
        let normalized = s.magnitude / tw.normalizer;
        let contribution =
            tw.weight * tw.gameability_discount * recency * normalized * s.direction as f64;
        delta += contribution;
        decomposition.push(NewsDecompositionEntry {
            signal_id: s.signal_id,
            signal_type: s.signal_type,
            weight: tw.weight,
            gameability_discount: tw.gameability_discount,
            recency_factor: recency,
            magnitude: normalized,
            direction: s.direction,
            contribution,
        });
    }
    // Sort by |contribution| so the API's first entry is the headline reason.
    decomposition.sort_by(|a, b| {
        b.contribution.abs().partial_cmp(&a.contribution.abs()).unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_abs: f64 = decomposition.iter().map(|d| d.contribution.abs()).sum();
    let u = &weights.uncertainty;
    let uncertainty =
        u.sigma_baseline + u.k_signal * total_abs + u.k_coverage * (1.0 - ctx.coverage_score);

    GapNowcast {
        geo_unit_id: ctx.geo_unit_id.clone(),
        as_of,
        baseline_gap: ctx.baseline_gap,
        nowcast_gap: (ctx.baseline_gap + delta).max(0.0),
        uncertainty,
        coverage_score: ctx.coverage_score,
        news_decomposition: decomposition,
        model_version: weights.model_version.clone(),
        weights_version: weights.version.clone(),
    }
}

/// Tract-level baseline_gap: county MMG rate adjusted by the ratio of tract
/// poverty to county poverty, clamped so a noisy tract can't explode the
/// anchor. Falls back to the county rate when tract poverty is missing.
pub fn tract_baseline_gap(
    county_mmg_rate: f64,
    tract_poverty_rate: Option<f64>,
    county_poverty_rate: Option<f64>,
) -> f64 {
    match (tract_poverty_rate, county_poverty_rate) {
        (Some(t), Some(c)) if c > 0.0 => {
            let ratio = (t / c).clamp(0.25, 4.0);
            county_mmg_rate * ratio
        }
        _ => county_mmg_rate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn ctx() -> TractContext {
        TractContext {
            geo_unit_id: "36119000100".into(),
            baseline_gap: 0.084,
            coverage_score: 1.0,
        }
    }

    /// Commitment in METHODOLOGY.md: zero signals ⇒ nowcast == baseline.
    #[test]
    fn zero_signals_equals_baseline() {
        let w = Weights::v1();
        let nc = nowcast(&ctx(), &[], &w, Utc::now());
        assert_eq!(nc.nowcast_gap, nc.baseline_gap);
        assert!(nc.news_decomposition.is_empty());
        assert!((nc.uncertainty - w.uncertainty.sigma_baseline).abs() < 1e-12);
    }

    #[test]
    fn warn_signal_raises_nowcast_and_is_attributed() {
        let w = Weights::v1();
        let as_of = Utc::now();
        let sig = ApportionedSignal {
            signal_id: uuid::Uuid::new_v4(),
            signal_type: SignalType::LayoffWarn,
            observed_at: as_of - Duration::days(6),
            magnitude: 400.0,
            direction: 1,
        };
        let nc = nowcast(&ctx(), &[sig.clone()], &w, as_of);
        assert!(nc.nowcast_gap > nc.baseline_gap);
        assert_eq!(nc.news_decomposition.len(), 1);
        assert_eq!(nc.news_decomposition[0].signal_id, sig.signal_id);
        assert!(nc.uncertainty > w.uncertainty.sigma_baseline);
    }

    #[test]
    fn old_signals_decay() {
        let w = Weights::v1();
        let as_of = Utc::now();
        let mk = |days_ago: i64| ApportionedSignal {
            signal_id: uuid::Uuid::new_v4(),
            signal_type: SignalType::LayoffWarn,
            observed_at: as_of - Duration::days(days_ago),
            magnitude: 400.0,
            direction: 1,
        };
        let fresh = nowcast(&ctx(), &[mk(1)], &w, as_of);
        let stale = nowcast(&ctx(), &[mk(360)], &w, as_of);
        assert!(fresh.nowcast_gap > stale.nowcast_gap);
        assert!(stale.nowcast_gap > stale.baseline_gap); // decayed, not discarded
    }

    #[test]
    fn low_coverage_raises_uncertainty_not_need() {
        let w = Weights::v1();
        let mut blind = ctx();
        blind.coverage_score = 0.2;
        let nc_full = nowcast(&ctx(), &[], &w, Utc::now());
        let nc_blind = nowcast(&blind, &[], &w, Utc::now());
        assert_eq!(nc_full.nowcast_gap, nc_blind.nowcast_gap); // need unchanged
        assert!(nc_blind.uncertainty > nc_full.uncertainty);
    }

    #[test]
    fn baseline_adjustment_clamps() {
        assert!((tract_baseline_gap(0.10, Some(0.50), Some(0.05)) - 0.40).abs() < 1e-12); // clamped at 4x
        assert_eq!(tract_baseline_gap(0.10, None, Some(0.05)), 0.10);
    }
}
