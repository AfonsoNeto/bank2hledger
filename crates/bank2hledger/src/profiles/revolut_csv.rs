//! Revolut account statement export, CSV flavour (the app's newer
//! "Account → Statement → Export" produces CSV; columns are identical to the
//! XLS export — see [`super::revolut_common`]).
//!
//! Newer exports also carry row types the XLS one didn't: `Charge` (posted as
//! Amount 0.00 with the fee in the Fee column), `Card Refund`, and a
//! `REVERTED` state whose rows never happened and are skipped.

use std::path::Path;

use anyhow::{Context, Result};

use crate::config::AccountConfig;
use crate::model::Transaction;

pub fn parse(account: &AccountConfig, _path: &Path, bytes: &[u8]) -> Result<Vec<Transaction>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_reader(bytes);
    let headers: Vec<String> = reader
        .headers()
        .cloned()
        .context("revolut_csv: no header row")?
        .iter()
        .map(|h| h.trim().to_string())
        .collect();
    let data: Vec<Vec<String>> = reader
        .records()
        .enumerate()
        .map(|(i, r)| {
            r.map(|row| row.iter().map(|c| c.trim().to_string()).collect())
                .with_context(|| format!("revolut_csv: row {}", i + 2))
        })
        .collect::<Result<Vec<_>>>()?;

    super::revolut_common::rows_to_transactions(account, "revolut_csv", &headers, &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn acct() -> AccountConfig {
        AccountConfig {
            name: "rev-csv".into(),
            profile: "revolut_csv".into(),
            hledger_account: "assets:bank:rev".into(),
            generic: None,
        }
    }

    fn parse_fixture() -> Vec<Transaction> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/revolut-sample.csv");
        let bytes = std::fs::read(&path).unwrap();
        parse(&acct(), &path, &bytes).unwrap()
    }

    #[test]
    fn parses_the_csv_fixture() {
        // 13 rows: one CANCELLED and one REVERTED skipped, one PENDING kept.
        let txs = parse_fixture();
        assert_eq!(txs.len(), 11);
    }

    #[test]
    fn reverted_and_cancelled_are_skipped_pending_is_kept() {
        let txs = parse_fixture();
        assert!(!txs.iter().any(|t| t.payee.contains("Reverted Shop")));
        assert!(!txs.iter().any(|t| t.payee.contains("Cancelled Shop")));
        assert!(txs.iter().any(|t| t.payee == "Paying Pending"));
    }

    #[test]
    fn charge_rows_post_the_fee() {
        let txs = parse_fixture();
        let fee = txs.iter().find(|t| t.payee == "Ultra plan fee").unwrap();
        assert_eq!(fee.amount, Decimal::from_str_exact("-12.99").unwrap());
    }

    #[test]
    fn topup_and_refund_are_positive() {
        let txs = parse_fixture();
        assert_eq!(
            txs.iter()
                .find(|t| t.payee.contains("BENEFIT OFFICE"))
                .unwrap()
                .amount,
            Decimal::from_str_exact("179.80").unwrap()
        );
        assert_eq!(
            txs.iter()
                .find(|t| t.payee.contains("Shop Refund"))
                .unwrap()
                .amount,
            Decimal::from_str_exact("19.08").unwrap()
        );
    }

    #[test]
    fn vault_transfers_keep_their_own_wording() {
        // "To GBP Savings" rows stay as-is; the user's rules route them to
        // their savings account, and the overlap transfer signal catches
        // double-side imports.
        let txs = parse_fixture();
        assert!(txs.iter().any(|t| t.payee == "To GBP Savings"));
    }

    #[test]
    fn fees_fold_and_multi_currency_is_kept() {
        let txs = parse_fixture();
        let hw = txs.iter().find(|t| t.payee == "Handy Hardware").unwrap();
        assert_eq!(hw.amount, Decimal::from_str_exact("-9.15").unwrap());
        assert!(txs.iter().any(|t| t.currency == "EUR"));
    }

    #[test]
    fn headerless_input_fails_loudly() {
        let err = parse(&acct(), Path::new("x.csv"), b"garbage,only\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing column"), "{err}");
    }
}
