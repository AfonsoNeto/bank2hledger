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
