# Exporting from Monzo

## CSV export (app)

1. Account → pick the account (personal / joint)
2. Statement → pick a range (choose "Custom" and a wide range for first import)
3. Share / Export → **CSV**

Save the file into your `in/` directory named after the account, e.g. `monzo-personal.csv` or `monzo-personal-2026-09.csv`.

> Note: exports include pending card transactions in some cases; re-export later or use the API fetcher, which settles on a `since` cursor.

## API fetcher

See [../fetchers.md](../fetchers.md). Advantages over manual export: transactions appear with a stable id (exact dedup), no date-range fiddling, schedulable.
