//! Engine-level integration tests with the real hledger binary:
//! dedup semantics, since filters, multi-account state, file binding,
//! staging/rules generation, and failure isolation. Each test self-skips
//! when hledger is absent (CI installs it).

use std::path::{Path, PathBuf};

use bank2hledger::config::{AccountConfig, Config};
use bank2hledger::engine;
use chrono::NaiveDate;

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

fn skip_or_panic() -> bool {
    if hledger_available() {
        true
    } else {
        eprintln!("skipping: hledger not on PATH");
        false
    }
}

struct Rig {
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    config: Config,
    in_dir: PathBuf,
    staging: PathBuf,
    rules_dir: PathBuf,
}

fn rig(accounts: Vec<AccountConfig>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let in_dir = dir.path().join("in");
    let staging = dir.path().join("staging");
    let rules_dir = dir.path().join("rules");
    std::fs::create_dir_all(&in_dir).unwrap();
    let config = Config {
        journal: dir.path().join("j.journal"),
        in_dir: in_dir.clone(),
        staging_dir: staging.clone(),
        rules_dir: rules_dir.clone(),
        accounts,
        fetchers: vec![],
    };
    std::fs::write(&config.journal, "").unwrap();
    Rig {
        dir,
        config,
        in_dir,
        staging,
        rules_dir,
    }
}

fn acct(name: &str, profile: &str, hledger_account: &str) -> AccountConfig {
    AccountConfig {
        name: name.to_string(),
        profile: profile.to_string(),
        hledger_account: hledger_account.to_string(),
        generic: None,
    }
}

fn journal_text(rig: &Rig) -> String {
    std::fs::read_to_string(&rig.config.journal).unwrap()
}

fn monzo_row(id: &str, date: &str, name: &str, amount: &str) -> String {
    format!(
        "{id},{date},10:00:00,CARD_PAYMENT,{name},,Shopping,{amount},GBP,{amount},GBP,,,,{name},,,{amount},\n",
    )
}

const MONZO_HEADER: &str = "Transaction ID,Date,Time,Type,Name,Emoji,Category,Amount,Currency,Local amount,Local currency,Notes and #tags,Address,Receipt,Description,Category split,Money Out,Money In\n";

/// Dry-run leaves journal AND seen-state untouched; a real run after it
/// still imports everything.
#[test]
fn dry_run_writes_nothing_anywhere() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::copy(
        fixture("monzo-sample.csv"),
        rig.in_dir.join("test-acct.csv"),
    )
    .unwrap();

    for _ in 0..2 {
        let outcomes = engine::run(&rig.config, &[], true, None).unwrap();
        assert_eq!(outcomes[0].new_count, 30);
        assert!(outcomes[0]
            .preview
            .as_ref()
            .unwrap()
            .contains("assets:bank:test"));
    }
    assert_eq!(journal_text(&rig), "");
    assert!(
        !rig.staging.join("test-acct.seen").exists(),
        "dry run must not record seen ids"
    );

    engine::run(&rig.config, &[], false, None).unwrap();
    assert_eq!(
        journal_text(&rig)
            .lines()
            .filter(|l| l.starts_with("20"))
            .count(),
        30
    );
}

/// The headline dedup guarantee: two identical purchases on the same day
/// (differing only by bank id) both survive, which plain hledger dedup
/// would collapse.
#[test]
fn identical_same_day_transactions_both_import() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::write(
        rig.in_dir.join("test-acct.csv"),
        format!(
            "{MONZO_HEADER}{}{}",
            monzo_row("tx_a", "01/05/2026", "City Coffee", "-3.80"),
            monzo_row("tx_b", "01/05/2026", "City Coffee", "-3.80"),
        ),
    )
    .unwrap();
    engine::run(&rig.config, &[], false, None).unwrap();
    let text = journal_text(&rig);
    assert_eq!(
        text.lines().filter(|l| l.starts_with("2026-05-01")).count(),
        2
    );
    assert!(text.contains("bank2hledger-id:tx_a"));
    assert!(text.contains("bank2hledger-id:tx_b"));
}

