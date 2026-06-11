CREATE TABLE gap_nowcasts (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    geo_unit_id        TEXT NOT NULL REFERENCES geo_units(id),
    as_of              TIMESTAMPTZ NOT NULL,
    baseline_gap       DOUBLE PRECISION NOT NULL,
    nowcast_gap        DOUBLE PRECISION NOT NULL,
    uncertainty        DOUBLE PRECISION NOT NULL,
    coverage_score     DOUBLE PRECISION NOT NULL,
    news_decomposition JSONB NOT NULL DEFAULT '[]',
    model_version      TEXT NOT NULL,
    weights_version    TEXT NOT NULL,
    UNIQUE (geo_unit_id, as_of)
);

CREATE INDEX gap_nowcasts_asof_idx ON gap_nowcasts (as_of DESC);
