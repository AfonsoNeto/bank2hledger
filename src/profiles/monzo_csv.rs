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
