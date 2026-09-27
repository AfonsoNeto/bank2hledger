# bank2hledger

Import bank transactions into your [hledger](https://hledger.org) journal — from file exports or live bank APIs — with deduplication, categorization rules, and a **review-before-you-approve** workflow.

Built for the reality of modern banking: you can't always get a clean API, but you can almost always get an export. bank2hledger treats a dropped export and an API fetch identically, so you can mix both freely — and move banks or add banks without touching your workflow.

```console
$ bank2hledger import --dry-run     # what's new since last time?
monzo-personal: 12 new transaction(s) (0 already imported)
--- would be added ---
2026-09-14 Corner Grocer
    assets:banks:monzo:personal       GBP-23.14
    expenses:groceries                 GBP23.14
...
(dry run — nothing written; run without --dry-run to import)

$ bank2hledger import               # add them
$ bank2hledger status               # balances, to compare with the bank app
$ git commit                        # your approval
```

## Why not just `hledger import`?

You can — bank2hledger builds on it. What it adds:

- **Account binding by file name.** Exports are matched to your configured accounts by filename (`monzo-personal-*.csv`), so multi-account setups never mis-assign transactions. There is no guessing.
- **Exact duplicate detection.** Bank transaction ids (where exports provide them) are tracked in a seen-file, so two identical coffees on the same day both survive — plain hledger dedup would silently drop one.
- **Starter rules per account.** `init` generates a standard hledger CSV-rules file per account with a catch-all to `expenses:other`; you edit plain hledger syntax, the tool never rewrites it. Unmatched payees are visible in every import for review; each fix makes every future import smarter.
- **A balance checkpoint.** `status` prints the balances of exactly the accounts you import for, so comparing against the bank app is one glance.
- **API fetchers (optional).** Monzo and Wise fetchers write real exports into your drop zone automatically; the rest of the pipeline doesn't know or care.

## Supported banks

| Bank     | Export profile | API fetcher | Notes |
|----------|----------------|-------------|-------|
| Monzo    | `monzo_csv`    | ✅ `monzo`   | fetcher uses the official API (OAuth, read-only) |
| Revolut  | `revolut_xls`  | —           | no official personal API; an unofficial fetcher is on the roadmap |
| Wise     | `wise_csv`     | ✅ `wise`    | fetcher uses personal API tokens (full-access; not scoping-able) |
| Aqua (NewDay) | `aqua_pdf` | —          | monthly PDF statement → `pdftotext`; no CSV export exists |
| *any bank* | `generic_csv` | —          | describe your bank's CSV in config; no code needed |

Contributions for more banks are very welcome — Starling, Chase UK, NatWest, N26 and Amex would all be small profiles. See [docs/adding-a-profile.md](docs/adding-a-profile.md).

## Install

```console
$ cargo install bank2hledger      # or grab a binary from Releases
```

Requires [hledger](https://hledger.org/install.html) on `$PATH`. Aqua PDF parsing additionally needs `pdftotext` (poppler-utils).

## Setup

```console
$ cd ~/.finance                   # wherever your journals live (a git repo is ideal)
$ bank2hledger init               # writes bank2hledger.toml + directories
$ $EDITOR bank2hledger.toml       # name your accounts and hledger account names
```

Then either drop exports into `in/` (name them after the account, e.g. `monzo-personal.csv`), or set up a fetcher:

```console
$ bank2hledger auth monzo         # one-time OAuth browser flow → OS keychain
$ bank2hledger fetch
```

## The review loop

1. **Get data in**: drop exports into `in/`, or run `fetch`.
2. **Preview**: `bank2hledger import --dry-run` — shows exactly the batch that would be added.
3. **Categorize**: unmatched payees land in `expenses:other`; add a mapping to the account's rules file (`rules/<account>.rules`) and re-run. Mappings are plain hledger regexes, matched case-insensitively; *later rules win*, so the catch-all sits first and specific blocks after it.
4. **Import**: `bank2hledger import` — appends only new transactions; re-running or re-dropping files can never duplicate.
5. **Verify**: `bank2hledger status` — compare against the real balances in your bank apps.
6. **Approve**: `git commit`. To reject a batch: `git checkout -- 2026.journal`, fix the rules, re-run.

## Privacy

- No telemetry, no analytics, no network calls — except the fetchers you explicitly configure.
- Secrets (OAuth client secret, refresh tokens, API tokens) live in your OS keychain, never in config or logs. Without a keychain, a `0600` file under `~/.config/bank2hledger/secrets/` is used, with a warning.
- Your journals, exports, and rules files stay in your private directory. The repo's fixtures are synthetic or fully anonymized.
- Monzo fetcher requests only transaction-read scopes. Wise personal API tokens cannot be scoped — that's a Wise limitation; see [docs/fetchers.md](docs/fetchers.md).

## Development

```console
$ cargo test                      # parser + end-to-end tests (uses real hledger if present)
$ cargo build --no-default-features   # verify the offline core builds without fetchers
```

See [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/adding-a-profile.md](docs/adding-a-profile.md).

## License

MIT
