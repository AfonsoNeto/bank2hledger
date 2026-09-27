//! Aqua (NewDay) credit-card monthly statement PDF.
//!
//! Aqua offers no CSV export and no individual-facing API, so the practical
//! path is the monthly PDF statement. We shell out to `pdftotext` (poppler)
//! with `-layout`, which preserves column alignment, and parse transaction
//! rows with a line regex. Card statements vary between NewDay brands, so
//! rows that don't match are skipped and counted — if nothing parses, we
//! fail with a clear error instead of silently importing nothing.

use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use regex::Regex;
use rust_decimal::Decimal;

use crate::config::AccountConfig;
use crate::model::Transaction;

/// Matches lines like `01 MAY  TESCO STORES 1234      12.34` or
/// `01 MAY  TESCO STORES 1234            £45.67   £1,234.56` (balance column
/// optional; may appear on statement-running rows only).
const ROW: &str = r#"^(?<day>\d{1,2})\s+(?<mon>[A-Z]{3})\s+(?<desc>.+?)\s+(?<amount>-?£?\d[\d,]*\.\d{2}-?)(\s+-?£?\d[\d,]*\.\d{2})?\s*$"#;

pub fn parse(account: &AccountConfig, path: &Path) -> Result<Vec<Transaction>> {
    let text = pdftotext(path)?;
    let re = Regex::new(ROW).context("internal: bad row regex")?;
    let current_year = chrono::Utc::now().year();

    let mut txs = Vec::new();
    let mut skipped = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(caps) = re.captures(line) else {
            skipped += 1;
            continue;
        };
        let day: u32 = caps["day"].parse().context("bad day")?;
        let mon = month_index(&caps["mon"])?;
        // Statements show dates without a year; assume the statement covers
        // the most recent occurrence of that month (statement cadence).
        let year = infer_year(current_year, mon);
        let date = NaiveDate::from_ymd_opt(year, mon, day)
            .with_context(|| format!("bad date {day} {mon} {year}"))?;
        let amount_raw = caps["amount"].replace(['£', ','], "");
        let amount = Decimal::from_str(&amount_raw)
            .with_context(|| format!("bad amount '{}'", amount_raw))?;
        // On card statements, purchases are positive but they *increase* the
        // debt owed; the hledger liability account needs them negative.
        let amount = -amount;
        txs.push(Transaction {
            date,
            payee: caps["desc"].trim().to_string(),
            amount,
            currency: "GBP".to_string(),
            account: account.hledger_account.clone(),
            external_id: None,
            notes: None,
        });
    }

    if txs.is_empty() {
        bail!(
            "aqua_pdf: no transaction rows parsed from {} ({} lines skipped). \
             The statement layout may differ from what this profile supports — \
             please open an issue with the line format (redact personal data).",
            path.display(),
            skipped
        );
    }
    Ok(txs)
}

fn pdftotext(path: &Path) -> Result<String> {
    let output = Command::new("pdftotext")
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "aqua_pdf: failed to run pdftotext ({e}). Install poppler-utils \
                 (`pacman -S poppler`, `apt install poppler-utils`, `brew install poppler`)."
            )
        })?;
    if !output.status.success() {
        bail!(
            "aqua_pdf: pdftotext failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout).context("pdftotext produced non-UTF8 output")
}

fn month_index(mon: &str) -> Result<u32> {
    Ok(match mon.to_ascii_uppercase().as_str() {
        "JAN" => 1,
        "FEB" => 2,
        "MAR" => 3,
        "APR" => 4,
        "MAY" => 5,
        "JUN" => 6,
        "JUL" => 7,
        "AUG" => 8,
        "SEP" => 9,
        "OCT" => 10,
        "NOV" => 11,
        "DEC" => 12,
        other => bail!("unknown month '{}'", other),
    })
}

/// For month m, pick the year that puts the statement within the last ~13
/// months: the most recent m that isn't in the future.
fn infer_year(current_year: i32, mon: u32) -> i32 {
    let now = chrono::Utc::now();
    let candidate = chrono::NaiveDate::from_ymd_opt(current_year, mon, 1).unwrap();
    if candidate <= now.date_naive() {
        current_year
    } else {
        current_year - 1
    }
}

use chrono::Datelike;
