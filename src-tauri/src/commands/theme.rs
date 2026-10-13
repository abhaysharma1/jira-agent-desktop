use tauri::State;

use crate::db;
use crate::state::AppState;

/// The stored theme preference (`"light"`, `"dark"`, or `"system"`).
#[tauri::command]
pub fn get_theme(state: State<AppState>) -> Result<String, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    Ok(db::theme(&connection))
}

/// Persists the theme preference. Unknown values are rejected so the stored
/// setting can never drift away from what the UI understands.
#[tauri::command]
pub fn set_theme(state: State<AppState>, theme: String) -> Result<(), String> {
    if !db::is_valid_theme(&theme) {
        return Err(format!("invalid theme: {theme}"));
    }
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::set_setting(&connection, "theme", &serde_json::json!(theme))
}
