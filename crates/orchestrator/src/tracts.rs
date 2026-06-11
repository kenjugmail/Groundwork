//! Load census tract geometries from TIGER/Line shapefiles into geo_units.
//! Pure-Rust shapefile reading — no GDAL dependency on Windows.

use geo_types::{LineString, MultiPolygon, Polygon};
use std::io::Read;
use std::path::Path;
use store::Db;

const TIGER_URL: &str = "https://www2.census.gov/geo/tiger/TIGER2024/TRACT/tl_2024_36_tract.zip";
const US_COUNTY_URL: &str = "https://www2.census.gov/geo/tiger/GENZ2023/shp/cb_2023_us_county_20m.zip";
const SCOPE_COUNTIES: [&str; 6] = ["005", "047", "061", "081", "085", "119"];

pub async fn load_tracts(db: &Db, zip_path_override: Option<&str>) -> anyhow::Result<usize> {
    let zip_path = match zip_path_override {
        Some(p) => p.to_string(),
        None => {
            let dest = "data/tl_2024_36_tract.zip";
            if !Path::new(dest).exists() {
                tracing::info!("downloading TIGER/Line tracts for NY ({TIGER_URL})");
                tokio::fs::create_dir_all("data").await?;
                let bytes = reqwest::get(TIGER_URL).await?.error_for_status()?.bytes().await?;
                tokio::fs::write(dest, &bytes).await?;
            }
            dest.to_string()
        }
    };

    let shp = extract_shp(&zip_path, "groundwork_tiger")?;
    let mut reader = shapefile::Reader::from_path(&shp)?;
    let mut count = 0usize;

    // Register the state and ALL NY counties as geo units before tracts:
    // adapters emit county/state GEOIDs as geo_unit_id (WARN covers the whole
    // state), so the rows must exist. Out-of-scope counties have no tracts,
    // so their signals never reach the nowcast — but they're stored with
    // provenance rather than dropped.
    db.upsert_geo_unit_wkt("36", "state", "New York", "36", None, None).await?;
    for (fips, name) in adapters::geo::NY_COUNTIES {
        db.upsert_geo_unit_wkt(
            &format!("36{fips}"), "county", &format!("{name} County, NY"), "36", Some(fips), None,
        )
        .await?;
    }

    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result?;
        let get_str = |k: &str| -> Option<String> {
            match record.get(k) {
                Some(shapefile::dbase::FieldValue::Character(Some(s))) => Some(s.trim().to_string()),
                _ => None,
            }
        };
        let Some(countyfp) = get_str("COUNTYFP") else { continue };
        if !SCOPE_COUNTIES.contains(&countyfp.as_str()) {
            continue;
        }
        let geoid = get_str("GEOID").ok_or_else(|| anyhow::anyhow!("tract missing GEOID"))?;
        let name = get_str("NAMELSAD").unwrap_or_else(|| geoid.clone());

        let mpoly: MultiPolygon<f64> = match shape {
            shapefile::Shape::Polygon(p) => shp_polygon_to_geo(p),
            _ => continue,
        };
        let wkt = multipolygon_wkt(&mpoly);
        db.upsert_geo_unit_wkt(&geoid, "tract", &name, "36", Some(&countyfp), Some(&wkt))
            .await?;
        count += 1;
    }
    Ok(count)
}

/// Extract the .shp/.dbf/.shx triple from a zip into a temp dir and return
/// the .shp path (the shapefile crate reads from paths).
fn extract_shp(zip_path: &str, tmp_name: &str) -> anyhow::Result<std::path::PathBuf> {
    let tmp = std::env::temp_dir().join(tmp_name);
    std::fs::create_dir_all(&tmp)?;
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.ends_with(".shp") || name.ends_with(".dbf") || name.ends_with(".shx") {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            std::fs::write(tmp.join(&name), &buf)?;
        }
    }
    std::fs::read_dir(&tmp)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().map(|x| x == "shp").unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("no .shp in {zip_path}"))
}

