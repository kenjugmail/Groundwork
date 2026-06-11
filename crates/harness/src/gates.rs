//! Drift gates for the agentic path. The classic DriftGate trait can't see
//! extraction drop counts, so these run over per-batch ExtractionStats.
//! Any failure quarantines the batch — model drift, prompt rot, or feed
//! format changes must lower coverage, never corrupt the nowcast.

#[derive(Debug, Default, Clone)]
pub struct ExtractionStats {
    pub docs_processed: usize,
    pub docs_schema_failed: usize,
    pub signals_extracted: usize,
    pub signals_dropped_no_span: usize,
    pub signals_dropped_no_geo: usize,
    pub signals_kept: usize,
}

#[derive(Debug, PartialEq)]
pub enum AgenticGateResult {
    Pass,
    Fail(String),
}

pub fn check(stats: &ExtractionStats) -> AgenticGateResult {
    use AgenticGateResult::*;
    if stats.docs_processed == 0 {
        return Pass; // nothing to judge
    }
    let schema_rate = stats.docs_schema_failed as f64 / stats.docs_processed as f64;
    if schema_rate > 0.2 {
        return Fail(format!(
            "schema_failure_rate {:.0}% > 20% — model or prompt drift",
            schema_rate * 100.0
        ));
    }
    if stats.signals_extracted > 0 {
        let halluc = stats.signals_dropped_no_span as f64 / stats.signals_extracted as f64;
        if halluc > 0.3 {
            return Fail(format!(
                "excerpt_not_found_rate {:.0}% > 30% — hallucination spike",
                halluc * 100.0
            ));
        }
        let after_span = stats.signals_extracted - stats.signals_dropped_no_span;
        if after_span > 0 {
            let geo_fail = stats.signals_dropped_no_geo as f64 / after_span as f64;
            if geo_fail > 0.5 {
                return Fail(format!(
                    "geo_resolution_rate {:.0}% < 50% — place vocabulary drift",
                    (1.0 - geo_fail) * 100.0
                ));
            }
        }
    }
    let per_doc = stats.signals_extracted as f64 / stats.docs_processed as f64;
    if per_doc > 5.0 {
        return Fail(format!(
            "signals_per_doc {per_doc:.1} > 5 — over-extraction (prompt drift?)"
        ));
    }
    Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_batch_passes() {
        let s = ExtractionStats {
            docs_processed: 10, signals_extracted: 4, signals_kept: 4, ..Default::default()
        };
        assert_eq!(check(&s), AgenticGateResult::Pass);
    }

    #[test]
    fn hallucination_spike_quarantines() {
        let s = ExtractionStats {
            docs_processed: 10, signals_extracted: 10, signals_dropped_no_span: 4,
            ..Default::default()
        };
        assert!(matches!(check(&s), AgenticGateResult::Fail(r) if r.contains("hallucination")));
    }

    #[test]
    fn over_extraction_quarantines() {
        let s = ExtractionStats {
            docs_processed: 2, signals_extracted: 14, ..Default::default()
        };
        assert!(matches!(check(&s), AgenticGateResult::Fail(r) if r.contains("over-extraction")));
    }
}
