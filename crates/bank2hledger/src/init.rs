//! Workspace scaffolding shared by the CLI (`init`) and the GUI.
//!
//! The caller owns presentation: this module creates files and reports what
//! it did; the CLI prints the same messages it always has, the GUI renders
//! them as cards.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::{config, rules};

/// What `init` created, for callers to present.
#[derive(Debug)]
pub struct InitReport {
    pub config_path: PathBuf,
    /// Configured data directories (in, staging, rules) — only present when
    /// the written config parses.
    pub dirs: Vec<PathBuf>,
    /// Starter rules files ensured for the configured accounts (never
    /// overwritten if they exist).
    pub rules_files: Vec<PathBuf>,
    /// Next-step hint shown to the user.
    pub next_step: String,
}

/// Write a starter config (plus directories and rules files when it parses)
/// at `explicit`, or `./bank2hledger.toml` when `explicit` is `None`.
pub fn run(explicit: Option<&Path>, force: bool) -> Result<InitReport> {
    let path = explicit
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bank2hledger.toml"));
    if path.exists() && !force {
        bail!(
            "{} already exists — edit it, or re-run with --force to overwrite (rules files are never touched)",
            path.display()
        );
    }
    // Same guard as the rules files: a symlink planted at the config path
    // (shared/synced directory) must not redirect the write.
    crate::fs_guard::write_refusing_symlinks(&path, config::Config::template().as_bytes())?;

    // If the config parses and has accounts, create directories and starter
    // rules files. (The template has everything commented out, so a fresh
    // init only creates the config.)
    let mut report = InitReport {
        config_path: path.clone(),
        dirs: Vec::new(),
        rules_files: Vec::new(),
        next_step: String::new(),
    };
    if let Ok(cfg) = config::Config::load(&path) {
        for dir in [&cfg.in_dir, &cfg.staging_dir, &cfg.rules_dir] {
            std::fs::create_dir_all(dir)?;
            report.dirs.push(dir.clone());
        }
        for account in &cfg.accounts {
            let p = rules::ensure_rules_file(&cfg.rules_dir, account)?;
            report.rules_files.push(p);
        }
        report.next_step = format!(
            "Next: edit {} to name your real accounts, drop exports into {}, and run \
             `bank2hledger import --dry-run`.",
            path.display(),
            cfg.in_dir.display()
        );
    } else {
        report.next_step =
            "Next: edit the config — uncomment and fill in your [[accounts]] entries.".to_string();
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    // The only test here exercises the symlink guard, which is unix-only;
    // gating the import too keeps Windows clippy free of an unused import.
    #[cfg(unix)]
    use super::*;

    #[cfg(unix)]
    #[test]
    fn init_refuses_to_write_through_a_planted_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, b"innocent").unwrap();
        let link = dir.path().join("bank2hledger.toml");
        std::os::unix::fs::symlink(&victim, &link).unwrap();

        // Without --force, init refuses because the path already exists —
        // the guard matters for the `--force` overwrite path.
        let err = run(Some(&link), true).unwrap_err().to_string();
        assert!(err.contains("symlink"), "{err}");
        // The symlink target is untouched.
        assert_eq!(std::fs::read(&victim).unwrap(), b"innocent");
    }
}
