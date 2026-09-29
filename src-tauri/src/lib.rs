mod commands;
mod db;
mod domain;
mod git;
mod implementation;
mod opencode;
mod planning;
mod repository;
mod state;
mod validation;
mod workspace;

use std::path::PathBuf;
use tauri::Manager;

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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app_root()?;
            for dir in [root.clone(), root.join("workspaces"), root.join("logs")] {
                std::fs::create_dir_all(&dir)?;
            }
            app.manage(state::AppState::load(root)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::list_repositories,
            commands::add_repository,
            commands::get_repository_status,
            commands::remove_repository,
            commands::list_workspaces,
            commands::create_workspace,
            commands::delete_workspace,
            commands::get_workspace_status,
            commands::get_workspace_stats,
            commands::get_workspace_diffs,
            commands::commit_changes,
            commands::push_branch,
            commands::start_agent,
            commands::send_agent_message,
            commands::stop_agent,
            commands::get_agent_status,
            commands::list_agents,
            commands::list_models,
            commands::start_planning,
            commands::start_implementation,
            commands::list_tasks,
            commands::get_task,
            commands::get_plan,
            commands::approve_plan,
            commands::reject_plan,
            commands::get_plan_approval,
            commands::revise_plan,
            commands::list_plan_versions,
            commands::list_plan_messages,
            commands::get_validation_config,
            commands::set_validation_config,
            commands::list_test_runs,
            commands::run_tests,
            commands::run_final_validation,
            commands::list_validation_runs,
            commands::get_repair_on_validation_failure,
            commands::set_repair_on_validation_failure,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app_handle.try_state::<state::AppState>() {
                    if let Ok(mut agents) = state.agents.lock() {
                        for (_, runtime) in agents.iter_mut() {
                            runtime.server.stop(runtime.run.session_id.as_deref());
                        }
                        agents.clear();
                    }
                }
            }
        });
}
