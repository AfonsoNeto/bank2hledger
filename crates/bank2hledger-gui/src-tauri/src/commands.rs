//! Tauri commands: thin wrappers over the `bank2hledger` core library.
//! All business logic (parsing, dedup, rules, hledger invocation) lives in
//! the shared crate — nothing here decides account names or touches files
//! beyond remembering which workspace the GUI opened last.
//!
//! Anything that touches disk or spawns a process runs on the blocking
//! thread pool (`spawn_blocking`): synchronous commands would run on the
//! main thread and freeze the window for the duration.

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

type WsLock<'a> = std::sync::MutexGuard<'a, Option<Workspace>>;

/// Lock that survives a panic in another command (poisoned mutex) instead of
/// cascading panics into every later command.
fn lock_state<'a>(state: &'a tauri::State<AppState>) -> WsLock<'a> {
    state.workspace.lock().unwrap_or_else(|p| p.into_inner())
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

fn workspace_info(
    config: &config::Config,
    config_path: &Path,
    hledger_version: Option<String>,
) -> WorkspaceInfo {
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
        hledger_version,
    }
}

fn remember_workspace(config_path: &Path) {
    if let Some(file) = remembered_path_file() {
        if std::fs::create_dir_all(file.parent().unwrap_or(Path::new("."))).is_ok() {
            // Same guard as the core's user-data writes: don't follow a
            // symlink planted at the state file's path.
            let _ = bank2hledger::fs_guard::write_refusing_symlinks(
                &file,
                config_path.display().to_string().as_bytes(),
            );
        }
    }
}

/// Clone the loaded workspace out of the state so blocking work can run
/// without holding the lock.
fn current_workspace(state: &tauri::State<AppState>) -> Result<(PathBuf, config::Config), String> {
    let ws = lock_state(state);
    let ws = ws.as_ref().ok_or("No workspace loaded")?;
    Ok((ws.config_path.clone(), ws.config.clone()))
}

// --- commands ---

#[tauri::command]
pub async fn load_workspace(
    path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<WorkspaceInfo, String> {
    let (config_path, config, version) = tauri::async_runtime::spawn_blocking(move || {
        let config_path = find_config(path.as_deref()).ok_or_else(|| {
            "No bank2hledger.toml found — create a new workspace or open an existing config file."
                .to_string()
        })?;
        let config = config::Config::load(&config_path).map_err(err)?;
        remember_workspace(&config_path);
        Ok::<_, String>((config_path, config, hledger_version()))
    })
    .await
    .map_err(|e| e.to_string())??;

    let info = {
        let mut ws = lock_state(&state);
        *ws = Some(Workspace {
            config_path: config_path.clone(),
            config,
        });
        workspace_info(&ws.as_ref().unwrap().config, &config_path, version)
    };
    Ok(info)
}

#[tauri::command]
pub async fn init_workspace(
    dir: String,
    force: bool,
    state: tauri::State<'_, AppState>,
) -> Result<WorkspaceInfo, String> {
    let config_path = Path::new(&dir).join("bank2hledger.toml");
    tauri::async_runtime::spawn_blocking({
        let config_path = config_path.clone();
        move || bank2hledger::init::run(Some(&config_path), force).map_err(err)
    })
    .await
    .map_err(|e| e.to_string())??;
    load_workspace(Some(config_path.display().to_string()), state).await
}

#[tauri::command]
pub fn inbox_files(state: tauri::State<AppState>) -> Result<Vec<AccountInbox>, String> {
    let (_, config) = current_workspace(&state)?;
    let entries = std::fs::read_dir(&config.in_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(config
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
pub async fn preview_import(
    account: String,
    state: tauri::State<'_, AppState>,
) -> Result<PreviewDto, String> {
    let (_, config) = current_workspace(&state)?;
    let out = tauri::async_runtime::spawn_blocking(move || {
        engine::preview_account(&config, &account, None).map_err(err)
    })
    .await
    .map_err(|e| e.to_string())??;

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
pub async fn run_import(
    account: String,
    dry_run: bool,
    state: tauri::State<'_, AppState>,
) -> Result<ImportResult, String> {
    let (_, config) = current_workspace(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        status::require_journal(&config.journal).map_err(err)?;
        // engine::run returns exactly one outcome per requested account.
        let mut outcomes = engine::run(&config, &[account], dry_run, None).map_err(err)?;
        let o = outcomes
            .pop()
            .ok_or_else(|| "engine returned no outcome".to_string())?;
        Ok(ImportResult {
            account: o.account,
            new_count: o.new_count,
            already_seen: o.already_seen,
            preview: o.preview,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_status(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let (_, config) = current_workspace(&state)?;
    tauri::async_runtime::spawn_blocking(move || status::balances(&config, &[]).map_err(err))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn list_rules_files(state: tauri::State<AppState>) -> Result<Vec<FileEntry>, String> {
    let (_, config) = current_workspace(&state)?;
    let mut files = std::fs::read_dir(&config.rules_dir)
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
pub fn open_path(path: String, state: tauri::State<AppState>) -> Result<(), String> {
    // Defense in depth against a compromised renderer: only open paths
    // inside the loaded workspace (config, journal, data dirs). open_path
    // launches files with their default application, so an unrestricted
    // path would be arbitrary program execution.
    let (config_path, config) = current_workspace(&state)?;
    let target = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
    let mut roots = vec![
        config_path.clone(),
        config.journal.clone(),
        config.in_dir.clone(),
        config.staging_dir.clone(),
        config.rules_dir.clone(),
    ];
    if let Some(parent) = config_path.parent() {
        roots.push(parent.to_path_buf());
    }
    let allowed = roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|r| target == r || target.starts_with(&r))
            .unwrap_or(false)
    });
    if !allowed {
        return Err(format!("path is outside the loaded workspace: {path}"));
    }
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}
