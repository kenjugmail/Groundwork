CREATE TABLE baselines (
    geo_unit_id TEXT NOT NULL REFERENCES geo_units(id),
    metric      TEXT NOT NULL CHECK (metric IN
                ('mmg_food_insecurity_rate','acs_poverty_rate','acs_snap_rate')),
    year        INT NOT NULL,
    value       DOUBLE PRECISION NOT NULL,
    source_id   TEXT NOT NULL REFERENCES sources(id),
    PRIMARY KEY (geo_unit_id, metric, year)
);
