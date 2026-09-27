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
        crate::fs_guard::write_refusing_symlinks(
            &path,
            serde_json::to_string_pretty(&self.values)?.as_bytes(),
        )?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_state_file_loads_as_default() {
        let dir = tempfile::tempdir().unwrap();
        let st = FetcherState::load(dir.path(), "nope").unwrap();
        assert_eq!(st.last_fetch(), None);
        assert_eq!(st.get_str("anything"), None);
    }

    #[test]
    fn save_load_round_trip_preserves_all_value_types() {
        let dir = tempfile::tempdir().unwrap();
        let mut st = FetcherState::default();
        st.set(
            "monzo_account_id",
            serde_json::Value::String("acc_1".into()),
        );
        st.set("profile_id", serde_json::Value::Number(42.into()));
        st.set_last_fetch(Utc::now());
        st.save(dir.path(), "k").unwrap();

        let loaded = FetcherState::load(dir.path(), "k").unwrap();
        assert_eq!(loaded.get_str("monzo_account_id").as_deref(), Some("acc_1"));
        assert_eq!(loaded.get_u64("profile_id"), Some(42));
        assert!(loaded.last_fetch().is_some());
        // Timestamps must round-trip exactly (gap-free fetches depend on it).
        assert_eq!(loaded.last_fetch(), st.last_fetch());
    }

    #[test]
    fn keys_are_namespaced_per_fetcher() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = FetcherState::default();
        a.set("k", serde_json::Value::String("a".into()));
        a.save(dir.path(), "monzo:acct1").unwrap();
        let b = FetcherState::load(dir.path(), "monzo:acct2").unwrap();
        assert_eq!(b.get_str("k"), None);
    }

    #[test]
    fn corrupt_state_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("k.json"), "{not json").unwrap();
        assert!(FetcherState::load(dir.path(), "k").is_err());
    }
}
