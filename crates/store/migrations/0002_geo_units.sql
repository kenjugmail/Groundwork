CREATE TABLE geo_units (
    id              TEXT PRIMARY KEY,          -- GEOID
    kind            TEXT NOT NULL CHECK (kind IN ('tract','county','place','state')),
    name            TEXT NOT NULL,
    state_fips      TEXT NOT NULL,
    county_fips     TEXT,                      -- NULL for state-level units
    geom            geometry(MultiPolygon, 4326),
    centroid        geometry(Point, 4326),
    baseline_population DOUBLE PRECISION,
    baseline_poverty_rate DOUBLE PRECISION
);

CREATE INDEX geo_units_geom_gist ON geo_units USING GIST (geom);
CREATE INDEX geo_units_kind_idx ON geo_units (kind);
CREATE INDEX geo_units_county_idx ON geo_units (state_fips, county_fips);
