//! Partner-facing sample exports: the current nowcast and the signal
//! evidence chain for one county, as CSV + GeoJSON. CC-BY-4.0.

use sqlx::Row;
use store::Db;

pub async fn export_county(db: &Db, county_geoid: &str, out_dir: &str) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(out_dir).await?;
    let state_fips = &county_geoid[..2];
    let county_fips = &county_geoid[2..];

    // Nowcast CSV per tract.
    let rows = sqlx::query(
        r#"SELECT g.id, g.name, n.as_of, n.nowcast_gap, n.baseline_gap,
                  n.uncertainty, n.coverage_score
           FROM geo_units g
           JOIN LATERAL (SELECT * FROM gap_nowcasts n WHERE n.geo_unit_id = g.id
                         ORDER BY n.as_of DESC LIMIT 1) n ON TRUE
           WHERE g.kind = 'tract' AND g.state_fips = $1 AND g.county_fips = $2
           ORDER BY g.id"#,
    )
    .bind(state_fips)
    .bind(county_fips)
    .fetch_all(&db.pool)
    .await?;
    let mut csv = String::from(
        "geoid,tract_name,as_of,nowcast_gap,baseline_gap,delta,uncertainty,coverage_score\n",
    );
    for r in &rows {
        let nc: f64 = r.get("nowcast_gap");
        let bl: f64 = r.get("baseline_gap");
        csv.push_str(&format!(
            "{},{},{},{:.5},{:.5},{:.5},{:.5},{:.3}\n",
            r.get::<String, _>("id"),
            r.get::<String, _>("name").replace(',', ";"),
            r.get::<chrono::DateTime<chrono::Utc>, _>("as_of").to_rfc3339(),
            nc, bl, nc - bl,
            r.get::<f64, _>("uncertainty"),
            r.get::<f64, _>("coverage_score"),
        ));
    }
    tokio::fs::write(format!("{out_dir}/nowcast_{county_geoid}.csv"), csv).await?;

    // Signals CSV with full provenance.
    let sigs = sqlx::query(
        r#"SELECT s.id::text, s.source_id, s.signal_type, s.observed_at, s.magnitude,
                  s.direction, s.status, s.provenance_url, s.raw_excerpt
           FROM signals s
           WHERE s.geo_unit_id = $1 OR s.geo_unit_id = $2
           ORDER BY s.observed_at DESC"#,
    )
    .bind(county_geoid)
    .bind(state_fips) // state-level signals also touch this county
    .fetch_all(&db.pool)
    .await?;
    let mut scsv = String::from(
        "signal_id,source,type,observed_at,magnitude,direction,status,provenance_url,raw_excerpt\n",
    );
    for r in &sigs {
        scsv.push_str(&format!(
            "{},{},{},{},{},{},{},{},\"{}\"\n",
            r.get::<String, _>("id"),
            r.get::<String, _>("source_id"),
            r.get::<String, _>("signal_type"),
            r.get::<chrono::DateTime<chrono::Utc>, _>("observed_at").to_rfc3339(),
            r.get::<f64, _>("magnitude"),
            r.get::<i16, _>("direction"),
            r.get::<String, _>("status"),
            r.get::<String, _>("provenance_url"),
            r.get::<String, _>("raw_excerpt").replace('"', "'"),
        ));
    }
    tokio::fs::write(format!("{out_dir}/signals_{county_geoid}.csv"), scsv).await?;

    // GeoJSON nowcast for mapping tools.
    let geo = sqlx::query(
        r#"SELECT json_build_object(
               'type','FeatureCollection',
               'license','CC-BY-4.0 — Groundwork',
               'features', COALESCE(json_agg(json_build_object(
                   'type','Feature',
                   'geometry', ST_AsGeoJSON(g.geom)::json,
                   'properties', json_build_object(
                       'geoid', g.id, 'name', g.name,
                       'nowcast_gap', n.nowcast_gap, 'baseline_gap', n.baseline_gap,
                       'uncertainty', n.uncertainty, 'coverage_score', n.coverage_score))), '[]'::json)
           )::text AS fc
           FROM geo_units g
           JOIN LATERAL (SELECT * FROM gap_nowcasts n WHERE n.geo_unit_id = g.id
                         ORDER BY n.as_of DESC LIMIT 1) n ON TRUE
           WHERE g.kind = 'tract' AND g.state_fips = $1 AND g.county_fips = $2"#,
    )
    .bind(state_fips)
    .bind(county_fips)
    .fetch_one(&db.pool)
    .await?;
    tokio::fs::write(
        format!("{out_dir}/nowcast_{county_geoid}.geojson"),
        geo.get::<String, _>("fc"),
    )
    .await?;

    println!(
        "exported {} tracts + {} signals for county {county_geoid} to {out_dir}/",
        rows.len(),
        sigs.len()
    );
    Ok(())
}
