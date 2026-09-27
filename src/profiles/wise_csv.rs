//! Wise statement CSV (from the website: Statement → Export → CSV, or written
//! by the `wise` API fetcher).
//!
//! The export format has drifted over the years, so columns are matched by
//! header *name* with several accepted spellings; anything else fails loudly.

use std::str::FromStr;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::config::AccountConfig;
use crate::model::Transaction;

const ID_HEADERS: [&str; 3] = ["transferwise id", "id", "transaction id"];
const DATE_HEADERS: [&str; 1] = ["date"];
const AMOUNT_HEADERS: [&str; 2] = ["amount", "amount in"];
const CURRENCY_HEADERS: [&str; 2] = ["currency", "amount currency"];
const DESCRIPTION_HEADERS: [&str; 3] = ["description", "details", "merchant"];

pub fn parse(account: &AccountConfig, bytes: &[u8]) -> Result<Vec<Transaction>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_reader(bytes);
    let headers = reader
        .headers()
        .cloned()
        .context("wise_csv: no header row")?;

    let col = |names: &[&str], label: &str| -> Result<usize> {
        names
            .iter()
            .find_map(|n| {
                headers
                    .iter()
                    .position(|h| h.trim().eq_ignore_ascii_case(n))
            })
            .with_context(|| {
                format!(
                    "wise_csv: no column matching {} (headers: {:?})",
                    label, headers
                )
            })
    };
    let c_id = col(&ID_HEADERS, "transaction id").ok();
    let c_date = col(&DATE_HEADERS, "date")?;
    let c_amount = col(&AMOUNT_HEADERS, "amount")?;
    let c_currency = col(&CURRENCY_HEADERS, "currency")?;
    let c_desc = col(&DESCRIPTION_HEADERS, "description")?;

    let mut txs = Vec::new();
    for (i, row) in reader.records().enumerate() {
        let row = row.with_context(|| format!("wise_csv: row {}", i + 2))?;
        let get = |idx: usize| row.get(idx).unwrap_or("").trim().to_string();
        let date = NaiveDate::parse_from_str(&get(c_date), "%Y-%m-%d %H:%M:%S")
            .or_else(|_| NaiveDate::parse_from_str(&get(c_date), "%Y-%m-%d"))
            .or_else(|_| NaiveDate::parse_from_str(&get(c_date), "%d-%m-%Y"))
            .or_else(|_| NaiveDate::parse_from_str(&get(c_date), "%d/%m/%Y"))
            .with_context(|| format!("wise_csv: row {}: bad date '{}'", i + 2, get(c_date)))?;
        let amount = Decimal::from_str(&get(c_amount))
            .with_context(|| format!("wise_csv: row {}: bad amount '{}'", i + 2, get(c_amount)))?;
        if amount == Decimal::ZERO && get(c_desc).is_empty() {
            continue;
        }
        let mut payee = get(c_desc);
        if payee.is_empty() {
            payee = "Wise transfer".to_string();
        }
        txs.push(Transaction {
            date,
            payee,
            amount,
            currency: get(c_currency),
            account: account.hledger_account.clone(),
            external_id: c_id.map(get).filter(|s| !s.is_empty()),
            notes: None,
        });
    }
    Ok(txs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AccountConfig;
    use rust_decimal::Decimal;

    fn acct() -> AccountConfig {
        AccountConfig {
            name: "w".into(),
            profile: "wise_csv".into(),
            hledger_account: "assets:bank:w".into(),
            generic: None,
        }
    }

    fn parse_str(csv: &str) -> Result<Vec<Transaction>> {
        parse(&acct(), csv.as_bytes())
    }

    #[test]
    fn standard_headers_parse() {
        let csv = "TransferWise ID,Date,Amount,Currency,Description\n\
                   W1,2026-05-02 09:15:00,250.00,GBP,Add money\n\
                   W2,2026-05-03 14:20:00,-18.60,GBP,Corner Grocer\n";
        let txs = parse_str(csv).unwrap();
        assert_eq!(txs.len(), 2);
        assert_eq!(txs[0].external_id.as_deref(), Some("W1"));
        assert_eq!(txs[0].amount, Decimal::from_str_exact("250.00").unwrap());
        assert_eq!(txs[1].amount, Decimal::from_str_exact("-18.60").unwrap());
    }

    #[test]
    fn alternate_header_spellings_are_accepted() {
        // Older/other Wise exports: "id", "details", "amount currency".
        let csv = "id,date,amount,amount currency,details\n\
                   W1,2026-05-02,10.00,EUR,Shop\n";
        let txs = parse_str(csv).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].external_id.as_deref(), Some("W1"));
        assert_eq!(txs[0].currency, "EUR");
        assert_eq!(txs[0].payee, "Shop");
    }

    #[test]
    fn merchant_header_variant() {
        let csv = "TransferWise ID,Date,Amount,Currency,Merchant\nW1,02/05/2026,1.00,GBP,Corner\n";
        let txs = parse_str(csv).unwrap();
        assert_eq!(txs[0].payee, "Corner");
        assert_eq!(txs[0].date.to_string(), "2026-05-02");
    }

    #[test]
    fn date_format_variants() {
        for (d, want) in [
            ("2026-05-02 09:15:00", "2026-05-02"),
            ("2026-05-02", "2026-05-02"),
            ("02-05-2026", "2026-05-02"),
            ("02/05/2026", "2026-05-02"),
        ] {
            let csv =
                format!("TransferWise ID,Date,Amount,Currency,Description\nW1,{d},1.00,GBP,X\n");
            assert_eq!(
                parse_str(&csv).unwrap()[0].date.to_string(),
                want,
                "input {d}"
            );
        }
    }

    #[test]
    fn empty_description_becomes_wise_transfer() {
        let csv = "TransferWise ID,Date,Amount,Currency,Description\nW1,2026-05-02,1.00,GBP,\n";
        assert_eq!(parse_str(csv).unwrap()[0].payee, "Wise transfer");
    }

    #[test]
    fn zero_amount_and_blank_description_rows_are_skipped() {
        let csv = "TransferWise ID,Date,Amount,Currency,Description\n\
                   W1,2026-05-02,0.00,GBP,\n\
                   W2,2026-05-03,5.00,GBP,Real\n";
        let txs = parse_str(csv).unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].payee, "Real");
    }

    #[test]
    fn unrecognized_headers_fail_loudly_listing_them() {
        let err = parse_str("Foo,Bar\n1,2\n").unwrap_err().to_string();
        assert!(err.contains("no column matching date"), "{err}");
        assert!(err.contains("Foo"), "{err}");
    }

    #[test]
    fn bad_date_and_amount_report_row_numbers() {
        let err = parse_str(
            "TransferWise ID,Date,Amount,Currency,Description\nW1,not-a-date,1.00,GBP,X\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("row 2") && err.contains("bad date"), "{err}");

        let err = parse_str(
            "TransferWise ID,Date,Amount,Currency,Description\nW1,2026-05-02,abc,GBP,X\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("row 2") && err.contains("bad amount"), "{err}");
    }
}
