use crate::commands::repository::not_implemented;
use crate::domain::Workspace;

#[tauri::command]
pub fn create_workspace(
    repository_id: String,
    task_id: String,
) -> Result<Workspace, String> {
    let _ = (repository_id, task_id);
    Err(not_implemented("create_workspace (Phase 3)"))
}

#[tauri::command]
pub fn delete_workspace(workspace_path: String) -> Result<(), String> {
    let _ = workspace_path;
    Err(not_implemented("delete_workspace (Phase 3)"))
}

#[tauri::command]
pub fn get_workspace_status(workspace_path: String) -> Result<Workspace, String> {
    let _ = workspace_path;
    Err(not_implemented("get_workspace_status (Phase 3)"))
}
