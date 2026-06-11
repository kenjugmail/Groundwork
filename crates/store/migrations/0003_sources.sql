CREATE TABLE sources (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('structured','survey','baseline','news')),
    cadence_seconds BIGINT NOT NULL,
    license         TEXT NOT NULL,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    last_ok_ingest  TIMESTAMPTZ,
    last_quarantine TIMESTAMPTZ
);

INSERT INTO sources (id, name, kind, cadence_seconds, license, enabled) VALUES
    ('warn_ny',        'NY DOL WARN Act notices',               'structured', 86400,   'public record',   TRUE),
    ('acs',            'Census ACS 5-year (poverty, SNAP)',     'baseline',   31536000,'public domain',   TRUE),
    ('meal_gap',       'Feeding America Map the Meal Gap',      'baseline',   31536000,'restricted',      TRUE),
    ('household_pulse','Census Household Pulse (food suff.)',   'survey',     1209600, 'public domain',   TRUE),
    ('socrata_snap',   'NYC Open Data SNAP recipients (HRA)',   'structured', 2592000, 'public domain',   TRUE),
    ('two11',          '211 Counts food-pantry requests',       'structured', 86400,   'data-sharing agreement', FALSE);
