//! Feed registry loader (harness/feeds.v1.toml — bundled at compile time,
//! overridable via GROUNDWORK_FEEDS_FILE for deployments).

use serde::Deserialize;

pub const FEEDS_V1: &str = include_str!("../../../harness/feeds.v1.toml");

#[derive(Debug, Clone, Deserialize)]
pub struct FeedsConfig {
    pub harness: HarnessConfig,
    #[serde(rename = "feed")]
    pub feeds: Vec<Feed>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarnessConfig {
    pub max_docs_per_day: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Feed {
    pub name: String,
    pub url: String,
    pub region: String,
}

impl FeedsConfig {
    pub fn load() -> anyhow::Result<Self> {
        let raw = match std::env::var("GROUNDWORK_FEEDS_FILE") {
            Ok(path) => std::fs::read_to_string(path)?,
            Err(_) => FEEDS_V1.to_string(),
        };
        Ok(toml::from_str(&raw)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_config_parses() {
        let cfg: FeedsConfig = toml::from_str(FEEDS_V1).unwrap();
        assert!(cfg.feeds.len() >= 3);
        assert!(cfg.harness.max_docs_per_day > 0);
    }
}
