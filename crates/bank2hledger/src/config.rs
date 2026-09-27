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

# Built-in profiles: monzo_csv, revolut_xls, wise_csv, aqua_pdf, generic_csv

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
            validate_account_name(&a.name)?;
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

/// Account names become filename components (`<name>.rules`,
/// `<name>-monzo-<stamp>.csv`, `<name>.seen`). Restrict them to a safe
/// charset so a mistyped or hostile config cannot write outside the data
/// directories (path traversal via `../`, absolute paths, dotfiles).
fn validate_account_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        && !name.contains("..");
    if ok {
        Ok(())
    } else {
        bail!(
            "invalid account name '{}': use letters, digits, '-', '_', '.' or spaces \
             (it is used as a filename component)",
            name
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_config(dir: &std::path::Path, body: &str) -> PathBuf {
        let path = dir.join("bank2hledger.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    fn minimal_body(extra: &str) -> String {
        format!(
            "journal = \"j.journal\"\n[[accounts]]\nname = \"a\"\nprofile = \"monzo_csv\"\n\
             hledger_account = \"assets:a\"\n{extra}"
        )
    }

    #[test]
    fn loads_and_resolves_relative_paths_against_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), &minimal_body(""));
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.journal, dir.path().join("j.journal"));
        assert_eq!(cfg.in_dir, dir.path().join("in"));
        assert_eq!(cfg.staging_dir, dir.path().join("staging"));
        assert_eq!(cfg.rules_dir, dir.path().join("rules"));
    }

    #[test]
    fn absolute_paths_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let body = "journal = \"/tmp/somewhere/j.journal\"\n\
                    in_dir = \"/tmp/somewhere/in\"\n[[accounts]]\nname = \"a\"\n\
                    profile = \"monzo_csv\"\nhledger_account = \"assets:a\"\n";
        let cfg = Config::load(&write_config(dir.path(), body)).unwrap();
        assert_eq!(cfg.journal, PathBuf::from("/tmp/somewhere/j.journal"));
        assert_eq!(cfg.in_dir, PathBuf::from("/tmp/somewhere/in"));
    }

    #[test]
    fn missing_file_is_a_clear_error() {
        let err = Config::load(Path::new("/nonexistent/bank2hledger.toml")).unwrap_err();
        assert!(err.to_string().contains("reading config"), "{err}");
    }

    #[test]
    fn malformed_toml_is_a_parse_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let err = Config::load(&write_config(dir.path(), "journal = [unclosed")).unwrap_err();
        assert!(err.to_string().contains("parsing config"), "{err}");
    }

    #[test]
    fn empty_accounts_list_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let err = Config::load(&write_config(
            dir.path(),
            "journal = \"j\"\naccounts = []\n",
        ))
        .unwrap_err();
        assert!(err.to_string().contains("no [[accounts]]"), "{err}");
    }

    #[test]
    fn missing_accounts_key_is_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = Config::load(&write_config(dir.path(), "journal = \"j\"")).unwrap_err();
        assert!(err.to_string().contains("parsing config"), "{err}");
    }

    #[test]
    fn account_names_reject_path_traversal_and_unsafe_characters() {
        for bad in [
            "../evil",
            "a/b",
            "/abs",
            ".hidden",
            "a..b",
            "",
            "semi;colon",
            "tab\tname",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let body = minimal_body("").replace("name = \"a\"", &format!("name = \"{}\"", bad));
            let err = Config::load(&write_config(dir.path(), &body))
                .unwrap_err()
                .to_string();
            assert!(err.contains("invalid account name"), "{bad}: {err}");
        }
        for good in ["monzo-personal", "revolut_pro", "wise 2026", "acct.1"] {
            let dir = tempfile::tempdir().unwrap();
            let body = minimal_body("").replace("name = \"a\"", &format!("name = \"{}\"", good));
            assert!(
                Config::load(&write_config(dir.path(), &body)).is_ok(),
                "{good}"
            );
        }
    }

    #[test]
    fn duplicate_account_names_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\n[[accounts]]\nname = \"a\"\nprofile = \"monzo_csv\"\nhledger_account = \"assets:b\"\n",
            minimal_body("")
        );
        let err = Config::load(&write_config(dir.path(), &body)).unwrap_err();
        assert!(err.to_string().contains("duplicate account name"), "{err}");
    }

    #[test]
    fn unknown_profile_is_rejected_with_known_list() {
        let dir = tempfile::tempdir().unwrap();
        let body = minimal_body("");
        let body = body.replace("monzo_csv", "chase_zip");
        let err = Config::load(&write_config(dir.path(), &body)).unwrap_err();
        assert!(
            err.to_string().contains("unknown profile 'chase_zip'"),
            "{err}"
        );
        assert!(err.to_string().contains("generic_csv"), "{err}");
    }

    #[test]
    fn generic_csv_without_spec_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let body = minimal_body("").replace("monzo_csv", "generic_csv");
        let err = Config::load(&write_config(dir.path(), &body)).unwrap_err();
        assert!(
            err.to_string()
                .contains("requires an [accounts.generic] spec"),
            "{err}"
        );
    }

    #[test]
    fn generic_csv_with_spec_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\n[accounts.generic]\ndate_column = \"Date\"\ndate_format = \"%d/%m/%Y\"\n\
             description_column = \"M\"\namount_column = \"A\"\n",
            minimal_body("").replace("monzo_csv", "generic_csv")
        );
        assert!(Config::load(&write_config(dir.path(), &body)).is_ok());
    }

    #[test]
    fn account_lookup_is_case_sensitive_and_errors_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(&write_config(dir.path(), &minimal_body(""))).unwrap();
        assert!(cfg.account("a").is_ok());
        assert!(cfg.account("A").is_err());
        assert!(cfg
            .account("missing")
            .unwrap_err()
            .to_string()
            .contains("unknown account"));
    }

    #[cfg(feature = "fetch")]
    #[test]
    fn fetcher_referencing_unknown_account_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\n[[fetchers]]\ntype = \"monzo\"\naccount = \"nope\"\nclient_id = \"x\"\n",
            minimal_body("")
        );
        let err = Config::load(&write_config(dir.path(), &body)).unwrap_err();
        assert!(err.to_string().contains("unknown account 'nope'"), "{err}");
    }

    #[cfg(feature = "fetch")]
    #[test]
    fn fetcher_configs_deserialize_with_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}\n[[fetchers]]\ntype = \"monzo\"\naccount = \"a\"\nclient_id = \"cid\"\n\
             \n[[fetchers]]\ntype = \"wise\"\naccount = \"a\"\ncurrencies = [\"GBP\"]\n",
            minimal_body("")
        );
        let cfg = Config::load(&write_config(dir.path(), &body)).unwrap();
        assert_eq!(cfg.fetchers.len(), 2);
        match &cfg.fetchers[0] {
            FetcherConfig::Monzo(c) => {
                assert_eq!(c.redirect_port, 8765);
                assert_eq!(c.base_url, "https://api.monzo.com");
            }
            _ => panic!("expected monzo"),
        }
        match &cfg.fetchers[1] {
            FetcherConfig::Wise(c) => {
                assert_eq!(c.profile_id, None);
                assert_eq!(c.base_url, "https://api.wise.com");
            }
            _ => panic!("expected wise"),
        }
    }

    #[test]
    fn template_parses_and_mentions_all_builtins() {
        // The template keeps everything active-but-minimal: it must parse, and
        // it must document every profile and both fetchers.
        let dir = tempfile::tempdir().unwrap();
        let path = write_config(dir.path(), Config::template());
        let cfg = Config::load(&path).unwrap();
        assert_eq!(cfg.accounts.len(), 1); // monzo-personal active as the example
        let t = Config::template();
        for mention in [
            "revolut_xls",
            "aqua_pdf",
            "wise_csv",
            "generic_csv",
            "type = \"monzo\"",
            "type = \"wise\"",
        ] {
            assert!(t.contains(mention), "template missing {mention}");
        }
    }
}
