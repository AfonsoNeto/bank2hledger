//! Round-trip tests: fixture → profile parse → staging CSV → `hledger import`
//! with the generated rules. The hledger-dependent tests are skipped with a
//! warning when hledger isn't on PATH (CI installs it).

use std::path::{Path, PathBuf};

use bank2hledger::config::{AccountConfig, Config};
use bank2hledger::engine;
use rust_decimal::Decimal;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn hledger_available() -> bool {
    std::process::Command::new(std::env::var("HLEDGER").unwrap_or_else(|_| "hledger".into()))
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn test_account(profile: &str) -> AccountConfig {
    AccountConfig {
        name: "test-acct".to_string(),
        profile: profile.to_string(),
        hledger_account: "assets:bank:test".to_string(),
        generic: None,
    }
}

#[test]
fn parses_monzo_fixture() {
    let account = test_account("monzo_csv");
    let txs = bank2hledger::profiles::parse_file(&account, &fixture("monzo-sample.csv")).unwrap();
    assert_eq!(txs.len(), 30);
    let first = &txs[0];
    assert_eq!(first.date.to_string(), "2026-05-01");
    assert_eq!(first.currency, "GBP");
    assert!(first.amount < Decimal::ZERO);
    assert!(first.external_id.as_deref().unwrap().starts_with("tx_anon"));
    assert!(txs
        .iter()
        .all(|t| t.payee.len() < 30 && !t.payee.is_empty()));
}

#[test]
fn parses_revolut_fixture() {
    let account = test_account("revolut_xls");
    let txs = bank2hledger::profiles::parse_file(&account, &fixture("revolut-sample.xls")).unwrap();
    // 12 rows, one CANCELLED row skipped, one PENDING kept.
    assert_eq!(txs.len(), 11);
    // Fee folding: Handy Hardware -8.65 with 0.50 fee → -9.15.
    let hardware = txs
        .iter()
        .find(|t| t.payee.contains("Handy Hardware"))
        .unwrap();
    assert_eq!(hardware.amount, Decimal::from_str_exact("-9.15").unwrap());
    // Multi-currency preserved.
    assert!(txs.iter().any(|t| t.currency == "EUR"));
    // Exchange row is renamed so rules can route it to a transfer.
    assert!(txs.iter().any(|t| t.payee.contains("currency exchange")));
    // Synthetic-id path: no external ids.
    assert!(txs.iter().all(|t| t.external_id.is_none()));
}

#[test]
fn parses_wise_fixture() {
    let account = test_account("wise_csv");
    let txs = bank2hledger::profiles::parse_file(&account, &fixture("wise-sample.csv")).unwrap();
    assert_eq!(txs.len(), 4);
    assert_eq!(txs[0].external_id.as_deref(), Some("WID-anon-0001"));
    let eur = txs.iter().find(|t| t.currency == "EUR").unwrap();
    assert_eq!(eur.amount, Decimal::from_str_exact("-45.00").unwrap());
}

#[test]
fn parses_aqua_fixture() {
    if which_pdftotext().is_none() {
        eprintln!("skipping: pdftotext not installed");
        return;
    }
    let mut account = test_account("aqua_pdf");
    account.hledger_account = "liabilities:cards:test".to_string();
    let txs = bank2hledger::profiles::parse_file(&account, &fixture("aqua-sample.pdf")).unwrap();
    // Purchases are negative (they grow the debt on a liability account)…
    let groceries = txs
        .iter()
        .find(|t| t.payee.contains("CORNER GROCER"))
        .unwrap();
    assert_eq!(groceries.amount, Decimal::from_str_exact("-12.40").unwrap());
    // …and the payment is positive (it reduces the debt).
    let payment = txs
        .iter()
        .find(|t| t.payee.contains("PAYMENT RECEIVED"))
        .unwrap();
    assert_eq!(payment.amount, Decimal::from_str_exact("120.00").unwrap());
}

fn which_pdftotext() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join("pdftotext"))
            .find(|p| p.is_file())
    })
}

#[test]
fn parses_generic_csv_spec() {
    let csv = "Date;Merchant;Debit;Credit;Reference\n\
               02/03/2026;KIOSK BAR;5.50;;REF1\n\
               03/03/2026;;0.00;120.00;REF2\n";
    let mut account = test_account("generic_csv");
    account.generic = Some(bank2hledger::config::GenericCsvSpec {
        has_header: true,
        delimiter: ';',
        date_column: "Date".into(),
        date_format: "%d/%m/%Y".into(),
        description_column: "Merchant".into(),
        amount_column: None,
        amount_in_column: Some("Credit".into()),
        amount_out_column: Some("Debit".into()),
        id_column: Some("Reference".into()),
        currency: Some("GBP".into()),
        currency_column: None,
        status_column: None,
        status_accepted: None,
    });
    account.generic.as_mut().unwrap();
    // Write to a temp file so parse_file can read bytes.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test-acct.csv");
    std::fs::write(&path, csv).unwrap();
    let txs = bank2hledger::profiles::parse_file(&account, &path).unwrap();
    assert_eq!(txs.len(), 2);
    assert_eq!(txs[0].amount, Decimal::from_str_exact("-5.50").unwrap());
    assert_eq!(txs[1].amount, Decimal::from_str_exact("120.00").unwrap());
    assert_eq!(txs[0].external_id.as_deref(), Some("REF1"));
}

