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

#[cfg(test)]
mod tests {
    use super::*;

    fn tx(id: Option<&str>, payee: &str, amount: &str, currency: &str) -> Transaction {
        Transaction {
            date: chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            payee: payee.to_string(),
            amount: rust_decimal::Decimal::from_str_exact(amount).unwrap(),
            currency: currency.to_string(),
            account: "assets:bank:test".to_string(),
            external_id: id.map(String::from),
            notes: None,
        }
    }

    #[test]
    fn dedup_key_prefers_bank_id() {
        let t = tx(Some("tx_123"), "Coffee", "-3.80", "GBP");
        assert_eq!(t.dedup_key(), "id:tx_123");
    }

    #[test]
    fn dedup_key_synthetic_is_stable_and_discriminating() {
        let a = tx(None, "Coffee", "-3.80", "GBP");
        let b = tx(None, "Coffee", "-3.80", "GBP");
        let c = tx(None, "Coffee", "-3.80", "EUR");
        let d = tx(None, "Coffee", "-4.00", "GBP");
        let e = tx(None, "Other shop", "-3.80", "GBP");
        assert_eq!(a.dedup_key(), b.dedup_key());
        // Two identical coffees on the same day must NOT collapse into one.
        let a2 = tx(None, "Coffee", "-3.80", "GBP");
        assert_eq!(a.dedup_key(), a2.dedup_key());
        assert_ne!(a.dedup_key(), c.dedup_key());
        assert_ne!(a.dedup_key(), d.dedup_key());
        assert_ne!(a.dedup_key(), e.dedup_key());
    }

    #[test]
    fn dedup_key_normalizes_trailing_zeros() {
        let a = tx(None, "Coffee", "-3.80", "GBP");
        let b = tx(None, "Coffee", "-3.8", "GBP");
        assert_eq!(a.amount.normalize(), b.amount.normalize());
        assert_eq!(a.dedup_key(), b.dedup_key());
    }

    #[test]
    fn display_is_human_readable() {
        let t = tx(Some("tx_1"), "Coffee", "-3.8", "GBP");
        let s = t.to_string();
        assert!(s.contains("2026-05-01"), "{s}");
        assert!(s.contains("\"Coffee\""), "{s}");
        assert!(s.contains("-3.8GBP"), "{s}");
        assert!(s.contains("assets:bank:test"), "{s}");
    }
}
