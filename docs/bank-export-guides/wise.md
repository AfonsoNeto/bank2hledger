# Exporting from Wise

## CSV statement (website)

1. wise.com → Account → Statement (or Balances → the currency → Statement)
2. Choose period → Export → **CSV**

Save into `in/` as `wise-personal.csv` (name = your `[[accounts]]` entry).

## API fetcher

See [../fetchers.md](../fetchers.md). One `[[fetchers]]` entry per Wise account, listing each balance currency.

## Notes

- Wise statements are per-currency; multi-currency balances need one fetcher (or export) per currency. Your rules file can then map by description or currency.
- The API's COMPACT statement CSV and the website export differ slightly in columns; the `wise_csv` profile matches columns by header name with several accepted spellings and fails loudly otherwise.
