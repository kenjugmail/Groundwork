# Methodology

**Model version: `additive-v1`. Weights version: `v1` ([crates/fusion/weights.v1.toml](crates/fusion/weights.v1.toml)).**

Changing any weight, discount, or decay constant is a pull request against
`weights.v1.toml` (or a new versioned file) with a rationale and a replay diff
showing how the map moved. Every nowcast row is stamped with the
`model_version` and `weights_version` that produced it.

## The two clocks

- **Fast clock — `GapNowcast`.** A per-tract nowcast of where food-insecurity
  need appears to be moving, recomputed whenever signals arrive. Labeled a
  signal, not proof.
- **Slow clock — `ImpactRecord`.** Verified outcomes over months. Separate
  table, separate API path (`/v1/impact/...`). Empty in v0 by design.

## The additive model (v1)

```
nowcast_gap(tract, t) = baseline_gap(tract)
                      + Σ_signals  weight(type)
                                 · gameability_discount(type)
                                 · recency_decay(observed_at, t, type)
                                 · magnitude · direction
```

- **Baseline-anchored.** `baseline_gap` is the Map the Meal Gap county
  food-insecurity rate, adjusted to tract level by the ratio of the tract's
  ACS poverty rate to its county's (clamped to [0.25, 4.0]); where ACS is
  missing the county rate is used directly. Zero signals ⇒ nowcast == baseline.
- **Recency decay** is exponential per signal type:
  `exp(-ln2 · Δdays / half_life_days(type))`.
- **Magnitude normalization** is per signal type and documented in the weights
  file (e.g. WARN: affected workers per 1,000 county residents).
- **Coarse-resolution apportionment.** Signals resolved at county/state level
  are apportioned to member tracts **uniformly** in v1 (population-weighted is
  a planned change; the choice is material and PR-able).
- **News decomposition.** Every term in the sum is stored on the nowcast row
  as `{signal_id, signal_type, weight, gameability_discount, recency_factor,
  magnitude, direction, contribution}`. The API returns it verbatim — the
  attribution sentence *is* the product.

## Coverage and uncertainty

- `coverage_score(tract, t)` = fraction of enabled signal-bearing sources that
  are *fresh* (last successful, non-quarantined ingest within 2× their
  configured cadence). Quarantined sources count as stale. A source going dark
  visibly lowers coverage; it never lowers apparent need.
- `uncertainty(tract, t)` = `sigma_baseline + k_signal · Σ|contribution| +
  k_coverage · (1 − coverage_score)`, constants in the weights file. Crude and
  honest; replaced by a model-based density in the DFM upgrade (M3).

## Quarantine (drift gates)

Each adapter asserts invariants on every capture (expected fields, value
ranges, row counts in band). A failed gate ingests that capture's signals with
`status='quarantined'` and `coverage_flag='coverage_degraded'`; fusion excludes
them and coverage drops. Raw bytes of every fetch are recorded append-only
before parsing; the whole pipeline is replayable against history.

## Anti-Goodhart posture

No money flows through this system; the gap is anchored to a hard-to-fake
annual baseline; gameable sources carry explicit discounts. Goodhart isn't
eliminated — it's made expensive and visible.

## Signal weights (v1) — summary

See [crates/fusion/weights.v1.toml](crates/fusion/weights.v1.toml) for the
authoritative values. Rationale per type:

| type | weight | gameability discount | half-life | rationale |
|---|---|---|---|---|
| `layoff_warn` | 0.40 | 0.95 | 90d | legally mandated filing, very hard to fake; jobs→food lag is months |
| `survey_food_insufficiency` | 0.20 | 1.00 | 45d | federal survey, coarse geography, sampling noise |
| `snap_enrollment_change` | 0.30 | 0.95 | 120d | administrative data, lagged confirmation |
| `pantry_capacity` | 0.15 | 0.50 | 30d | self-reported supply side — heavily discounted |

## Planned upgrade (M3)

Dynamic Factor Model + Kalman filter with exact news decomposition (NY Fed
nowcast family). Handles mixed frequency, missing observations, and ragged-edge
arrival natively, and outputs a density rather than a point. The additive model
above is the transparent stepping stone.
