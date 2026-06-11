//! Model-agnostic extraction: prompt prefix assembly (byte-stable, cacheable)
//! and the schema-constrained extraction loop with one reject-and-reask.

use groundwork_types::SignalType;
use serde::{Deserialize, Serialize};

/// The stable, cache-friendly prompt prefix. Everything here is byte-stable
/// across documents within a deployment — the cache contract. Only the
/// document body varies per call.
#[derive(Debug, Clone)]
pub struct PromptPrefix {
    pub text: String,
    pub schema: serde_json::Value,
}

pub const SYSTEM_MD: &str = include_str!("../../../harness/prompt/system.md");
pub const SCHEMA_JSON: &str = include_str!("../../../harness/schema/signal_extraction.json");
const EXEMPLARS: [(&str, &str); 3] = [
    ("01_pantry_hours_cut", include_str!("../../../harness/prompt/exemplars/01_pantry_hours_cut.json")),
    ("02_no_signal", include_str!("../../../harness/prompt/exemplars/02_no_signal.json")),
    ("03_layoff_with_ambiguity", include_str!("../../../harness/prompt/exemplars/03_layoff_with_ambiguity.json")),
];

impl PromptPrefix {
    /// Render the full stable prefix. MUST be deterministic byte-for-byte:
    /// exemplars in fixed order, no timestamps, no map iteration.
    pub fn build() -> anyhow::Result<Self> {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA_JSON)?;
        let mut text = String::new();
        text.push_str(SYSTEM_MD);
        text.push_str("\n\nOutput JSON Schema (your output must validate against this):\n");
        text.push_str(SCHEMA_JSON);
        text.push_str("\n\nWorked examples:\n");
        for (name, raw) in EXEMPLARS {
            let ex: serde_json::Value = serde_json::from_str(raw)?;
            text.push_str(&format!(
                "\n--- Example: {name} ---\nDocument:\n{}\nCorrect output:\n{}\nWhy: {}\n",
                ex["document"].as_str().unwrap_or(""),
                serde_json::to_string(&ex["expected"])?,
                ex["why"].as_str().unwrap_or(""),
            ));
        }
        Ok(Self { text, schema })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedSignal {
    pub signal_type: SignalType,
    pub direction: i16,
    pub magnitude: f64,
    pub raw_excerpt: String,
    pub geo_text: String,
    #[serde(default)]
    pub observed_date_text: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedSignals {
    pub signals: Vec<ExtractedSignal>,
}

#[derive(Debug, Clone)]
pub struct ModelOutput {
    pub text: String,
    pub cache_read_input_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// The seam that keeps the harness model-agnostic: swap in a local OSS model
/// by implementing this trait.
#[async_trait::async_trait]
pub trait ExtractionModel: Send + Sync {
    async fn extract(&self, prefix: &PromptPrefix, document: &str) -> anyhow::Result<ModelOutput>;
    /// One retry turn after invalid output; `error` describes the failure.
    async fn reask(
        &self,
        prefix: &PromptPrefix,
        document: &str,
        bad_output: &str,
        error: &str,
    ) -> anyhow::Result<ModelOutput>;
    fn model_id(&self) -> String;
}

/// Run extraction with schema validation and one reject-and-reask.
/// Returns (validated signals, raw model output text for the capture record).
pub async fn extract_validated(
    model: &dyn ExtractionModel,
    prefix: &PromptPrefix,
    document: &str,
) -> anyhow::Result<(ExtractedSignals, String)> {
    let validator = jsonschema::validator_for(&prefix.schema)
        .map_err(|e| anyhow::anyhow!("schema compile: {e}"))?;
    let first = model.extract(prefix, document).await?;
    match validate(&validator, &first.text) {
        Ok(parsed) => Ok((parsed, first.text)),
        Err(err) => {
            tracing::warn!("extraction output invalid ({err}); reasking once");
            let second = model.reask(prefix, document, &first.text, &err).await?;
            let parsed = validate(&validator, &second.text)
                .map_err(|e| anyhow::anyhow!("invalid after reask: {e}"))?;
            Ok((parsed, second.text))
        }
    }
}

fn validate(validator: &jsonschema::Validator, text: &str) -> Result<ExtractedSignals, String> {
    // Tolerate accidental markdown fences without weakening the contract.
    let cleaned = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let value: serde_json::Value =
        serde_json::from_str(cleaned).map_err(|e| format!("not JSON: {e}"))?;
    if let Err(e) = validator.validate(&value) {
        return Err(format!("schema violation: {e}"));
    }
    serde_json::from_value(value).map_err(|e| format!("deserialize: {e}"))
}

/// Canned-output model for tests and offline development.
pub struct MockModel {
    pub outputs: std::sync::Mutex<Vec<String>>,
}

impl MockModel {
    pub fn new(outputs: Vec<&str>) -> Self {
        Self { outputs: std::sync::Mutex::new(outputs.into_iter().rev().map(String::from).collect()) }
    }
}

#[async_trait::async_trait]
impl ExtractionModel for MockModel {
    async fn extract(&self, _p: &PromptPrefix, _d: &str) -> anyhow::Result<ModelOutput> {
        let text = self.outputs.lock().unwrap().pop().ok_or_else(|| anyhow::anyhow!("mock exhausted"))?;
        Ok(ModelOutput { text, cache_read_input_tokens: 0, input_tokens: 0, output_tokens: 0 })
    }
    async fn reask(&self, p: &PromptPrefix, d: &str, _bad: &str, _err: &str) -> anyhow::Result<ModelOutput> {
        self.extract(p, d).await
    }
    fn model_id(&self) -> String {
        "mock".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cache contract: the prefix must render identically every time.
    #[test]
    fn prefix_is_byte_stable() {
        let a = PromptPrefix::build().unwrap();
        let b = PromptPrefix::build().unwrap();
        assert_eq!(a.text, b.text);
        assert!(a.text.len() > 4000, "prefix should be substantial enough to cache");
    }

    #[tokio::test]
    async fn valid_output_passes() {
        let model = MockModel::new(vec![
            r#"{"signals":[{"signal_type":"pantry_capacity","direction":-1,"magnitude":3,"raw_excerpt":"cut its distribution schedule","geo_text":"Mount Vernon","observed_date_text":null,"confidence":0.9}]}"#,
        ]);
        let prefix = PromptPrefix::build().unwrap();
        let (out, _) = extract_validated(&model, &prefix, "doc").await.unwrap();
        assert_eq!(out.signals.len(), 1);
    }

    #[tokio::test]
    async fn invalid_then_valid_uses_reask() {
        let model = MockModel::new(vec![
            r#"{"signals": "not an array"}"#,
            r#"{"signals":[]}"#,
        ]);
        let prefix = PromptPrefix::build().unwrap();
        let (out, _) = extract_validated(&model, &prefix, "doc").await.unwrap();
        assert!(out.signals.is_empty());
    }

    #[tokio::test]
    async fn invalid_twice_errors() {
        let model = MockModel::new(vec!["garbage", "more garbage"]);
        let prefix = PromptPrefix::build().unwrap();
        assert!(extract_validated(&model, &prefix, "doc").await.is_err());
    }
}
