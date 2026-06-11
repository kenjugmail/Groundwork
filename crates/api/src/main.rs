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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt().init();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL not set (copy .env.example to .env)"))?;
    let db = Db::connect(&database_url).await?;
    db.migrate().await?;

    let app = Router::new()
        .route("/v1/nowcast", get(nowcast))
        .route("/v1/signals/:id", get(signal))
        .route("/v1/alerts", get(alerts))
        // Slow clock: separate path, separate cadence label, empty in v0.
        .route("/v1/impact", get(impact_stub))
        .route("/v1/impact/*rest", get(impact_stub))
        .nest_service("/", ServeDir::new("ui"))
        .layer(CorsLayer::permissive())
        .with_state(AppState { db });

    let addr = std::env::var("API_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into());
    println!("Groundwork API on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
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

#[derive(Deserialize)]
struct AlertsQuery {
    #[allow(dead_code)]
    since: Option<DateTime<Utc>>,
}

async fn alerts(Query(_q): Query<AlertsQuery>) -> impl IntoResponse {
    Json(serde_json::json!({
        "alerts": [],
        "status": "not_implemented_v0",
        "planned_alert_types": ["widening_gap", "coverage_collapsed"],
    }))
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
