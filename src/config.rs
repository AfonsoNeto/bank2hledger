use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level user config (`bank2hledger.toml`).
///
/// Relative paths are resolved against the directory containing the config
/// file, so a config in `~/.finance` keeps all its state inside `~/.finance`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the main hledger journal that imports append to.
    pub journal: PathBuf,
    /// Directory where raw bank exports are dropped (or written by fetchers).
    #[serde(default = "default_in_dir")]
    pub in_dir: PathBuf,
    /// Directory for normalized CSVs handed to `hledger import`.
    #[serde(default = "default_staging_dir")]
    pub staging_dir: PathBuf,
    /// Directory of per-account hledger CSV rules files.
    #[serde(default = "default_rules_dir")]
    pub rules_dir: PathBuf,
    pub accounts: Vec<AccountConfig>,
    /// API fetchers. Only present when the `fetch` feature is enabled.
    #[cfg(feature = "fetch")]
    #[serde(default)]
    pub fetchers: Vec<FetcherConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    /// Short name identifying this account in file names, rules files, and
    /// fetcher config. e.g. `monzo-personal`.
    pub name: String,
    /// Which built-in profile parses this account's exports.
    pub profile: String,
    /// The hledger account for the bank side of postings.
    pub hledger_account: String,
    /// Extra spec required by the `generic_csv` profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generic: Option<GenericCsvSpec>,
}

/// Declarative description of an arbitrary bank CSV, for the `generic_csv`
/// profile. Column references are header names (when `has_header`) or
/// zero-based column indices as strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericCsvSpec {
    #[serde(default = "default_true")]
    pub has_header: bool,
    #[serde(default)]
    pub delimiter: char,
    pub date_column: String,
    /// chrono strftime format of the date column.
    pub date_format: String,
    pub description_column: String,
    /// Signed amount column ("123.45" / "-8.75"). Mutually exclusive with
    /// `amount_in_column` + `amount_out_column`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_in_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_out_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_column: Option<String>,
    /// Fixed currency, if the CSV has no currency column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency_column: Option<String>,
    /// Only import rows whose `state`/`status` column equals this (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_accepted: Option<Vec<String>>,
}

#[cfg(feature = "fetch")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FetcherConfig {
    Monzo(MonzoFetcherConfig),
    Wise(WiseFetcherConfig),
}

#[cfg(feature = "fetch")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonzoFetcherConfig {
    /// The [[accounts]] entry this fetcher pulls for.
    pub account: String,
    /// Local TCP port for the OAuth redirect. You must register
    /// `http://localhost:<port>` as your client's redirect URL at
    /// developers.monzo.com.
    #[serde(default = "default_redirect_port")]
    pub redirect_port: u16,
    /// OAuth client id (not secret — safe in config).
    pub client_id: String,
    #[serde(default = "default_monzo_api")]
    pub base_url: String,
    #[serde(default = "default_monzo_auth")]
    pub auth_url: String,
}

#[cfg(feature = "fetch")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiseFetcherConfig {
    /// The [[accounts]] entry this fetcher pulls for.
    pub account: String,
    /// One fetcher entry per currency your balance is held in.
    pub currencies: Vec<String>,
    /// Wise profile id, discovered during `auth` and stored in fetcher state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<u64>,
    #[serde(default = "default_wise_api")]
    pub base_url: String,
}

fn default_in_dir() -> PathBuf {
    PathBuf::from("in")
}
fn default_staging_dir() -> PathBuf {
    PathBuf::from("staging")
}
fn default_rules_dir() -> PathBuf {
    PathBuf::from("rules")
}
fn default_true() -> bool {
    true
}
#[cfg(feature = "fetch")]
fn default_redirect_port() -> u16 {
    8765
}
#[cfg(feature = "fetch")]
fn default_monzo_api() -> String {
    "https://api.monzo.com".to_string()
}
#[cfg(feature = "fetch")]
fn default_monzo_auth() -> String {
    "https://auth.monzo.com".to_string()
}
#[cfg(feature = "fetch")]
fn default_wise_api() -> String {
    "https://api.wise.com".to_string()
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut config: Config =
            toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))?;
        config.resolve_paths(path.parent().unwrap_or(Path::new(".")))?;
        config.validate()?;
        Ok(config)
    }

    /// A commented template written by `init`.
    pub fn template() -> &'static str {
        r#"# bank2hledger configuration
