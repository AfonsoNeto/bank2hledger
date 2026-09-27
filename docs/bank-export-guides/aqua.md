# Exporting from Aqua (NewDay)

Aqua offers **no CSV export and no customer-facing API**, so bank2hledger parses the monthly **PDF statement**.

1. In the Aqua app (or the aquacard.co.uk account manager), open Statements.
2. Download the PDF statement(s) into `in/` named after the account: `aqua-2026-05.pdf`.
3. Requires `pdftotext` (poppler-utils) on your PATH.

## Notes

- Statements show dates without a year; bank2hledger assumes the most recent occurrence of that month (statement cadence). Import soon after the statement lands and this is always correct.
- Purchases are posted **negative** to your configured liability account (they grow the debt); payments received post positive (they shrink it). Set `hledger_account = "liabilities:credit_cards:aqua"`-style accounts.
- The parser is regex-based on the statement layout; if Aqua changes it, the profile fails loudly rather than importing garbage — please open an issue with the (redacted) line format.
- If a transaction appears in two consecutive statements (settling), the date+payee+amount dedup catches re-drops.
