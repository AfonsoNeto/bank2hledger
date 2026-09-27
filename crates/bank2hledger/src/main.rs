use bank2hledger::{cli, config, engine, init, status};

#[cfg(feature = "fetch")]
use bank2hledger::fetchers;

use anyhow::{Context, Result};
use clap::Parser as _;
use cli::{Cli, Command};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { force } => init(cli.config.as_deref(), force),
        Command::Import {
            dry_run,
            account,
            since,
        } => {
            let config = load_config(cli.config.as_deref())?;
            status::require_journal(&config.journal)?;
            let outcomes = engine::run(&config, &account, dry_run, since)?;
            let mut total_new = 0;
            for o in &outcomes {
                match (o.new_count, &o.preview) {
                    (0, _) => println!("{}: nothing new", o.account),
                    (_, Some(preview)) => {
                        total_new += o.new_count;
                        println!(
                            "{}: {} new transaction(s) ({} already imported)",
                            o.account, o.new_count, o.already_seen
                        );
                        if dry_run {
                            println!("--- would be added ---\n{}", preview.trim_end());
                        }
                    }
                    (_, None) => {
                        total_new += o.new_count;
                        println!("{}: {} new transaction(s)", o.account, o.new_count);
                    }
                }
            }
            if dry_run && total_new > 0 {
                println!("\n(dry run — nothing written; run without --dry-run to import)");
            } else if !dry_run && total_new > 0 {
                println!(
                    "\nImported {} transaction(s). Now run `bank2hledger status` and \
                     compare the balances against your bank apps, then git commit to approve.",
                    total_new
                );
            }
            Ok(())
        }
        Command::Status { account } => {
            let config = load_config(cli.config.as_deref())?;
            status::run(&config, &account)
        }
        #[cfg(feature = "fetch")]
        Command::Auth { fetcher } => {
            let config = load_config(cli.config.as_deref())?;
            match fetcher {
                cli::AuthFetcher::Monzo => {
                    let c = find_fetcher(&config, "monzo")?;
                    fetchers::monzo::auth(&config, c)
                }
                cli::AuthFetcher::Wise => {
                    let c = find_fetcher(&config, "wise")?;
                    fetchers::wise::auth(&config, c)
                }
            }
        }
        #[cfg(feature = "fetch")]
        Command::Fetch { account, since } => {
            let config = load_config(cli.config.as_deref())?;
            fetchers::run(&config, account.as_deref(), since)?;
            println!("\nFetched. Now run `bank2hledger import --dry-run` to review.");
            Ok(())
        }
    }
}

#[cfg(feature = "fetch")]
fn find_fetcher<'a, T>(config: &'a config::Config, kind: &str) -> Result<&'a T>
where
    config::FetcherConfig: FetcherKind<'a, T>,
{
    config
        .fetchers
        .iter()
        .find_map(|f| f.as_kind())
        .context(format!(
            "no '{}' [[fetchers]] entry in the config — add one (see `bank2hledger init` template)",
            kind
        ))
}

#[cfg(feature = "fetch")]
trait FetcherKind<'a, T> {
    fn as_kind(&'a self) -> Option<&'a T>;
}

#[cfg(feature = "fetch")]
impl<'a> FetcherKind<'a, config::MonzoFetcherConfig> for config::FetcherConfig {
    fn as_kind(&'a self) -> Option<&'a config::MonzoFetcherConfig> {
        match self {
            config::FetcherConfig::Monzo(c) => Some(c),
            _ => None,
        }
    }
}

#[cfg(feature = "fetch")]
impl<'a> FetcherKind<'a, config::WiseFetcherConfig> for config::FetcherConfig {
    fn as_kind(&'a self) -> Option<&'a config::WiseFetcherConfig> {
        match self {
            config::FetcherConfig::Wise(c) => Some(c),
            _ => None,
        }
    }
}

fn load_config(explicit: Option<&std::path::Path>) -> Result<config::Config> {
    let path = match explicit {
        Some(p) => p.to_path_buf(),
        None => {
            let local = std::path::PathBuf::from("bank2hledger.toml");
            if local.exists() {
                local
            } else if let Ok(env) = std::env::var("BANK2HLEDGER_CONFIG") {
                std::path::PathBuf::from(env)
            } else {
                dirs::config_dir()
                    .map(|d| d.join("bank2hledger").join("config.toml"))
                    .context("no config found — run `bank2hledger init` (or pass --config)")?
            }
        }
    };
    config::Config::load(&path)
}

fn init(explicit: Option<&std::path::Path>, force: bool) -> Result<()> {
    let report = init::run(explicit, force)?;
    println!("wrote {}", report.config_path.display());
    let base = report
        .config_path
        .parent()
        .unwrap_or(std::path::Path::new("."));
    for dir in &report.dirs {
        let rel = dir.strip_prefix(base).unwrap_or(dir);
        println!("created {}", rel.display());
    }
    for p in &report.rules_files {
        println!("wrote rules file {}", p.display());
    }
    println!("\n{}", report.next_step);
    Ok(())
}
