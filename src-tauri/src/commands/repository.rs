use crate::domain::{Repository, RepositoryStatus};

pub fn not_implemented(feature: &str) -> String {
    format!("{feature} is not implemented yet")
}

#[tauri::command]
pub fn list_repositories() -> Vec<Repository> {
    Vec::new()
}

#[tauri::command]
pub fn add_repository(local_path: String) -> Result<Repository, String> {
    let _ = local_path;
    Err(not_implemented("add_repository (Phase 2)"))
}

#[tauri::command]
pub fn get_repository_status(repository_id: String) -> Result<RepositoryStatus, String> {
    let _ = repository_id;
    Err(not_implemented("get_repository_status (Phase 2)"))
}
