-- Alerts: persisted at nowcast-recompute time, read by the API, pushed to
-- webhooks. geo_unit_id has no FK so the '_global' sentinel can carry
-- system-wide alerts (e.g. coverage collapse).
CREATE TABLE alerts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    alert_type  TEXT NOT NULL CHECK (alert_type IN ('widening_gap','coverage_collapsed','gone_blind')),
    geo_unit_id TEXT NOT NULL,
    as_of       TIMESTAMPTZ NOT NULL,
    severity    TEXT NOT NULL CHECK (severity IN ('info','warning','critical')),
    details     JSONB NOT NULL DEFAULT '{}',
    alert_key   TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX alerts_created_idx ON alerts (created_at DESC);
CREATE INDEX alerts_key_idx ON alerts (alert_key, created_at DESC);

-- Subscriber webhooks (v1: rows managed by direct SQL; no admin API yet).
CREATE TABLE webhook_endpoints (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    url          TEXT NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT TRUE,
    min_severity TEXT NOT NULL DEFAULT 'warning' CHECK (min_severity IN ('info','warning','critical')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
