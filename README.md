# Groundwork

An open-source civic **needs-transparency** layer. Groundwork ingests messy,
real-time signals about a community, fuses them into an explainable,
geographically-resolved map of where unmet need is *widening*, and publishes
that map — with its full evidence chain — as open data and an open API.

It does **not** allocate money. It equips the people and funds who do.

**v0 scope:** food insecurity, NYC + Westchester, census-tract resolution.

## Design commitments

1. **Transparency over cleverness.** Every number clicks down to the raw
   signals that produced it. If we can't show *why* a tract is flagged, we
   don't flag it.
2. **Two clocks, never merged.** A fast sensing layer (nowcast — a signal,
   not proof) and a slow impact layer (verified outcomes). Separate stores,
   separate endpoints, separate confidence regimes.
3. **We inform, we do not allocate.**
4. **Absence of signal ≠ absence of need.** Coverage is modeled explicitly
   and surfaced as its own dimension; a source going dark lowers coverage,
   never apparent need.
5. **Adversarial-aware weighting.** Hard-to-fake signals outweigh easy-to-fake
   ones; every signal type carries a gameability discount.
6. **Fully open, ground-up.** Code Apache-2.0, data CC-BY-4.0, methodology
   versioned and PR-able.

Read [METHODOLOGY.md](METHODOLOGY.md) for the fusion model and weights, and
[WHAT_THIS_IS_NOT.md](WHAT_THIS_IS_NOT.md) before citing the map.

## Quick start

```powershell
# 1. Postgres + PostGIS
docker compose up -d
Copy-Item .env.example .env

# 2. Build
cargo build --workspace

# 3. Load census tract geometries (downloads TIGER/Line for NY)
cargo run -p orchestrator -- load-tracts

# 4. Ingest sources
cargo run -p orchestrator -- ingest warn-ny      # NY DOL WARN layoff notices
cargo run -p orchestrator -- ingest acs          # ACS poverty/SNAP baseline
cargo run -p orchestrator -- ingest meal-gap --file fixtures/mmg_sample.csv
cargo run -p orchestrator -- nowcast             # recompute nowcasts

# 5. Serve API + map
cargo run -p api
# open http://127.0.0.1:8080
```

## API (v0, read-only)

| Endpoint | Returns |
|---|---|
| `GET /v1/nowcast?bbox=minx,miny,maxx,maxy&as_of=...` | GeoJSON FeatureCollection: per-tract `nowcast_gap`, `baseline_gap`, `uncertainty`, `coverage_score`, `news_decomposition[]` |
| `GET /v1/signals/{id}` | Full signal incl. `raw_excerpt` + `provenance_url` |
| `GET /v1/alerts?since=...` | Stub in v0 |
| `GET /v1/impact/...` | Slow-clock impact records (schema only in v0) |

## Repo layout

- `crates/orchestrator` — scheduler/coordinator: cadence, backoff, transactional writes
- `crates/adapters` — one module per structured source (WARN, ACS, MMG, Pulse, Socrata, 211 stub)
- `crates/fusion` — additive nowcast model + versioned `weights.v1.toml` (DFM/Kalman planned for M3)
- `crates/store` — Postgres+PostGIS migrations, repos, append-only raw doc store
- `crates/api` — public read API (axum) + static map UI
- `crates/replay` — record/replay + drift-gate harness
- `harness/` — agentic extraction harness (stub in this milestone)
- `coverage/` — bias & blind-spot register
- `ui/` — MapLibre demo of the API

## License

Code: Apache-2.0 ([LICENSE](LICENSE)). Data outputs: CC-BY-4.0 ([DATA_LICENSE](DATA_LICENSE)).
