use tauri::State;

use crate::db;
use crate::domain::{Workspace, WorkspaceStatus};
use crate::state::AppState;
use crate::workspace::WorkspaceManager;

fn manager(state: &AppState) -> WorkspaceManager {
    WorkspaceManager::new(state.data_dir.join("workspaces"))
}

#[tauri::command]
pub fn list_workspaces(state: State<AppState>) -> Result<Vec<Workspace>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::list_workspaces(&connection)
}

#[tauri::command]
pub fn create_workspace(
    state: State<AppState>,
    repository_id: String,
    task_id: String,
    branch_slug: Option<String>,
) -> Result<Workspace, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    let repository = db::get_repository(&connection, &repository_id)?
        .ok_or_else(|| format!("repository not found: {repository_id}"))?;

    if db::get_workspace_by_task(&connection, &task_id)?.is_some() {
        return Err(format!("workspace already tracked for task {task_id}"));
    }

    let workspace = manager(&state).create(&repository, &task_id, branch_slug.as_deref())?;
    db::insert_workspace(&connection, &workspace)?;
    Ok(workspace)
}

#[tauri::command]
pub fn delete_workspace(state: State<AppState>, task_id: String) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    let workspace = db::get_workspace_by_task(&connection, &task_id)?
        .ok_or_else(|| format!("workspace not found for task: {task_id}"))?;
    let repository = db::get_repository(&connection, &workspace.repository_id)?
        .ok_or_else(|| format!("repository not found: {}", workspace.repository_id))?;

    manager(&state).remove(&repository, &workspace)?;
    db::remove_workspace_by_task(&connection, &task_id)
}

#[tauri::command]
pub fn get_workspace_status(
    state: State<AppState>,
    task_id: String,
) -> Result<WorkspaceStatus, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    let workspace = db::get_workspace_by_task(&connection, &task_id)?
        .ok_or_else(|| format!("workspace not found for task: {task_id}"))?;
    Ok(manager(&state).status(&workspace))
}
