# Exporting from Revolut

## XLS statement (app or web)

1. Account → Account statement / Statements
2. Select the account (personal, pro, joint…) and a date range
3. Export → **XLS** (or PDF — bank2hledger uses the XLS)

Save into `in/` named after the account: `revolut-personal-2026.xls`.

## Notes

- Revolut has **no official personal-account API**, so there is no fetcher. An unofficial one (reverse-engineered device-token API) is on the roadmap; it will be optional and off by default.
- The export carries no transaction ids; bank2hledger falls back to date+payee+amount dedup, which is exact for re-dropped files.
- Internal currency-exchange rows are renamed to `Revolut currency exchange` so you can route them to a transfer/cost account in your rules file.
- Fees are folded into the transaction amount so postings match the real balance change.
