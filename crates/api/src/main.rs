//! Groundwork public read API. Three v0 endpoints plus the physically
//! separate slow-clock stub. The static map UI is served from /.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::Row;
use store::Db;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db: Db,
    actions: std::sync::Arc<serde_json::Value>,
    /// (fetched_at, payload) cache for the World Bank world baseline.
    world_cache: std::sync::Arc<tokio::sync::Mutex<Option<(std::time::Instant, serde_json::Value)>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt().init();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL not set (copy .env.example to .env)"))?;
    let db = Db::connect(&database_url).await?;
    db.migrate().await?;

    // Curated action links (donate / volunteer / get help) — a versioned,
    // PR-able register, like the fusion weights. See actions/README.md.
    let actions: serde_json::Value =
        serde_json::from_str(include_str!("../../../actions/actions.v1.json"))?;

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/nowcast", get(nowcast))
        .route("/v1/signals/:id", get(signal))
        .route("/v1/actions", get(actions_for))
        .route("/v1/world", get(world_baseline))
        .route("/v1/us", get(us_baseline))
        .route("/v1/alerts", get(alerts))
        .route("/v1/alerts.atom", get(alerts_atom))
        // Slow clock: separate path, separate cadence label, empty in v0.
        .route("/v1/impact", get(impact_stub))
        .route("/v1/impact/*rest", get(impact_stub))
        .nest_service("/", ServeDir::new("ui"))
        .layer(CorsLayer::permissive())
        // The US and world payloads are MBs of slow-clock data; gzip cuts
        // them ~5x and clients may cache them for an hour.
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(AppState {
            db,
            actions: std::sync::Arc::new(actions),
            world_cache: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        });

    let addr = std::env::var("API_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    println!("Groundwork API on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// Liveness + DB reachability, for hosting health checks.
async fn healthz(State(st): State<AppState>) -> Result<Response, ApiError> {
    sqlx::query("SELECT 1").execute(&st.db.pool).await?;
    Ok(Json(serde_json::json!({"status": "ok"})).into_response())
}

struct ApiError(anyhow::Error);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!("api error: {:#}", self.0);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": self.0.to_string()})))
            .into_response()
    }
}
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}

#[derive(Deserialize)]
struct NowcastQuery {
    /// minx,miny,maxx,maxy (WGS84). Omit for the full extent.
    bbox: Option<String>,
    as_of: Option<DateTime<Utc>>,
    /// "polygon" (default) or "centroid" — centroid keeps the full-extent
    /// payload small enough for thin clients like the demo UI.
    geometry: Option<String>,
}

