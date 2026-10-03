# Exporting from Revolut

## Statement export (app or web)

1. Account → Account statement / Statements
2. Select the account (personal, pro, joint…) and a date range
3. Export → **CSV** (the newer app format) or **XLS** — both are supported:
   use `profile = "revolut_csv"` or `"revolut_xls"` per account in the config.

Save into `in/` named after the account: `revolut-personal-2026-09.csv` or
`revolut-personal-2026.xls`.

## Notes

- Revolut has **no official personal-account API**, so there is no fetcher. An unofficial one (reverse-engineered device-token API) is on the roadmap; it will be optional and off by default.
- The exports carry no transaction ids; bank2hledger falls back to date+payee+amount dedup, which is exact for re-dropped files.
- `REVERTED` rows (never happened) are skipped; `PENDING` rows are kept — they re-resolve when the settled export arrives, with the duplicate detector's help.
- `Charge` rows (e.g. the Ultra plan fee) post the fee folded into the amount; `Card Refund` rows post positive.
- Internal currency-exchange rows are renamed to `Revolut currency exchange` so you can route them to a transfer/cost account in your rules file.
- Fees are folded into the transaction amount so postings match the real balance change.
