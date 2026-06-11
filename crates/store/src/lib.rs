//! Postgres + PostGIS persistence layer and the append-only raw doc store.

pub mod raw_store;

use chrono::{DateTime, Utc};
use groundwork_types::{NewSignal, NewsDecompositionEntry, Signal};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct Db {
    pub pool: PgPool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SourceRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub cadence_seconds: i64,
    pub enabled: bool,
    pub last_ok_ingest: Option<DateTime<Utc>>,
    pub last_quarantine: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct GeoUnitRef {
    pub id: String,
    pub kind: String,
    pub name: String,
}

impl Db {
    pub async fn connect(database_url: &str) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> anyhow::Result<()> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    // ---- geo units ----

    pub async fn upsert_geo_unit_wkt(
        &self,
        id: &str,
        kind: &str,
        name: &str,
        state_fips: &str,
        county_fips: Option<&str>,
        wkt_multipolygon: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"INSERT INTO geo_units (id, kind, name, state_fips, county_fips, geom, centroid)
               VALUES ($1,$2,$3,$4,$5,
                       CASE WHEN $6::text IS NULL THEN NULL
                            ELSE ST_Multi(ST_GeomFromText($6, 4326)) END,
                       CASE WHEN $6::text IS NULL THEN NULL
                            ELSE ST_PointOnSurface(ST_GeomFromText($6, 4326)) END)
               ON CONFLICT (id) DO UPDATE SET
                   name = EXCLUDED.name,
                   geom = COALESCE(EXCLUDED.geom, geo_units.geom),
                   centroid = COALESCE(EXCLUDED.centroid, geo_units.centroid)"#,
        )
        .bind(id)
        .bind(kind)
        .bind(name)
        .bind(state_fips)
        .bind(county_fips)
        .bind(wkt_multipolygon)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn geo_unit_exists(&self, id: &str) -> anyhow::Result<bool> {
        let row = sqlx::query("SELECT 1 FROM geo_units WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_some())
    }

    /// All tract GEOIDs belonging to a parent unit (county or state).
    pub async fn tracts_under(&self, parent_geoid: &str) -> anyhow::Result<Vec<String>> {
        let rows = if parent_geoid.len() == 5 {
            sqlx::query(
                "SELECT id FROM geo_units WHERE kind='tract'
                 AND state_fips = $1 AND county_fips = $2",
            )
            .bind(&parent_geoid[..2])
            .bind(&parent_geoid[2..])
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query("SELECT id FROM geo_units WHERE kind='tract' AND state_fips = $1")
                .bind(parent_geoid)
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows.into_iter().map(|r| r.get::<String, _>("id")).collect())
    }

    pub async fn all_tract_ids(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT id FROM geo_units WHERE kind='tract'")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.get::<String, _>("id")).collect())
    }

    // ---- sources ----

    pub async fn sources(&self) -> anyhow::Result<Vec<SourceRow>> {
        Ok(sqlx::query_as::<_, SourceRow>(
            "SELECT id, name, kind, cadence_seconds, enabled, last_ok_ingest, last_quarantine
             FROM sources",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn mark_source_ingest(&self, source_id: &str, quarantined: bool) -> anyhow::Result<()> {
        if quarantined {
            sqlx::query("UPDATE sources SET last_quarantine = now() WHERE id = $1")
                .bind(source_id)
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("UPDATE sources SET last_ok_ingest = now() WHERE id = $1")
                .bind(source_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    // ---- signals ----

    /// Insert a batch of signals in one transaction, idempotent on dedupe_key.
    /// If `quarantine` is set, rows land with status='quarantined' and
    /// coverage_flag='coverage_degraded' (drift gate failed: garbage must
    /// lower coverage, never feed the nowcast). Returns count inserted.
    pub async fn insert_signals(
        &self,
        signals: &[NewSignal],
        quarantine: bool,
    ) -> anyhow::Result<u64> {
        let (status, flag): (&str, Option<&str>) = if quarantine {
            ("quarantined", Some("coverage_degraded"))
        } else {
            ("active", None)
        };
        let mut tx = self.pool.begin().await?;
        let mut inserted = 0u64;
        for s in signals {
            let res = sqlx::query(
                r#"INSERT INTO signals
                   (source_id, geo_unit_id, signal_type, observed_at, magnitude, direction,
                    payload, provenance_url, raw_excerpt, raw_capture_id, resolution_level,
                    status, coverage_flag, dedupe_key)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
                   ON CONFLICT (dedupe_key) DO NOTHING"#,
            )
            .bind(&s.source_id)
            .bind(&s.geo_unit_id)
            .bind(s.signal_type.as_str())
            .bind(s.observed_at)
            .bind(s.magnitude)
            .bind(s.direction)
            .bind(&s.payload)
            .bind(&s.provenance_url)
            .bind(&s.raw_excerpt)
            .bind(&s.raw_capture_id)
            .bind(s.resolution_level.as_str())
            .bind(status)
            .bind(flag)
            .bind(&s.dedupe_key)
            .execute(&mut *tx)
            .await?;
            inserted += res.rows_affected();
        }
        tx.commit().await?;
        Ok(inserted)
    }

    pub async fn signal(&self, id: Uuid) -> anyhow::Result<Option<Signal>> {
        Ok(sqlx::query_as::<_, Signal>(
            "SELECT id, source_id, geo_unit_id, signal_type, observed_at, ingested_at,
                    magnitude, direction, payload, provenance_url, raw_excerpt,
                    raw_capture_id, resolution_level, status, coverage_flag,
                    supersedes, dedupe_key
             FROM signals WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// All active (non-superseded, non-quarantined) signals.
    pub async fn active_signals(&self) -> anyhow::Result<Vec<Signal>> {
        Ok(sqlx::query_as::<_, Signal>(
            "SELECT id, source_id, geo_unit_id, signal_type, observed_at, ingested_at,
                    magnitude, direction, payload, provenance_url, raw_excerpt,
                    raw_capture_id, resolution_level, status, coverage_flag,
                    supersedes, dedupe_key
             FROM signals WHERE status = 'active'",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    // ---- baselines ----

    pub async fn upsert_baseline(
        &self,
        geo_unit_id: &str,
        metric: &str,
        year: i32,
        value: f64,
        source_id: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO baselines (geo_unit_id, metric, year, value, source_id)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (geo_unit_id, metric, year) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(geo_unit_id)
        .bind(metric)
        .bind(year)
        .bind(value)
        .bind(source_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Latest value per geo unit for a metric.
    pub async fn latest_baselines(&self, metric: &str) -> anyhow::Result<Vec<(String, f64)>> {
        let rows = sqlx::query(
            "SELECT DISTINCT ON (geo_unit_id) geo_unit_id, value
             FROM baselines WHERE metric = $1
             ORDER BY geo_unit_id, year DESC",
        )
        .bind(metric)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get::<String, _>("geo_unit_id"), r.get::<f64, _>("value")))
            .collect())
    }

    // ---- nowcasts ----

    pub async fn upsert_nowcast(
        &self,
        geo_unit_id: &str,
        as_of: DateTime<Utc>,
        baseline_gap: f64,
        nowcast_gap: f64,
        uncertainty: f64,
        coverage_score: f64,
        news_decomposition: &[NewsDecompositionEntry],
        model_version: &str,
        weights_version: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"INSERT INTO gap_nowcasts
               (geo_unit_id, as_of, baseline_gap, nowcast_gap, uncertainty,
                coverage_score, news_decomposition, model_version, weights_version)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
               ON CONFLICT (geo_unit_id, as_of) DO UPDATE SET
                   baseline_gap = EXCLUDED.baseline_gap,
                   nowcast_gap = EXCLUDED.nowcast_gap,
                   uncertainty = EXCLUDED.uncertainty,
                   coverage_score = EXCLUDED.coverage_score,
                   news_decomposition = EXCLUDED.news_decomposition,
                   model_version = EXCLUDED.model_version,
                   weights_version = EXCLUDED.weights_version"#,
        )
        .bind(geo_unit_id)
        .bind(as_of)
        .bind(baseline_gap)
        .bind(nowcast_gap)
        .bind(uncertainty)
        .bind(coverage_score)
        .bind(serde_json::to_value(news_decomposition)?)
        .bind(model_version)
        .bind(weights_version)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