/// GeoJSON FeatureCollection: latest nowcast at or before as_of, per tract.
async fn nowcast(
    State(st): State<AppState>,
    Query(q): Query<NowcastQuery>,
) -> Result<Response, ApiError> {
    let as_of = q.as_of.unwrap_or_else(Utc::now);
    let bbox: Option<[f64; 4]> = match &q.bbox {
        Some(s) => {
            let parts: Vec<f64> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            if parts.len() != 4 {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"error": "bbox must be minx,miny,maxx,maxy"})),
                )
                    .into_response());
            }
            Some([parts[0], parts[1], parts[2], parts[3]])
        }
        None => None,
    };

    let rows = sqlx::query(
        r#"SELECT g.id, g.name,
                  ST_AsGeoJSON(g.geom)::text AS geometry,
                  ST_AsGeoJSON(g.centroid)::text AS centroid,
                  n.nowcast_gap, n.baseline_gap, n.uncertainty, n.coverage_score,
                  n.news_decomposition::text AS news_decomposition,
                  n.as_of, n.model_version, n.weights_version
           FROM geo_units g
           JOIN LATERAL (
               SELECT * FROM gap_nowcasts n
               WHERE n.geo_unit_id = g.id AND n.as_of <= $1
               ORDER BY n.as_of DESC LIMIT 1
           ) n ON TRUE
           WHERE g.kind = 'tract'
             AND ($2::float8 IS NULL OR g.geom && ST_MakeEnvelope($2,$3,$4,$5,4326))"#,
    )
    .bind(as_of)
    .bind(bbox.map(|b| b[0]))
    .bind(bbox.map(|b| b[1]))
    .bind(bbox.map(|b| b[2]))
    .bind(bbox.map(|b| b[3]))
    .fetch_all(&st.db.pool)
    .await?;

    let centroid_only = q.geometry.as_deref() == Some("centroid");
    let features: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let centroid: serde_json::Value = r
                .get::<Option<String>, _>("centroid")
                .and_then(|c| serde_json::from_str(&c).ok())
                .unwrap_or(serde_json::Value::Null);
            let geometry: serde_json::Value = if centroid_only {
                centroid.clone()
            } else {
                serde_json::from_str(&r.get::<String, _>("geometry")).unwrap_or(serde_json::Value::Null)
            };
            let decomposition: serde_json::Value =
                serde_json::from_str(&r.get::<String, _>("news_decomposition"))
                    .unwrap_or(serde_json::Value::Array(vec![]));
            serde_json::json!({
                "type": "Feature",
                "geometry": geometry,
                "properties": {
                    "geo_unit_id": r.get::<String, _>("id"),
                    "name": r.get::<String, _>("name"),
                    "centroid": centroid,
                    "nowcast_gap": r.get::<f64, _>("nowcast_gap"),
                    "baseline_gap": r.get::<f64, _>("baseline_gap"),
                    "uncertainty": r.get::<f64, _>("uncertainty"),
                    "coverage_score": r.get::<f64, _>("coverage_score"),
                    "news_decomposition": decomposition,
                    "as_of": r.get::<DateTime<Utc>, _>("as_of"),
                    "model_version": r.get::<String, _>("model_version"),
                    "weights_version": r.get::<String, _>("weights_version"),
                }
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "type": "FeatureCollection",
        "features": features,
        "clock": "fast",
        "disclaimer": "Nowcast = signal, not proof. See WHAT_THIS_IS_NOT.md. Data CC-BY-4.0.",
    }))
    .into_response())
}

/// The "click any number down to its source" path.
async fn signal(
    State(st): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    match st.db.signal(id).await? {
        Some(s) => Ok(Json(serde_json::to_value(&s)?).into_response()),
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "signal not found"})),
        )
            .into_response()),
    }
}

