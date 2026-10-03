//! The import engine: raw files in `in_dir` → normalized staging CSV →
//! `hledger import` into the user's journal.
//!
//! Duplicate protection is two-layered:
//! 1. Our own seen-ids state file (exact, handles two identical coffees on
//!    the same day, which plain hledger dedup would collapse).
//! 2. `hledger import`'s built-in dedup as a safety net underneath.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;

use crate::config::Config;
use crate::model::Transaction;
use crate::overlap;
use crate::profiles;
use crate::rules;

#[derive(Debug)]
pub struct ImportOutcome {
    pub account: String,
    pub new_count: usize,
    pub already_seen: usize,
    /// Journal-format preview of the transactions that would be added.
    pub preview: Option<String>,
    /// Staged rows that look like transactions already in the journal
    /// (advisory — see the `overlap` module). Never populated for batches
    /// with no new rows.
    pub warnings: Vec<overlap::Warning>,
}

pub fn hledger_cmd() -> Command {
    Command::new(std::env::var("HLEDGER").unwrap_or_else(|_| "hledger".into()))
}

/// Import (or preview) new transactions for all configured accounts, or the
/// given subset.
pub fn run(
    config: &Config,
    accounts: &[String],
    dry_run: bool,
    since: Option<NaiveDate>,
) -> Result<Vec<ImportOutcome>> {
    let wanted: Vec<&str> = if accounts.is_empty() {
        config.accounts.iter().map(|a| a.name.as_str()).collect()
    } else {
        for name in accounts {
            config.account(name)?;
        }
        accounts.iter().map(|s| s.as_str()).collect()
    };

    let mut outcomes = Vec::new();
    for name in wanted {
        let account = config.account(name)?;
        let outcome = import_account(config, account, dry_run, since)?;
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Parsed, dedup-checked transactions for one account — without writing
/// staging CSVs or invoking hledger. The GUI uses this for its review
/// table; the CLI equivalent is [`run`] with `dry_run = true`.
#[derive(Debug)]
pub struct PreviewOutcome {
    pub account: String,
    pub new: Vec<Transaction>,
    pub already_seen: usize,
}

pub fn preview_account(
    config: &Config,
    account_name: &str,
    since: Option<NaiveDate>,
) -> Result<PreviewOutcome> {
    let account = config.account(account_name)?;
    let files = collect_files(&config.in_dir, account_name)?;
    let mut txs = Vec::new();
    for file in &files {
        txs.extend(profiles::parse_file(account, file)?);
    }
    if let Some(since) = since {
        txs.retain(|t| t.date >= since);
    }
    txs.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.payee.cmp(&b.payee)));

    let seen = load_seen(&config.staging_dir, account_name)?;
    let mut outcome = PreviewOutcome {
        account: account_name.to_string(),
        new: Vec::new(),
        already_seen: 0,
    };
    for tx in txs {
        if seen.contains(&tx.dedup_key()) {
            outcome.already_seen += 1;
        } else {
            outcome.new.push(tx);
        }
    }
    Ok(outcome)
}

