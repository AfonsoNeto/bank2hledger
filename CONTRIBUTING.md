# Contributing

Thanks for helping make bank2hledger useful for more people (and more banks).

## Ground rules

- **No real financial data in the repo.** Fixtures must be synthetic or mechanically anonymized (see [docs/adding-a-profile.md](docs/adding-a-profile.md#fixtures-and-the-anonymization-rule)). `.gitignore` blocks raw export file types as a backstop.
- **No telemetry, no network calls outside the fetchers.** The offline core must stay offline; `cargo build --no-default-features` must keep working.
- hledger rules files are the user's property: generated once, never rewritten.

## Setup

```console
$ git clone <repo> && cd bank2hledger
$ cargo test          # parser tests always run; end-to-end tests need hledger
```

Install hledger for the full suite (`apt install hledger`, `brew install hledger`, or see https://hledger.org/install.html). Optional: `pdftotext` (poppler-utils) for the Aqua profile test.

## Good first issues

- **New export profiles** — see [docs/adding-a-profile.md](docs/adding-a-profile.md). Starling, Chase UK, NatWest, Lloyds, N26, Amex are all wanted. Start from `generic_csv` in your own config; if it fits, contribute the mapping; if not, write a small parser.
- **Improving an existing profile** — banks change export formats; tests make those changes easy to absorb.
- **Docs** — export guides per bank with current menu paths are always out of date somewhere.

## Pull requests

- `cargo fmt` and `cargo clippy` clean.
- Add/adjust tests for behavior changes; parser changes need a fixture.
- Keep error messages actionable ("what to do next"), especially around parsing failures — they're the main UX for new banks.
