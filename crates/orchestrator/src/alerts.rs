//! Alert evaluation, run at the end of every nowcast recompute.
//!
//! Two families, per the spec: "widening gap" (need moving fast) and
//! "we've gone blind" (coverage collapsed / high-stakes tract gone quiet).
//! Hysteresis: an alert_key (which includes severity) only re-fires after a
//! cooldown, so a standing condition doesn't spam every recompute, but an
//! escalation always gets through.

use chrono::{DateTime, Utc};
use sqlx::Row;
use store::Db;

pub struct AlertConfig {
    pub widening_abs_threshold: f64,
    pub widening_lookback_days: i64,
    pub coverage_floor: f64,
    pub blind_uncertainty: f64,
    pub cooldown_hours: i64,
}

impl AlertConfig {
    pub fn from_env() -> Self {
        let f = |k: &str, d: f64| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
        Self {
            widening_abs_threshold: f("GROUNDWORK_ALERT_GAP_DELTA", 0.02),
            widening_lookback_days: f("GROUNDWORK_ALERT_LOOKBACK_DAYS", 7.0) as i64,
            coverage_floor: f("GROUNDWORK_ALERT_COVERAGE_FLOOR", 0.5),
            blind_uncertainty: f("GROUNDWORK_ALERT_BLIND_UNCERTAINTY", 0.06),
            cooldown_hours: f("GROUNDWORK_ALERT_COOLDOWN_HOURS", 72.0) as i64,
        }
    }
}

pub async fn evaluate_and_persist(db: &Db, as_of: DateTime<Utc>) -> anyhow::Result<u64> {
    let cfg = AlertConfig::from_env();
    let mut fired = 0u64;

    // 1) widening_gap: latest delta above threshold AND grew vs the latest
    //    nowcast at/before (as_of - lookback). The cited top decomposition
    //    entry makes the alert self-explaining.
    fired += sqlx::query(
        r#"
        WITH latest AS (
            SELECT DISTINCT ON (geo_unit_id) geo_unit_id, as_of,
                   nowcast_gap, baseline_gap,
                   nowcast_gap - baseline_gap AS delta, news_decomposition
            FROM gap_nowcasts WHERE as_of <= $1
            ORDER BY geo_unit_id, as_of DESC
        ), prior AS (
            SELECT DISTINCT ON (geo_unit_id) geo_unit_id,
                   nowcast_gap - baseline_gap AS delta
            FROM gap_nowcasts WHERE as_of <= $1 - make_interval(days => $2::int)
            ORDER BY geo_unit_id, as_of DESC
        ), cand AS (
            SELECT l.geo_unit_id, l.as_of, l.delta, COALESCE(p.delta, 0) AS prev_delta,
                   l.nowcast_gap, l.baseline_gap, l.news_decomposition,
                   CASE WHEN l.delta > 2 * $3 THEN 'critical' ELSE 'warning' END AS severity
            FROM latest l LEFT JOIN prior p USING (geo_unit_id)
            WHERE l.delta > $3 AND l.delta > COALESCE(p.delta, 0)
        )
        INSERT INTO alerts (alert_type, geo_unit_id, as_of, severity, details, alert_key)
        SELECT 'widening_gap', c.geo_unit_id, c.as_of, c.severity,
               jsonb_build_object(
                   'nowcast_gap', c.nowcast_gap, 'baseline_gap', c.baseline_gap,
                   'delta', c.delta, 'prev_delta', c.prev_delta,
                   'top_contribution', c.news_decomposition -> 0),
               'widening_gap:' || c.geo_unit_id || ':' || c.severity
        FROM cand c
        WHERE NOT EXISTS (
            SELECT 1 FROM alerts a
            WHERE a.alert_key = 'widening_gap:' || c.geo_unit_id || ':' || c.severity
              AND a.created_at > now() - make_interval(hours => $4::int))
        "#,
    )
    .bind(as_of)
    .bind(cfg.widening_lookback_days)
    .bind(cfg.widening_abs_threshold)
    .bind(cfg.cooldown_hours)
    .execute(&db.pool)
    .await?
    .rows_affected();

    // 2) coverage_collapsed: global sentinel (coverage is global in v0).
    let row = sqlx::query(
        "SELECT coverage_score FROM gap_nowcasts WHERE as_of <= $1
         ORDER BY as_of DESC LIMIT 1",
    )
    .bind(as_of)
    .fetch_optional(&db.pool)
    .await?;
    if let Some(r) = row {
        let coverage: f64 = r.get("coverage_score");
        if coverage < cfg.coverage_floor {
            fired += sqlx::query(
                r#"INSERT INTO alerts (alert_type, geo_unit_id, as_of, severity, details, alert_key)
                   SELECT 'coverage_collapsed', '_global', $1, 'critical',
                          jsonb_build_object('coverage_score', $2::float8, 'floor', $3::float8),
                          'coverage_collapsed:_global:critical'
                   WHERE NOT EXISTS (
                       SELECT 1 FROM alerts a
                       WHERE a.alert_key = 'coverage_collapsed:_global:critical'
                         AND a.created_at > now() - make_interval(hours => $4::int))"#,
            )
            .bind(as_of)
            .bind(coverage)
            .bind(cfg.coverage_floor)
            .bind(cfg.cooldown_hours)
            .execute(&db.pool)
            .await?
            .rows_affected();
        }
    }

    // 3) gone_blind: high uncertainty + historically high baseline + no
    //    active signal touching the tract — "we may be blind here".
    fired += sqlx::query(
        r#"
        WITH latest AS (
            SELECT DISTINCT ON (geo_unit_id) geo_unit_id, as_of, baseline_gap,
                   uncertainty, news_decomposition
            FROM gap_nowcasts WHERE as_of <= $1
            ORDER BY geo_unit_id, as_of DESC
        ), p75 AS (
            SELECT percentile_cont(0.75) WITHIN GROUP (ORDER BY baseline_gap) AS v FROM latest
        ), cand AS (
            SELECT l.* FROM latest l, p75
            WHERE l.uncertainty > $2
              AND l.baseline_gap >= p75.v
              AND l.news_decomposition = '[]'::jsonb
        )
        INSERT INTO alerts (alert_type, geo_unit_id, as_of, severity, details, alert_key)
        SELECT 'gone_blind', c.geo_unit_id, c.as_of, 'critical',
               jsonb_build_object('uncertainty', c.uncertainty, 'baseline_gap', c.baseline_gap),
               'gone_blind:' || c.geo_unit_id || ':critical'
        FROM cand c
        WHERE NOT EXISTS (
            SELECT 1 FROM alerts a
            WHERE a.alert_key = 'gone_blind:' || c.geo_unit_id || ':critical'
              AND a.created_at > now() - make_interval(hours => $3::int))
        "#,
    )
    .bind(as_of)
    .bind(cfg.blind_uncertainty)
    .bind(cfg.cooldown_hours)
    .execute(&db.pool)
    .await?
    .rows_affected();

    if fired > 0 {
        tracing::info!(fired, "alerts persisted");
        dispatch_webhooks(db).await;
    }
    Ok(fired)
}

