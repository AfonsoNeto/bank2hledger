//! Monzo API fetcher.
//!
//! Auth: OAuth2 confidential client (user registers one at
//! developers.monzo.com with redirect URL `http://localhost:<port>`).
//! `bank2hledger auth monzo` runs the browser flow, stores the client secret
//! and refresh token in the OS keychain, and lets the user pick which Monzo
//! account (personal/joint) this fetcher pulls.
//!
//! Fetch: refresh the access token (access tokens are never persisted), pull
//! `/transactions` with a `since` cursor, and write them out as a CSV in
//! exactly the Monzo app-export format so the `monzo_csv` profile can parse
//! it — the fetcher is a drop-in for a manual export.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use reqwest::blocking::Client;
use serde_json::Value;

use super::keyring;
use super::state::FetcherState;
use super::{store_drops, RawDrop};
use crate::config::{Config, MonzoFetcherConfig};

const REDIRECT_HOST: &str = "127.0.0.1";

fn client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("reqwest client")
}

fn secret_key(account: &str) -> String {
    format!("monzo:{account}:client_secret")
}
fn refresh_key(account: &str) -> String {
    format!("monzo:{account}:refresh_token")
}

pub fn auth(config: &Config, c: &MonzoFetcherConfig) -> Result<()> {
    println!("Monzo OAuth setup for account '{}'", c.account);
    println!(
        "One-time steps (see docs/fetchers.md):\n  \
         1. Go to https://developers.monzo.com and create an OAuth client.\n  \
         2. Set its redirect URL to http://localhost:{}\n  \
         3. Paste the client id into [[fetchers]] client_id in your config.\n",
        c.redirect_port
    );

    let stored = keyring::load(&secret_key(&c.account))?;
    let prompt = "Client secret (empty = reuse stored): ".to_string();
    let secret = rpassword::prompt_password(prompt)?;
    let secret = if secret.trim().is_empty() {
        stored.context("no stored client secret — paste it once to store it")?
    } else {
        keyring::store(&secret_key(&c.account), secret.trim())?;
        secret.trim().to_string()
    };

    // Browser flow.
    let state: String = (0..24).map(|_| rand_char()).collect();
    let listener = TcpListener::bind((REDIRECT_HOST, c.redirect_port)).with_context(|| {
        format!(
            "binding 127.0.0.1:{} — is another process using it?",
            c.redirect_port
        )
    })?;
    let redirect_uri = format!("http://localhost:{}", c.redirect_port);
    let auth_url = format!(
        "{}/?client_id={}&redirect_uri={}&response_type=code&state={}",
        c.auth_url,
        urlencode(&c.client_id),
        urlencode(&redirect_uri),
        urlencode(&state)
    );
    println!("Opening browser for Monzo login…\n{}", auth_url);
    let _ = open::that_detached(&auth_url);

    let stream = listener.incoming().next().context("no callback received")?;
    let mut stream = stream?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]).to_string();
    let code = extract_param(&request, "code")
        .context("callback did not contain ?code= — did you approve the request?")?;
    let returned_state = extract_param(&request, "state").unwrap_or_default();
    if returned_state != state {
        bail!("OAuth state mismatch — aborting (possible CSRF)");
    }
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
          <html><body><p>bank2hledger: authorized. You can close this window.</p></body></html>",
    );
    drop(stream);

    let resp = client()
        .post(format!("{}/oauth2/token", c.base_url))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", c.client_id.as_str()),
            ("client_secret", secret.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("code", code.as_str()),
        ])
        .send()?
        .error_for_status()
        .context("token exchange failed (check client_id/secret and redirect URL)")?;
    let tokens: Value = resp.json()?;
    let refresh = tokens
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(String::from)
        .context(
            "Monzo did not return a refresh token; only confidential clients do — \
             double-check the client type at developers.monzo.com",
        )?;
    keyring::store(&refresh_key(&c.account), &refresh)?;

    // List accounts so the user can bind this fetcher to one.
    let access = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .context("token response missing access_token")?;
    let accounts: Value = client()
        .get(format!("{}/accounts", c.base_url))
        .bearer_auth(access)
        .send()?
        .error_for_status()?
        .json()?;
    println!("\nYour Monzo accounts:");
    let mut ids = Vec::new();
    if let Some(list) = accounts.get("accounts").and_then(|v| v.as_array()) {
        for a in list {
            let id = a.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let desc = a.get("description").and_then(|v| v.as_str()).unwrap_or("?");
            let currency = a.get("currency").and_then(|v| v.as_str()).unwrap_or("?");
            let closed = a.get("closed").and_then(|v| v.as_bool()).unwrap_or(false);
            if closed {
                continue;
            }
            println!("  {}  {}  ({})", id, desc, currency);
            ids.push(id.to_string());
        }
    }
    let chosen = if ids.len() == 1 {
        println!("Binding to the only account: {}", ids[0]);
        ids[0].clone()
    } else if ids.is_empty() {
        bail!("no open Monzo accounts found on this token");
    } else {
        print!("Account id to bind to this fetcher: ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let line = line.trim().to_string();
        if !ids.iter().any(|id| id == &line) {
            bail!("'{}' is not one of the listed account ids", line);
        }
        line
    };

    let mut st = FetcherState::load(&state_dir(config), &fetcher_key(c))?;
    st.set("monzo_account_id", serde_json::Value::String(chosen));
    st.set_last_fetch(Utc::now() - Duration::days(31));
    st.save(&state_dir(config), &fetcher_key(c))?;
    println!("Done. Run `bank2hledger fetch` to pull transactions.");
    Ok(())
}

