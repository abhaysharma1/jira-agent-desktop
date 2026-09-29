use std::path::PathBuf;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::commands::{implementation, runtime};
use crate::db;
use crate::domain::{
    AgentRun, AgentRunStatus, AgentTask, PlanApproval, PlanMessage, PlanVersion, TaskStatus,
};
use crate::opencode::{self, PromptCallbacks};
use crate::planning::{self, TicketInput};
use crate::state::AppState;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn find_planning_runtime(
    state: &AppState,
    task_id: &str,
) -> Option<(String, String, String, String, Option<String>, Option<String>)> {
    let agents = state.agents.lock().ok()?;
    let runtime = agents
        .values()
        .filter(|runtime| runtime.run.task_id == task_id && runtime.run.mode == "planning")
        .max_by_key(|runtime| runtime.run.started_at.clone())?;
    let session_id = runtime.run.session_id.clone()?;
    Some((
        runtime.run.id.clone(),
        runtime.server.base_url.clone(),
        runtime.server.password.clone(),
        session_id,
        runtime.run.agent.clone(),
        runtime.model.clone(),
    ))
}

#[tauri::command]
pub fn start_planning(
    app: AppHandle,
    state: State<AppState>,
    repository_id: String,
    key: String,
    title: String,
    description: String,
    acceptance_criteria: Option<String>,
    model: Option<String>,
) -> Result<AgentTask, String> {
    let repository = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::get_repository(&connection, &repository_id)?
            .ok_or_else(|| format!("repository not found: {repository_id}"))?
    };

    let dir = PathBuf::from(&repository.local_path);
    if !dir.is_dir() {
        return Err(format!(
            "repository path does not exist: {}",
            repository.local_path
        ));
    }

    let ticket = TicketInput {
        key: key.clone(),
        title: title.clone(),
        description: description.clone(),
        acceptance_criteria: acceptance_criteria.clone(),
    };

    let task = AgentTask {
        id: Uuid::new_v4().to_string(),
        jira_issue_key: key,
        title,
        description,
        repository_id: repository.id.clone(),
        status: TaskStatus::Planning,
        plan_version: None,
        approved_plan_version: None,
        workspace_path: None,
        branch_name: None,
        created_at: now(),
        updated_at: now(),
    };

    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::insert_task(&connection, &task)?;
    }
    let _ = app.emit("task://updated", &task);

    let program = opencode::resolve_program(opencode::DEFAULT_COMMAND)?;
    let permission = opencode::READ_ONLY_PERMISSION.to_string();
    let log_dir = state.data_dir.join("logs");
    let selected_model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| opencode::models::DEFAULT_MODEL.to_string());
    let config = opencode::models::inline_config(&selected_model);
    let session_title = format!("Plan {}", ticket.key);
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
        task_id: task.id.clone(),
        mode: "planning".to_string(),
        agent: Some("plan".to_string()),
        status: AgentRunStatus::Running,
        session_id: Some(session_id.clone()),
        started_at: now(),
        completed_at: None,
    };

    let base_url = server.base_url.clone();
    let password = server.password.clone();
    runtime::register_run(
        &state,
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

    let prompt = planning::build_prompt(&ticket, &repository.local_path);
    let task_id = task.id.clone();
    let ticket_key = ticket.key.clone();
    let callbacks = PromptCallbacks {
        on_success: Some(Box::new(move |app, value| {
            plan_success(app, &task_id, &ticket_key, value)
        })),
        on_failure: Some(Box::new({
            let task_id = task.id.clone();
            move |app, error| task_failed(app, &task_id, error)
        })),
    };

    opencode::spawn_prompt_with(
        app,
        run_id,
        base_url,
        password,
        session_id,
        prompt,
        Some("plan".to_string()),
        Some(selected_model),
        None,
        callbacks,
    );

    Ok(task)
}

fn plan_success(app: &AppHandle, task_id: &str, ticket_key: &str, value: &Value) {
    let content = match planning::parse_plan(ticket_key, value) {
        Ok(content) => content,
        Err(error) => return task_failed(app, task_id, &error),
    };

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let timestamp = now();

    let result = {
        let connection = match state.db.lock() {
            Ok(connection) => connection,
            Err(_) => return,
        };
        db::store_plan_version(&connection, task_id, content, "agent", &timestamp)
            .and_then(|(plan, version)| {
                db::update_task_status(&connection, task_id, TaskStatus::PlanReady, &now())?;
                Ok((plan, version))
            })
    };

    match result {
        Ok((plan, version)) => {
            let _ = app.emit(
                "plan://ready",
                serde_json::json!({ "taskId": task_id, "plan": plan, "version": version }),
            );
        }
        Err(error) => task_failed(app, task_id, &error),
    }
}

