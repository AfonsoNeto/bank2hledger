use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bank2hledger",
    version,
    about = "Import bank transaction exports into your hledger journal — with dedup, categorization rules, and a review-before-approve workflow.",
    after_help = "Typical loop:\n  1. drop exports in in/ (or run `fetch`)\n  2. bank2hledger import --dry-run\n  3. bank2hledger import\n  4. bank2hledger status   # compare with the bank app\n  5. git commit           # your approval"
)]
pub struct Cli {
    /// Path to bank2hledger.toml. Defaults to ./bank2hledger.toml, then
    /// $BANK2HLEDGER_CONFIG, then ~/.config/bank2hledger/config.toml.
    #[arg(long, global = true)]
    pub config: Option<std::path::PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a starter bank2hledger.toml, directories, and rules files.
    Init {
        /// Overwrite the config file even if it exists (rules files are
        /// never overwritten).
        #[arg(long)]
        force: bool,
    },
    /// Convert dropped exports and import the new transactions.
    Import {
        /// Show what would be added without touching the journal.
        #[arg(long)]
        dry_run: bool,
        /// Pause on each possible duplicate and choose interactively
        /// (arrow keys) whether to import it or skip it as a duplicate.
        #[cfg(feature = "interactive")]
        #[arg(short, long)]
        interactive: bool,
        /// Restrict to these account names (from the config).
        #[arg(long)]
        account: Vec<String>,
        /// Only offer transactions on or after this date (YYYY-MM-DD).
        #[arg(long)]
        since: Option<chrono::NaiveDate>,
    },
    /// Show current balances of the configured bank accounts.
    Status {
        /// Restrict to these account names (from the config).
        #[arg(long)]
        account: Vec<String>,
    },
    /// One-time authentication for an API fetcher (stores secrets in the OS keychain).
    #[cfg(feature = "fetch")]
    Auth {
        #[arg(value_enum)]
        fetcher: AuthFetcher,
    },
    /// Pull new transactions via the configured API fetchers.
    #[cfg(feature = "fetch")]
    Fetch {
        /// Only run this fetcher (by account name).
        #[arg(long)]
        account: Option<String>,
        /// Override the start date of the pull (YYYY-MM-DD).
        #[arg(long)]
        since: Option<chrono::NaiveDate>,
    },
}

#[cfg(feature = "fetch")]
#[derive(clap::ValueEnum, Clone, Copy)]
pub enum AuthFetcher {
    Monzo,
    Wise,
}
