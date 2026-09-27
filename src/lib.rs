//! bank2hledger — import bank transaction exports into an hledger journal
//! with dedup, categorization rules, and a review-before-approve workflow.
//!
//! Library layout mirrors the CLI: [`config`] (user's TOML), [`model`]
//! (normalized transactions), [`profiles`] (bank export parsers), [`rules`]
//! (hledger CSV rules generation), [`engine`] (import pipeline), [`status`]
//! (balance reporting), and — behind the `fetch` feature — [`fetchers`]
//! (Monzo/Wise API pullers).

pub mod cli;
pub mod config;
pub mod engine;
pub mod model;
pub mod profiles;
pub mod rules;
pub mod status;

#[cfg(feature = "fetch")]
pub mod fetchers;
