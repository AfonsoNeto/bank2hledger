//! `generic_csv` profile: describe any bank's CSV in the user's config.
//!
//! This is the escape hatch for banks without a built-in profile, and the
//! path from "my config works" to "I contributed a profile" (docs/adding-a-profile.md).

use std::str::FromStr;

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::config::AccountConfig;
use crate::model::Transaction;

pub fn parse(account: &AccountConfig, bytes: &[u8]) -> Result<Vec<Transaction>> {
    let spec = account
        .generic
        .clone()
        .context("generic_csv: config missing [accounts.generic] spec")?;

    let reader = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(spec.has_header)
        .delimiter(spec.delimiter as u8)
        .from_reader(bytes);
    let mut reader = reader;
    let headers: Vec<String> = if spec.has_header {
        reader
            .headers()
            .cloned()
            .context("generic_csv: no header row")?
            .iter()
            .map(|h| h.trim().to_string())
            .collect()
    } else {
        Vec::new()
    };

    // Column references: header name or numeric index.
    let resolve = |name: &str| -> Result<usize> {
        if let Ok(idx) = name.parse::<usize>() {
            if !headers.is_empty() && idx >= headers.len() {
                bail!("generic_csv: column index {} out of range", idx);
            }
            return Ok(idx);
        }
        headers
            .iter()
            .position(|h| h.eq_ignore_ascii_case(name))
            .with_context(|| format!("generic_csv: column '{}' not found", name))
    };
    let c_date = resolve(&spec.date_column)?;
    let c_desc = resolve(&spec.description_column)?;
    let c_amount = spec
        .amount_column
        .as_ref()
        .map(|c| resolve(c))
        .transpose()?;
    let c_in = spec
        .amount_in_column
        .as_ref()
        .map(|c| resolve(c))
        .transpose()?;
    let c_out = spec
        .amount_out_column
        .as_ref()
        .map(|c| resolve(c))
        .transpose()?;
    if c_amount.is_none() && (c_in.is_none() || c_out.is_none()) {
        bail!(
            "generic_csv: set either `amount_column` (signed) or both \
             `amount_in_column` + `amount_out_column`"
        );
    }
    let c_id = spec.id_column.as_ref().map(|c| resolve(c)).transpose()?;
    let c_cur = spec
        .currency_column
        .as_ref()
        .map(|c| resolve(c))
        .transpose()?;
    let c_status = spec
        .status_column
        .as_ref()
        .map(|c| resolve(c))
        .transpose()?;

    let mut txs = Vec::new();
    for (i, row) in reader.records().enumerate() {
        let row = row.with_context(|| format!("generic_csv: row {}", i + 2))?;
        let get = |idx: usize| row.get(idx).unwrap_or("").trim().to_string();

        if let Some(sc) = c_status {
            let status = get(sc);
            let accepted = spec.status_accepted.as_deref();
            let keep = match accepted {
                Some(list) => list.iter().any(|s| status.eq_ignore_ascii_case(s)),
                None => !status.is_empty(),
            };
            if !keep {
                continue;
            }
        }

        let date =
            NaiveDate::parse_from_str(&get(c_date), &spec.date_format).with_context(|| {
                format!(
                    "generic_csv: row {}: date '{}' doesn't match format '{}'",
                    i + 2,
                    get(c_date),
                    spec.date_format
                )
            })?;
        let amount = if let Some(a) = c_amount {
            Decimal::from_str(&get(a))
                .with_context(|| format!("generic_csv: row {}: bad amount '{}'", i + 2, get(a)))?
        } else {
            let in_amt: Decimal = get(c_in.expect("checked above"))
                .parse()
                .unwrap_or(Decimal::ZERO);
            let out_amt: Decimal = get(c_out.expect("checked above"))
                .parse()
                .unwrap_or(Decimal::ZERO);
            in_amt - out_amt
        };
        if amount == Decimal::ZERO && get(c_desc).is_empty() {
            continue;
        }
        let currency = match c_cur {
            Some(c) => get(c),
            None => spec
                .currency
                .clone()
                .context("generic_csv: set either `currency` or `currency_column`")?,
        };

        txs.push(Transaction {
            date,
            payee: get(c_desc),
            amount,
            currency,
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
    use crate::config::{AccountConfig, GenericCsvSpec};
    use rust_decimal::Decimal;

    fn acct(spec: Option<GenericCsvSpec>) -> AccountConfig {
        AccountConfig {
            name: "g".into(),
            profile: "generic_csv".into(),
            hledger_account: "assets:bank:g".into(),
            generic: spec,
        }
    }

    fn base_spec() -> GenericCsvSpec {
        GenericCsvSpec {
            has_header: true,
            delimiter: ',',
            date_column: "Date".into(),
            date_format: "%d/%m/%Y".into(),
            description_column: "Merchant".into(),
            amount_column: Some("Amount".into()),
            amount_in_column: None,
            amount_out_column: None,
            id_column: None,
            currency: Some("GBP".into()),
            currency_column: None,
            status_column: None,
            status_accepted: None,
        }
    }

    fn run(spec: &GenericCsvSpec, csv: &str) -> Result<Vec<Transaction>> {
        parse(&acct(Some(spec.clone())), csv.as_bytes())
    }

    #[test]
    fn signed_amount_column() {
        let spec = base_spec();
        let txs = run(&spec, "Date,Merchant,Amount\n01/05/2026,Kiosk,-5.50\n02/05/2026,Refund,12.00\n").unwrap();
        assert_eq!(txs[0].amount, Decimal::from_str_exact("-5.50").unwrap());
        assert_eq!(txs[1].amount, Decimal::from_str_exact("12.00").unwrap());
        assert_eq!(txs[0].date.to_string(), "2026-05-01");
    }

    #[test]
    fn in_out_columns_compute_net_amount() {
        let mut spec = base_spec();
        spec.amount_column = None;
        spec.amount_in_column = Some("In".into());
        spec.amount_out_column = Some("Out".into());
        let txs = run(&spec, "Date,Merchant,In,Out\n01/05/2026,Kiosk,,5.50\n02/05/2026,Refund,12.00,\n").unwrap();
        assert_eq!(txs[0].amount, Decimal::from_str_exact("-5.50").unwrap());
        assert_eq!(txs[1].amount, Decimal::from_str_exact("12.00").unwrap());
    }

    #[test]
    fn in_without_out_column_is_rejected_upfront() {
        let mut spec = base_spec();
        spec.amount_column = None;
        spec.amount_in_column = Some("In".into());
        let err = run(&spec, "Date,Merchant,In\n").unwrap_err().to_string();
        assert!(err.contains("amount_in_column"), "{err}");
    }

    #[test]
    fn no_amount_configuration_is_rejected_upfront() {
        let mut spec = base_spec();
        spec.amount_column = None;
        let err = run(&spec, "Date,Merchant\n").unwrap_err().to_string();
        assert!(err.contains("amount_column"), "{err}");
    }

    #[test]
    fn numeric_column_indexes_work_with_and_without_headers() {
        let mut spec = base_spec();
        spec.date_column = "0".into();
        spec.description_column = "1".into();
        spec.amount_column = Some("2".into());
        let txs = run(&spec, "H1,H2,H3\n01/05/2026,Kiosk,-5.50\n").unwrap();
        assert_eq!(txs[0].payee, "Kiosk");

        let mut no_header = base_spec();
        no_header.has_header = false;
        no_header.date_column = "0".into();
        no_header.description_column = "1".into();
        no_header.amount_column = Some("2".into());
        let txs = run(&no_header, "01/05/2026,Kiosk,-5.50\n").unwrap();
        assert_eq!(txs[0].payee, "Kiosk");
    }

    #[test]
    fn custom_delimiter() {
        let mut spec = base_spec();
        spec.delimiter = ';';
        let txs = run(&spec, "Date;Merchant;Amount\n01/05/2026;Kiosk;-5.50\n").unwrap();
        assert_eq!(txs[0].payee, "Kiosk");
    }

    #[test]
    fn unknown_header_column_errors_clearly() {
        let spec = base_spec();
        let err = run(&spec, "Date,Shop,Amount\n01/05/2026,Kiosk,-1\n").unwrap_err().to_string();
        assert!(err.contains("column 'Merchant' not found"), "{err}");
    }

    #[test]
    fn out_of_range_index_is_rejected() {
        let mut spec = base_spec();
        spec.date_column = "9".into();
        let err = run(&spec, "Date,Merchant,Amount\n").unwrap_err().to_string();
        assert!(err.contains("out of range"), "{err}");
    }

    #[test]
    fn bad_date_reports_value_and_format() {
        let spec = base_spec();
        let err = run(&spec, "Date,Merchant,Amount\n2026-05-01,Kiosk,-1\n").unwrap_err().to_string();
        assert!(err.contains("2026-05-01"), "{err}");
        assert!(err.contains("%d/%m/%Y"), "{err}");
    }

    #[test]
    fn status_filter_keeps_only_accepted_states() {
        let mut spec = base_spec();
        spec.status_column = Some("State".into());
        spec.status_accepted = Some(vec!["COMPLETED".into()]);
        let csv = "Date,Merchant,Amount,State\n01/05/2026,A,-1,COMPLETED\n02/05/2026,B,-2,PENDING\n03/05/2026,C,-3,completed\n";
        let txs = run(&spec, csv).unwrap();
        assert_eq!(txs.len(), 2);
        assert_eq!(txs[0].payee, "A");
        assert_eq!(txs[1].payee, "C"); // case-insensitive acceptance
    }

    #[test]
    fn currency_from_column_or_fixed() {
        let mut spec = base_spec();
        spec.currency = None;
        spec.currency_column = Some("Ccy".into());
        let txs = run(&spec, "Date,Merchant,Amount,Ccy\n01/05/2026,Kiosk,-5,EUR\n").unwrap();
        assert_eq!(txs[0].currency, "EUR");

        let neither = base_spec();
        let mut neither = neither;
        neither.currency = None;
        let err = run(&neither, "Date,Merchant,Amount\n01/05/2026,Kiosk,-5\n").unwrap_err().to_string();
        assert!(err.contains("`currency` or `currency_column`"), "{err}");
    }

    #[test]
    fn id_column_enables_external_ids_and_blanks_are_dropped() {
        let mut spec = base_spec();
        spec.id_column = Some("Ref".into());
        let txs = run(&spec, "Date,Merchant,Amount,Ref\n01/05/2026,A,-1,X1\n02/05/2026,B,-2,\n").unwrap();
        assert_eq!(txs[0].external_id.as_deref(), Some("X1"));
        assert_eq!(txs[1].external_id, None);
    }

    #[test]
    fn missing_spec_is_a_clear_error() {
        let err = parse(&acct(None), b"Date,Merchant,Amount\n").unwrap_err().to_string();
        assert!(err.contains("missing [accounts.generic]"), "{err}");
    }
}
