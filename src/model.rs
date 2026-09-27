use std::fmt;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A bank transaction normalized from any source (file profile or API fetcher).
///
/// `account` is the *user's* hledger account name for the bank side of the
/// posting — it comes from their config, never from the tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub date: NaiveDate,
    pub payee: String,
    /// Signed amount in `currency` (positive = money in, negative = money out),
    /// as seen from the bank account.
    pub amount: Decimal,
    pub currency: String,
    pub account: String,
    /// Bank's unique transaction id, if the source provides one. Used for
    /// exact duplicate detection across re-dropped files and re-fetches.
    pub external_id: Option<String>,
    pub notes: Option<String>,
}

impl Transaction {
    /// Stable key used by the import engine's seen-ids state file.
    pub fn dedup_key(&self) -> String {
        match &self.external_id {
            Some(id) => format!("id:{}", id),
            None => format!(
                "synthetic:{}:{}:{}:{}",
                self.date.format("%Y-%m-%d"),
                self.payee,
                self.amount.normalize(),
                self.currency
            ),
        }
    }
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {:?} {}{} [{}]",
            self.date.format("%Y-%m-%d"),
            self.payee,
            self.amount.normalize(),
            self.currency,
            self.account
        )
    }
}

/// Columns of the normalized staging CSV the engine hands to `hledger import`.
/// The generated rules files reference these by position, so the order here
/// is load-bearing.
pub const STAGING_COLUMNS: [&str; 5] = ["date", "description", "amount", "currency", "id"];
