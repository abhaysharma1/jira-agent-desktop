use crate::commands::repository::not_implemented;
use crate::domain::ValidationResult;

#[tauri::command]
pub fn get_changed_files(workspace_path: String) -> Result<Vec<String>, String> {
    let _ = workspace_path;
    Err(not_implemented("get_changed_files (Phase 2)"))
}

#[tauri::command]
pub fn get_diff(workspace_path: String) -> Result<String, String> {
    let _ = workspace_path;
    Err(not_implemented("get_diff (Phase 2)"))
}

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
pub fn run_validation(
    workspace_path: String,
    command: String,
) -> Result<ValidationResult, String> {
    let _ = (workspace_path, command);
    Err(not_implemented("run_validation (Phase 13)"))
}
