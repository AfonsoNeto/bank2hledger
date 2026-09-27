//! Monzo CSV export (from the app: Account → Statement → Share → Export).
//!
//! Header (May 2026):
//! Transaction ID,Date,Time,Type,Name,Emoji,Category,Amount,Currency,
//! Local amount,Local currency,Notes and #tags,Address,Receipt,Description,
//! Category split,Money Out,Money In
//!
//! Dates are dd/mm/yyyy. `Amount` is signed in the account's currency;
//! `Local amount`/`Local currency` hold the original currency for FX spend.

use std::str::FromStr;

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::config::AccountConfig;
use crate::model::Transaction;

pub fn parse(account: &AccountConfig, bytes: &[u8]) -> Result<Vec<Transaction>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_reader(bytes);

    let headers = reader
        .headers()
        .cloned()
        .context("monzo_csv: export has no header row")?;
    let col = |name: &str| -> Result<usize> {
        headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
            .with_context(|| format!("monzo_csv: missing column '{}'", name))
    };
    let c_id = col("Transaction ID")?;
    let c_date = col("Date")?;
    let c_time = col("Time")?;
    let c_name = col("Name")?;
    let c_desc = col("Description")?;
    let c_notes = col("Notes and #tags")?;
    let c_amount = col("Amount")?;
    let c_currency = col("Currency")?;

    let mut txs = Vec::new();
    for (i, row) in reader.records().enumerate() {
        let row = row.with_context(|| format!("monzo_csv: row {}", i + 2))?;
        let get = |idx: usize| row.get(idx).unwrap_or("").trim();

        let date = NaiveDate::parse_from_str(get(c_date), "%d/%m/%Y")
            .with_context(|| format!("monzo_csv: row {}: bad date '{}'", i + 2, get(c_date)))?;
        let amount = Decimal::from_str(get(c_amount))
            .with_context(|| format!("monzo_csv: row {}: bad amount '{}'", i + 2, get(c_amount)))?;
        let payee = pick_payee(get(c_name), get(c_desc), get(c_id))?;
        let notes = {
            let mut parts: Vec<String> = Vec::new();
            if !get(c_time).is_empty() {
                parts.push(format!("time:{}", get(c_time)));
            }
            if !get(c_notes).is_empty() {
                parts.push(get(c_notes).to_string());
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" | "))
            }
        };

        txs.push(Transaction {
            date,
            payee,
            amount,
            currency: get(c_currency).to_string(),
            account: account.hledger_account.clone(),
            external_id: Some(get(c_id).to_string()),
            notes,
        });
    }
    Ok(txs)
}

