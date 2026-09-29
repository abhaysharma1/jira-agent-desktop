use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::commands::runtime;
use crate::db;
use crate::domain::{AgentRun, AgentRunStatus, TaskStatus};
use crate::implementation::build_implementation_prompt;
use crate::opencode::{self, PromptCallbacks};
use crate::planning::TicketInput;
use crate::state::AppState;
use crate::workspace::WorkspaceManager;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn slug_from_title(title: &str) -> String {
    title
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join(" ")
}

fn emit_task(app: &AppHandle, state: &AppState, task_id: &str) {
    if let Ok(connection) = state.db.lock() {
        if let Ok(Some(task)) = db::get_task(&connection, task_id) {
            let _ = app.emit("task://updated", &task);
        }
    }
}

pub fn start_implementation_inner(
    app: &AppHandle,
    state: &AppState,
    task_id: &str,
    model: Option<String>,
) -> Result<(), String> {
    let (task, repository, plan_content) = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        let task = db::get_task(&connection, task_id)?
            .ok_or_else(|| format!("task not found: {task_id}"))?;
        if !task.status.can_transition_to(TaskStatus::WorkspaceCreating) {
            return Err(format!(
                "task {task_id} cannot start implementation from status {:?}",
                task.status
            ));
        }
        let repository = db::get_repository(&connection, &task.repository_id)?
            .ok_or_else(|| format!("repository not found: {}", task.repository_id))?;
        let version_number = task
            .approved_plan_version
            .ok_or_else(|| "task has no approved plan version".to_string())?;
        let plan_version = db::get_plan_version(&connection, task_id, version_number)?
            .ok_or_else(|| format!("approved plan version {version_number} not found"))?;
        (task, repository, plan_version.content)
    };

    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::update_task_status(&connection, task_id, TaskStatus::WorkspaceCreating, &now())?;
    }
    emit_task(app, state, task_id);

    let manager = WorkspaceManager::new(state.data_dir.join("workspaces"));
    let workspace = {
        let existing = {
            let connection = state.db.lock().map_err(|error| error.to_string())?;
            db::get_workspace_by_task(&connection, task_id)?
        };
        match existing {
            Some(workspace) => workspace,
            None => {
                let slug = slug_from_title(&task.title);
                let workspace = manager.create(&repository, task_id, Some(&slug))?;
                let connection = state.db.lock().map_err(|error| error.to_string())?;
                db::insert_workspace(&connection, &workspace)?;
                workspace
            }
        }
    };

    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::set_task_workspace(
            &connection,
            task_id,
            &workspace.path,
            &workspace.branch_name,
            &now(),
        )?;
        db::update_task_status(&connection, task_id, TaskStatus::Implementing, &now())?;
    }
    emit_task(app, state, task_id);

    let dir = PathBuf::from(&workspace.path);
    if !dir.is_dir() {
        return Err(format!("workspace path does not exist: {}", workspace.path));
    }

    let program = opencode::resolve_program(opencode::DEFAULT_COMMAND)?;
    let permission = opencode::IMPLEMENT_PERMISSION.to_string();
    let log_dir = state.data_dir.join("logs");
    let selected_model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| opencode::models::DEFAULT_MODEL.to_string());
    let config = opencode::models::inline_config(&selected_model);
    let session_title = format!("Implement {}", task.jira_issue_key);
    let dir_for_thread = dir.clone();

    let (server, session_id) = opencode::run_blocking(move || {
        let server =
            opencode::start_server(&program, &dir_for_thread, &log_dir, &permission, &config)?;
        let client = server.client();
        if client.connected_providers().unwrap_or_default().is_empty() {
            return Err(
                "No authenticated provider found. Run `opencode auth login` and try again."
                    .to_string(),
            );
        }
        let session_id = client.create_session(&session_title)?;
        Ok((server, session_id))
    })?;

    let run_id = Uuid::new_v4().to_string();
    let run = AgentRun {
        id: run_id.clone(),
        task_id: task_id.to_string(),
        mode: "implementation".to_string(),
        agent: Some("build".to_string()),
        status: AgentRunStatus::Running,
        session_id: Some(session_id.clone()),
        started_at: now(),
        completed_at: None,
    };

    let base_url = server.base_url.clone();
    let password = server.password.clone();
    runtime::register_run(
        state,
        run,
        String::new(),
        0.0,
        Some(selected_model.clone()),
        server,
    )?;

    opencode::spawn_event_listener(
        app.clone(),
        run_id.clone(),
        base_url.clone(),
        password.clone(),
        session_id.clone(),
    );

    let ticket = TicketInput {
        key: task.jira_issue_key.clone(),
        title: task.title.clone(),
        description: task.description.clone(),
        acceptance_criteria: None,
    };
    let prompt = build_implementation_prompt(&ticket, &plan_content);

    let success_task = task_id.to_string();
    let failure_task = task_id.to_string();
    let callbacks = PromptCallbacks {
        on_success: Some(Box::new(move |app, _value| {
            implementation_done(app, &success_task)
        })),
        on_failure: Some(Box::new(move |app, error| {
            implementation_failed(app, &failure_task, error)
        })),
    };

    opencode::spawn_prompt_with(
        app.clone(),
        run_id,
        base_url,
        password,
        session_id,
        prompt,
        Some("build".to_string()),
        Some(selected_model),
        None,
        callbacks,
    );

    Ok(())
}

fn implementation_done(app: &AppHandle, task_id: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(connection) = state.db.lock() {
            let _ = db::update_task_status(&connection, task_id, TaskStatus::Testing, &now());
        }
        emit_task(app, &state, task_id);
    }
    crate::commands::validation::start_test_repair(app.clone(), task_id.to_string(), None);
}

fn implementation_failed(app: &AppHandle, task_id: &str, error: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(connection) = state.db.lock() {
            let _ = db::update_task_status(&connection, task_id, TaskStatus::Failed, &now());
        }
        emit_task(app, &state, task_id);
    }
    let _ = app.emit(
        "implementation://error",
        serde_json::json!({ "taskId": task_id, "error": error }),
    );
}

#[tauri::command]
pub fn start_implementation(
    app: AppHandle,
    state: State<AppState>,
    task_id: String,
    model: Option<String>,
) -> Result<(), String> {
    start_implementation_inner(&app, &state, &task_id, model)
}
