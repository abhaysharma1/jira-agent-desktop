mod cloud;
mod commands;
mod db;
mod domain;
mod git;
mod github;
mod implementation;
mod jira;
mod logging;
mod metrics;
mod oauth;
mod opencode;
mod panic_hook;
mod planning;
mod repository;
mod secure_store;
mod secrets;
mod state;
#[cfg(test)]
mod test_support;
mod tray;
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
        .plugin(tauri_plugin_notification::init())
        // The close button hides to the tray when the user hasn't turned that
        // off; tray "Quit" bypasses this and exits through the normal path.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let hide = app
                    .try_state::<state::AppState>()
                    .and_then(|state| state.db.lock().ok().map(|connection| db::close_to_tray(&connection)))
                    .unwrap_or(false);
                if hide {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let root = app_root()?;
            for dir in [root.clone(), root.join("workspaces"), root.join("logs")] {
                std::fs::create_dir_all(&dir)?;
            }
            // Phase 26: structured logs land in ~/.jira-agent/logs.
            crate::logging::init(&root.join("logs"));
            // Phase 27: capture uncaught panics in that same log.
            crate::panic_hook::install();
            crate::logging::info(
                "app",
                "started",
                serde_json::json!({ "version": env!("CARGO_PKG_VERSION") }),
            );
            app.manage(state::AppState::load(root)?);
            crate::tray::build(app.handle())?;
            // Terminate any opencode server left behind by a previous crash.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                if let Some(state) = handle.try_state::<state::AppState>() {
                    match crate::opencode::orphans::sweep(&state) {
                        Ok(killed) if killed > 0 => crate::logging::warn(
                            "crash-recovery",
                            "terminated orphaned opencode servers",
                            serde_json::json!({ "killed": killed }),
                        ),
                        Ok(_) => {}
                        Err(error) => crate::logging::error(
                            "crash-recovery",
                            "orphan sweep failed",
                            serde_json::json!({ "error": error }),
                        ),
                    }
                }
            });
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
            commands::resume_task,
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
            commands::set_github_token,
            commands::get_github_account,
            commands::create_pull_request,
            commands::get_pull_request,
            commands::start_jira_oauth,
            commands::get_jira_connection_info,
            commands::register_jira_webhook,
            commands::disconnect_jira,
            commands::get_jira_account,
            commands::get_jira_status_map,
            commands::set_jira_status_map,
            commands::get_jira_issue,
            commands::list_jira_transitions,
            commands::get_jira_sync,
            commands::sync_task_jira,
            commands::list_jira_project_repos,
            commands::set_jira_project_repo,
            commands::delete_jira_project_repo,
            commands::login_cloud,
            commands::get_cloud_account,
            commands::logout_cloud,
            commands::start_cloud_sync,
            commands::stop_cloud_sync,
            commands::mark_notification_read,
            commands::skip_ticket,
            commands::prepare_ticket_intake,
            commands::search,
            commands::set_tray_stats,
            commands::show_main_window,
            commands::quit_app,
            commands::get_close_to_tray,
            commands::set_close_to_tray,
            commands::list_metrics,
            commands::get_logs_dir,
            commands::open_logs_folder,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                crate::logging::info("app", "exiting", serde_json::json!({}));
                if let Some(state) = app_handle.try_state::<state::AppState>() {
                    let run_ids = if let Ok(mut agents) = state.agents.lock() {
                        let ids: Vec<String> = agents.keys().cloned().collect();
                        for (_, runtime) in agents.iter_mut() {
                            runtime.server.stop(runtime.run.session_id.as_deref());
                        }
                        agents.clear();
                        ids
                    } else {
                        Vec::new()
                    };
                    if let Ok(connection) = state.db.lock() {
                        for run_id in run_ids {
                            let _ = db::delete_opencode_server(&connection, &run_id);
                        }
                    }
                }
            }
        });
}
