//! Profiles parse raw bank exports into normalized [`Transaction`]s.
//!
//! A profile knows only how to read a bank's export *format* — column names,
//! date formats, amount conventions. It never decides hledger account names:
//! those come from the user's config and are applied by the caller.

pub mod aqua_pdf;
pub mod generic_csv;
pub mod monzo_csv;
pub mod revolut_xls;
pub mod wise_csv;

use anyhow::{bail, Context, Result};

use crate::config::AccountConfig;
use crate::model::Transaction;

/// Parse a raw export file for `account` into normalized transactions.
pub fn parse_file(account: &AccountConfig, path: &std::path::Path) -> Result<Vec<Transaction>> {
    let parse = |bytes: &[u8]| -> Result<Vec<Transaction>> {
        match account.profile.as_str() {
            "monzo_csv" => monzo_csv::parse(account, bytes),
            "revolut_xls" => revolut_xls::parse(account, path, bytes),
            "wise_csv" => wise_csv::parse(account, bytes),
            "aqua_pdf" => aqua_pdf::parse(account, path),
            "generic_csv" => generic_csv::parse(account, bytes),
            other => bail!("unknown profile '{}'", other),
        }
    };

    if account.profile == "aqua_pdf" {
        // PDF parser shells out to pdftotext; it never reads raw bytes.
        return aqua_pdf::parse(account, path);
    }

    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let txs = parse(&bytes).with_context(|| {
        format!(
            "parsing {} with profile '{}' — if the columns look right but parsing still fails, \
             the export format may have changed; please open an issue",
            path.display(),
            account.profile
        )
    })?;
    for tx in &txs {
        debug_assert_eq!(tx.account, account.hledger_account);
    }
    Ok(txs)
}

/// Files dropped into `in_dir` bind to an account by filename prefix:
/// `<name>.csv`, `<name>-anything.xls`, `<name>_anything.pdf`, etc.
/// This keeps a multi-account setup unambiguous — there is no guessing.
pub fn matches_account(file_name: &str, account_name: &str) -> bool {
    if file_name == account_name {
        return true;
    }
    file_name
        .strip_prefix(account_name)
        .is_some_and(|rest| rest.starts_with(['-', '_', '.']))
}
