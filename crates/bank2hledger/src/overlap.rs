//! Overlap detection: flag staged transactions that look like transactions
//! already in the journal.
//!
//! Hand-entered history is invisible to the tool's id-based dedup, and
//! hledger 1.52's import does not compare CSV records against journal
//! content either — so re-importing an already-logged period silently
//! duplicates it. This module is the safety net: for each staged row, find
//! the most similar existing journal transaction and warn.
//!
//! A single signal is not enough (amounts drift with FX/fees, payees are
//! spelled differently, dates slip by a few clearing days), so candidates
//! are scored compositely:
//!
//! - **date**: within `max_days` (hard gate), linear decay
//! - **amount**: same currency, relative difference ≤ `amount_tolerance`
//!   (hard gate), linear decay
//! - **payee**: token Jaccard similarity (catches wording differences)
//! - **category**: the account the rules would assign vs the account the
//!   existing entry posted to — exact match 1.0, shared two-level prefix 0.5
//!
//! Score = weighted sum; ≥ `threshold` is flagged. Advisory only: warnings
//! never drop transactions, the user decides (optionally with `--drop-flagged`).

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::engine::hledger_cmd;

/// Tunable knobs. Deliberately not user-configurable yet: defaults were
/// chosen against real recurring-transaction data, and the tests pin them.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Existing transactions further than this many days from a staged row
    /// can never match it (statement clearing lag).
    pub max_days: i64,
    /// Relative amount difference tolerated (0.05 = 5%: FX spreads, fees).
    pub amount_tolerance: f64,
    /// Payee similarity that counts as an identifying signal (one shared
    /// token of ≥3 characters ≈ 1/3).
    pub min_payee_signal: f64,
    /// Category agreement that counts as an identifying signal (top-level +
    /// leaf match = 0.5, exact = 1.0).
    pub min_category_signal: f64,
    pub weight_amount: f64,
    pub weight_date: f64,
    pub weight_payee: f64,
    pub weight_category: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            max_days: 4,
            amount_tolerance: 0.05,
            min_payee_signal: 1.0 / 3.0,
            min_category_signal: 0.5,
            weight_amount: 0.35,
            weight_date: 0.25,
            weight_payee: 0.25,
            weight_category: 0.15,
        }
    }
}

/// A transaction in comparable form: the bank-side posting's amount plus the
/// counter-accounts the entry moved money to/from.
#[derive(Debug, Clone)]
pub struct Tx {
    pub date: NaiveDate,
    pub payee: String,
    pub amount: Decimal,
    pub currency: String,
    pub counter_accounts: Vec<String>,
}

/// One above-threshold match between a staged row and an existing entry.
#[derive(Debug, Clone)]
pub struct Warning {
    pub staged: Tx,
    pub staged_key: String,
    pub existing: Tx,
    pub score: f64,
    pub score_amount: f64,
    pub score_date: f64,
    pub score_payee: f64,
    pub score_category: f64,
}

impl Warning {
    pub fn summary(&self) -> String {
        format!(
            "{} {} {}{}  ←  {} {} {}{}  (score {:.2}: amount {:.2}, date {:.2}, payee {:.2}, category {:.2})",
            self.staged.date,
            self.staged.payee,
            self.staged.amount.normalize(),
            self.staged.currency,
            self.existing.date,
            self.existing.payee,
            self.existing.amount.normalize(),
            self.existing.currency,
            self.score,
            self.score_amount,
            self.score_date,
            self.score_payee,
            self.score_category,
        )
    }

    /// The existing side of the pair, as shown in an interactive choice menu.
    pub fn candidate_line(&self) -> String {
        format!(
            "{} {} {}{}  (score {:.2}: amount {:.2}, date {:.2}, payee {:.2}, category {:.2})",
            self.existing.date,
            self.existing.payee,
            self.existing.amount.normalize(),
            self.existing.currency,
            self.score,
            self.score_amount,
            self.score_date,
            self.score_payee,
            self.score_category,
        )
    }
}

