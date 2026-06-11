-- Slow clock. Schema only in v0: no write path exists yet, by design.
-- Physically separate from the fast clock from day one.
CREATE TABLE impact_records (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    geo_unit_id      TEXT NOT NULL REFERENCES geo_units(id),
    intervention_ref TEXT NOT NULL,
    measured_at      TIMESTAMPTZ NOT NULL,
    outcome_metric   TEXT NOT NULL,
    value            DOUBLE PRECISION NOT NULL,
    method           TEXT NOT NULL,
    confidence       DOUBLE PRECISION NOT NULL
);
