use tauri::State;

use crate::db;
use crate::state::AppState;

/// The OpenCode model to use when a run does not specify one: the user's saved
/// default, or the built-in default (`opencode::models::DEFAULT_MODEL`) when
/// unset. A poisoned lock falls back to the built-in default rather than failing
/// the run.
pub fn resolve_default_model(state: &AppState) -> String {
    state
        .db
        .lock()
        .ok()
        .and_then(|connection| db::default_model(&connection))
        .unwrap_or_else(|| crate::opencode::models::DEFAULT_MODEL.to_string())
}

/// The saved default model, or `None` when the built-in default is in effect.
#[tauri::command]
pub fn get_default_model(state: State<AppState>) -> Result<Option<String>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    Ok(db::default_model(&connection))
}

/// Persists the default model. An empty/whitespace value clears the override so
/// the built-in default applies again.
#[tauri::command]
pub fn set_default_model(state: State<AppState>, model: Option<String>) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    match model
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(model) => db::set_setting(&connection, "defaultModel", &serde_json::json!(model)),
        None => {
            connection
                .execute("DELETE FROM settings WHERE key = ?1", ["defaultModel"])
                .map_err(|error| error.to_string())?;
            Ok(())
        }
    }
}
