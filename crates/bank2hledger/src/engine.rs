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
    /// Rows offered for import this run (before interactive skips).
    pub new_count: usize,
    pub already_seen: usize,
    /// Rows the user resolved as duplicates of existing journal entries
    /// during an interactive run. Zero otherwise.
    pub skipped_as_duplicates: usize,
    /// Journal-format preview of the transactions that would be added
    /// (reflecting interactive skips, if any).
    pub preview: Option<String>,
    /// Staged rows that look like transactions already in the journal —
    /// one entry per flagged row, candidates best-first (see `overlap`).
    pub warnings: Vec<overlap::RowMatches>,
}

/// A decision callback for [`run_interactive`]: given a staged row with
/// above-threshold overlap matches, return `Ok(true)` to import it as-is,
/// `Ok(false)` to skip it as a duplicate of an existing entry. Returning
/// `Err` aborts the whole import before anything is written.
pub type DecisionFn<'a> = dyn FnMut(&overlap::RowMatches) -> Result<bool> + 'a;

pub fn hledger_cmd() -> Command {
    Command::new(std::env::var("HLEDGER").unwrap_or_else(|_| "hledger".into()))
}

/// Import (or preview) new transactions for all configured accounts, or the
/// given subset. Non-interactive: overlap matches are reported, never acted on.
pub fn run(
    config: &Config,
    accounts: &[String],
    dry_run: bool,
    since: Option<NaiveDate>,
) -> Result<Vec<ImportOutcome>> {
    run_inner(config, accounts, dry_run, since, &mut Decider::None)
}

/// Like [`run`], but pauses on every flagged row and asks `decider` whether
/// to import it as-is (`true`) or skip it as a duplicate (`false`). Skipped
/// rows are recorded as resolved (never re-offered) and excluded from the
/// batch written to the journal.
pub fn run_interactive(
    config: &Config,
    accounts: &[String],
    dry_run: bool,
    since: Option<NaiveDate>,
    decider: &mut DecisionFn,
) -> Result<Vec<ImportOutcome>> {
    run_inner(
        config,
        accounts,
        dry_run,
        since,
        &mut Decider::Callback(decider),
    )
}

/// Like [`run`], but skips the staged rows whose dedup keys appear in
/// `skip_keys` — the GUI collects these decisions up front from the
/// preview's duplicate choices. Skipped rows are recorded as resolved
/// (never re-offered), counted in `skipped_as_duplicates`, and keys that
/// don't correspond to flagged rows are ignored.
pub fn run_with_decisions(
    config: &Config,
    accounts: &[String],
    dry_run: bool,
    since: Option<NaiveDate>,
    skip_keys: &HashSet<String>,
) -> Result<Vec<ImportOutcome>> {
    run_inner(
        config,
        accounts,
        dry_run,
        since,
        &mut Decider::SkipKeys(skip_keys),
    )
}

/// How interactive duplicate decisions reach the import: none (advisory
/// only — flagged rows are reported, never acted on), a per-row callback
/// (CLI `--interactive`), or a pre-collected set of staged keys to skip
/// (GUI: decisions made in the preview).
pub enum Decider<'a> {
    None,
    Callback(&'a mut DecisionFn<'a>),
    SkipKeys(&'a HashSet<String>),
}