/// US-wide slow-baseline layer: county-level multi-category baselines from
/// County Health Rankings (food insecurity, child poverty, unemployment,
/// uninsured, median household income). SLOW clock — annual data with
/// provenance, never a nowcast. Tract-level nowcasting exists only inside
/// the sensing metro (NYC + Westchester).
async fn us_baseline(State(st): State<AppState>) -> Result<Response, ApiError> {
    let county_rows = sqlx::query(
        r#"SELECT g.id, g.name, g.state_fips,
                  ST_AsGeoJSON(g.geom, 5)::text AS geometry,
                  COALESCE((
                      SELECT jsonb_object_agg(b.metric, b.value)
                      FROM (
                          SELECT DISTINCT ON (metric) metric, value
                          FROM baselines
                          WHERE geo_unit_id = g.id AND metric LIKE 'chr\_%'
                          ORDER BY metric, year DESC
                      ) b
                  ), '{}'::jsonb)::text AS metrics
           FROM geo_units g
           WHERE g.kind = 'county' AND g.geom IS NOT NULL
             AND EXISTS (SELECT 1 FROM baselines b2
                         WHERE b2.geo_unit_id = g.id AND b2.metric LIKE 'chr\_%')"#,
    )
    .fetch_all(&st.db.pool)
    .await?;

    let state_rows = sqlx::query(
        r#"SELECT g.id, g.name,
                  COALESCE((
                      SELECT jsonb_object_agg(b.metric, b.value)
                      FROM (
                          SELECT DISTINCT ON (metric) metric, value
                          FROM baselines
                          WHERE geo_unit_id = g.id AND metric LIKE 'chr\_%'
                          ORDER BY metric, year DESC
                      ) b
                  ), '{}'::jsonb)::text AS metrics
           FROM geo_units g
           WHERE g.kind = 'state'
             AND EXISTS (SELECT 1 FROM baselines b2
                         WHERE b2.geo_unit_id = g.id AND b2.metric LIKE 'chr\_%')"#,
    )
    .fetch_all(&st.db.pool)
    .await?;

    let features: Vec<serde_json::Value> = county_rows
        .iter()
        .map(|r| {
            let metrics: serde_json::Value =
                serde_json::from_str(&r.get::<String, _>("metrics")).unwrap_or_default();
            serde_json::json!({
                "type": "Feature",
                "geometry": serde_json::from_str::<serde_json::Value>(&r.get::<String, _>("geometry")).unwrap_or(serde_json::Value::Null),
                "properties": {
                    "geo_unit_id": r.get::<String, _>("id"),
                    "name": r.get::<String, _>("name"),
                    "state_fips": r.get::<String, _>("state_fips"),
                    "metrics": metrics,
                }
            })
        })
        .collect();
    let states: Vec<serde_json::Value> = state_rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "geo_unit_id": r.get::<String, _>("id"),
                "name": r.get::<String, _>("name"),
                "metrics": serde_json::from_str::<serde_json::Value>(&r.get::<String, _>("metrics")).unwrap_or_default(),
            })
        })
        .collect();

    Ok((
        [(axum::http::header::CACHE_CONTROL, "public, max-age=3600")],
        Json(serde_json::json!({
        "type": "FeatureCollection",
        "clock": "slow",
        "source": "County Health Rankings & Roadmaps (University of Wisconsin Population Health Institute), 2025",
        "provenance_url": "https://www.countyhealthrankings.org/health-data",
        "note": "Annual county/state baselines across need categories. Not a nowcast — fast-clock sensing exists only where Groundwork has live local sources.",
        "states": states,
        "features": features,
        })),
    )
        .into_response())
}

/// Global slow-baseline layer: prevalence of undernourishment by country
/// (World Bank SN.ITK.DEFC.ZS, sourced from FAO). This is the SLOW clock at
/// world scale — an annual baseline with full provenance, never a nowcast.
/// Groundwork has no fast-clock sources outside its sensing metros, and it
/// does not pretend otherwise.
async fn world_baseline(State(st): State<AppState>) -> Result<Response, ApiError> {
    const WB_URL: &str = "https://api.worldbank.org/v2/country/all/indicator/SN.ITK.DEFC.ZS?format=json&date=2015:2024&per_page=4000";
    const TTL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

    let mut cache = st.world_cache.lock().await;
    if let Some((at, payload)) = cache.as_ref() {
        if at.elapsed() < TTL {
            return Ok((
                [(axum::http::header::CACHE_CONTROL, "public, max-age=3600")],
                Json(payload.clone()),
            )
                .into_response());
        }
    }

    let raw: serde_json::Value = reqwest::Client::new()
        .get(WB_URL)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let rows = raw
        .get(1)
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("World Bank response shape changed"))?;

    // Latest non-null observation per ISO3 code.
    let mut latest: std::collections::HashMap<String, (i64, f64, String)> =
        std::collections::HashMap::new();
    for r in rows {
        let (Some(iso3), Some(year), Some(value)) = (
            r.get("countryiso3code").and_then(|v| v.as_str()),
            r.get("date").and_then(|v| v.as_str()).and_then(|d| d.parse::<i64>().ok()),
            r.get("value").and_then(|v| v.as_f64()),
        ) else {
            continue;
        };
        if iso3.len() != 3 {
            continue;
        }
        let name = r
            .pointer("/country/value")
            .and_then(|v| v.as_str())
            .unwrap_or(iso3)
            .to_string();
        let e = latest.entry(iso3.to_string()).or_insert((year, value, name.clone()));
        if year > e.0 {
            *e = (year, value, name);
        }
    }

    let countries: Vec<serde_json::Value> = latest
        .into_iter()
        .map(|(iso3, (year, value, name))| {
            serde_json::json!({
                "iso3": iso3,
                "name": name,
                "year": year,
                "undernourishment_pct": value,
                "provenance_url": format!("https://data.worldbank.org/indicator/SN.ITK.DEFC.ZS?locations={iso3}"),
            })
        })
        .collect();

    let payload = serde_json::json!({
        "cache_control_hint": "public, max-age=3600",
        "clock": "slow",
        "metric": "prevalence_of_undernourishment_pct",
        "source": "World Bank (FAO), indicator SN.ITK.DEFC.ZS",
        "license": "CC-BY-4.0 (World Bank Open Data)",
        "note": "Annual country-level baseline. Includes World Bank regional aggregates (e.g. AFE); join against country geometries to drop them. No fast-clock signals exist at world scale in this deployment — this is context, not a nowcast.",
        "countries": countries,
    });
    *cache = Some((std::time::Instant::now(), payload.clone()));
    Ok((
        [(axum::http::header::CACHE_CONTROL, "public, max-age=3600")],
        Json(payload),
    )
        .into_response())
}