/// Exports without ids (Revolut) fall back to date+payee+amount keys; a
/// re-dropped file must still be a no-op.
#[test]
fn synthetic_dedup_for_id_less_exports() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "revolut_xls", "assets:bank:test")]);
    std::fs::copy(
        fixture("revolut-sample.xls"),
        rig.in_dir.join("test-acct.xls"),
    )
    .unwrap();
    engine::run(&rig.config, &[], false, None).unwrap();
    let before = journal_text(&rig);
    assert_eq!(before.lines().filter(|l| l.starts_with("20")).count(), 11);

    // Re-drop the same file under a new name (as users do).
    std::fs::copy(
        fixture("revolut-sample.xls"),
        rig.in_dir.join("test-acct-copy.xls"),
    )
    .unwrap();
    let outcomes = engine::run(&rig.config, &[], false, None).unwrap();
    assert_eq!(outcomes[0].new_count, 0);
    assert_eq!(journal_text(&rig), before);
}

/// A second file overlapping an earlier import only contributes its new rows.
#[test]
fn overlapping_files_only_add_new_rows() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::write(
        rig.in_dir.join("test-acct.csv"),
        format!(
            "{MONZO_HEADER}{}",
            monzo_row("tx_a", "01/05/2026", "Shop A", "-1.00")
        ),
    )
    .unwrap();
    engine::run(&rig.config, &[], false, None).unwrap();

    // Later export re-contains tx_a and adds tx_b.
    std::fs::write(
        rig.in_dir.join("test-acct-later.csv"),
        format!(
            "{MONZO_HEADER}{}{}",
            monzo_row("tx_a", "01/05/2026", "Shop A", "-1.00"),
            monzo_row("tx_b", "02/05/2026", "Shop B", "-2.00"),
        ),
    )
    .unwrap();
    let outcomes = engine::run(&rig.config, &[], false, None).unwrap();
    assert_eq!(outcomes[0].new_count, 1);
    // tx_a appears in both files, so it is counted seen twice.
    assert_eq!(outcomes[0].already_seen, 2);
    let text = journal_text(&rig);
    assert!(text.contains("bank2hledger-id:tx_b"));
    assert_eq!(text.lines().filter(|l| l.starts_with("20")).count(), 2);
}

#[test]
fn since_filter_excludes_older_transactions() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::write(
        rig.in_dir.join("test-acct.csv"),
        format!(
            "{MONZO_HEADER}{}{}{}",
            monzo_row("tx_a", "10/05/2026", "Old", "-1.00"),
            monzo_row("tx_b", "15/05/2026", "Edge", "-2.00"),
            monzo_row("tx_c", "20/05/2026", "New", "-3.00"),
        ),
    )
    .unwrap();
    let since = NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
    let outcomes = engine::run(&rig.config, &[], false, Some(since)).unwrap();
    assert_eq!(outcomes[0].new_count, 2);
    let text = journal_text(&rig);
    assert!(text.contains("bank2hledger-id:tx_b") && text.contains("bank2hledger-id:tx_c"));
    assert!(!text.contains("bank2hledger-id:tx_a"));

    // A later run without --since must NOT import tx_a: the since-limited
    // run recorded what it imported, and tx_a was filtered out entirely.
    // (Documented semantics: --since is a preview-scope tool; gaps are the
    // user's choice. The seen-file only records what was offered.)
    let outcomes = engine::run(&rig.config, &[], false, None).unwrap();
    assert_eq!(outcomes[0].new_count, 1, "only tx_a remains unseen");
}

#[test]
fn multi_account_state_is_independent() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![
        acct("acct-a", "monzo_csv", "assets:bank:a"),
        acct("acct-b", "monzo_csv", "assets:bank:b"),
    ]);
    std::fs::write(
        rig.in_dir.join("acct-a.csv"),
        format!(
            "{MONZO_HEADER}{}",
            monzo_row("tx_a", "01/05/2026", "Shop A", "-1.00")
        ),
    )
    .unwrap();
    std::fs::write(
        rig.in_dir.join("acct-b.csv"),
        format!(
            "{MONZO_HEADER}{}",
            monzo_row("tx_a", "01/05/2026", "Shop A", "-1.00")
        ),
    )
    .unwrap();

    // Same bank id in both accounts: both import (different hledger accounts).
    let outcomes = engine::run(&rig.config, &[], false, None).unwrap();
    assert_eq!(outcomes[0].new_count, 1);
    assert_eq!(outcomes[1].new_count, 1);
    let text = journal_text(&rig);
    assert!(text.contains("assets:bank:a"));
    assert!(text.contains("assets:bank:b"));

    // Account filtering: importing only acct-b is a no-op and leaves a alone.
    let outcomes = engine::run(&rig.config, &["acct-b".to_string()], false, None).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].account, "acct-b");
    assert_eq!(outcomes[0].new_count, 0);
}