fn fetcher_key(c: &MonzoFetcherConfig) -> String {
    format!("monzo:{}", c.account)
}

fn state_dir(config: &Config) -> std::path::PathBuf {
    config.staging_dir.join("state")
}

pub fn fetch(config: &Config, c: &MonzoFetcherConfig, since: Option<NaiveDate>) -> Result<()> {
    let secret = keyring::load(&secret_key(&c.account))?
        .context("no stored client secret — run `bank2hledger auth monzo` first")?;
    let refresh = keyring::load(&refresh_key(&c.account))?
        .context("no stored refresh token — run `bank2hledger auth monzo` first")?;
    let mut st = FetcherState::load(&state_dir(config), &fetcher_key(c))?;
    let account_id = st
        .get_str("monzo_account_id")
        .context("no Monzo account bound — run `bank2hledger auth monzo` first")?;

    // Access tokens are never persisted; mint a fresh one every fetch.
    let resp = client()
        .post(format!("{}/oauth2/token", c.base_url))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", c.client_id.as_str()),
            ("client_secret", secret.as_str()),
            ("refresh_token", refresh.as_str()),
        ])
        .send()?
        .error_for_status()
        .map_err(|e| {
            anyhow::anyhow!(
                "token refresh failed ({}). Monzo may have revoked the session — run `bank2hledger auth monzo` again.",
                redact_status(e.status())
            )
        })?;
    let tokens: Value = resp.json()?;
    let access = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .context("refresh response missing access_token")?;
    if let Some(new_refresh) = tokens.get("refresh_token").and_then(|v| v.as_str()) {
        keyring::store(&refresh_key(&c.account), new_refresh)?;
    }

    let start = since
        .map(|d| d.and_hms_opt(0, 0, 0).unwrap())
        .or_else(|| st.last_fetch().map(|t| t.naive_utc()))
        .unwrap_or_else(|| (Utc::now() - Duration::days(31)).naive_utc());
    let since_iso = start.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut txs: Vec<Value> = Vec::new();
    let mut cursor = since_iso.clone();
    loop {
        let page: Value = client()
            .get(format!("{}/transactions", c.base_url))
            .bearer_auth(access)
            .query(&[
                ("account_id", account_id.as_str()),
                ("since", cursor.as_str()),
                ("limit", "100"),
            ])
            .send()?
            .error_for_status()?
            .json()?;
        let batch = page
            .get("transactions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let got = batch.len();
        txs.extend(batch);
        if got < 100 {
            break;
        }
        // Page is full: advance the cursor past the newest transaction we've
        // seen and pull again (ids dedup downstream, so overlap is safe).
        let newest = txs
            .iter()
            .filter_map(|t| t.get("created").and_then(|v| v.as_str()))
            .max()
            .context("page full but transactions carry no created timestamps")?;
        cursor = newest.to_string();
        if cursor == since_iso {
            break;
        }
    }

    let csv = render_monzo_csv(&txs)?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let drops = vec![RawDrop {
        file_name: format!("{}-monzo-{}.csv", c.account, stamp),
        contents: csv.into_bytes(),
    }];
    let n = store_drops(&config.in_dir, &drops)?;
    st.set_last_fetch(Utc::now());
    st.save(&state_dir(config), &fetcher_key(c))?;
    println!(
        "monzo[{}]: fetched {} transactions → {}",
        c.account,
        txs.len(),
        config.in_dir.join(&drops[0].file_name).display()
    );
    let _ = n;
    Ok(())
}