/// Every above-threshold match for one staged row, best first. Staged rows
/// with no matches are omitted entirely.
#[derive(Debug, Clone)]
pub struct RowMatches {
    pub staged: Tx,
    pub staged_key: String,
    pub matches: Vec<Warning>,
}

// --- hledger `print -O json` model (only the fields we need) ---

#[derive(Debug, Deserialize)]
struct JsonTx {
    #[serde(rename = "tdate")]
    date: NaiveDate,
    #[serde(rename = "tdescription")]
    description: String,
    #[serde(rename = "tpostings")]
    postings: Vec<JsonPosting>,
}

#[derive(Debug, Deserialize)]
struct JsonPosting {
    #[serde(rename = "paccount")]
    account: String,
    #[serde(rename = "pamount")]
    amount: Vec<JsonAmount>,
}

#[derive(Debug, Deserialize)]
struct JsonAmount {
    #[serde(rename = "acommodity")]
    commodity: String,
    #[serde(rename = "aquantity")]
    quantity: JsonQuantity,
}

#[derive(Debug, Deserialize)]
struct JsonQuantity {
    #[serde(rename = "decimalMantissa")]
    mantissa: i64,
    #[serde(rename = "decimalPlaces")]
    places: u32,
}

fn print_json(args: &[&str]) -> Result<Vec<JsonTx>> {
    let mut cmd = hledger_cmd();
    cmd.arg("print").arg("-O").arg("json");
    for a in args {
        cmd.arg(a);
    }
    let output = cmd
        .output()
        .context("running hledger print for overlap detection")?;
    if !output.status.success() {
        anyhow::bail!(
            "hledger print failed during overlap detection:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("parsing hledger print JSON output")
}

/// View a printed transaction from the perspective of `bank_account`: the
/// posting on that account supplies (amount, currency); every other posting
/// is a counter-account. Returns `None` when the entry doesn't touch the
/// account (not comparable) or carries no amount in a comparable commodity.
fn perspective(tx: JsonTx, bank_account: &str) -> Option<Tx> {
    let mut bank_amount = None;
    let mut counters = Vec::new();
    for p in tx.postings {
        if p.account == bank_account {
            bank_amount = Some(p.amount);
        } else {
            counters.push(p.account);
        }
    }
    let amounts = bank_amount?;
    // Multi-commodity postings are possible; the bank-side posting of
    // interest is whichever matches the dominant (first) commodity.
    let amount = amounts.first()?;
    Some(Tx {
        date: tx.date,
        payee: tx.description,
        amount: Decimal::new(amount.quantity.mantissa, amount.quantity.places),
        currency: amount.commodity.clone(),
        counter_accounts: counters,
    })
}

/// Compare staged rows against the existing journal. Returns one
/// [`RowMatches`] per staged row that has at least one above-threshold
/// match, in staging order, candidates sorted best-first.
pub fn find_overlaps(
    journal: &Path,
    staging_csv: &Path,
    rules: &Path,
    bank_account: &str,
    staged_keys: &[String],
    params: &Params,
) -> Result<Vec<RowMatches>> {
    // Predicted view of the staged rows: this runs the *user's* rules via
    // hledger itself, so counter-accounts are exactly what import will post.
    let staged_json = print_json(&[
        "-f",
        staging_csv.to_str().context("staging path not UTF-8")?,
        "--rules",
        rules.to_str().context("rules path not UTF-8")?,
    ])?;
    // The staging rows are date-sorted and their dedup keys collected in
    // that same order; hledger print preserves input order, so zipping the
    // printed transactions with the keys keeps them aligned. A length
    // mismatch would mean a parsing surprise — be conservative and skip
    // flagging entirely rather than mis-attribute warnings.
    let staged: Vec<Tx> = staged_json
        .into_iter()
        .filter_map(|t| perspective(t, bank_account))
        .collect();
    if staged.len() != staged_keys.len() {
        return Ok(vec![]);
    }

    let journal_json = print_json(&["-f", journal.to_str().context("journal path not UTF-8")?])?;
    let existing: Vec<Tx> = journal_json
        .into_iter()
        .filter_map(|t| perspective(t, bank_account))
        .collect();

    let mut rows = Vec::new();
    for (staged_tx, key) in staged.iter().zip(staged_keys.iter()) {
        let mut matches: Vec<Warning> = existing
            .iter()
            .filter_map(|ex| score_pair(staged_tx, key, ex, params))
            .collect();
        if matches.is_empty() {
            continue;
        }
        matches.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        rows.push(RowMatches {
            staged: staged_tx.clone(),
            staged_key: key.clone(),
            matches,
        });
    }
    Ok(rows)
}

/// Score one pair; `Some` when both hard gates pass and the composite score
/// reaches the threshold.
fn score_pair(staged: &Tx, staged_key: &str, existing: &Tx, params: &Params) -> Option<Warning> {
    if staged.currency != existing.currency {
        return None;
    }
    let day_diff =
        ((staged.date.num_days_from_ce() - existing.date.num_days_from_ce()).abs()) as i64;
    if day_diff > params.max_days {
        return None;
    }
    let denom = staged.amount.abs().max(existing.amount.abs());
    if denom.is_zero() {
        return None;
    }
    let rel_diff = ((staged.amount - existing.amount).abs() / denom).to_f64()?;
    if rel_diff > params.amount_tolerance {
        return None;
    }

    let score_amount = 1.0 - rel_diff / params.amount_tolerance;
    let score_date = 1.0 - day_diff as f64 / (params.max_days as f64 + 1.0);
    let score_payee = payee_similarity(&staged.payee, &existing.payee);
    let score_category = category_similarity(&staged.counter_accounts, &existing.counter_accounts);

    // Beyond the amount+date gates, require at least one identifying
    // signal: similar wording OR agreeing category. Same amount on the same
    // day alone matches every same-priced coffee and must stay silent.
    if score_payee < params.min_payee_signal && score_category < params.min_category_signal {
        return None;
    }

    let score = params.weight_amount * score_amount
        + params.weight_date * score_date
        + params.weight_payee * score_payee
        + params.weight_category * score_category;
    Some(Warning {
        staged: staged.clone(),
        staged_key: staged_key.to_string(),
        existing: existing.clone(),
        score,
        score_amount,
        score_date,
        score_payee,
        score_category,
    })
}

/// Token Jaccard on lowercased alphanumeric runs of length ≥ 3. Handles the
/// common wording drift: "BIG LANDLORD LTD" vs "Big Landlord" share "landlord".
fn payee_similarity(a: &str, b: &str) -> f64 {
    let tokens = |s: &str| -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| t.chars().count() >= 3)
            .map(String::from)
            .collect()
    };
    let (a, b) = (tokens(a), tokens(b));
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(&b).count() as f64;
    let union = (a.len() + b.len()) as f64 - inter;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// Counter-account agreement: exact match 1.0; 0.5 when top-level and leaf
/// components match (e.g. `expenses:rent` vs `expenses:home:rent`, or
/// `expenses:fuel` vs `expenses:car:fuel` — the taxonomy near-duplicates that
/// actually occur in hand-maintained journals); else 0. Splits take the best
/// counter-account.
fn category_similarity(staged: &[String], existing: &[String]) -> f64 {
    staged
        .iter()
        .map(|s| {
            existing
                .iter()
                .map(|e| {
                    if s == e {
                        1.0
                    } else {
                        let sp: Vec<&str> = s.split(':').collect();
                        let ep: Vec<&str> = e.split(':').collect();
                        if sp[0] == ep[0] && sp.last() == ep.last() {
                            0.5
                        } else {
                            0.0
                        }
                    }
                })
                .fold(0.0, f64::max)
        })
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn tx(date: NaiveDate, payee: &str, amount: &str, counters: &[&str]) -> Tx {
        Tx {
            date,
            payee: payee.into(),
            amount: Decimal::from_str_exact(amount).unwrap(),
            currency: "GBP".into(),
            counter_accounts: counters.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn params() -> Params {
        Params::default()
    }

    #[test]
    fn exact_duplicate_scores_full() {
        let a = tx(
            d(2026, 9, 1),
            "Big Landlord",
            "-1350.00",
            &["expenses:home:rent"],
        );
        let b = tx(
            d(2026, 9, 1),
            "Big Landlord",
            "-1350.00",
            &["expenses:home:rent"],
        );
        let w = score_pair(&a, "k", &b, &params()).unwrap();
        assert!(w.score > 0.95, "{}", w.score);
    }

    #[test]
    fn different_description_same_category_still_flags() {
        // The user's scenario: wording drifts, amount drifts a little,
        // category agrees.
        let a = tx(
            d(2026, 9, 1),
            "BIG LANDLORD LTD",
            "-1347.50",
            &["expenses:home:rent"],
        );
        let b = tx(
            d(2026, 9, 3),
            "Big Landlord",
            "-1350.00",
            &["expenses:home:rent"],
        );
        let w = score_pair(&a, "k", &b, &params()).unwrap();
        assert!(w.score >= 0.7, "{}", w.score);
        assert!(w.score_payee > 0.3 && w.score_payee < 1.0);
        assert_eq!(w.score_category, 1.0);
    }

    #[test]
    fn near_prefix_category_counts_partially() {
        assert_eq!(
            category_similarity(
                &["expenses:rent".to_string()],
                &["expenses:home:rent".to_string()]
            ),
            0.5
        );
        // Through the full scorer: with a ≥3-char payee the partial category
        // credit plus agreement on amount/date/payee crosses the threshold.
        let a = tx(d(2026, 9, 1), "Landlord", "-10.00", &["expenses:rent"]);
        let b = tx(d(2026, 9, 1), "Landlord", "-10.00", &["expenses:home:rent"]);
        let w = score_pair(&a, "k", &b, &params()).unwrap();
        assert_eq!(w.score_category, 0.5);
    }

    #[test]
    fn amount_beyond_tolerance_never_matches() {
        let a = tx(
            d(2026, 9, 1),
            "Big Landlord",
            "-1350.00",
            &["expenses:home:rent"],
        );
        let b = tx(
            d(2026, 9, 1),
            "Big Landlord",
            "-1000.00",
            &["expenses:home:rent"],
        );
        assert!(score_pair(&a, "k", &b, &params()).is_none());
    }

    #[test]
    fn date_beyond_window_never_matches() {
        let a = tx(
            d(2026, 9, 1),
            "Big Landlord",
            "-1350.00",
            &["expenses:home:rent"],
        );
        let b = tx(
            d(2026, 9, 20),
            "Big Landlord",
            "-1350.00",
            &["expenses:home:rent"],
        );
        assert!(score_pair(&a, "k", &b, &params()).is_none());
    }

    #[test]
    fn different_category_and_payee_stays_below_threshold() {
        // Same amount and day, but nothing else in common: a genuinely new
        // transaction that happens to cost the same.
        let a = tx(
            d(2026, 9, 1),
            "WEIRD UNKNOWN SHOP",
            "-1350.00",
            &["expenses:other"],
        );
        let b = tx(
            d(2026, 9, 1),
            "Coffee Bar 9",
            "-1350.00",
            &["expenses:foods:restaurants"],
        );
        assert!(score_pair(&a, "k", &b, &params()).is_none());
    }

    #[test]
    fn currency_mismatch_never_matches() {
        let a = tx(d(2026, 9, 1), "X", "-10.00", &["expenses:other"]);
        let mut b = tx(d(2026, 9, 1), "X", "-10.00", &["expenses:other"]);
        b.currency = "EUR".into();
        assert!(score_pair(&a, "k", &b, &params()).is_none());
    }

    #[test]
    fn payee_similarity_handles_wording_drift() {
        // {big, landlord, ltd} ∩ {big, landlord} / union(3) = 2/3
        assert!((payee_similarity("BIG LANDLORD LTD", "Big Landlord") - 2.0 / 3.0).abs() < 1e-9);
        // Jaccard penalizes extra tokens: {tesco, stores, 1234} vs {tesco}.
        assert!((payee_similarity("tesco stores 1234", "Tesco") - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(payee_similarity("abc", "xyz"), 0.0);
    }
}