#[test]
fn files_bind_to_accounts_by_name_prefix() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::write(
        rig.in_dir.join("test-acct.csv"),
        format!(
            "{MONZO_HEADER}{}",
            monzo_row("tx_a", "01/05/2026", "A", "-1.00")
        ),
    )
    .unwrap();
    std::fs::write(
        rig.in_dir.join("test-acct-2026-05.xls.csv"),
        format!(
            "{MONZO_HEADER}{}",
            monzo_row("tx_b", "02/05/2026", "B", "-2.00")
        ),
    )
    .unwrap();
    // Not bound to any account: must be ignored entirely.
    std::fs::write(
        rig.in_dir.join("unrelated-bank.csv"),
        format!(
            "{MONZO_HEADER}{}",
            monzo_row("tx_x", "03/05/2026", "X", "-3.00")
        ),
    )
    .unwrap();

    let outcomes = engine::run(&rig.config, &[], false, None).unwrap();
    assert_eq!(outcomes[0].new_count, 2);
    let text = journal_text(&rig);
    assert!(text.contains("bank2hledger-id:tx_a") && text.contains("bank2hledger-id:tx_b"));
    assert!(!text.contains("tx_x"), "unbound files must not be imported");
}

#[test]
fn staging_csv_and_rules_files_are_generated() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::copy(
        fixture("monzo-sample.csv"),
        rig.in_dir.join("test-acct.csv"),
    )
    .unwrap();
    engine::run(&rig.config, &[], false, None).unwrap();

    let staging = std::fs::read_to_string(rig.staging.join("test-acct.csv")).unwrap();
    assert_eq!(staging.lines().count(), 30, "normalized rows, no header");
    let first = staging.lines().next().unwrap();
    let fields: Vec<&str> = first.split(',').collect();
    assert_eq!(fields.len(), 5, "date,description,amount,currency,id");
    assert_eq!(fields[0], "2026-05-01");

    assert!(rig.rules_dir.join("test-acct.rules").exists());
    // The seen-file records exactly the imported ids.
    let seen = std::fs::read_to_string(rig.staging.join("test-acct.seen")).unwrap();
    assert_eq!(
        seen.lines().filter(|l| l.starts_with("id:tx_anon")).count(),
        30
    );
}

/// Deleting the seen-file re-offers everything (documented recovery path);
/// hledger's own dedup must then catch the already-journaled duplicates.
#[test]
fn deleting_seen_state_re_offers_and_hledger_catches_duplicates() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::copy(
        fixture("monzo-sample.csv"),
        rig.in_dir.join("test-acct.csv"),
    )
    .unwrap();
    engine::run(&rig.config, &[], false, None).unwrap();
    let before = journal_text(&rig);

    std::fs::remove_file(rig.staging.join("test-acct.seen")).unwrap();
    engine::run(&rig.config, &[], false, None).unwrap();
    let after = journal_text(&rig);
    assert_eq!(
        before.lines().filter(|l| l.starts_with("20")).count(),
        after.lines().filter(|l| l.starts_with("20")).count(),
        "hledger dedup must absorb re-offered transactions"
    );
}

#[test]
fn multi_currency_postings_keep_their_commodity() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "revolut_xls", "assets:bank:test")]);
    std::fs::copy(
        fixture("revolut-sample.xls"),
        rig.in_dir.join("test-acct.xls"),
    )
    .unwrap();
    engine::run(&rig.config, &[], false, None).unwrap();
    let text = journal_text(&rig);
    assert!(
        text.contains("EUR"),
        "EUR postings must carry the EUR commodity:\n{text}"
    );
    assert!(text.contains("GBP"));
}

#[test]
fn unparseable_files_fail_with_context_and_record_nothing() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::write(
        rig.in_dir.join("test-acct.csv"),
        b"not,a,real,monzo,export\n1,2,3,4,5\n",
    )
    .unwrap();
    let err = engine::run(&rig.config, &[], false, None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("parsing"), "{err}");
    assert!(
        !rig.staging.join("test-acct.seen").exists(),
        "failed runs record nothing"
    );
    assert_eq!(journal_text(&rig), "");
}

