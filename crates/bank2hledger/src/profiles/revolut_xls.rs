//! Revolut account statement export, XLS/OLE2 flavour (app & web "Account →
//! Statement → Export"). The row conversion is shared with `revolut_csv` —
//! see [`super::revolut_common`].

use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result};
use calamine::{Data, Reader, Xls};

use crate::config::AccountConfig;
use crate::model::Transaction;

pub fn parse(account: &AccountConfig, path: &Path, _bytes: &[u8]) -> Result<Vec<Transaction>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let workbook: Result<Xls<_>, _> = Xls::new(BufReader::new(file)).map_err(|e| {
        anyhow::anyhow!(
            "revolut_xls: not a readable .xls (OLE2) file — did the app export CSV instead? ({e})"
        )
    });
    let mut workbook = workbook?;

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
    let data: Vec<Vec<String>> = rows.map(|r| r.iter().map(cell_string).collect()).collect();

    super::revolut_common::rows_to_transactions(account, "revolut_xls", &header, &data)
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
        let fee = txs
            .iter()
            .find(|t| t.notes.as_deref() == Some("FEE"))
            .unwrap();
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
            txs.iter()
                .find(|t| t.payee == "Bakery Lane")
                .unwrap()
                .date
                .to_string(),
            "2026-05-02"
        );
        // "2026-05-03" (bare date)
        assert_eq!(
            txs.iter()
                .find(|t| t.payee == "Bare date transfer")
                .unwrap()
                .date
                .to_string(),
            "2026-05-03"
        );
    }

    #[test]
    fn string_and_float_amount_cells_both_parse() {
        let txs = parse_fixture("revolut-edge.xls");
        assert_eq!(
            txs.iter()
                .find(|t| t.payee == "String amount")
                .unwrap()
                .amount,
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
