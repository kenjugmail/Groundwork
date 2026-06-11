//! Fetcher implementations: the seam that makes every network read recorded
//! (live) or served from history (replay). Adapters cannot bypass it.

use adapters::Fetcher;
use std::collections::BTreeMap;
use std::path::PathBuf;
use store::raw_store::{Capture, RawDocStore};

/// Live fetcher: records raw bytes + headers to the append-only store
/// BEFORE returning them to the adapter for parsing.
pub struct RecordingFetcher<S: RawDocStore> {
    client: reqwest::Client,
    store: S,
}

impl<S: RawDocStore> RecordingFetcher<S> {
    pub fn new(store: S) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("groundwork/0.1 (+https://github.com/ephemerent/groundwork)")
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        Self { client, store }
    }
}

#[async_trait::async_trait]
impl<S: RawDocStore> Fetcher for RecordingFetcher<S> {
    async fn fetch(&self, source_id: &str, url: &str) -> anyhow::Result<Capture> {
        // fixture:// URLs let config-driven sources (e.g. Household Pulse,
        // until a stable wave URL is configured) run the full pipeline from
        // committed samples.
        if let Some(name) = url.strip_prefix("fixture://") {
            let path = PathBuf::from("fixtures").join(name);
            let bytes = tokio::fs::read(&path).await?;
            return self.store.put(source_id, url, 200, BTreeMap::new(), bytes).await;
        }
        let resp = self.client.get(url).send().await?;
        let status = resp.status().as_u16();
        let headers: BTreeMap<String, String> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<binary>").to_string()))
            .collect();
        let bytes = resp.bytes().await?.to_vec();
        // Record even error responses: they are evidence of drift.
        let capture = self.store.put(source_id, url, status, headers, bytes).await?;
        if status >= 400 {
            anyhow::bail!("fetch {url} returned HTTP {status} (capture {})", capture.meta.capture_id);
        }
        Ok(capture)
    }
}

/// Replay fetcher: serves the most recent recorded capture for the source.
/// (The replay binary drives adapters directly; this exists for future
/// in-orchestrator replays.)
#[allow(dead_code)]
pub struct ReplayFetcher<S: RawDocStore> {
    store: S,
}

impl<S: RawDocStore> ReplayFetcher<S> {
    #[allow(dead_code)]
    pub fn new(store: S) -> Self {
        Self { store }
    }
}

#[async_trait::async_trait]
impl<S: RawDocStore> Fetcher for ReplayFetcher<S> {
    async fn fetch(&self, source_id: &str, _url: &str) -> anyhow::Result<Capture> {
        let metas = self.store.list(source_id).await?;
        let latest = metas
            .last()
            .ok_or_else(|| anyhow::anyhow!("no captures recorded for source {source_id}"))?;
        self.store.get(&latest.capture_id).await
    }
}
