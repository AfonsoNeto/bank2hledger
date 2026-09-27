//! Tauri commands: thin wrappers over the `bank2hledger` core library.
//! All business logic (parsing, dedup, rules, hledger invocation) lives in
//! the shared crate — nothing here decides account names or touches files
//! beyond remembering which workspace the GUI opened last.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bank2hledger::{config, engine, profiles, status};
use serde::Serialize;

#[derive(Default)]
pub struct AppState {
    workspace: Mutex<Option<Workspace>>,
}

struct Workspace {
    config_path: PathBuf,
    config: config::Config,
}

// --- DTOs sent to the frontend ---

#[derive(Serialize)]
pub struct AccountInfo {
    pub name: String,
    pub profile: String,
    pub hledger_account: String,
    pub rules_file: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub config_path: String,
    pub journal: String,
    pub in_dir: String,
    pub staging_dir: String,
    pub rules_dir: String,
    pub accounts: Vec<AccountInfo>,
    pub hledger_version: Option<String>,
}

#[derive(Serialize)]
pub struct InboxFile {
    pub file_name: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Serialize)]
pub struct AccountInbox {
    pub account: String,
    pub profile: String,
    pub files: Vec<InboxFile>,
    /// The aqua_pdf profile shells out to `pdftotext` (poppler), which is
    /// not bundled with the GUI — surfaced so the UI can warn up front.
    pub needs_pdftotext: bool,
}

#[derive(Serialize)]
pub struct PreviewTransaction {
    pub date: String,
    pub payee: String,
    pub amount: String,
    pub currency: String,
    pub external_id: Option<String>,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct PreviewDto {
    pub account: String,
    pub new: Vec<PreviewTransaction>,
    pub already_seen: usize,
}

#[derive(Serialize)]
pub struct ImportResult {
    pub account: String,
    pub new_count: usize,
    pub already_seen: usize,
    /// hledger's journal-format rendering of what was (or would be) added.
    pub preview: Option<String>,
}

#[derive(Serialize)]
pub struct FileEntry {
    pub file_name: String,
    pub path: String,
}

// --- helpers ---

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Where the GUI remembers the last-opened workspace. GUI-local state only —
/// the core CLI never reads this.
fn remembered_path_file() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("bank2hledger-gui").join("workspace.txt"))
}

