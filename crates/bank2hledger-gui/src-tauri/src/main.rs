#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::load_workspace,
            commands::init_workspace,
            commands::inbox_files,
            commands::preview_import,
            commands::run_import,
            commands::get_status,
            commands::list_rules_files,
            commands::open_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running the bank2hledger GUI");
}