/// Render transactions into the same column layout as the app's CSV export,
/// so `monzo_csv` parses fetcher output and manual exports identically.
fn render_monzo_csv(txs: &[Value]) -> Result<String> {
    let mut out = String::from(
        "Transaction ID,Date,Time,Type,Name,Emoji,Category,Amount,Currency,Local amount,Local currency,Notes and #tags,Address,Receipt,Description,Category split,Money Out,Money In\n",
    );
    for t in txs {
        let id = t.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let created = t
            .get("created")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        let (date, time) = match created {
            Some(dt) => (
                dt.with_timezone(&Utc).format("%d/%m/%Y").to_string(),
                dt.with_timezone(&Utc).format("%H:%M:%S").to_string(),
            ),
            None => (String::new(), String::new()),
        };
        let category = t.get("category").and_then(|v| v.as_str()).unwrap_or("");
        let amount = t.get("amount").and_then(|v| v.as_str()).unwrap_or("0");
        let currency = t.get("currency").and_then(|v| v.as_str()).unwrap_or("");
        let local_amount = t.get("local_amount").and_then(|v| v.as_str()).unwrap_or("");
        let local_currency = t
            .get("local_currency")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let notes = t.get("notes").and_then(|v| v.as_str()).unwrap_or("");
        let description = t.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let merchant_name = t
            .get("merchant")
            .filter(|m| m.is_object())
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let amount_f: f64 = amount.parse().unwrap_or(0.0);
        let money_out = if amount_f < 0.0 {
            amount.to_string()
        } else {
            String::new()
        };
        let money_in = if amount_f >= 0.0 {
            amount.to_string()
        } else {
            String::new()
        };

        let name = if merchant_name.is_empty() {
            description
        } else {
            merchant_name
        };
        let typ = if t.get("is_load").and_then(|v| v.as_bool()).unwrap_or(false) {
            "Top up"
        } else {
            t.get("scheme").and_then(|v| v.as_str()).unwrap_or("")
        };
        let line = write_csv_line(&[
            id,
            &date,
            &time,
            typ,
            name,
            "",
            category,
            amount,
            currency,
            local_amount,
            local_currency,
            notes,
            "",
            "",
            description,
            "",
            &money_out,
            &money_in,
        ]);
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn write_csv_line(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|f| {
            if f.contains(',') || f.contains('"') || f.contains('\n') {
                format!("\"{}\"", f.replace('"', "\"\""))
            } else {
                f.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn extract_param(request: &str, name: &str) -> Option<String> {
    let line = request.lines().next()?;
    let query = line.split_whitespace().nth(1)?; // "GET /path?query HTTP/1.1"
    let query = query.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        let k = kv.next()?;
        if k == name {
            return kv.next().map(|v| v.to_string());
        }
    }
    None
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn redact_status(status: Option<reqwest::StatusCode>) -> String {
    status
        .map(|s| s.to_string())
        .unwrap_or_else(|| "network error".into())
}

// Small non-crypto state value for CSRF protection; not used as a secret.
fn rand_char() -> char {
    let mut b = [0u8; 1];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b));
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    ALPHABET[b[0] as usize % ALPHABET.len()] as char
}
