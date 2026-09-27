//! Black-box smoke tests driving the compiled binary: init, import
//! (dry-run and real), status, and the guard rails around them.

use std::path::{Path, PathBuf};
use std::process::Output;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_bank2hledger")
}

fn hledger_available() -> bool {
    std::process::Command::new(std::env::var("HLEDGER").unwrap_or_else(|_| "hledger".into()))
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(cwd: &Path, args: &[&str]) -> Output {
    std::process::Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env_remove("BANK2HLEDGER_CONFIG")
        .output()
        .expect("binary should run")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn version_and_help_work_offline() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(dir.path(), &["--version"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("bank2hledger"));

    let o = run(dir.path(), &["--help"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("import"), "help lists subcommands");
}

#[test]
fn init_creates_config_dirs_and_rules() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(dir.path(), &["init"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(dir.path().join("bank2hledger.toml").exists());
    assert!(dir.path().join("in").exists());
    assert!(dir.path().join("staging").exists());
    assert!(dir.path().join("rules").exists());
    // The template's example account gets a starter rules file.
    assert!(dir.path().join("rules/monzo-personal.rules").exists());
    let rules = std::fs::read_to_string(dir.path().join("rules/monzo-personal.rules")).unwrap();
    assert!(
        rules.contains("account1 assets:banks:monzo:personal"),
        "{rules}"
    );
}

#[test]
fn init_refuses_to_clobber_and_force_overwrites_config_only() {
    let dir = tempfile::tempdir().unwrap();
    assert!(run(dir.path(), &["init"]).status.success());
    let rules = dir.path().join("rules/monzo-personal.rules");
    std::fs::write(&rules, "# user edit\n").unwrap();

    let o = run(dir.path(), &["init"]);
    assert!(
        !o.status.success(),
        "second init without --force must refuse"
    );
    assert!(stderr(&o).contains("already exists"));

    let o = run(dir.path(), &["init", "--force"]);
    assert!(o.status.success(), "{}", stderr(&o));
    // Config was overwritten; rules files were NOT.
    assert!(dir.path().join("bank2hledger.toml").exists());
    assert_eq!(std::fs::read_to_string(&rules).unwrap(), "# user edit\n");
}

#[test]
fn import_before_journal_exists_is_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init"]);
    let o = run(dir.path(), &["import"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("journal"), "{:}", stderr(&o));
}

#[test]
fn full_cli_loop_dry_run_import_status() {
    if !hledger_available() {
        eprintln!("skipping: hledger not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    assert!(run(dir.path(), &["init"]).status.success());
    std::fs::write(dir.path().join("2026.journal"), "").unwrap();

    // Drop an export under the template account's name.
    std::fs::copy(
        fixture("monzo-sample.csv"),
        dir.path().join("in/monzo-personal.csv"),
    )
    .unwrap();

    // Dry run: preview, journal untouched.
    let o = run(dir.path(), &["import", "--dry-run"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("dry run"), "{:}", stdout(&o));
    assert!(stdout(&o).contains("expenses:"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("2026.journal")).unwrap(),
        ""
    );

    // Real import.
    let o = run(dir.path(), &["import"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(
        stdout(&o).contains("30 new transaction(s)"),
        "{}",
        stdout(&o)
    );
    let journal = std::fs::read_to_string(dir.path().join("2026.journal")).unwrap();
    assert!(journal.contains("assets:banks:monzo:personal"));
    assert_eq!(journal.lines().filter(|l| l.starts_with("20")).count(), 30);

    // Re-import: nothing new.
    let o = run(dir.path(), &["import"]);
    assert!(stdout(&o).contains("nothing new"), "{}", stdout(&o));

    // Status reports the imported account's balance.
    let o = run(dir.path(), &["status"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(
        stdout(&o).contains("assets:banks:monzo:personal"),
        "{}",
        stdout(&o)
    );
}

#[test]
fn status_with_unknown_account_errors_cleanly() {
    if !hledger_available() {
        eprintln!("skipping: hledger not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init"]);
    std::fs::write(dir.path().join("j.journal"), "").unwrap();
    let o = run(dir.path(), &["status", "--account", "ghost"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("unknown account"), "{}", stderr(&o));
}

#[test]
fn config_flag_points_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("elsewhere.toml");
    std::fs::write(
        &cfg,
        "journal = \"j.journal\"\n[[accounts]]\nname = \"x\"\nprofile = \"monzo_csv\"\nhledger_account = \"assets:x\"\n",
    )
    .unwrap();
    // Run from a directory with no local config; --config must win.
    let empty = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("j.journal"), "").unwrap();
    let o = run(empty.path(), &["--config", cfg.to_str().unwrap(), "import"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("x: nothing new"), "{}", stdout(&o));
}
