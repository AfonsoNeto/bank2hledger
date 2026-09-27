//! Revolut account statement export (.xls, OLE2/Excel binary — the format the
//! app and web "Account → Statement → Export" produce).
//!
//! Columns (checked against a real 2025–2026 export):
//! Type | Product | Started Date | Completed Date | Description | Amount |
//! Fee | Currency | State | Balance | Account ID | (extra card columns)
//!
//! Dates are ISO-ish ("2025-02-01 12:34:56"). `Amount` is signed in the
//! transaction's own `Currency`; FX top-ups show as exchange rows.

use std::io::BufReader;
use std::path::Path;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use calamine::{Data, Reader, Xls};
use chrono::NaiveDateTime;
use rust_decimal::{prelude::FromPrimitive, Decimal};

use crate::config::AccountConfig;
use crate::model::Transaction;

pub fn parse(account: &AccountConfig, path: &Path, _bytes: &[u8]) -> Result<Vec<Transaction>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut workbook: Xls<_> = Xls::new(BufReader::new(file))
        .context("revolut_xls: not a readable .xls (OLE2) file — did the app export CSV instead?")
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let range = workbook
        .worksheet_range_at(0)
        .transpose()?
        .context("revolut_xls: workbook has no sheets")?;

    let mut rows = range.rows();
    let header: Vec<String> = rows
        .next()
        .context("revolut_xls: export is empty")?
        .iter()
        .map(cell_string)
        .collect();
    let col = |name: &str| -> Result<usize> {
        header
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
            .with_context(|| format!("revolut_xls: missing column '{}'", name))
    };
    let c_type = col("Type")?;
    let c_started = col("Started Date")?;
    let c_desc = col("Description")?;
    let c_amount = col("Amount")?;
    let c_fee = col("Fee")?;
    let c_currency = col("Currency")?;
    let c_state = col("State")?;
    let _c_balance = col("Balance")?;

    let mut txs = Vec::new();
    for (i, row) in rows.enumerate() {
        let get = |idx: usize| -> String {
            row.get(idx)
                .map(cell_string)
                .unwrap_or_default()
                .trim()
                .to_string()
        };

        // Skip incomplete rows (pending card auths have empty Completed Date;
        // they are still real spend). Skip non-transactions instead.
        let state = get(c_state);
        if state.eq_ignore_ascii_case("cancelled") || state.is_empty() {
            continue;
        }
        let typ = get(c_type);
        if typ.eq_ignore_ascii_case("TOP UP") && get(c_desc).contains("Exchange") {
            // Revolut logs internal FX exchanges as TOP UP/Exchange rows; they
            // are balance-neutral between currencies and handled via the
            // balance check in `status`. Keep them but tag the payee so rules
            // can categorize them as transfers.
        }

        let date = parse_date(&get(c_started), i)?;
        let amount = match &row.get(c_amount) {
            Some(Data::Float(f)) => Decimal::from_f64(*f)
                .with_context(|| format!("revolut_xls: row {}: bad amount {}", i + 2, f))?,
            Some(Data::Int(n)) => Decimal::from(*n),
            Some(Data::String(s)) => Decimal::from_str(s.trim())
                .with_context(|| format!("revolut_xls: row {}: bad amount '{}'", i + 2, s))?,
            _ => bail!("revolut_xls: row {}: missing amount", i + 2),
        };
        let fee = match row.get(c_fee) {
            Some(Data::Float(f)) => Decimal::from_f64(*f).unwrap_or_default(),
            Some(Data::Int(n)) => Decimal::from(*n),
            Some(Data::String(s)) => Decimal::from_str(s.trim()).unwrap_or_default(),
            _ => Decimal::ZERO,
        };
        // Fold the fee into the amount so the journal posting matches the
        // actual balance change on the account.
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

fn parse_date(s: &str, row: usize) -> Result<NaiveDateTime> {
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
        "%d/%m/%Y %H:%M",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(dt);
        }
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Ok(d.and_hms_opt(0, 0, 0).unwrap());
        }
    }
    bail!("revolut_xls: row {}: unparseable date '{}'", row + 2, s)
}