fn commit_revision(app: &AppHandle, task_id: &str, ticket_key: &str, value: &Value) {
    let content = match planning::parse_plan(ticket_key, value) {
        Ok(content) => content,
        Err(error) => return fail_revision(app, task_id, &error),
    };

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let timestamp = now();

    let result = {
        let connection = match state.db.lock() {
            Ok(connection) => connection,
            Err(_) => return,
        };
        let (plan, version) =
            match db::store_plan_version(&connection, task_id, content.clone(), "agent", &timestamp)
            {
                Ok(result) => result,
                Err(error) => return fail_revision(app, task_id, &error),
            };
        let message = PlanMessage {
            id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            role: "agent".to_string(),
            content: content.summary.clone(),
            created_at: timestamp.clone(),
            plan_version: Some(version.version),
        };
        if let Err(error) = db::insert_plan_message(&connection, &message) {
            return fail_revision(app, task_id, &error);
        }
        (plan, version, message)
    };

    let (plan, version, message) = result;
    let _ = app.emit(
        "plan://ready",
        serde_json::json!({ "taskId": task_id, "plan": plan, "version": version }),
    );
    let _ = app.emit("plan://message", &message);
}

fn task_failed(app: &AppHandle, task_id: &str, error: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(connection) = state.db.lock() {
            let _ = db::update_task_status(&connection, task_id, TaskStatus::Failed, &now());
        }
    }
    let _ = app.emit(
        "plan://error",
        serde_json::json!({ "taskId": task_id, "error": error }),
    );
}

fn fail_revision(app: &AppHandle, task_id: &str, error: &str) {
    let _ = app.emit(
        "plan://error",
        serde_json::json!({ "taskId": task_id, "error": error }),
    );
}

#[tauri::command]
pub fn list_tasks(state: State<AppState>) -> Result<Vec<AgentTask>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::list_tasks(&connection)
}

#[tauri::command]
pub fn get_task(state: State<AppState>, task_id: String) -> Result<AgentTask, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::get_task(&connection, &task_id)?.ok_or_else(|| format!("task not found: {task_id}"))
}

#[tauri::command]
pub fn get_plan(
    state: State<AppState>,
    task_id: String,
) -> Result<Option<PlanVersion>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::latest_plan_version(&connection, &task_id)
}

#[tauri::command]
pub fn list_plan_versions(
    state: State<AppState>,
    task_id: String,
) -> Result<Vec<PlanVersion>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::list_plan_versions(&connection, &task_id)
}

#[tauri::command]
pub fn list_plan_messages(
    state: State<AppState>,
    task_id: String,
) -> Result<Vec<PlanMessage>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::list_plan_messages(&connection, &task_id)
}

#[tauri::command]
pub fn approve_plan(
    app: AppHandle,
    state: State<AppState>,
    task_id: String,
    plan_version: Option<u32>,
    user_id: Option<String>,
) -> Result<PlanApproval, String> {
    let user = user_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "local-user".to_string());

    let approval = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::approve_plan(&connection, &task_id, plan_version, &user, &now())?
    };

    if let Ok(connection) = state.db.lock() {
        if let Ok(Some(task)) = db::get_task(&connection, &task_id) {
            let _ = app.emit("task://updated", &task);
        }
    }
    let _ = app.emit(
        "plan://approved",
        serde_json::json!({ "taskId": &task_id, "approval": &approval }),
    );

    if let Err(error) = implementation::start_implementation_inner(&app, &state, &task_id, None) {
        let _ = app.emit(
            "implementation://error",
            serde_json::json!({ "taskId": &task_id, "error": error }),
        );
    }

    Ok(approval)
}

#[tauri::command]
pub fn reject_plan(
    app: AppHandle,
    state: State<AppState>,
    task_id: String,
) -> Result<AgentTask, String> {
    let task = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::reject_plan(&connection, &task_id, &now())?
    };
    let _ = app.emit("task://updated", &task);
    Ok(task)
}

