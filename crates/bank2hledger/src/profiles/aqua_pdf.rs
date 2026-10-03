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
    parse_text(account, &text)
}

/// Parse statement text (as `pdftotext -layout` produces). Split from
/// `parse` so the layout logic is testable without poppler installed.
pub(crate) fn parse_text(account: &AccountConfig, text: &str) -> Result<Vec<Transaction>> {
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
        // Card payments received are redundant: they are already captured by
        // the paying bank's own import (routed to liabilities:credit_cards:aqua
        // via that account's rules). Importing them again would double-reduce
        // the debt and book a phantom expense. Refunds are NOT payments — they
        // have no bank-side counterpart and must still be imported.
        let desc = caps["desc"].trim();
        let lower = desc.to_ascii_lowercase();
        if lower.contains("payment") && (lower.contains("thank you") || lower.contains("received"))
        {
            skipped += 1;
            continue;
        }
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
            payee: desc.to_string(),
            amount,
            currency: "GBP".to_string(),
            account: account.hledger_account.clone(),
            external_id: None,
            notes: None,
        });
    }

    if txs.is_empty() {
        bail!(
            "aqua_pdf: no transaction rows parsed ({} lines skipped). \
             The statement layout may differ from what this profile supports — \
             please open an issue with the line format (redact personal data).",
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

pub(crate) fn month_index(mon: &str) -> Result<u32> {
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
pub(crate) fn infer_year(current_year: i32, mon: u32) -> i32 {
    let now = chrono::Utc::now();
    let candidate = chrono::NaiveDate::from_ymd_opt(current_year, mon, 1).unwrap();
    if candidate <= now.date_naive() {
        current_year
    } else {
        current_year - 1
    }
}

use chrono::Datelike;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AccountConfig;
    use rust_decimal::Decimal;

    fn acct() -> AccountConfig {
        AccountConfig {
            name: "aqua".into(),
            profile: "aqua_pdf".into(),
            hledger_account: "liabilities:cards:aqua".into(),
            generic: None,
        }
    }

    const STATEMENT: &str = "\
Aqua Card Statement  May 2026
Account: **** 1234        Statement date: 28 May 2026

Date        Description                   Amount        Balance
01 MAY  CORNER GROCER 4021              12.40        845.60
02 MAY  CITY COFFEE CO                    3.80         849.40
03 MAY  PAYMENT RECEIVED - THANK YOU   -120.00        729.40
04 MAY  FUEL STOP 88                      45.00        774.40
05 MAY  STREAM SERVICE                    9.99         784.39
06 MAY  INTEREST CHARGED                  6.12         790.51
06 MAY  FEE - LATE PAYMENT               12.00         802.51

For queries call the number on the back of your card.
";

    fn txs() -> Vec<Transaction> {
        parse_text(&acct(), STATEMENT).unwrap()
    }

    #[test]
    fn parses_all_transaction_rows_and_skips_header_footer() {
        // 7 rows minus the payment row, which is deliberately skipped.
        assert_eq!(txs().len(), 6);
    }

    #[test]
    fn purchases_are_negative_on_the_liability_account() {
        let t = txs()
            .into_iter()
            .find(|t| t.payee.contains("CORNER GROCER"))
            .unwrap();
        assert_eq!(t.amount, Decimal::from_str_exact("-12.40").unwrap());
        assert_eq!(t.account, "liabilities:cards:aqua");
        assert_eq!(t.currency, "GBP");
    }

    #[test]
    fn payments_received_are_skipped_not_imported() {
        // The paying bank's own import already posts the payment to the
        // liability account; importing the statement's payment row too would
        // double-reduce the debt and book a phantom expense.
        assert!(!txs().iter().any(|t| t.payee.contains("PAYMENT RECEIVED")));
    }

    #[test]
    fn merchant_names_containing_payment_are_not_skipped() {
        let t = parse_text(
            &acct(),
            "01 MAY  SWAN ENERGY PAYMENTS LTD      45.00        890.00\n",
        )
        .unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].payee, "SWAN ENERGY PAYMENTS LTD");
    }

    #[test]
    fn refunds_are_still_imported() {
        // Refunds have no bank-side counterpart; only payment rows are skipped.
        let t = parse_text(
            &acct(),
            "02 MAY  REFUND - BROKEN GOODS    -25.00        820.40\n",
        )
        .unwrap();
        assert_eq!(t[0].payee, "REFUND - BROKEN GOODS");
        assert_eq!(t[0].amount, Decimal::from_str_exact("25.00").unwrap());
    }

    #[test]
    fn thousands_separators_and_pound_signs_parse() {
        let text = "01 MAY  BIG PURCHASE          £1,234.56   £5,000.00\n";
        let t = parse_text(&acct(), text).unwrap();
        assert_eq!(t[0].amount, Decimal::from_str_exact("-1234.56").unwrap());
    }

    #[test]
    fn negative_amounts_parse_too() {
        let text = "01 MAY  REFUND POSTED           -25.00     100.00\n";
        let t = parse_text(&acct(), text).unwrap();
        assert_eq!(t[0].amount, Decimal::from_str_exact("25.00").unwrap());
    }

    #[test]
    fn dates_get_inferred_year_for_may() {
        let t = txs().into_iter().next().unwrap();
        let year = infer_year(chrono::Utc::now().year(), 5);
        assert_eq!(t.date.to_string(), format!("{year}-05-01"));
    }

    #[test]
    fn no_rows_is_a_loud_failure_not_silence() {
        let err = parse_text(&acct(), "some unrelated text\nmore text\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no transaction rows parsed"), "{err}");
        assert!(err.contains("lines skipped"), "{err}");
    }

    #[test]
    fn month_index_table() {
        assert_eq!(month_index("JAN").unwrap(), 1);
        assert_eq!(month_index("dec").unwrap(), 12); // case-insensitive
        assert!(month_index("XYZ").is_err());
    }

    #[test]
    fn infer_year_never_lands_in_the_future() {
        let now = chrono::Utc::now();
        for mon in 1..=12 {
            let y = infer_year(now.year(), mon);
            let d = chrono::NaiveDate::from_ymd_opt(y, mon, 1).unwrap();
            assert!(d <= now.date_naive(), "mon {mon} inferred future {d}");
        }
    }

    #[test]
    fn synthetic_pdf_fixture_parses_via_text_layer() {
        // Mirrors the layout of tests/fixtures/aqua-sample.pdf.
        let text = "01 MAY  CORNER GROCER 4021              12.40        845.60\n\
                    02 MAY  CITY COFFEE CO                    3.80         849.40\n";
        let t = parse_text(&acct(), text).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[1].amount, Decimal::from_str_exact("-3.80").unwrap());
    }
}