fn run_inner(
    config: &Config,
    accounts: &[String],
    dry_run: bool,
    since: Option<NaiveDate>,
    decider: &mut Decider<'_>,
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
        let outcome = import_account(config, account, dry_run, since, &mut *decider)?;
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

/// Flagged rows for one account without importing: parses, dedups, writes
/// the staging CSV and asks hledger to render the staged rows through the
/// account's rules, so candidates carry the accounts the import would use.
/// Unlike [`preview_account`], this invokes hledger and writes the staging
/// CSV — the GUI calls it only when the user opts into duplicate resolution.
pub fn preview_duplicates(
    config: &Config,
    account_name: &str,
    since: Option<NaiveDate>,
) -> Result<Vec<overlap::RowMatches>> {
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
    let new: Vec<&Transaction> = txs
        .iter()
        .filter(|t| !seen.contains(&t.dedup_key()))
        .collect();
    if new.is_empty() {
        return Ok(vec![]);
    }

    std::fs::create_dir_all(&config.staging_dir)?;
    let staging_csv = config.staging_dir.join(format!("{}.csv", account_name));
    write_staging_csv(&staging_csv, &new)?;
    let rules_file = rules::ensure_rules_file(&config.rules_dir, account)?;
    let keys: Vec<String> = new.iter().map(|t| t.dedup_key()).collect();
    overlap::find_overlaps(
        &config.journal,
        &staging_csv,
        &rules_file,
        &account.hledger_account,
        &keys,
        &overlap::Params::default(),
    )
}

fn import_account(
    config: &Config,
    account: &crate::config::AccountConfig,
    dry_run: bool,
    since: Option<NaiveDate>,
    decider: &mut Decider<'_>,
) -> Result<ImportOutcome> {
    let files = collect_files(&config.in_dir, &account.name)?;
    if files.is_empty() {
        return Ok(ImportOutcome {
            account: account.name.clone(),
            new_count: 0,
            already_seen: 0,
            skipped_as_duplicates: 0,
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
            skipped_as_duplicates: 0,
            preview: None,
            warnings: vec![],
        });
    }

    std::fs::create_dir_all(&config.staging_dir)?;
    let staging_csv = config.staging_dir.join(format!("{}.csv", account.name));
    let full_keys: Vec<String> = new_txs.iter().map(|t| t.dedup_key()).collect();
    let new_refs: Vec<&Transaction> = new_txs.iter().collect();
    write_staging_csv(&staging_csv, &new_refs)?;

    let rules_file = rules::ensure_rules_file(&config.rules_dir, account)?;

    // Overlap detection runs against the journal as it exists BEFORE this
    // batch (so a real import can't match against its own writes). It must
    // come after the staging write — hledger print reads the staged CSV —
    // and never blocks the import: a failure here is a warning gap, not a
    // data hazard.
    let staged_keys = full_keys;
    let flagged_rows = overlap::find_overlaps(
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

    // Interactive decisions: each flagged row is presented (in staging order,
    // i.e. chronological) to the decider, which picks "import as-is" or
    // "skip as duplicate". Skipped rows are recorded as resolved — the user
    // made a call, so they must not be re-offered next run.
    let mut skip_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    match decider {
        Decider::None => {}
        Decider::Callback(decide) => {
            for row in &flagged_rows {
                if !decide(row)? {
                    skip_keys.insert(row.staged_key.clone());
                }
            }
        }
        Decider::SkipKeys(keys) => {
            for row in &flagged_rows {
                if keys.contains(&row.staged_key) {
                    skip_keys.insert(row.staged_key.clone());
                }
            }
        }
    }
    let skipped_as_duplicates = skip_keys.len();
    let accepted: Vec<&Transaction> = if skip_keys.is_empty() {
        new_txs.iter().collect()
    } else {
        new_txs
            .iter()
            .filter(|t| !skip_keys.contains(&t.dedup_key()))
            .collect()
    };
    // The batch handed to hledger contains only accepted rows.
    write_staging_csv(&staging_csv, &accepted)?;

    let output = if accepted.is_empty() {
        // Everything was resolved as a duplicate; nothing to hand to hledger.
        None
    } else {
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
        let out = cmd
            .output()
            .context("running hledger — is it installed and on PATH? (or set $HLEDGER)")?;
        if !out.status.success() {
            bail!(
                "hledger import failed for account '{}':\n{}",
                account.name,
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Some(out)
    };
    let preview = output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());

    if !dry_run {
        // Record ids once hledger accepted the batch — including rows the
        // user resolved as duplicates interactively (their decision stands).
        save_seen(&config.staging_dir, &account.name, &seen)?;
    }

    let _ = &files;
    Ok(ImportOutcome {
        account: account.name.clone(),
        new_count,
        already_seen,
        skipped_as_duplicates,
        preview,
        warnings: flagged_rows,
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

fn write_staging_csv(path: &Path, txs: &[&Transaction]) -> Result<()> {
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
