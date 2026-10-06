use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::db;
use crate::domain::{AgentRun, Metric};
use crate::logging;
use crate::state::AppState;

/// How many rows to keep; older rows are pruned on every insert so the local
/// database does not grow without bound.
const MAX_ROWS: i64 = 10_000;

/// Records one metric: a queryable SQLite row plus a matching `metric` line in
/// the structured log. Best-effort — instrumentation must never break a task.
pub fn record(app: &AppHandle, name: &str, value: f64, task_id: Option<&str>, dims: Value) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    record_in_state(&state, name, value, task_id, dims);
}

pub fn record_in_state(
    state: &AppState,
    name: &str,
    value: f64,
    task_id: Option<&str>,
    dims: Value,
) {
    let metric = Metric {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        value,
        task_id: task_id.map(str::to_string),
        dims: dims.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    if let Ok(connection) = state.db.lock() {
        let _ = db::insert_metric(&connection, &metric);
        let _ = db::prune_metrics(&connection, MAX_ROWS);
    }

    let mut fields = json!({ "metric": name, "value": value });
    if let Some(task_id) = task_id {
        fields["taskId"] = Value::String(task_id.to_string());
    }
    if let Some(object) = dims.as_object() {
        for (key, value) in object {
            fields[key] = value.clone();
        }
    }
    logging::info("metrics", name, fields);
}

/// Emits the run-scoped metrics derived from a finished agent run: planning and
/// implementation durations, and agent failures.
pub fn record_run(app: &AppHandle, run: &AgentRun, success: bool, error: Option<&str>) {
    let duration = duration_ms(run.started_at.as_str(), run.completed_at.as_deref());
    let success_dim = json!({ "success": success });

    match run.mode.as_str() {
        "planning" => {
            if let Some(ms) = duration {
                record(app, "planning.duration_ms", ms, Some(&run.task_id), success_dim.clone());
            }
        }
        "implementation" => {
            if let Some(ms) = duration {
                record(
                    app,
                    "implementation.duration_ms",
                    ms,
                    Some(&run.task_id),
                    success_dim.clone(),
                );
            }
        }
        _ => {}
    }

    if !success {
        record(
            app,
            "agent.failures",
            1.0,
            Some(&run.task_id),
            json!({ "mode": run.mode, "error": truncate(error.unwrap_or_default(), 500) }),
        );
    }
}

/// Milliseconds between two RFC3339 timestamps, or `None` if either is
/// missing/unparseable (e.g. a run recovered from a crash).
pub fn duration_ms(started_at: &str, completed_at: Option<&str>) -> Option<f64> {
    let completed_at = completed_at?;
    let started = chrono::DateTime::parse_from_rfc3339(started_at).ok()?;
    let completed = chrono::DateTime::parse_from_rfc3339(completed_at).ok()?;
    let millis = completed.signed_duration_since(started).num_milliseconds();
    Some(millis.max(0) as f64)
}

pub fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut truncated: String = value.chars().take(max).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_duration_in_milliseconds() {
        assert_eq!(
            duration_ms("2026-10-07T00:00:00Z", Some("2026-10-07T00:00:02.500Z")),
            Some(2500.0)
        );
    }

    #[test]
    fn duration_is_none_without_finish_time() {
        assert_eq!(duration_ms("2026-10-07T00:00:00Z", None), None);
        assert_eq!(duration_ms("not-a-date", Some("2026-10-07T00:00:00Z")), None);
    }

    #[test]
    fn truncates_long_errors() {
        let long = "x".repeat(600);
        assert_eq!(truncate(&long, 500).chars().count(), 501);
        assert_eq!(truncate("short", 500), "short");
    }

    #[test]
    fn record_in_state_persists_a_metric_row() {
        let temp = tempfile::tempdir().unwrap();
        let state = crate::state::AppState::load_with_key_source(
            temp.path().to_path_buf(),
            crate::secure_store::KeySource::File,
        )
        .unwrap();

        record_in_state(
            &state,
            "files.changed",
            3.0,
            Some("task-1"),
            json!({ "additions": 5, "deletions": 1 }),
        );

        let connection = state.db.lock().unwrap();
        let metrics = db::list_metrics(&connection, Some("files.changed"), 10).unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].value, 3.0);
        assert_eq!(metrics[0].task_id.as_deref(), Some("task-1"));
        assert_eq!(metrics[0].dims["additions"], 5);
    }
}
