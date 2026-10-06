use tauri::State;

use crate::db;
use crate::domain::SearchHit;
use crate::state::AppState;

/// Hits per source (tickets/plans/pull requests/agent runs) unless overridden.
const DEFAULT_LIMIT: usize = 8;
const MAX_LIMIT: usize = 50;

/// Backs the Ctrl+K palette: one query, grouped results across the local store.
#[tauri::command]
pub fn search(
    state: State<AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<SearchHit>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::search(&connection, query, limit)
}
