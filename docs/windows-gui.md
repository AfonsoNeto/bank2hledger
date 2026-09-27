# Windows GUI

`crates/bank2hledger-gui` is a Tauri 2 app that shares the core `bank2hledger`
library with the CLI. It targets Windows (10 with WebView2 update, 11 natively)
and is styled after Windows 11 Settings.

## Architecture

- **Backend** (`src-tauri/src/commands.rs`): thin Tauri commands that wrap the
  core library — `engine::preview_account` / `engine::run` for preview and
  import, `status::balances`, `config::Config`, `init::run` for onboarding.
  No business logic lives here; the core decides account binding and dedup.
- **Frontend** (`src/`): React + TypeScript with
  [@fluentui/react-components v9](https://react.fluentui.dev) (Fluent 2 themes,
  the same control set as Win11's Settings app), a hand-rolled NavigationView
  style left pane, and a transparent window background so Tauri's `mica`
  window effect shows through.
- **Window effects**: `mica` with `acrylic`/`blur` fallbacks, configured in
  `tauri.conf.json`. Light/dark follows the system via `prefers-color-scheme`.
- **Browser dev mode**: without the Tauri IPC bridge (`npm run dev` opened in a
  plain browser) the frontend falls back to synthetic mock data
  (`src/mock.ts`), so UI work and the README screenshots don't need a Rust
  build. Screenshots live in `docs/screenshots/`.

## Workspace layout

```
crates/bank2hledger-gui/
├── index.html, vite.config.ts, package.json   # frontend build (Vite)
├── src/          # React frontend (pages mirror the CLI workflow)
├── src-tauri/    # Rust backend: Cargo.toml, tauri.conf.json, commands
├── msix/         # AppxManifest + Assets for the MSIX package
└── gen_icons.py  # regenerates src-tauri/icons and msix/Assets
```

The Tauri crate depends on the core with `default-features = false` (no
fetchers, fully offline) and is `publish = false` — only the core CLI crate
ships to crates.io.

## Known v1 limitations

- The `aqua_pdf` profile shells out to `pdftotext` (Poppler), which is not
  bundled — the UI warns when an account uses that profile. A follow-up
  should swap in a bundled PDF extractor behind the profile trait.
- API fetchers (Monzo/Wise) are CLI-only.
- The MSIX is built unsigned in CI; sign it (`signtool`) or submit via the
  Microsoft Store (which signs at ingestion) before distributing it.

## Release artifacts

The release workflow (`gui-windows` job) attaches to GitHub Releases:

- `bank2hledger-gui_<ver>_x64-setup.exe` (NSIS installer)
- `bank2hledger-gui_<ver>_x64_en-US.msi` (WiX MSI)
- `bank2hledger-gui_<ver>-portable.zip` (single exe, WebView2 required)
- `bank2hledger-gui_<ver>.msix` (unsigned)
