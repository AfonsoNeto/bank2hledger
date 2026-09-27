//! Per-fetcher persisted state (last successful fetch, chosen account /
//! profile ids), stored as JSON next to the staging dir so it stays inside
//! the user's private finance directory.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

#[derive(Debug, Default)]
pub struct FetcherState {
    values: BTreeMap<String, serde_json::Value>,
}

impl FetcherState {
    pub fn load(dir: &Path, key: &str) -> Result<FetcherState> {
        let path = dir.join(format!("{key}.json"));
        if !path.exists() {
            return Ok(FetcherState::default());
        }
        let raw = std::fs::read_to_string(&path)?;
        let values =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        Ok(FetcherState { values })
    }

    pub fn save(&self, dir: &Path, key: &str) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{key}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&self.values)?)?;
        Ok(())
    }

    pub fn get_str(&self, k: &str) -> Option<String> {
        self.values
            .get(k)
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    pub fn get_u64(&self, k: &str) -> Option<u64> {
        self.values.get(k).and_then(|v| v.as_u64())
    }

    pub fn set(&mut self, k: &str, v: serde_json::Value) {
        self.values.insert(k.to_string(), v);
    }

    pub fn last_fetch(&self) -> Option<DateTime<Utc>> {
        self.values
            .get("last_fetch")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
    }

    pub fn set_last_fetch(&mut self, t: DateTime<Utc>) {
        self.values.insert(
            "last_fetch".into(),
            serde_json::Value::String(t.to_rfc3339()),
        );
    }
}