/// Prefer the counterparty name; fall back to the raw description, then to
/// "Monzo <type-less>" using the transaction id so payees are never empty.
fn pick_payee(name: &str, desc: &str, id: &str) -> Result<String> {
    let mut payee = name.to_string();
    if payee.is_empty() {
        payee = desc.to_string();
    }
    if payee.is_empty() {
        // Monzo uses Description for things like "Top-up" or ref codes with
        // no Name; if both are empty (e.g. internal holds) show a placeholder.
        if id.is_empty() {
            bail!("monzo_csv: row has neither Name nor Description nor Transaction ID");
        }
        payee = format!("Monzo tx {}", id);
    }
    Ok(payee)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "Transaction ID,Date,Time,Type,Name,Emoji,Category,Amount,Currency,Local amount,Local currency,Notes and #tags,Address,Receipt,Description,Category split,Money Out,Money In\n";

    fn acct() -> AccountConfig {
        AccountConfig {
            name: "m".into(),
            profile: "monzo_csv".into(),
            hledger_account: "assets:bank:m".into(),
            generic: None,
        }
    }

    fn parse_str(csv: &str) -> Result<Vec<Transaction>> {
        parse(&acct(), csv.as_bytes())
    }

    fn row(id: &str, date: &str, time: &str, typ: &str, name: &str, category: &str, amount: &str, currency: &str, local_amount: &str, local_ccy: &str, notes: &str, desc: &str, money_out: &str, money_in: &str) -> String {
        format!("{id},{date},{time},{typ},{name},,{category},{amount},{currency},{local_amount},{local_ccy},{notes},,,{desc},,,{money_out},{money_in}\n")
    }

    #[test]
    fn basic_row_is_normalized() {
        let csv = format!("{}{}", HEADER, row("tx1", "01/05/2026", "04:40:51", "Direct Debit", "HES", "Bills", "-8.75", "GBP", "-8.75", "GBP", "", "VBFS", "-8.75", ""));
        let txs = parse_str(&csv).unwrap();
        assert_eq!(txs.len(), 1);
        let t = &txs[0];
        assert_eq!(t.date.to_string(), "2026-05-01");
        assert_eq!(t.payee, "HES");
        assert_eq!(t.amount.to_string(), "-8.75");
        assert_eq!(t.currency, "GBP");
        assert_eq!(t.external_id.as_deref(), Some("tx1"));
        assert_eq!(t.account, "assets:bank:m");
        assert_eq!(t.notes.as_deref(), Some("time:04:40:51"));
    }

    #[test]
    fn notes_and_time_are_combined_into_notes() {
        let csv = format!("{}{}", HEADER, row("tx1", "01/05/2026", "10:00:00", "M", "N", "C", "-1.00", "GBP", "-1.00", "GBP", "some #tag", "d", "-1.00", ""));
        let t = &parse_str(&csv).unwrap()[0];
        let notes = t.notes.as_deref().unwrap();
        assert!(notes.contains("time:10:00:00"), "{notes}");
        assert!(notes.contains("some #tag"), "{notes}");
        assert!(notes.contains(" | "));
    }

    #[test]
    fn local_currency_rows_keep_account_currency_for_amount() {
        // Spend in EUR from a GBP account: Amount stays the GBP figure.
        let csv = format!("{}{}", HEADER, row("tx1", "01/05/2026", "10:00:00", "CARD", "Hotel", "Travel", "-86.00", "GBP", "-100.00", "EUR", "", "", "-86.00", ""));
        let t = &parse_str(&csv).unwrap()[0];
        assert_eq!(t.currency, "GBP");
        assert_eq!(t.amount.to_string(), "-86.00");
    }

    #[test]
    fn money_in_is_positive() {
        let csv = format!("{}{}", HEADER, row("tx1", "01/05/2026", "10:00:00", "FPS", "Refund", "Transfers", "", "GBP", "", "GBP", "", "", "", "25.00"));
        // Note: amount column empty here — that must be an error, Monzo always fills Amount.
        assert!(parse_str(&csv).is_err());
    }

    #[test]
    fn payee_falls_back_to_description_then_id() {
        let csv = format!("{}{}", HEADER, row("tx1", "01/05/2026", "10:00:00", "M", "", "C", "-1.00", "GBP", "-1.00", "GBP", "", "Top-up", "-1.00", ""));
        assert_eq!(parse_str(&csv).unwrap()[0].payee, "Top-up");
        let csv2 = format!("{}{}", HEADER, row("tx2", "01/05/2026", "10:00:00", "M", "", "C", "-1.00", "GBP", "-1.00", "GBP", "", "", "-1.00", ""));
        assert_eq!(parse_str(&csv2).unwrap()[0].payee, "Monzo tx tx2");
    }

    #[test]
    fn quoted_fields_with_commas_parse() {
        let csv = format!("{}{}", HEADER, row("tx1", "01/05/2026", "10:00:00", "M", "\"Shop, Ltd\"", "C", "-1.00", "GBP", "-1.00", "GBP", "\"note, with comma\"", "\"desc, x\"", "-1.00", ""));
        let t = &parse_str(&csv).unwrap()[0];
        assert_eq!(t.payee, "Shop, Ltd");
        assert!(t.notes.as_deref().unwrap().contains("note, with comma"));
    }

    #[test]
    fn crlf_line_endings_ok() {
        let csv = format!("{}\r\n", row("tx1", "01/05/2026", "10:00:00", "M", "N", "C", "-1.00", "GBP", "-1.00", "GBP", "", "d", "-1.00", ""));
        let with_header = format!("{}\r\n{}", HEADER.trim_end(), csv);
        assert_eq!(parse_str(&with_header).unwrap()[0].payee, "N");
    }

    #[test]
    fn empty_rows_are_rejected_with_context() {
        let csv = format!("{}\n,,,M,,,,,,,,,,,\n", HEADER.trim_end());
        let err = parse_str(&csv).unwrap_err().to_string();
        assert!(err.contains("bad date"), "{err}");
    }

    #[test]
    fn bad_amount_is_rejected_with_row_number() {
        let csv = format!("{}\ntx1,01/05/2026,10:00:00,M,N,C,abc,GBP,,,,,,,\n", HEADER.trim_end());
        let err = parse_str(&csv).unwrap_err().to_string();
        assert!(err.contains("row 2"), "{err}");
        assert!(err.contains("bad amount"), "{err}");
    }

    #[test]
    fn missing_column_errors_name_the_column() {
        let csv = "Transaction ID,Date,Time,Type,Name,Emoji,Category,Amount,Local amount,Local currency,Notes and #tags,Address,Receipt,Description,Category split,Money Out,Money In\n";
        let err = parse_str(csv).unwrap_err().to_string();
        assert!(err.contains("'Currency'"), "{err}");
    }

    #[test]
    fn completely_empty_row_with_no_id_is_rejected() {
        // Name, Description and ID all empty → clear error, not a silent transaction.
        let csv = format!("{}{}", HEADER, row("", "01/05/2026", "10:00:00", "M", "", "C", "-1.00", "GBP", "-1.00", "GBP", "", "", "-1.00", ""));
        let err = parse_str(&csv).unwrap_err().to_string();
        assert!(err.contains("neither Name nor Description"), "{err}");
    }
}