#[tauri::command]
pub fn get_plan_approval(
    state: State<AppState>,
    task_id: String,
) -> Result<Option<PlanApproval>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::latest_plan_approval(&connection, &task_id)
}

#[tauri::command]
pub fn revise_plan(
    app: AppHandle,
    state: State<AppState>,
    task_id: String,
    feedback: String,
    model: Option<String>,
) -> Result<PlanMessage, String> {
    if feedback.trim().is_empty() {
        return Err("feedback is required".to_string());
    }

    let (task, repository, current_version, current_content) = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        let task = db::get_task(&connection, &task_id)?
            .ok_or_else(|| format!("task not found: {task_id}"))?;
        if task.status != TaskStatus::PlanReady {
            return Err(format!(
                "task {task_id} is not ready for revision (status {:?})",
                task.status
            ));
        }
        let repository = db::get_repository(&connection, &task.repository_id)?
            .ok_or_else(|| format!("repository not found: {}", task.repository_id))?;
        let (version, content) = db::latest_plan_content(&connection, &task_id)?
            .ok_or_else(|| "task has no plan to revise".to_string())?;
        (task, repository, version, content)
    };

    let user_message = PlanMessage {
        id: Uuid::new_v4().to_string(),
        task_id: task_id.clone(),
        role: "user".to_string(),
        content: feedback.clone(),
        created_at: now(),
        plan_version: Some(current_version),
    };
    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::insert_plan_message(&connection, &user_message)?;
    }
    let _ = app.emit("plan://message", &user_message);

    let ticket = TicketInput {
        key: task.jira_issue_key.clone(),
        title: task.title.clone(),
        description: task.description.clone(),
        acceptance_criteria: None,
    };

    if let Some((run_id, base_url, password, session_id, agent, runtime_model)) =
        find_planning_runtime(&state, &task_id)
    {
        let prompt = planning::build_revision_prompt(&feedback);
        let selected_model = model.or(runtime_model);
        let ticket_key = ticket.key.clone();
        let success_task = task_id.clone();
        let failure_task = task_id.clone();
        let callbacks = PromptCallbacks {
            on_success: Some(Box::new(move |app, value| {
                commit_revision(app, &success_task, &ticket_key, value)
            })),
            on_failure: Some(Box::new(move |app, error| {
                fail_revision(app, &failure_task, error)
            })),
        };
        opencode::spawn_prompt_with(
            app,
            run_id,
            base_url,
            password,
            session_id,
            prompt,
            agent,
            selected_model,
            None,
            callbacks,
        );
        return Ok(user_message);
    }

    let dir = PathBuf::from(&repository.local_path);
    if !dir.is_dir() {
        return Err(format!(
            "repository path does not exist: {}",
            repository.local_path
        ));
    }

    let program = opencode::resolve_program(opencode::DEFAULT_COMMAND)?;
    let permission = opencode::READ_ONLY_PERMISSION.to_string();
    let log_dir = state.data_dir.join("logs");
    let selected_model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| opencode::models::DEFAULT_MODEL.to_string());
    let config = opencode::models::inline_config(&selected_model);
    let seed_prompt = planning::build_revision_seed_prompt(
        &ticket,
        &current_content,
        &repository.local_path,
        &feedback,
    );
    let session_title = format!("Revise {}", ticket.key);
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
        task_id: task_id.clone(),
        mode: "planning".to_string(),
        agent: Some("plan".to_string()),
        status: AgentRunStatus::Running,
        session_id: Some(session_id.clone()),
        started_at: now(),
        completed_at: None,
    };

    let base_url = server.base_url.clone();
    let password = server.password.clone();
    runtime::register_run(
        &state,
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

    let ticket_key = ticket.key.clone();
    let success_task = task_id.clone();
    let failure_task = task_id.clone();
    let callbacks = PromptCallbacks {
        on_success: Some(Box::new(move |app, value| {
            commit_revision(app, &success_task, &ticket_key, value)
        })),
        on_failure: Some(Box::new(move |app, error| {
            fail_revision(app, &failure_task, error)
        })),
    };
    opencode::spawn_prompt_with(
        app,
        run_id,
        base_url,
        password,
        session_id,
        seed_prompt,
        Some("plan".to_string()),
        Some(selected_model),
        None,
        callbacks,
    );

    Ok(user_message)
}