fn find_config(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(PathBuf::from(p));
    }
    if let Some(remembered) = remembered_path_file() {
        if let Ok(p) = std::fs::read_to_string(&remembered) {
            let p = PathBuf::from(p.trim());
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let local = PathBuf::from("bank2hledger.toml");
    if local.exists() {
        return Some(local);
    }
    if let Ok(env) = std::env::var("BANK2HLEDGER_CONFIG") {
        return Some(PathBuf::from(env));
    }
    let default = dirs::config_dir()
        .map(|d| d.join("bank2hledger").join("config.toml"))
        .filter(|p| p.is_file())?;
    Some(default)
}

fn hledger_version() -> Option<String> {
    let out = engine::hledger_cmd().arg("--version").output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn workspace_info(config: &config::Config, config_path: &Path) -> WorkspaceInfo {
    WorkspaceInfo {
        config_path: config_path.display().to_string(),
        journal: config.journal.display().to_string(),
        in_dir: config.in_dir.display().to_string(),
        staging_dir: config.staging_dir.display().to_string(),
        rules_dir: config.rules_dir.display().to_string(),
        accounts: config
            .accounts
            .iter()
            .map(|a| AccountInfo {
                name: a.name.clone(),
                profile: a.profile.clone(),
                hledger_account: a.hledger_account.clone(),
                rules_file: bank2hledger::rules::rules_path(&config.rules_dir, a)
                    .exists()
                    .then(|| {
                        bank2hledger::rules::rules_path(&config.rules_dir, a)
                            .display()
                            .to_string()
                    }),
            })
            .collect(),
        hledger_version: hledger_version(),
    }
}

fn remember_workspace(config_path: &Path) {
    if let Some(file) = remembered_path_file() {
        if std::fs::create_dir_all(file.parent().unwrap_or(Path::new("."))).is_ok() {
            let _ = std::fs::write(&file, config_path.display().to_string());
        }
    }
}

// --- commands ---

#[tauri::command]
pub fn load_workspace(
    path: Option<String>,
    state: tauri::State<AppState>,
) -> Result<WorkspaceInfo, String> {
    let config_path = find_config(path.as_deref()).ok_or_else(|| {
        "No bank2hledger.toml found — create a new workspace or open an existing config file."
            .to_string()
    })?;
    let config = config::Config::load(&config_path).map_err(err)?;
    remember_workspace(&config_path);
    *state.workspace.lock().unwrap() = Some(Workspace {
        config_path: config_path.clone(),
        config,
    });
    let ws = state.workspace.lock().unwrap();
    let ws = ws.as_ref().unwrap();
    Ok(workspace_info(&ws.config, &ws.config_path))
}

#[tauri::command]
pub fn init_workspace(
    dir: String,
    force: bool,
    state: tauri::State<AppState>,
) -> Result<WorkspaceInfo, String> {
    let config_path = Path::new(&dir).join("bank2hledger.toml");
    bank2hledger::init::run(Some(&config_path), force).map_err(err)?;
    load_workspace(Some(config_path.display().to_string()), state)
}

#[tauri::command]
pub fn inbox_files(state: tauri::State<AppState>) -> Result<Vec<AccountInbox>, String> {
    let ws = state.workspace.lock().unwrap();
    let ws = ws.as_ref().ok_or("No workspace loaded")?;
    let entries = std::fs::read_dir(&ws.config.in_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(ws
        .config
        .accounts
        .iter()
        .map(|a| {
            let mut files = entries
                .iter()
                .filter(|p| {
                    profiles::matches_account(
                        p.file_name().unwrap_or_default().to_string_lossy().as_ref(),
                        &a.name,
                    )
                })
                .map(|p| InboxFile {
                    file_name: p
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: p.display().to_string(),
                    size_bytes: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
                })
                .collect::<Vec<_>>();
            files.sort_by(|x, y| x.file_name.cmp(&y.file_name));
            AccountInbox {
                account: a.name.clone(),
                profile: a.profile.clone(),
                files,
                needs_pdftotext: a.profile == "aqua_pdf",
            }
        })
        .collect())
}

#[tauri::command]
pub fn preview_import(
    account: String,
    state: tauri::State<AppState>,
) -> Result<PreviewDto, String> {
    let ws = state.workspace.lock().unwrap();
    let ws = ws.as_ref().ok_or("No workspace loaded")?;
    let out = engine::preview_account(&ws.config, &account, None).map_err(err)?;
    Ok(PreviewDto {
        account: out.account,
        already_seen: out.already_seen,
        new: out
            .new
            .iter()
            .map(|t| PreviewTransaction {
                date: t.date.format("%Y-%m-%d").to_string(),
                payee: t.payee.clone(),
                amount: t.amount.normalize().to_string(),
                currency: t.currency.clone(),
                external_id: t.external_id.clone(),
                notes: t.notes.clone(),
            })
            .collect(),
    })
}

#[tauri::command]
pub fn run_import(
    account: String,
    dry_run: bool,
    state: tauri::State<AppState>,
) -> Result<ImportResult, String> {
    let ws = state.workspace.lock().unwrap();
    let ws = ws.as_ref().ok_or("No workspace loaded")?;
    status::require_journal(&ws.config.journal).map_err(err)?;
    // engine::run returns exactly one outcome per requested account.
    let mut outcomes = engine::run(&ws.config, &[account], dry_run, None).map_err(err)?;
    let o = outcomes
        .pop()
        .ok_or_else(|| "engine returned no outcome".to_string())?;
    Ok(ImportResult {
        account: o.account,
        new_count: o.new_count,
        already_seen: o.already_seen,
        preview: o.preview,
    })
}

#[tauri::command]
pub fn get_status(state: tauri::State<AppState>) -> Result<String, String> {
    let ws = state.workspace.lock().unwrap();
    let ws = ws.as_ref().ok_or("No workspace loaded")?;
    status::balances(&ws.config, &[]).map_err(err)
}

#[tauri::command]
pub fn list_rules_files(state: tauri::State<AppState>) -> Result<Vec<FileEntry>, String> {
    let ws = state.workspace.lock().unwrap();
    let ws = ws.as_ref().ok_or("No workspace loaded")?;
    let mut files = std::fs::read_dir(&ws.config.rules_dir)
        .map_err(|e| err(e.into()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .map(|p| FileEntry {
            file_name: p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: p.display().to_string(),
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(files)
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}
