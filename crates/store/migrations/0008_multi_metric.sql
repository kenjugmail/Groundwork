-- Open the baseline metric vocabulary: Groundwork now carries multiple need
-- categories (food insecurity, poverty, unemployment, ...) at multiple
-- scales. Metric names are namespaced by source prefix (acs_, mmg_, chr_).
ALTER TABLE baselines DROP CONSTRAINT baselines_metric_check;

INSERT INTO sources (id, name, kind, cadence_seconds, license, enabled) VALUES
    ('chr', 'County Health Rankings & Roadmaps (UWPHI)', 'baseline', 31536000,
     'CC-BY 4.0 (countyhealthrankings.org)', TRUE)
ON CONFLICT (id) DO NOTHING;