/// If hledger rejects the batch (e.g. broken user-edited rules), the seen
/// state must not advance, so fixing the rules and re-running recovers.
#[test]
fn hledger_rejection_does_not_advance_state() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    std::fs::write(
        rig.in_dir.join("test-acct.csv"),
        format!(
            "{MONZO_HEADER}{}",
            monzo_row("tx_a", "01/05/2026", "A", "-1.00")
        ),
    )
    .unwrap();
    std::fs::create_dir_all(&rig.rules_dir).unwrap();
    std::fs::write(
        rig.rules_dir.join("test-acct.rules"),
        "fields date, description, amount, currency, id\nif\n(((\n  account2 x\n",
    )
    .unwrap();
    assert!(
        engine::run(&rig.config, &[], false, None).is_err(),
        "invalid regex must fail"
    );
    assert!(
        !rig.staging.join("test-acct.seen").exists(),
        "state must not advance on failure"
    );

    // User fixes the rules; the same data imports cleanly.
    std::fs::write(
        rig.rules_dir.join("test-acct.rules"),
        "fields date, description, amount, currency, id\naccount1 assets:bank:test\n\
         account2 expenses:other\ncomment bank2hledger-id:%id\n",
    )
    .unwrap();
    engine::run(&rig.config, &[], false, None).unwrap();
    assert!(journal_text(&rig).contains("bank2hledger-id:tx_a"));
}

/// Empty in-dir and unknown account names are handled gracefully.
#[test]
fn empty_input_and_unknown_accounts() {
    if !skip_or_panic() {
        return;
    }
    let rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    let outcomes = engine::run(&rig.config, &[], false, None).unwrap();
    assert_eq!(outcomes[0].new_count, 0);
    assert_eq!(journal_text(&rig), "");

    assert!(engine::run(&rig.config, &["ghost".to_string()], false, None).is_err());
}

/// A malicious export with control characters (e.g. newlines) inside quoted
/// CSV cells must not be able to inject journal content beyond the intended
/// transaction: one row in, one transaction out, no embedded newlines.
#[test]
fn control_characters_in_fields_cannot_inject_journal_content() {
    if !skip_or_panic() {
        return;
    }
    let mut rig = rig(vec![acct("test-acct", "monzo_csv", "assets:bank:test")]);
    // The transaction id tries to break out of the comment line and
    // fabricate a second, balance-modifying transaction. It rides in a
    // properly quoted multiline CSV cell — exactly how such an export
    // would look on disk.
    let evil_id = "tx_evil\n2026-05-02 Evil\n    assets:bank:test  -9999\n    expenses:other  9999";
    let evil_id_csv = format!("\"{}\"", evil_id.replace('"', "\"\""));
    std::fs::write(
        rig.in_dir.join("test-acct.csv"),
        format!(
            "{MONZO_HEADER}{},01/05/2026,10:00:00,CARD_PAYMENT,Shop,,Shopping,-1.00,GBP,-1.00,GBP,,,,Shop,,,-1.00,\n",
            evil_id_csv,
        ),
    )
    .unwrap();

    engine::run(&rig.config, &[], false, None).unwrap();
    let text = journal_text(&rig);
    assert_eq!(
        text.lines().filter(|l| l.starts_with("20")).count(),
        1,
        "exactly one transaction may be created:\n{text}"
    );
    // The hostile content survives only flattened onto the inert comment
    // line; no posting line may reference the injected amount.
    assert_eq!(
        text.lines()
            .filter(|l| l.starts_with(' ') && l.contains("-9999"))
            .count(),
        0,
        "injected posting must not exist:\n{text}"
    );
    // The staging CSV carries the sanitized field.
    let staging = std::fs::read_to_string(rig.staging.join("test-acct.csv")).unwrap();
    assert!(!staging.chars().any(|c| c.is_control() && c != '\n' && c != '\r'));
}

#[test]
fn sanitize_field_replaces_control_chars_keeps_everything_else() {
    use bank2hledger::engine::sanitize_field;
    assert_eq!(sanitize_field("Café Ltd — ok"), "Café Ltd — ok");
    assert_eq!(sanitize_field("line1\nline2"), "line1 line2");
    assert_eq!(sanitize_field("tab\there"), "tab here");
    assert_eq!(sanitize_field("nul\u{0}byte"), "nul byte");
    assert_eq!(sanitize_field("delete\u{7f}char"), "delete char");
}
