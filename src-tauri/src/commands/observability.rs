use tauri::{AppHandle, Manager, State};

use crate::db;
use crate::domain::Metric;
use crate::state::AppState;

/// Recent metric events for the Settings "Diagnostics" card. Optionally
/// filtered to a single metric name.
#[tauri::command]
pub fn list_metrics(
    state: State<AppState>,
    name: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<Metric>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    let limit = limit.unwrap_or(50).min(500);
    db::list_metrics(&connection, name.as_deref(), limit)
}

/// Absolute path to the structured log directory, for display.
#[tauri::command]
pub fn get_logs_dir(state: State<AppState>) -> Result<String, String> {
    Ok(state.data_dir.join("logs").to_string_lossy().to_string())
}

/// Opens `~/.jira-agent/logs` in the OS file manager.
#[tauri::command]
pub fn open_logs_folder(app: AppHandle) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "state unavailable".to_string())?;
    let dir = state.data_dir.join("logs");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    tauri_plugin_opener::open_path(&dir, None::<&str>).map_err(|error| error.to_string())
}
