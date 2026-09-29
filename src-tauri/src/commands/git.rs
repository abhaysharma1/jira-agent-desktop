use tauri::State;

use crate::commands::repository::not_implemented;
use crate::db;
use crate::domain::{DiffStats, FileDiff};
use crate::state::AppState;

#[tauri::command]
pub fn commit_changes(workspace_path: String, message: String) -> Result<String, String> {
    let _ = (workspace_path, message);
    Err(not_implemented("commit_changes (Phase 15)"))
}

#[tauri::command]
pub fn push_branch(workspace_path: String) -> Result<(), String> {
    let _ = workspace_path;
    Err(not_implemented("push_branch (Phase 16)"))
}

#[tauri::command]
pub fn get_workspace_stats(state: State<AppState>, task_id: String) -> Result<DiffStats, String> {
    let workspace_path = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        let task = db::get_task(&connection, &task_id)?
            .ok_or_else(|| format!("task not found: {task_id}"))?;
        task.workspace_path
            .ok_or_else(|| "task has no workspace yet".to_string())?
    };

    let path = std::path::Path::new(&workspace_path);
    if !path.is_dir() {
        return Err(format!("workspace path does not exist: {workspace_path}"));
    }

    crate::git::worktree_stats(path)
}

#[tauri::command]
pub fn get_workspace_diffs(
    state: State<AppState>,
    task_id: String,
) -> Result<Vec<FileDiff>, String> {
    let workspace_path = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        let task = db::get_task(&connection, &task_id)?
            .ok_or_else(|| format!("task not found: {task_id}"))?;
        task.workspace_path
            .ok_or_else(|| "task has no workspace yet".to_string())?
    };

    let path = std::path::Path::new(&workspace_path);
    if !path.is_dir() {
        return Err(format!("workspace path does not exist: {workspace_path}"));
    }

    let stats = crate::git::worktree_stats(path)?;
    Ok(stats
        .files
        .iter()
        .map(|file| crate::git::file_diff(path, &file.path))
        .collect())
}
