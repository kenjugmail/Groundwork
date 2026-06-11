//! Append-only raw capture store: every fetch is recorded (bytes + headers +
//! timestamp) BEFORE any parsing. The record half of record/replay.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureMeta {
    pub capture_id: String,
    pub source_id: String,
    pub url: String,
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub fetched_at: DateTime<Utc>,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub meta: CaptureMeta,
    pub bytes: Vec<u8>,
}

#[async_trait::async_trait]
pub trait RawDocStore: Send + Sync {
    async fn put(
        &self,
        source_id: &str,
        url: &str,
        status: u16,
        headers: BTreeMap<String, String>,
        bytes: Vec<u8>,
    ) -> anyhow::Result<Capture>;
    async fn get(&self, capture_id: &str) -> anyhow::Result<Capture>;
    async fn list(&self, source_id: &str) -> anyhow::Result<Vec<CaptureMeta>>;
}

/// Filesystem implementation. Layout (never overwritten):
/// `<root>/<source_id>/<YYYY>/<MM>/<DD>/<rfc3339-compact>_<sha256[..8]>.bin` + `.meta.json`
pub struct FsRawStore {
    root: PathBuf,
}

impl FsRawStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self { root: root.as_ref().to_path_buf() }
    }

    fn path_for(&self, meta: &CaptureMeta) -> PathBuf {
        let t = meta.fetched_at;
        self.root
            .join(&meta.source_id)
            .join(t.format("%Y").to_string())
            .join(t.format("%m").to_string())
            .join(t.format("%d").to_string())
            .join(format!(
                "{}_{}",
                t.format("%Y%m%dT%H%M%S%3fZ"),
                &meta.sha256[..8]
            ))
    }
}

#[async_trait::async_trait]
impl RawDocStore for FsRawStore {
    async fn put(
        &self,
        source_id: &str,
        url: &str,
        status: u16,
        headers: BTreeMap<String, String>,
        bytes: Vec<u8>,
    ) -> anyhow::Result<Capture> {
        let sha256 = hex::encode(Sha256::digest(&bytes));
        let fetched_at = Utc::now();
        let mut meta = CaptureMeta {
            capture_id: String::new(),
            source_id: source_id.to_string(),
            url: url.to_string(),
            status,
            headers,
            fetched_at,
            sha256,
        };
        let base = self.path_for(&meta);
        meta.capture_id = base
            .strip_prefix(&self.root)
            .unwrap_or(&base)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(parent) = base.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(base.with_extension("bin"), &bytes).await?;
        tokio::fs::write(
            base.with_extension("meta.json"),
            serde_json::to_vec_pretty(&meta)?,
        )
        .await?;
        Ok(Capture { meta, bytes })
    }

    async fn get(&self, capture_id: &str) -> anyhow::Result<Capture> {
        let base = self.root.join(capture_id);
        let bytes = tokio::fs::read(base.with_extension("bin")).await?;
        let meta: CaptureMeta =
            serde_json::from_slice(&tokio::fs::read(base.with_extension("meta.json")).await?)?;
        Ok(Capture { meta, bytes })
    }

    async fn list(&self, source_id: &str) -> anyhow::Result<Vec<CaptureMeta>> {
        let mut metas = Vec::new();
        let dir = self.root.join(source_id);
        if !dir.exists() {
            return Ok(metas);
        }
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            let mut rd = tokio::fs::read_dir(&d).await?;
            while let Some(entry) = rd.next_entry().await? {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.to_string_lossy().ends_with(".meta.json") {
                    let meta: CaptureMeta = serde_json::from_slice(&tokio::fs::read(&p).await?)?;
                    metas.push(meta);
                }
            }
        }
        metas.sort_by_key(|m| m.fetched_at);
        Ok(metas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_then_get_roundtrips() {
        let dir = std::env::temp_dir().join(format!("gw_raw_{}", uuid::Uuid::new_v4()));
        let store = FsRawStore::new(&dir);
        let cap = store
            .put("warn_ny", "https://example.org", 200, BTreeMap::new(), b"hello".to_vec())
            .await
            .unwrap();
        let got = store.get(&cap.meta.capture_id).await.unwrap();
        assert_eq!(got.bytes, b"hello");
        assert_eq!(got.meta.url, "https://example.org");
        let listed = store.list("warn_ny").await.unwrap();
        assert_eq!(listed.len(), 1);
        tokio::fs::remove_dir_all(&dir).await.ok();
    }
}