#[derive(Deserialize)]
struct ActionsQuery {
    /// Tract, county, or state GEOID. Omit for the full register.
    geo_unit_id: Option<String>,
}

/// Where to get help, donate, or volunteer for a given place. Links go
/// directly to independent organizations — no money flows through Groundwork,
/// and order is stable file order, never a ranking.
async fn actions_for(
    State(st): State<AppState>,
    Query(q): Query<ActionsQuery>,
) -> impl IntoResponse {
    // geo_unit_id: tract/county GEOID, "us" for the national view, or "world"
    // for the global view. Resource scope tokens: explicit county GEOIDs,
    // "US" (any US geography), "*" (everywhere).
    let county: Option<String> = q.geo_unit_id.as_ref().map(|g| {
        let g = g.to_lowercase();
        if g == "world" || g == "us" { g } else if g.len() >= 5 { g[..5].to_string() } else { g }
    });
    let all = st.actions.get("resources").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    let filtered: Vec<serde_json::Value> = match &county {
        None => all,
        Some(c) => all
            .into_iter()
            .filter(|r| {
                r.get("counties")
                    .and_then(|cs| cs.as_array())
                    .map(|cs| {
                        cs.iter().any(|x| match x.as_str() {
                            Some("*") => true,
                            Some("US") => c != "world", // any US geography incl. "us"
                            Some(other) => c != "world" && c != "us" && other == c,
                            None => false,
                        })
                    })
                    .unwrap_or(false)
            })
            .collect(),
    };
    Json(serde_json::json!({
        "version": st.actions.get("version"),
        "disclaimer": st.actions.get("disclaimer"),
        "geo_unit_id": q.geo_unit_id,
        "resources": filtered,
    }))
}

#[derive(Deserialize)]
struct AlertsQuery {
    since: Option<DateTime<Utc>>,
    severity: Option<String>,
    #[serde(rename = "type")]
    alert_type: Option<String>,
    limit: Option<i64>,
}

async fn fetch_alerts(
    db: &Db,
    q: &AlertsQuery,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        r#"SELECT a.id::text, a.alert_type, a.geo_unit_id, a.severity,
                  a.as_of, a.created_at, a.details::text AS details,
                  COALESCE(g.name, a.geo_unit_id) AS geo_name
           FROM alerts a LEFT JOIN geo_units g ON g.id = a.geo_unit_id
           WHERE ($1::timestamptz IS NULL OR a.created_at > $1)
             AND ($2::text IS NULL OR a.severity = $2)
             AND ($3::text IS NULL OR a.alert_type = $3)
           ORDER BY a.created_at DESC
           LIMIT $4"#,
    )
    .bind(q.since)
    .bind(&q.severity)
    .bind(&q.alert_type)
    .bind(q.limit.unwrap_or(100).clamp(1, 1000))
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.get::<String, _>("id"),
                "alert_type": r.get::<String, _>("alert_type"),
                "geo_unit_id": r.get::<String, _>("geo_unit_id"),
                "geo_name": r.get::<String, _>("geo_name"),
                "severity": r.get::<String, _>("severity"),
                "as_of": r.get::<DateTime<Utc>, _>("as_of"),
                "created_at": r.get::<DateTime<Utc>, _>("created_at"),
                "details": serde_json::from_str::<serde_json::Value>(&r.get::<String, _>("details")).unwrap_or_default(),
            })
        })
        .collect())
}

