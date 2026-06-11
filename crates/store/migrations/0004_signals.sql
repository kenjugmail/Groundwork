-- The atom of trust: immutable, provenance-mandatory, supersede-not-mutate.
CREATE TABLE signals (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_id        TEXT NOT NULL REFERENCES sources(id),
    geo_unit_id      TEXT NOT NULL REFERENCES geo_units(id),
    signal_type      TEXT NOT NULL,
    observed_at      TIMESTAMPTZ NOT NULL,
    ingested_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    magnitude        DOUBLE PRECISION NOT NULL,
    direction        SMALLINT NOT NULL CHECK (direction IN (-1, 1)),
    payload          JSONB NOT NULL DEFAULT '{}',
    provenance_url   TEXT NOT NULL,
    raw_excerpt      TEXT NOT NULL,
    raw_capture_id   TEXT,
    resolution_level TEXT NOT NULL CHECK (resolution_level IN ('tract','county','place','state')),
    status           TEXT NOT NULL DEFAULT 'active'
                     CHECK (status IN ('active','superseded','quarantined')),
    coverage_flag    TEXT,
    supersedes       UUID REFERENCES signals(id),
    dedupe_key       TEXT NOT NULL UNIQUE
);

CREATE INDEX signals_geo_idx ON signals (geo_unit_id, status);
CREATE INDEX signals_observed_idx ON signals (observed_at);
CREATE INDEX signals_source_idx ON signals (source_id);

-- Immutability: the only column app code may change is status
-- (active -> superseded when a correction lands, or quarantine resolution).
CREATE OR REPLACE FUNCTION signals_immutable_guard() RETURNS trigger AS $$
BEGIN
    IF (to_jsonb(NEW) - 'status' - 'coverage_flag') IS DISTINCT FROM
       (to_jsonb(OLD) - 'status' - 'coverage_flag') THEN
        RAISE EXCEPTION 'signals rows are immutable except for status/coverage_flag; insert a superseding signal instead';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER signals_immutable BEFORE UPDATE ON signals
    FOR EACH ROW EXECUTE FUNCTION signals_immutable_guard();

CREATE OR REPLACE FUNCTION signals_no_delete() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'signals rows are append-only and cannot be deleted';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER signals_append_only BEFORE DELETE ON signals
    FOR EACH ROW EXECUTE FUNCTION signals_no_delete();