/// Load all ~3,100 US counties (cartographic 1:20m boundaries — small enough
/// to ship over the API) as geo_units, with their states.
pub async fn load_us_counties(db: &Db, zip_path_override: Option<&str>) -> anyhow::Result<usize> {
    let zip_path = match zip_path_override {
        Some(p) => p.to_string(),
        None => {
            let dest = "data/cb_2023_us_county_20m.zip";
            if !Path::new(dest).exists() {
                tracing::info!("downloading US county boundaries ({US_COUNTY_URL})");
                tokio::fs::create_dir_all("data").await?;
                let bytes = reqwest::get(US_COUNTY_URL).await?.error_for_status()?.bytes().await?;
                tokio::fs::write(dest, &bytes).await?;
            }
            dest.to_string()
        }
    };

    let shp = extract_shp(&zip_path, "groundwork_us_counties")?;
    let mut reader = shapefile::Reader::from_path(&shp)?;
    let mut count = 0usize;
    let mut states_seen = std::collections::HashSet::new();

    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result?;
        let get_str = |k: &str| -> Option<String> {
            match record.get(k) {
                Some(shapefile::dbase::FieldValue::Character(Some(s))) => Some(s.trim().to_string()),
                _ => None,
            }
        };
        let (Some(statefp), Some(countyfp), Some(geoid)) =
            (get_str("STATEFP"), get_str("COUNTYFP"), get_str("GEOID"))
        else {
            continue;
        };
        let name = get_str("NAMELSAD")
            .or_else(|| get_str("NAME"))
            .unwrap_or_else(|| geoid.clone());
        let state_usps = get_str("STUSPS").unwrap_or_default();

        if states_seen.insert(statefp.clone()) {
            db.upsert_geo_unit_wkt(&statefp, "state", &state_usps, &statefp, None, None).await?;
        }
        let mpoly: MultiPolygon<f64> = match shape {
            shapefile::Shape::Polygon(p) => shp_polygon_to_geo(p),
            _ => continue,
        };
        let wkt = multipolygon_wkt(&mpoly);
        db.upsert_geo_unit_wkt(
            &geoid,
            "county",
            &format!("{name}, {state_usps}"),
            &statefp,
            Some(&countyfp),
            Some(&wkt),
        )
        .await?;
        count += 1;
    }
    Ok(count)
}

/// Shapefile polygons list rings with outer rings clockwise; build one
/// geo polygon per outer ring, attaching following inner rings as holes.
fn shp_polygon_to_geo(p: shapefile::Polygon) -> MultiPolygon<f64> {
    let mut polys: Vec<Polygon<f64>> = Vec::new();
    for ring in p.rings() {
        let coords: Vec<(f64, f64)> = ring.points().iter().map(|pt| (pt.x, pt.y)).collect();
        let ls = LineString::from(coords);
        match ring {
            shapefile::PolygonRing::Outer(_) => polys.push(Polygon::new(ls, vec![])),
            shapefile::PolygonRing::Inner(_) => {
                if let Some(last) = polys.last_mut() {
                    last.interiors_push(ls);
                }
            }
        }
    }
    MultiPolygon(polys)
}

fn multipolygon_wkt(mp: &MultiPolygon<f64>) -> String {
    let polys: Vec<String> = mp
        .0
        .iter()
        .map(|poly| {
            let ring_to_str = |ls: &LineString<f64>| {
                let pts: Vec<String> =
                    ls.coords().map(|c| format!("{} {}", c.x, c.y)).collect();
                format!("({})", pts.join(","))
            };
            let mut rings = vec![ring_to_str(poly.exterior())];
            rings.extend(poly.interiors().iter().map(ring_to_str));
            format!("({})", rings.join(","))
        })
        .collect();
    format!("MULTIPOLYGON({})", polys.join(","))
}
