//! Shared conversion for Revolut statement rows (both the XLS/OLE2 export and
//! the newer CSV export use the same columns):
//!
//! Type | Product | Started Date | Completed Date | Description | Amount |
//! Fee | Currency | State | Balance
//!
//! Dates are ISO-ish ("2025-02-01 12:34:56"). `Amount` is signed in the
//! transaction's own `Currency`; FX top-ups show as exchange rows.

use std::str::FromStr;

use anyhow::{bail, Context, Result};
use chrono::NaiveDateTime;
use rust_decimal::Decimal;

use crate::config::AccountConfig;
use crate::model::Transaction;

/// Convert statement rows (already read as strings, header row excluded) into
/// transactions. `label` names the calling profile for error messages.
pub fn rows_to_transactions(
    account: &AccountConfig,
    label: &str,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<Vec<Transaction>> {
    let col = |name: &str| -> Result<usize> {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
            .with_context(|| format!("{label}: missing column '{}'", name))
    };
    let c_type = col("Type")?;
    let c_started = col("Started Date")?;
    let c_desc = col("Description")?;
    let c_amount = col("Amount")?;
    let c_fee = col("Fee")?;
    let c_currency = col("Currency")?;
    let c_state = col("State")?;

    let mut txs = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let get =
            |idx: usize| -> String { row.get(idx).cloned().unwrap_or_default().trim().to_string() };

        // Cancelled and reverted rows never happened; rows with no state are
        // not transactions. Pending card auths (empty Completed Date) are
        // still real spend and are kept.
        let state = get(c_state);
        if state.is_empty()
            || state.eq_ignore_ascii_case("cancelled")
            || state.eq_ignore_ascii_case("reverted")
        {
            continue;
        }
        let typ = get(c_type);

        let date = parse_date(&get(c_started), i, label)?;
        let amount = Decimal::from_str(&get(c_amount))
            .with_context(|| format!("{label}: row {}: bad amount '{}'", i + 2, get(c_amount)))?;
        let fee = Decimal::from_str(&get(c_fee)).unwrap_or_default();
        // Fold the fee into the amount so the journal posting matches the
        // actual balance change on the account (e.g. subscription charges
        // post as Amount 0.00 with the fee in the Fee column).
        let amount = amount - fee;

        let mut payee = get(c_desc);
        if payee.is_empty() {
            payee = format!("Revolut {}", typ);
        }
        if typ.eq_ignore_ascii_case("exchange") || get(c_desc).contains("Exchange") {
            // Internal FX conversion rows are balance-neutral between
            // currencies; rename them so rules can route them to transfers.
            payee = "Revolut currency exchange".to_string();
        }
        // Synthetic id: Revolut exports carry no transaction ids; the date +
        // payee + amount key in `dedup_key` covers re-dropped files.
        let external_id = None;

        txs.push(Transaction {
            date: date.date(),
            payee,
            amount,
            currency: get(c_currency),
            account: account.hledger_account.clone(),
            external_id,
            notes: if typ.is_empty() { None } else { Some(typ) },
        });
    }
    Ok(txs)
}

fn parse_date(s: &str, row: usize, label: &str) -> Result<NaiveDateTime> {
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
        "%d/%m/%Y %H:%M",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(dt);
        }
        if let Ok(dd) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Ok(dd.and_hms_opt(0, 0, 0).unwrap());
        }
    }
    bail!("{label}: row {}: unparseable date '{}'", row + 2, s)
}
