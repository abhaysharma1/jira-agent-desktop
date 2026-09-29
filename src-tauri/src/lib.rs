mod commands;
mod domain;
mod git;
mod opencode;
mod workspace;

use std::path::PathBuf;

const APP_DIR: &str = ".jira-agent";

fn app_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .ok_or("could not resolve home directory")?;
    Ok(PathBuf::from(home).join(APP_DIR))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .setup(|_app| {
            let root = app_root()?;
            for dir in [root.clone(), root.join("workspaces"), root.join("logs")] {
                std::fs::create_dir_all(&dir)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::list_repositories,
            commands::add_repository,
            commands::get_repository_status,
            commands::create_workspace,
            commands::delete_workspace,
            commands::get_workspace_status,
            commands::get_changed_files,
            commands::get_diff,
            commands::commit_changes,
            commands::push_branch,
            commands::run_validation,
            commands::start_planning,
            commands::start_implementation,
            commands::send_agent_message,
            commands::stop_agent,
            commands::get_agent_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
