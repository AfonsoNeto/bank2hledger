# API fetchers

Fetchers pull transactions from bank APIs and write them into your `in/` directory as ordinary export files — byte-for-byte what a manual export would produce. Everything downstream (profiles, rules, import, review) is identical for both paths.

Enable them by building with the default `fetch` feature (they're compiled out with `cargo build --no-default-features`, which leaves a tool with zero network code).

## Monzo

The fetcher uses Monzo's **official API** with a confidential OAuth client and requests only the scopes needed to read transactions and balances. Access tokens are short-lived and never persisted; only the refresh token is stored (in your OS keychain).

### One-time setup

1. Sign in at [developers.monzo.com](https://developers.monzo.com) and create an **OAuth client**:
   - Name: anything (`bank2hledger`)
   - Redirect URL: `http://localhost:8765` (must match `redirect_port` in your config)
   - Confidentiality: **confidential client** (this is what makes refresh tokens available)
2. Put the client id in your `bank2hledger.toml`:

   ```toml
   [[fetchers]]
   type = "monzo"
   account = "monzo-personal"   # must match a [[accounts]] name
   client_id = "oauth2client_..."
   redirect_port = 8765
   ```

3. Run `bank2hledger auth monzo`. A browser opens; approve the request; the tool lists your Monzo accounts (personal, joint, …) and asks which one this fetcher should pull.
4. `bank2hledger fetch` pulls new transactions with a `since` cursor (per-fetcher state file, gap-free).

### Quirks

- Monzo revokes refresh tokens occasionally (app password change, long disuse, Monzo-side policy). When that happens, `fetch` tells you to re-run `auth` — it takes 30 seconds.
- Declined transactions are not returned by the API, matching the app's statement view.

## Wise

The fetcher uses Wise **personal API tokens**. Note honestly: Wise personal tokens grant full account access and cannot be scoped to read-only. If that bothers you, use the website's CSV export with the `wise_csv` profile instead — the rest of the pipeline is identical.

### One-time setup

1. Create a personal API token at <https://wise.com/settings/api-tokens> (requires 2FA).
2. Add a fetcher per Wise account:

   ```toml
   [[accounts]]
   name = "wise-personal"
   profile = "wise_csv"
   hledger_account = "assets:banks:wise"

   [[fetchers]]
   type = "wise"
   account = "wise-personal"
   currencies = ["GBP", "EUR"]   # one entry per balance currency
   ```

3. Run `bank2hledger auth wise` and paste the token (stored in the keychain). The tool lists your profiles and binds one.
4. `bank2hledger fetch` exports statements for each configured currency since the last successful fetch, automatically chunked (the API caps intervals at 30 days).

### Sandbox

Point `base_url` at `https://api.sandbox.transferwise.tech` to test against Wise's sandbox without touching real data.

## Security notes

- Secrets are stored via your OS keychain (Secret Service on Linux, Keychain on macOS, Credential Manager on Windows). If no keychain is available, secrets fall back to mode-`0600` files in `~/.config/bank2hledger/secrets/` and a warning is printed.
- Tokens are never written to config files, logs, or error messages; HTTP error output is redacted.
- All traffic uses TLS (rustls). Base URLs are configurable so you can point at sandboxes or, if you really must, a local proxy for debugging.
- Fetchers are pulled only when you run `fetch` (directly or from cron/systemd). The tool never runs in the background by itself; see `docs/` for an example systemd timer.

## Scheduling (optional)

Fetchers don't self-schedule. Example systemd user timer (`~/.config/systemd/user/bank2hledger.timer`):

```ini
[Unit]
Description=Fetch bank transactions

[Timer]
OnCalendar=*-*-* 06,18:00:00
Persistent=true

[Install]
WantedBy=timers.target
```

```ini
# ~/.config/systemd/user/bank2hledger.service
[Service]
Type=oneshot
ExecStart=%h/.cargo/bin/bank2hledger --config %h/.finance/bank2hledger.toml fetch
ExecStartPost=%h/.cargo/bin/bank2hledger --config %h/.finance/bank2hledger.toml import
```

`systemctl --user enable --now bank2hledger.timer` — transactions accumulate in the journal; your git commit remains the approval step.