fn import_account(
    config: &Config,
    account: &crate::config::AccountConfig,
    dry_run: bool,
    since: Option<NaiveDate>,
) -> Result<ImportOutcome> {
    let files = collect_files(&config.in_dir, &account.name)?;
    if files.is_empty() {
        return Ok(ImportOutcome {
            account: account.name.clone(),
            new_count: 0,
            already_seen: 0,
            preview: None,
            warnings: vec![],
        });
    }

    let mut txs = Vec::new();
    for file in &files {
        txs.extend(profiles::parse_file(account, file)?);
    }
    if let Some(since) = since {
        txs.retain(|t| t.date >= since);
    }
    txs.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.payee.cmp(&b.payee)));

    let mut seen = load_seen(&config.staging_dir, &account.name)?;
    let mut new_txs = Vec::new();
    let mut already_seen = 0usize;
    for tx in txs {
        let key = tx.dedup_key();
        if seen.contains(&key) {
            already_seen += 1;
        } else {
            seen.insert(key);
            new_txs.push(tx);
        }
    }
    let new_count = new_txs.len();

    if new_count == 0 {
        return Ok(ImportOutcome {
            account: account.name.clone(),
            new_count: 0,
            already_seen,
            preview: None,
            warnings: vec![],
        });
    }

    std::fs::create_dir_all(&config.staging_dir)?;
    let staging_csv = config.staging_dir.join(format!("{}.csv", account.name));
    write_staging_csv(&staging_csv, &new_txs)?;

    let rules_file = rules::ensure_rules_file(&config.rules_dir, account)?;

    // Overlap detection runs against the journal as it exists BEFORE this
    // batch (so a real import can't match against its own writes). It must
    // come after the staging write — hledger print reads the staged CSV —
    // and never blocks the import: a failure here is a warning gap, not a
    // data hazard.
    let staged_keys: Vec<String> = new_txs.iter().map(|t| t.dedup_key()).collect();
    let warnings = overlap::find_overlaps(
        &config.journal,
        &staging_csv,
        &rules_file,
        &account.hledger_account,
        &staged_keys,
        &overlap::Params::default(),
    )
    .unwrap_or_else(|e| {
        eprintln!("warning: overlap detection skipped: {e:#}");
        vec![]
    });

    let mut cmd = hledger_cmd();
    cmd.arg("import")
        .arg("-f")
        .arg(&config.journal)
        .arg("--rules")
        .arg(&rules_file);
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.arg(&staging_csv);
    let output = cmd
        .output()
        .context("running hledger — is it installed and on PATH? (or set $HLEDGER)")?;
    if !output.status.success() {
        bail!(
            "hledger import failed for account '{}':\n{}",
            account.name,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    if !dry_run {
        // Only record ids once hledger accepted the batch.
        save_seen(&config.staging_dir, &account.name, &seen)?;
    }

    let _ = &files;
    Ok(ImportOutcome {
        account: account.name.clone(),
        new_count,
        already_seen,
        preview: Some(String::from_utf8_lossy(&output.stdout).into_owned()),
        warnings,
    })
}

fn collect_files(in_dir: &Path, account_name: &str) -> Result<Vec<PathBuf>> {
    if !in_dir.exists() {
        return Ok(vec![]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(in_dir)
        .with_context(|| format!("reading {}", in_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            profiles::matches_account(
                p.file_name().unwrap_or_default().to_string_lossy().as_ref(),
                account_name,
            )
        })
        .collect();
    files.sort();
    if files.is_empty() {
        eprintln!(
            "note: no files in {} match account '{}' (name files like '{}.csv' or '{}-*.csv')",
            in_dir.display(),
            account_name,
            account_name,
            account_name
        );
    }
    Ok(files)
}

fn write_staging_csv(path: &Path, txs: &[Transaction]) -> Result<()> {
    let mut writer = csv::WriterBuilder::new().from_path(path)?;
    for tx in txs {
        writer.write_record(&[
            tx.date.format("%Y-%m-%d").to_string(),
            sanitize_field(&tx.payee),
            tx.amount.normalize().to_string(),
            sanitize_field(&tx.currency),
            sanitize_field(tx.external_id.as_deref().unwrap_or_default()),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

/// Strip control characters (including newlines) from fields that hledger
/// rules interpolate into the journal (`description`, `comment %id`,
/// `currency %currency`). A crafted export with a newline inside a quoted
/// CSV cell could otherwise inject arbitrary journal content — fake
/// transactions or balance-affecting postings — beyond the intended entry.
/// Everything else (spaces, unicode, punctuation) passes through untouched.
pub fn sanitize_field(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn seen_path(staging_dir: &Path, account: &str) -> PathBuf {
    staging_dir.join(format!("{}.seen", account))
}

fn load_seen(staging_dir: &Path, account: &str) -> Result<HashSet<String>> {
    let path = seen_path(staging_dir, account);
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(raw.lines().map(|l| l.trim().to_string()).collect())
}

fn save_seen(staging_dir: &Path, account: &str, seen: &HashSet<String>) -> Result<()> {
    std::fs::create_dir_all(staging_dir)?;
    let mut lines: Vec<&String> = seen.iter().collect();
    lines.sort();
    let mut out = lines
        .into_iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    crate::fs_guard::write_refusing_symlinks(&seen_path(staging_dir, account), out.as_bytes())?;
    Ok(())
}
