//! API fetchers pull transactions and write them into `in_dir` as raw export
//! files — byte-for-byte drop-ins for a manual export. Everything downstream
//! (profiles, rules, import, review) behaves identically for both paths.
//!
//! Network code lives only here; the file-drop core stays fully offline.

pub mod keyring;
pub mod monzo;
pub mod state;
pub mod wise;

use std::path::Path;

use anyhow::{bail, Result};

use crate::config::{Config, FetcherConfig};

/// A fetched raw export, ready to be dropped into `in_dir`.
pub struct RawDrop {
    pub file_name: String,
    pub contents: Vec<u8>,
}

pub fn store_drops(in_dir: &Path, drops: &[RawDrop]) -> Result<usize> {
    std::fs::create_dir_all(in_dir)?;
    for d in drops {
        std::fs::write(in_dir.join(&d.file_name), &d.contents)?;
    }
    Ok(drops.len())
}

pub fn run(config: &Config, account: Option<&str>, since: Option<chrono::NaiveDate>) -> Result<()> {
    if config.fetchers.is_empty() {
        bail!(
            "no [[fetchers]] configured — see the template `bank2hledger init` wrote, \
             and docs/fetchers.md"
        );
    }
    let mut ran = false;
    for f in &config.fetchers {
        if let Some(filter) = account {
            if f.account() != filter {
                continue;
            }
        }
        ran = true;
        match f {
            FetcherConfig::Monzo(c) => monzo::fetch(config, c, since)?,
            FetcherConfig::Wise(c) => wise::fetch(config, c, since)?,
        }
    }
    if let Some(filter) = account {
        if !ran {
            bail!("no fetcher configured for account '{}'", filter);
        }
    }
    Ok(())
}
