use std::path::Path;

use tauri::State;

use crate::db;
use crate::domain::{Repository, RepositoryStatus};
use crate::git;
use crate::repository;
use crate::state::AppState;

pub fn not_implemented(feature: &str) -> String {
    format!("{feature} is not implemented yet")
}

fn build_status(repository: &Repository) -> RepositoryStatus {
    let path = Path::new(&repository.local_path);
    let changed_files = git::changed_files(path).unwrap_or_default();
    RepositoryStatus {
        repository_id: repository.id.clone(),
        is_git_repository: git::is_git_repository(path),
        current_branch: git::current_branch(path),
        remote_url: git::remote_url(path),
        is_clean: changed_files.is_empty(),
        changed_files: changed_files.len() as u32,
    }
}

#[tauri::command]
pub fn list_repositories(state: State<AppState>) -> Result<Vec<Repository>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::list_repositories(&connection)
}

#[tauri::command]
pub fn add_repository(
    state: State<AppState>,
    local_path: String,
) -> Result<Repository, String> {
    let repository = repository::detect(Path::new(&local_path))?;
    let connection = state.db.lock().map_err(|error| error.to_string())?;

    if db::list_repositories(&connection)?
        .iter()
        .any(|existing| existing.local_path.eq_ignore_ascii_case(&repository.local_path))
    {
        return Err(format!("{} is already registered", repository.name));
    }

    db::insert_repository(&connection, &repository)?;
    Ok(repository)
}

#[tauri::command]
pub fn get_repository_status(
    state: State<AppState>,
    repository_id: String,
) -> Result<RepositoryStatus, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    let repository = db::get_repository(&connection, &repository_id)?
        .ok_or_else(|| format!("repository not found: {repository_id}"))?;
    Ok(build_status(&repository))
}

#[tauri::command]
pub fn remove_repository(
    state: State<AppState>,
    repository_id: String,
) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::remove_repository(&connection, &repository_id)
}
