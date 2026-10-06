use tauri::{AppHandle, State};

use crate::commands::{git, github, implementation, planning, validation};
use crate::db;
use crate::domain::TaskStatus;
use crate::state::AppState;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The status a resume puts the task into before re-entering its stage, chosen
/// so the stage's own entry guard accepts it.
fn entry_status(interrupted_from: &str) -> Result<TaskStatus, String> {
    match interrupted_from {
        "PLANNING" => Ok(TaskStatus::Planning),
        "APPROVED" | "WORKSPACE_CREATING" | "IMPLEMENTING" => Ok(TaskStatus::Approved),
        "TESTING" | "REPAIRING" | "VALIDATING" => Ok(TaskStatus::Testing),
        "COMMITTING" => Ok(TaskStatus::Committing),
        "PR_CREATING" => Ok(TaskStatus::PrCreating),
        other => Err(format!("cannot resume from stage {other}")),
    }
}

/// Restarts the pipeline stage a task was interrupted in. Only meaningful for
/// tasks marked `INTERRUPTED` by crash recovery.
#[tauri::command]
pub fn resume_task(app: AppHandle, state: State<AppState>, task_id: String) -> Result<(), String> {
    let task = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        let task = db::get_task(&connection, &task_id)?
            .ok_or_else(|| format!("task not found: {task_id}"))?;
        if task.status != TaskStatus::Interrupted {
            return Err(format!(
                "task {task_id} is not interrupted (status {:?})",
                task.status
            ));
        }
        let interrupted_from = task
            .interrupted_from
            .clone()
            .ok_or_else(|| format!("task {task_id} has no interrupted stage"))?;
        let entry = entry_status(&interrupted_from)?;
        db::set_task_running_state(&connection, &task_id, entry, &now())?;
        task
    };

    crate::commands::cloud_socket::emit_task_updated(&app, &state, &task_id);

    match task.interrupted_from.as_deref() {
        Some("PLANNING") => planning::resume_planning(&app, &state, &task),
        Some("APPROVED") | Some("WORKSPACE_CREATING") | Some("IMPLEMENTING") => {
            implementation::start_implementation_inner(&app, &state, &task_id, None)
        }
        Some("TESTING") | Some("REPAIRING") | Some("VALIDATING") => {
            validation::start_test_repair(app.clone(), task_id.clone(), None);
            Ok(())
        }
        Some("COMMITTING") => {
            git::start_commit(app.clone(), task_id.clone());
            Ok(())
        }
        Some("PR_CREATING") => {
            github::start_publish(app.clone(), task_id.clone());
            Ok(())
        }
        Some(other) => Err(format!("cannot resume task from stage {other}")),
        None => Err(format!("task {task_id} has no interrupted stage")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_interrupted_stages_to_entry_statuses() {
        assert_eq!(entry_status("PLANNING").unwrap(), TaskStatus::Planning);
        assert_eq!(entry_status("APPROVED").unwrap(), TaskStatus::Approved);
        assert_eq!(entry_status("WORKSPACE_CREATING").unwrap(), TaskStatus::Approved);
        assert_eq!(entry_status("IMPLEMENTING").unwrap(), TaskStatus::Approved);
        assert_eq!(entry_status("TESTING").unwrap(), TaskStatus::Testing);
        assert_eq!(entry_status("REPAIRING").unwrap(), TaskStatus::Testing);
        assert_eq!(entry_status("VALIDATING").unwrap(), TaskStatus::Testing);
        assert_eq!(entry_status("COMMITTING").unwrap(), TaskStatus::Committing);
        assert_eq!(entry_status("PR_CREATING").unwrap(), TaskStatus::PrCreating);
        assert!(entry_status("FAILED").is_err());
    }
}
