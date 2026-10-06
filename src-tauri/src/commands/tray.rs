use tauri::{AppHandle, State};

use crate::db;
use crate::state::AppState;

/// Updates the tray's "N pending tickets / N agents running" labels. The
/// frontend owns the counts because it already holds the task/agent state.
#[tauri::command]
pub fn set_tray_stats(app: AppHandle, pending: u32, running: u32) {
    crate::tray::set_stats(&app, pending, running);
}

/// Shows and focuses the main window (used by the recovery banner/palette).
#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    crate::tray::show_main(&app);
}

/// Exits through the normal shutdown path so agent servers are stopped.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn get_close_to_tray(state: State<AppState>) -> Result<bool, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    Ok(db::close_to_tray(&connection))
}

#[tauri::command]
pub fn set_close_to_tray(state: State<AppState>, enabled: bool) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::set_setting(&connection, "closeToTray", &serde_json::json!(enabled))
}