fn cell_string(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.clone(),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt
            .as_datetime()
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#ERR{:?}", e),
        Data::Empty => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AccountConfig;
    use rust_decimal::Decimal;

    fn acct() -> AccountConfig {
        AccountConfig {
            name: "r".into(),
            profile: "revolut_xls".into(),
            hledger_account: "assets:bank:r".into(),
            generic: None,
        }
    }

    fn parse_fixture(name: &str) -> Vec<Transaction> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        parse(&acct(), &path, &[]).unwrap()
    }

    #[test]
    fn fees_are_folded_into_amounts() {
        let txs = parse_fixture("revolut-edge.xls");
        let kiosk = txs.iter().find(|t| t.payee == "Kiosk Bar").unwrap();
        assert_eq!(kiosk.amount, Decimal::from_str_exact("-5.75").unwrap()); // -5.50 - 0.25
        let trail = txs.iter().find(|t| t.payee == "Trailing zero fee").unwrap();
        assert_eq!(trail.amount, Decimal::from_str_exact("-10.29").unwrap()); // -9.99 - 0.30
    }

    #[test]
    fn fee_only_rows_become_negative_outflows() {
        let txs = parse_fixture("revolut-edge.xls");
        let fee = txs.iter().find(|t| t.notes.as_deref() == Some("FEE")).unwrap();
        assert_eq!(fee.amount, Decimal::from_str_exact("-1.00").unwrap());
        // Empty description falls back to the type.
        assert_eq!(fee.payee, "Revolut FEE");
    }

    #[test]
    fn cancelled_and_empty_states_are_skipped_but_pending_is_kept() {
        let txs = parse_fixture("revolut-edge.xls");
        assert!(!txs.iter().any(|t| t.payee.contains("Cancelled Shop")));
        assert!(!txs.iter().any(|t| t.payee.contains("No state row")));
        assert!(txs.iter().any(|t| t.payee == "Pending Shop"));
        // Case-insensitive state matching keeps lowercase "completed".
        assert!(txs.iter().any(|t| t.payee == "Lowercase state"));
    }

    #[test]
    fn exchange_rows_are_renamed() {
        let txs = parse_fixture("revolut-edge.xls");
        let exchange = txs
            .iter()
            .find(|t| t.payee.starts_with("Revolut currency exchange"))
            .unwrap();
        assert_eq!(exchange.amount, Decimal::from_str_exact("-100.00").unwrap());
        assert_eq!(exchange.currency, "GBP");
    }

    #[test]
    fn date_format_variants_parse() {
        let txs = parse_fixture("revolut-edge.xls");
        // "2026-05-02 09:00" (no seconds)
        assert_eq!(
            txs.iter().find(|t| t.payee == "Bakery Lane").unwrap().date.to_string(),
            "2026-05-02"
        );
        // "2026-05-03" (bare date)
        assert_eq!(
            txs.iter().find(|t| t.payee == "Bare date transfer").unwrap().date.to_string(),
            "2026-05-03"
        );
    }

    #[test]
    fn string_and_float_amount_cells_both_parse() {
        let txs = parse_fixture("revolut-edge.xls");
        assert_eq!(
            txs.iter().find(|t| t.payee == "String amount").unwrap().amount,
            Decimal::from_str_exact("-7.77").unwrap()
        );
    }

    #[test]
    fn type_is_preserved_in_notes() {
        let txs = parse_fixture("revolut-edge.xls");
        let t = txs.iter().find(|t| t.payee == "Bakery Lane").unwrap();
        assert_eq!(t.notes.as_deref(), Some("CARD_PAYMENT"));
    }

    #[test]
    fn non_xls_file_fails_loudly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.xls");
        std::fs::write(&path, b"this is not an OLE2 file").unwrap();
        let err = parse(&acct(), &path, &[]).unwrap_err().to_string();
        assert!(err.contains("not a readable .xls"), "{err}");
    }

    #[test]
    fn corrupt_ole2_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.xls");
        std::fs::write(&path, b"\xD0\xCF\x11\xE0garbage-not-a-cfb").unwrap();
        assert!(parse(&acct(), &path, &[]).is_err());
    }

    #[test]
    fn main_sample_still_parses() {
        let txs = parse_fixture("revolut-sample.xls");
        assert_eq!(txs.len(), 11);
        assert!(txs.iter().any(|t| t.currency == "EUR"));
    }
}
