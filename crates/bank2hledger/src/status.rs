//! `status`: show current balances of the configured bank accounts so the
//! user can compare against the real numbers in their bank apps.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::engine::hledger_cmd;

pub fn run(config: &Config, accounts: &[String]) -> Result<()> {
    print!("{}", balances(config, accounts)?);
    Ok(())
}

/// Run `hledger bal` and return its output as text, so callers other than
/// the CLI (the GUI) can display it however they like.
pub fn balances(config: &Config, accounts: &[String]) -> Result<String> {
    let wanted: Vec<&str> = if accounts.is_empty() {
        config
            .accounts
            .iter()
            .map(|a| a.hledger_account.as_str())
            .collect()
    } else {
        accounts
            .iter()
            .map(|name| Ok(config.account(name)?.hledger_account.as_str()))
            .collect::<Result<Vec<_>>>()?
    };
    if wanted.is_empty() {
        bail!("no accounts configured");
    }

    let mut cmd = hledger_cmd();
    cmd.arg("bal")
        .arg("-f")
        .arg(&config.journal)
        .args(&wanted)
        .arg("--flat");
    let output = cmd
        .output()
        .context("running hledger — is it installed and on PATH? (or set $HLEDGER)")?;
    if !output.status.success() {
        bail!(
            "hledger bal failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Ensure the journal file exists so first imports don't fail confusingly.
pub fn ensure_journal(journal: &std::path::Path) -> Result<()> {
    if !journal.exists() {
        std::fs::create_dir_all(
            journal
                .parent()
                .context("journal path has no parent directory")?,
        )?;
        crate::fs_guard::write_refusing_symlinks(journal, b"")?;
    }
    Ok(())
}

/// Used by `import` to refuse to touch a journal that doesn't exist yet.
pub fn require_journal(journal: &std::path::Path) -> Result<()> {
    if !journal.exists() {
        bail!(
            "journal {} does not exist — create it (or fix `journal =` in the config) first",
            journal.display()
        );
    }
    let _ = Command::new("true");
    Ok(())
}