fn alert_sentence(a: &serde_json::Value) -> String {
    let name = a["geo_name"].as_str().unwrap_or("unknown area");
    match a["alert_type"].as_str().unwrap_or("") {
        "widening_gap" => {
            let d = &a["details"];
            let because = d["top_contribution"]["signal_type"]
                .as_str()
                .map(|t| format!(" — leading contributor: {t} signal"))
                .unwrap_or_default();
            format!(
                "Food-insecurity gap in {name} is {:.1} points above baseline and widening{because}.",
                d["delta"].as_f64().unwrap_or(0.0) * 100.0
            )
        }
        "coverage_collapsed" => format!(
            "Sensing coverage collapsed to {:.0}% — the map may be blind, not the need gone.",
            a["details"]["coverage_score"].as_f64().unwrap_or(0.0) * 100.0
        ),
        "gone_blind" => format!(
            "{name}: high baseline need, high uncertainty, and no live signals — we may be blind here."
        ),
        _ => format!("Alert in {name}"),
    }
}

async fn alerts(
    State(st): State<AppState>,
    Query(q): Query<AlertsQuery>,
) -> Result<Response, ApiError> {
    let alerts = fetch_alerts(&st.db, &q).await?;
    Ok(Json(serde_json::json!({
        "alerts": alerts,
        "alert_types": ["widening_gap", "coverage_collapsed", "gone_blind"],
        "subscribe": { "atom": "/v1/alerts.atom", "webhooks": "see README (webhook_endpoints table)" },
    }))
    .into_response())
}

/// Atom 1.0 feed: subscribable from any feed reader, zero infrastructure.
async fn alerts_atom(State(st): State<AppState>) -> Result<Response, ApiError> {
    let q = AlertsQuery { since: None, severity: None, alert_type: None, limit: Some(50) };
    let alerts = fetch_alerts(&st.db, &q).await?;
    let updated = alerts
        .first()
        .and_then(|a| a["created_at"].as_str().map(String::from))
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let entries: String = alerts
        .iter()
        .map(|a| {
            format!(
                r#"  <entry>
    <id>urn:uuid:{id}</id>
    <title>{sev}: {ty} — {name}</title>
    <updated>{ts}</updated>
    <content type="text">{sentence}</content>
    <link href="/v1/alerts"/>
  </entry>
"#,
                id = a["id"].as_str().unwrap_or(""),
                sev = a["severity"].as_str().unwrap_or(""),
                ty = a["alert_type"].as_str().unwrap_or(""),
                name = esc(a["geo_name"].as_str().unwrap_or("")),
                ts = a["created_at"].as_str().unwrap_or(""),
                sentence = esc(&alert_sentence(a)),
            )
        })
        .collect();
    let feed = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Groundwork alerts — widening gaps and blind spots</title>
  <id>urn:groundwork:alerts</id>
  <updated>{updated}</updated>
  <subtitle>A nowcast is a signal, not proof. Data CC-BY-4.0.</subtitle>
{entries}</feed>
"#
    );
    Ok(([(axum::http::header::CONTENT_TYPE, "application/atom+xml")], feed).into_response())
}

async fn impact_stub() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "clock": "slow",
            "status": "schema_only_v0",
            "note": "ImpactRecords are verified outcomes over months — deliberately separate from the fast-clock nowcast. No records exist in v0.",
        })),
    )
}
