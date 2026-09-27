//! Wise API fetcher (personal API tokens).
//!
//! Auth: the user creates a personal API token at wise.com/settings/api-tokens
//! and pastes it once; it's stored in the OS keychain. Personal tokens cannot
//! be scoped to read-only — this is documented, not fixable client-side.
//!
//! Fetch: for each configured currency, request a statement export for the
//! window since the last successful fetch (chunked — the API caps intervals
//! at 30 days), poll until it's ready, download the CSV, and drop it into
//! `in_dir` in the same shape as the website's CSV export.

use anyhow::{bail, Context, Result};
use chrono::{Duration, NaiveDate, Utc};
use reqwest::blocking::Client;
use serde_json::Value;

use super::keyring;
use super::state::FetcherState;
use super::{store_drops, RawDrop};
use crate::config::{Config, WiseFetcherConfig};

const MAX_CHUNK_DAYS: i64 = 29;

fn client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("reqwest client")
}

fn token_key(account: &str) -> String {
    format!("wise:{account}:token")
}

fn fetcher_key(c: &WiseFetcherConfig) -> String {
    format!("wise:{}", c.account)
}

fn state_dir(config: &Config) -> std::path::PathBuf {
    config.staging_dir.join("state")
}

pub fn auth(config: &Config, c: &WiseFetcherConfig) -> Result<()> {
    println!("Wise API setup for account '{}'", c.account);
    println!(
        "One-time steps:\n  \
         1. Go to https://wise.com/settings/api-tokens and create a personal token\n  \
            (requires 2FA). Note: personal tokens are full-access, not read-only.\n"
    );
    let token = rpassword::prompt_password("Personal API token: ")?;
    if token.trim().is_empty() {
        bail!("empty token");
    }
    let token = token.trim().to_string();

    let profiles: Value = client()
        .get(format!("{}/v1/profiles", c.base_url))
        .bearer_auth(&token)
        .send()?
        .error_for_status()
        .context("token rejected by Wise (check for typos / expiry)")?
        .json()?;
    let list = profiles
        .as_array()
        .context("unexpected /v1/profiles response shape")?;
    if list.is_empty() {
        bail!("no profiles visible to this token");
    }
    println!("\nYour Wise profiles:");
    for (i, p) in list.iter().enumerate() {
        let id = p.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let typ = p.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
        let name = p
            .get("fullName")
            .or_else(|| p.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        println!("  [{}] {}  {}  {}", i, id, typ, name);
    }

    let chosen: u64 = if list.len() == 1 {
        list[0]
            .get("id")
            .and_then(|v| v.as_u64())
            .context("profile missing id")?
    } else {
        print!("Profile number to bind: ");
        use std::io::Write as _;
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let idx: usize = line.trim().parse().context("enter the row number")?;
        list.get(idx)
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_u64())
            .context("invalid row")?
    };

    let mut st = FetcherState::load(&state_dir(config), &fetcher_key(c))?;
    st.set("profile_id", serde_json::Value::Number(chosen.into()));
    st.set_last_fetch(Utc::now() - Duration::days(31));
    st.save(&state_dir(config), &fetcher_key(c))?;
    keyring::store(&token_key(&c.account), &token)?;
    println!("Done. Run `bank2hledger fetch` to pull statements.");
    Ok(())
}

pub fn fetch(config: &Config, c: &WiseFetcherConfig, since: Option<NaiveDate>) -> Result<()> {
    let token = keyring::load(&token_key(&c.account))?
        .context("no stored Wise token — run `bank2hledger auth wise` first")?;
    let mut st = FetcherState::load(&state_dir(config), &fetcher_key(c))?;
    let profile_id = c
        .profile_id
        .or_else(|| st.get_u64("profile_id"))
        .context("no Wise profile bound — run `bank2hledger auth wise` first")?;

    let now = Utc::now();
    let start = since
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
        .or_else(|| st.last_fetch())
        .unwrap_or_else(|| now - Duration::days(31));

    let client = client();
    for currency in &c.currencies {
        let mut csv_out: Option<String> = None;
        let mut chunk_start = start;
        let mut total = 0usize;
        while chunk_start < now {
            let chunk_end = std::cmp::min(chunk_start + Duration::days(MAX_CHUNK_DAYS), now);
            let (csv, rows) = export_csv(
                &client,
                &token,
                c.base_url.as_str(),
                profile_id,
                currency,
                chunk_start,
                chunk_end,
            )?;
            total += rows;
            match csv_out.as_mut() {
                Some(existing) => {
                    // Subsequent chunks: skip their header line.
                    if let Some(pos) = csv.find('\n') {
                        existing.push_str(&csv[pos + 1..]);
                    }
                }
                None => csv_out = Some(csv),
            }
            chunk_start = chunk_end;
        }
        if let Some(csv) = csv_out {
            let stamp = now.format("%Y%m%d-%H%M%S");
            let drop = RawDrop {
                file_name: format!("{}-wise-{}-{}.csv", c.account, currency, stamp),
                contents: csv.into_bytes(),
            };
            let path = config.in_dir.join(&drop.file_name);
            store_drops(&config.in_dir, std::slice::from_ref(&drop))?;
            println!(
                "wise[{}]: {} rows for {} → {}",
                c.account,
                total,
                currency,
                path.display()
            );
        } else {
            println!("wise[{}]: no data for {}", c.account, currency);
        }
    }

    st.set_last_fetch(now);
    st.save(&state_dir(config), &fetcher_key(c))?;
    Ok(())
}

/// Request + poll + download one statement chunk. Returns (csv text, data rows).
fn export_csv(
    client: &Client,
    token: &str,
    base_url: &str,
    profile_id: u64,
    currency: &str,
    start: chrono::DateTime<Utc>,
    end: chrono::DateTime<Utc>,
) -> Result<(String, usize)> {
    let fmt = |t: chrono::DateTime<Utc>| t.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let initiate: Value = client
        .post(format!(
            "{}/v3/profiles/{}/statement-statements/export",
            base_url, profile_id
        ))
        .bearer_auth(token)
        .query(&[
            ("currency", currency),
            ("intervalStart", fmt(start).as_str()),
            ("intervalEnd", fmt(end).as_str()),
            ("format", "CSV"),
            ("type", "COMPACT"),
        ])
        .send()?
        .error_for_status()
        .context("statement export request failed")?
        .json()?;

    let uuid = initiate
        .get("uuid")
        .and_then(|v| v.as_str())
        .context("export response missing uuid")?
        .to_string();

    for _ in 0..30 {
        let status: Value = client
            .get(format!(
                "{}/v3/profiles/{}/statement-statements/{}",
                base_url, profile_id, uuid
            ))
            .bearer_auth(token)
            .send()?
            .error_for_status()?
            .json()?;
        let state = status.get("status").and_then(|v| v.as_str()).unwrap_or("");
        match state {
            "COMPLETED" => {
                let url = status
                    .get("downloadUrl")
                    .and_then(|v| v.as_str())
                    .context("completed export has no downloadUrl")?;
                let body = client
                    .get(url)
                    .bearer_auth(token)
                    .send()?
                    .error_for_status()?
                    .text()?;
                let rows = body.lines().count().saturating_sub(1);
                return Ok((body, rows));
            }
            "PENDING" => std::thread::sleep(std::time::Duration::from_secs(2)),
            other => bail!("statement export entered unexpected state '{}'", other),
        }
    }
    bail!("statement export did not complete in time")
}
