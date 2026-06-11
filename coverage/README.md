# Coverage & bias register

Commitment #4: **absence of signal ≠ absence of need.** Under-resourced areas
produce less data; low coverage must never read as low need. This register
documents known blind spots so users correct for them rather than be misled.

## How coverage is computed (v0)

`coverage_score` = fraction of enabled signal-bearing sources that are fresh
(last successful, non-quarantined ingest within 2× their cadence). It ships on
every nowcast row and renders as dot opacity in the demo UI. See
METHODOLOGY.md.

v0 limitation: the score is global, not per-tract. Per-tract coverage
(which sources can even *see* a given tract) is planned alongside the
Urban-Institute-style input-representation analysis below.

## Known blind spots (v0)

| Blind spot | Why | Status |
|---|---|---|
| 211 call volume absent | hero leading indicator awaits a data-sharing agreement with the NYC/Westchester 211 operator | source registered but disabled; counts against coverage |
| WARN sees only employers ≥50 layoffs | statutory threshold; small-business loss is invisible | document only; no fix planned in v0 |
| Household Pulse is state-level | survey design; cannot localize | apportioned thinly; calibration only |
| Westchester SNAP enrollment not in NYC Open Data | dataset covers the five boroughs only | Westchester tracts get no `snap_enrollment_change` signal — their coverage is overstated by the global score until per-tract coverage lands |
| Undocumented and unbanked populations under-appear in *all* administrative sources | structural | document; weight community-sourced signals when they exist |

## Planned (M3)

- Per-tract coverage channels and the "**we may be blind here**" alert:
  high uncertainty + historically high baseline + sources gone quiet.
- Input-representation analysis following the Urban Institute Spatial Equity
  Tool's approach.