# Docs: https://github.com/AfonsoNeto/bank2hledger
# Relative paths are resolved against this file's directory.

# Path to the hledger journal that imports append to.
journal = "2026.journal"

# Raw bank exports are dropped here (or written by `fetch`).
#in_dir = "in"

# Normalized CSVs handed to `hledger import`.
#staging_dir = "staging"

# Per-account hledger CSV rules files live here; edit them freely.
#rules_dir = "rules"

[[accounts]]
name = "monzo-personal"
profile = "monzo_csv"
hledger_account = "assets:banks:monzo:personal"

# [[accounts]]
# name = "revolut-personal"
# profile = "revolut_xls"
# hledger_account = "assets:banks:revolut:personal"

# A bank with no built-in profile: describe its CSV yourself.
# [[accounts]]
# name = "mybank"
# profile = "generic_csv"
# hledger_account = "assets:bank:mybank"
# [accounts.generic]
# date_column = "Date"
# date_format = "%d/%m/%Y"
# description_column = "Merchant"
# amount_column = "Amount"
# id_column = "Transaction ID"
# currency = "GBP"

# --- API fetchers (optional; built with the default `fetch` feature) ---
#
# Monzo: register a confidential OAuth client at https://developers.monzo.com
# with redirect URL http://localhost:8765, then run `bank2hledger auth monzo`.
# [[fetchers]]
# type = "monzo"
# account = "monzo-personal"
# client_id = "oauth2client_..."
# redirect_port = 8765

# Wise: create a personal API token at https://wise.com/settings/api-tokens,
# then run `bank2hledger auth wise`.
# [[fetchers]]
# type = "wise"
# account = "wise-personal"
# currencies = ["GBP", "EUR"]
"#
    }

    fn resolve_paths(&mut self, base: &Path) -> Result<()> {
        let resolve = |p: &Path| -> PathBuf {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                base.join(p)
            }
        };
        self.journal = resolve(&self.journal);
        self.in_dir = resolve(&self.in_dir);
        self.staging_dir = resolve(&self.staging_dir);
        self.rules_dir = resolve(&self.rules_dir);
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if self.accounts.is_empty() {
            bail!("config has no [[accounts]] entries");
        }
        let mut seen = std::collections::HashSet::new();
        for a in &self.accounts {
            if !seen.insert(a.name.as_str()) {
                bail!("duplicate account name: {}", a.name);
            }
            match a.profile.as_str() {
                "monzo_csv" | "revolut_xls" | "wise_csv" | "aqua_pdf" => {}
                "generic_csv" if a.generic.is_some() => {}
                "generic_csv" => bail!(
                    "account '{}': profile 'generic_csv' requires an [accounts.generic] spec",
                    a.name
                ),
                other => bail!(
                    "account '{}': unknown profile '{}' (known: monzo_csv, revolut_xls, wise_csv, aqua_pdf, generic_csv)",
                    a.name,
                    other
                ),
            }
        }
        #[cfg(feature = "fetch")]
        for f in &self.fetchers {
            let acct = f.account();
            if !self.accounts.iter().any(|a| a.name == acct) {
                bail!("fetcher references unknown account '{}'", acct);
            }
        }
        Ok(())
    }

    pub fn account(&self, name: &str) -> Result<&AccountConfig> {
        self.accounts
            .iter()
            .find(|a| a.name == name)
            .with_context(|| format!("unknown account '{}'", name))
    }
}

#[cfg(feature = "fetch")]
impl FetcherConfig {
    pub fn account(&self) -> &str {
        match self {
            FetcherConfig::Monzo(c) => &c.account,
            FetcherConfig::Wise(c) => &c.account,
        }
    }
}