/// Full loop with the real hledger binary: dry-run leaves the journal
/// untouched; import appends; re-import is a no-op (dedup).
#[test]
fn hledger_roundtrip_and_dedup() {
    if !hledger_available() {
        eprintln!("skipping: hledger not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let journal = dir.path().join("j.journal");
    std::fs::write(&journal, "").unwrap();
    let in_dir = dir.path().join("in");
    let staging = dir.path().join("staging");
    let rules_dir = dir.path().join("rules");
    std::fs::create_dir_all(&in_dir).unwrap();

    let config = Config {
        journal: journal.clone(),
        in_dir: in_dir.clone(),
        staging_dir: staging,
        rules_dir,
        accounts: vec![test_account("monzo_csv")],
        fetchers: vec![],
    };
    std::fs::copy(fixture("monzo-sample.csv"), in_dir.join("test-acct.csv")).unwrap();

    // Dry-run: preview only, journal stays empty.
    let outcomes = engine::run(&config, &[], true, None).unwrap();
    assert_eq!(outcomes[0].new_count, 30);
    assert!(outcomes[0]
        .preview
        .as_ref()
        .unwrap()
        .contains("assets:bank:test"));
    assert_eq!(std::fs::read_to_string(&journal).unwrap(), "");

    // Import: appends to the journal.
    let outcomes = engine::run(&config, &[], false, None).unwrap();
    assert_eq!(outcomes[0].new_count, 30);
    let journal_text = std::fs::read_to_string(&journal).unwrap();
    assert_eq!(
        journal_text.lines().filter(|l| l.starts_with("20")).count(),
        30,
        "one transaction header line per imported row"
    );
    // The bank account and the dedup tag are on the postings.
    assert!(journal_text.contains("assets:bank:test"));
    assert!(journal_text.contains("bank2hledger-id:tx_anon0000"));

    // Re-import: everything is already seen, nothing added.
    let outcomes = engine::run(&config, &[], false, None).unwrap();
    assert_eq!(outcomes[0].new_count, 0);
    assert_eq!(outcomes[0].already_seen, 30);
    let journal_text2 = std::fs::read_to_string(&journal).unwrap();
    assert_eq!(journal_text2, journal_text);
}

/// Rules semantics: the catch-all must NOT override specific mappings, and
/// unmatched payees land in expenses:other.
#[test]
fn hledger_rules_categorization() {
    if !hledger_available() {
        eprintln!("skipping: hledger not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let journal = dir.path().join("j.journal");
    std::fs::write(&journal, "").unwrap();
    let in_dir = dir.path().join("in");
    let rules_dir = dir.path().join("rules");
    std::fs::create_dir_all(&in_dir).unwrap();
    let staging = dir.path().join("staging");

    // Custom rules written BEFORE the run: engine must not overwrite them.
    std::fs::create_dir_all(&rules_dir).unwrap();
    std::fs::write(
        rules_dir.join("test-acct.rules"),
        "fields date, description, amount, currency, id\n\
         date-format %Y-%m-%d\n\
         currency %currency\n\
         account1 assets:bank:test\n\
         account2 expenses:other\n\
         if\nTESCO\n  account2 expenses:groceries\n",
    )
    .unwrap();

    let config = Config {
        journal,
        in_dir: in_dir.clone(),
        staging_dir: staging,
        rules_dir,
        accounts: vec![test_account("monzo_csv")],
        fetchers: vec![],
    };
    std::fs::write(
        in_dir.join("test-acct.csv"),
        "Transaction ID,Date,Time,Type,Name,Emoji,Category,Amount,Currency,Local amount,Local currency,Notes and #tags,Address,Receipt,Description,Category split,Money Out,Money In\n\
         tx_x1,01/05/2026,10:00:00,CARD_PAYMENT,TESCO STORES 2,,Groceries,-25.00,GBP,-25.00,GBP,,,,TESCO STORES 2,,-25.00,\n\
         tx_x2,02/05/2026,10:00:00,CARD_PAYMENT,WEIRD UNKNOWN SHOP,,Shopping,-5.00,GBP,-5.00,GBP,,,,WEIRD UNKNOWN SHOP,,-5.00,\n",
    )
    .unwrap();

    engine::run(&config, &[], false, None).unwrap();
    let text = std::fs::read_to_string(&config.journal).unwrap();
    assert!(
        text.contains("expenses:groceries"),
        "specific mapping must win over catch-all"
    );
    assert!(
        text.contains("expenses:other"),
        "unmatched payee must land in catch-all"
    );
}
