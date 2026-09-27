# Adding a bank profile

A profile is a small amount of code that knows how to read one bank's export format. It never decides hledger account names — those come from the user's config.

## Option 1: no code at all (`generic_csv`)

If the bank's export is a reasonably regular CSV, describe it in config:

```toml
[[accounts]]
name = "starling"
profile = "generic_csv"
hledger_account = "assets:bank:starling"

[accounts.generic]
date_column = "Date"
date_format = "%d/%m/%Y"
description_column = "Reference"
amount_in_column = "Amount In"     # or amount_column for signed amounts
amount_out_column = "Amount Out"
id_column = "Transaction ID"       # optional; enables exact dedup
currency = "GBP"                   # or currency_column
# status_column = "State"          # optional row filter, e.g. skip pending
# status_accepted = ["COMPLETED"]
```

If that works for your bank, you're done — please open an issue with the column mapping so we can document it, or turn it into Option 2.

## Option 2: a code profile

For quirky formats (binary XLS, PDF statements, headerless CSVs, columns that vary), add a Rust module:

1. Create `src/profiles/<bank>_xyz.rs` implementing:

   ```rust
   pub fn parse(account: &AccountConfig, path: &Path, bytes: &[u8]) -> Result<Vec<Transaction>> {
       // map columns → Transaction { date, payee, amount, currency, account, external_id, notes }
   }
   ```

   Rules of the road:
   - **Fail loudly.** If the format doesn't look like what you expect (missing columns, zero rows parsed), return an error telling the user to open an issue. Silent mis-parsing is the worst failure mode for financial data.
   - **Preserve the bank's own ids** in `external_id` when the export has them — it powers exact dedup.
   - **Don't transform semantics**: keep the signed amount as the bank reports it (the exception is card statements, where purchases must become negative on a liability account — see `aqua_pdf.rs`).
   - Never embed knowledge of a specific user's account names.

2. Register it in `src/profiles/mod.rs` (`parse_file`) and `src/config.rs` (`validate`).
3. Add a fixture and tests (below).
4. Update the README's supported-banks table and add an export guide in `docs/bank-export-guides/`.

## Fixtures and the anonymization rule

**No real financial data ever enters the repo.** Two acceptable kinds of fixtures:

- **Synthetic**: hand-written or generated with fake payees (`tests/fixtures/revolut-sample.xls` was generated with `xlwt` this way). Preferred.
- **Anonymized**: derived from a real export only via a mechanical transform that replaces every payee, id, note, address, and account reference with fakes, verified by a grep for the originals. Do this *outside* the repo (e.g. in your private finance directory) and copy only the result in.

The repo `.gitignore` blocks `*.csv`, `*.xls`, `*.pdf`, `*token*` by default; `tests/fixtures/` is explicitly re-included. That makes accidental commits of real exports hard but fixtures possible.

## Tests

Parser tests live in `tests/roundtrip.rs`: parse the fixture, assert row counts, amounts (including fee handling and FX), and id extraction. End-to-end tests run the real `hledger` (they self-skip when hledger is absent; CI installs it). A profile is done when its round-trip test shows the right postings on the right accounts and a second import is a no-op.
