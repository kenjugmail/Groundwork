-- Agentic local-news source. Disabled until ANTHROPIC_API_KEY is configured;
-- a disabled source visibly counts against coverage, per commitment #4.
INSERT INTO sources (id, name, kind, cadence_seconds, license, enabled) VALUES
    ('local_news', 'Local news (agentic extraction)', 'news', 21600, 'fair use (excerpts + links)', FALSE)
ON CONFLICT (id) DO NOTHING;

-- URL-level dedupe so articles are extracted at most once.
CREATE TABLE seen_urls (
    url_sha256 TEXT PRIMARY KEY,
    source_id  TEXT NOT NULL,
    url        TEXT NOT NULL,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT now()
);
