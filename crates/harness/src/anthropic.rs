//! Anthropic implementation of the extraction model.
//!
//! Prefix-caching discipline: the entire stable prefix (system prompt +
//! schema + exemplars) is one system block with a `cache_control` breakpoint
//! at its end; only the document body rides in the user message. This is the
//! difference between an affordable always-on civic pipeline and an
//! expensive toy.

use crate::extract::{ExtractionModel, ModelOutput, PromptPrefix};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_MODEL: &str = "claude-haiku-4-5";

pub struct AnthropicModel {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl AnthropicModel {
    pub fn from_env() -> anyhow::Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set (required for local_news extraction)"))?;
        let model =
            std::env::var("GROUNDWORK_EXTRACT_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into());
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            api_key,
            model,
        })
    }

    async fn call(&self, prefix: &PromptPrefix, messages: serde_json::Value) -> anyhow::Result<ModelOutput> {
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 2048,
            "system": [{
                "type": "text",
                "text": prefix.text,
                "cache_control": {"type": "ephemeral"}
            }],
            "messages": messages,
        });
        let resp = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let v: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            anyhow::bail!("Anthropic API {status}: {}", v["error"]["message"].as_str().unwrap_or("?"));
        }
        let text = v["content"]
            .as_array()
            .and_then(|blocks| blocks.iter().find(|b| b["type"] == "text"))
            .and_then(|b| b["text"].as_str())
            .unwrap_or("")
            .to_string();
        Ok(ModelOutput {
            text,
            cache_read_input_tokens: v["usage"]["cache_read_input_tokens"].as_u64().unwrap_or(0),
            input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
        })
    }
}

#[async_trait::async_trait]
impl ExtractionModel for AnthropicModel {
    async fn extract(&self, prefix: &PromptPrefix, document: &str) -> anyhow::Result<ModelOutput> {
        let out = self
            .call(
                prefix,
                serde_json::json!([{"role": "user", "content": format!("Document:\n{document}")}]),
            )
            .await?;
        tracing::debug!(
            cache_read = out.cache_read_input_tokens,
            input = out.input_tokens,
            "extraction call complete"
        );
        Ok(out)
    }

    async fn reask(
        &self,
        prefix: &PromptPrefix,
        document: &str,
        bad_output: &str,
        error: &str,
    ) -> anyhow::Result<ModelOutput> {
        self.call(
            prefix,
            serde_json::json!([
                {"role": "user", "content": format!("Document:\n{document}")},
                {"role": "assistant", "content": bad_output},
                {"role": "user", "content": format!("Output did not validate: {error}. Re-emit valid JSON only, matching the schema exactly.")}
            ]),
        )
        .await
    }

    fn model_id(&self) -> String {
        self.model.clone()
    }
}

/// Live smoke test: needs ANTHROPIC_API_KEY; run with `cargo test -- --ignored`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::extract_validated;

    #[tokio::test]
    #[ignore]
    async fn live_extraction_and_cache() {
        let model = AnthropicModel::from_env().unwrap();
        let prefix = PromptPrefix::build().unwrap();
        let doc = "NEW ROCHELLE — The Community Action Pantry said Monday it will close its Saturday distribution entirely after losing a county grant, reducing service from four days a week to three. The pantry serves 250 households weekly.";
        let (out1, _) = extract_validated(&model, &prefix, doc).await.unwrap();
        assert!(!out1.signals.is_empty(), "expected a pantry_capacity signal");
        // Second call should hit the prompt cache.
        let second = model.extract(&prefix, doc).await.unwrap();
        assert!(second.cache_read_input_tokens > 0, "prefix cache not hit");
    }
}