/// Fire-and-forget webhook POSTs for alerts created in the last minute.
async fn dispatch_webhooks(db: &Db) {
    let sev_rank = |s: &str| match s { "critical" => 2, "warning" => 1, _ => 0 };
    let Ok(endpoints) = sqlx::query("SELECT url, min_severity FROM webhook_endpoints WHERE enabled")
        .fetch_all(&db.pool)
        .await
    else {
        return;
    };
    if endpoints.is_empty() {
        return;
    }
    let Ok(fresh) = sqlx::query(
        "SELECT id::text, alert_type, geo_unit_id, severity, as_of, details::text
         FROM alerts WHERE created_at > now() - interval '1 minute'",
    )
    .fetch_all(&db.pool)
    .await
    else {
        return;
    };
    for ep in &endpoints {
        let url: String = ep.get("url");
        let min: String = ep.get("min_severity");
        for a in &fresh {
            let severity: String = a.get("severity");
            if sev_rank(&severity) < sev_rank(&min) {
                continue;
            }
            let body = serde_json::json!({
                "id": a.get::<String, _>("id"),
                "alert_type": a.get::<String, _>("alert_type"),
                "geo_unit_id": a.get::<String, _>("geo_unit_id"),
                "severity": severity,
                "as_of": a.get::<DateTime<Utc>, _>("as_of"),
                "details": serde_json::from_str::<serde_json::Value>(&a.get::<String, _>("details")).unwrap_or_default(),
            });
            let url = url.clone();
            tokio::spawn(async move {
                let client = reqwest::Client::new();
                if let Err(e) = client
                    .post(&url)
                    .json(&body)
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await
                {
                    tracing::warn!(%url, "webhook delivery failed: {e}");
                }
            });
        }
    }
}
