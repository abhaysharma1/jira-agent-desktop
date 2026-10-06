use std::path::PathBuf;
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::commands::runtime;
use crate::db;
use crate::domain::{
    AgentRun, AgentRunStatus, PlanContent, TaskStatus, TestRun, ValidationConfig,
    ValidationResult, ValidationRun,
};
use crate::opencode;
use crate::planning::TicketInput;
use crate::state::AppState;
use crate::validation;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn empty_plan() -> PlanContent {
    PlanContent {
        ticket: String::new(),
        summary: String::new(),
        understanding: String::new(),
        steps: Vec::new(),
        tests: Vec::new(),
        risks: Vec::new(),
    }
}

fn set_status(app: &AppHandle, task_id: &str, status: TaskStatus) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(connection) = state.db.lock() {
            let _ = db::update_task_status(&connection, task_id, status, &now());
        }
        crate::commands::cloud_socket::emit_task_updated(app, &state, task_id);
    }
}

fn fail(app: &AppHandle, task_id: &str, error: &str) {
    set_status(app, task_id, TaskStatus::Failed);
    crate::logging::error(
        "validation",
        "task failed",
        serde_json::json!({ "taskId": task_id, "error": error }),
    );
    let _ = app.emit(
        "validation://error",
        serde_json::json!({ "taskId": task_id, "error": error }),
    );
}

pub fn start_test_repair(app: AppHandle, task_id: String, model: Option<String>) {
    std::thread::spawn(move || run_loop(&app, &task_id, model));
}

fn run_loop(app: &AppHandle, task_id: &str, model: Option<String>) {
    let context = {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let Ok(connection) = state.db.lock() else {
            return;
        };
        (|| -> Result<_, String> {
            let task = db::get_task(&connection, task_id)?
                .ok_or_else(|| format!("task not found: {task_id}"))?;
            let workspace_path = task
                .workspace_path
                .clone()
                .ok_or_else(|| "task has no workspace".to_string())?;
            let config = db::get_validation_config(&connection, &task.repository_id)?;
            let max_attempts = db::max_repair_attempts(&connection);
            let plan = match task.approved_plan_version {
                Some(version) => db::get_plan_version(&connection, task_id, version)?
                    .map(|version| version.content),
                None => None,
            };
            Ok((task, workspace_path, config, max_attempts, plan))
        })()
    };

    let (task, workspace_path, config, max_attempts, plan) = match context {
        Ok(context) => context,
        Err(error) => return fail(app, task_id, &error),
    };

    let workspace = PathBuf::from(&workspace_path);
    let ticket = TicketInput {
        key: task.jira_issue_key.clone(),
        title: task.title.clone(),
        description: task.description.clone(),
        acceptance_criteria: None,
    };

    let Some(test_command) = config.test.clone().filter(|value| !value.trim().is_empty()) else {
        set_status(app, task_id, TaskStatus::Validating);
        start_final_validation(app.clone(), task_id.to_string());
        return;
    };

    let mut repairs = 0u32;
    for attempt in 1..=max_attempts {
        set_status(app, task_id, TaskStatus::Testing);
        let started = Instant::now();
        let (exit_code, output) = validation::run_command(&workspace, &test_command);
        let elapsed_ms = started.elapsed().as_millis() as f64;
        let passed = exit_code == 0;
        crate::metrics::record(
            app,
            "test.duration_ms",
            elapsed_ms,
            Some(task_id),
            serde_json::json!({ "phase": "test", "attempt": attempt, "passed": passed }),
        );
        let test_run = TestRun {
            id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            attempt,
            command: test_command.clone(),
            passed,
            exit_code,
            output: output.clone(),
            created_at: now(),
        };
        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(connection) = state.db.lock() {
                let _ = db::insert_test_run(&connection, &test_run);
            }
        }
        let _ = app.emit("test://result", &test_run);

        if passed {
            crate::metrics::record(
                app,
                "repair.attempts",
                repairs as f64,
                Some(task_id),
                serde_json::json!({ "phase": "test", "outcome": "passed" }),
            );
            set_status(app, task_id, TaskStatus::Validating);
            start_final_validation(app.clone(), task_id.to_string());
            return;
        }

        if attempt == max_attempts {
            crate::metrics::record(
                app,
                "repair.attempts",
                repairs as f64,
                Some(task_id),
                serde_json::json!({ "phase": "test", "outcome": "failed" }),
            );
            fail(
                app,
                task_id,
                &format!("Tests are still failing after {max_attempts} attempts."),
            );
            return;
        }

        set_status(app, task_id, TaskStatus::Repairing);
        let plan_content = plan.clone().unwrap_or_else(empty_plan);
        let prompt = validation::build_repair_prompt(&ticket, &plan_content, &output);
        if let Err(error) = send_repair(app, task_id, &workspace_path, prompt, model.clone()) {
            crate::metrics::record(
                app,
                "repair.attempts",
                repairs as f64,
                Some(task_id),
                serde_json::json!({ "phase": "test", "outcome": "error" }),
            );
            fail(app, task_id, &error);
            return;
        }
        repairs += 1;
    }
}

fn send_repair(
    app: &AppHandle,
    task_id: &str,
    workspace_path: &str,
    prompt: String,
    model: Option<String>,
) -> Result<(), String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "state unavailable".to_string())?;

    let existing = {
        let agents = state.agents.lock().map_err(|error| error.to_string())?;
        agents
            .values()
            .filter(|runtime| {
                runtime.run.task_id == task_id && runtime.run.mode == "implementation"
            })
            .max_by_key(|runtime| runtime.run.started_at.clone())
            .map(|runtime| {
                (
                    runtime.run.id.clone(),
                    runtime.server.base_url.clone(),
                    runtime.server.password.clone(),
                    runtime.run.session_id.clone(),
                    runtime.model.clone(),
                )
            })
    };

    if let Some((run_id, base_url, password, Some(session_id), runtime_model)) = existing {
        return opencode::run_prompt_blocking(
            app,
            &run_id,
            base_url,
            password,
            session_id,
            prompt,
            Some("build".to_string()),
            model.or(runtime_model),
        );
    }

    let program = opencode::resolve_program(opencode::DEFAULT_COMMAND)?;
    let permission = opencode::IMPLEMENT_PERMISSION.to_string();
    let log_dir = state.data_dir.join("logs");
    let selected_model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| opencode::models::DEFAULT_MODEL.to_string());
    let config = opencode::models::inline_config(&selected_model);
    let dir = PathBuf::from(workspace_path);
    let session_title = "Repair".to_string();
    let dir_for_thread = dir.clone();

    let (server, session_id) = opencode::run_blocking(move || {
        let server =
            opencode::start_server(&program, &dir_for_thread, &log_dir, &permission, &config)?;
        let client = server.client();
        let session_id = client.create_session(&session_title)?;
        Ok((server, session_id))
    })?;

    let run_id = Uuid::new_v4().to_string();
    let run = AgentRun {
        id: run_id.clone(),
        task_id: task_id.to_string(),
        mode: "repair".to_string(),
        agent: Some("build".to_string()),
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

    opencode::run_prompt_blocking(
        app,
        &run_id,
        base_url,
        password,
        session_id,
        prompt,
        Some("build".to_string()),
        Some(selected_model),
    )
}

#[tauri::command]
pub fn get_validation_config(
    state: State<AppState>,
    repository_id: String,
) -> Result<ValidationConfig, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::get_validation_config(&connection, &repository_id)
}

#[tauri::command]
pub fn set_validation_config(
    state: State<AppState>,
    repository_id: String,
    config: ValidationConfig,
) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::set_validation_config(&connection, &repository_id, &config)
}

#[tauri::command]
pub fn list_test_runs(state: State<AppState>, task_id: String) -> Result<Vec<TestRun>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::list_test_runs(&connection, &task_id)
}

#[tauri::command]
pub fn run_tests(app: AppHandle, task_id: String) -> Result<(), String> {
    start_test_repair(app, task_id, None);
    Ok(())
}

pub fn start_final_validation(app: AppHandle, task_id: String) {
    std::thread::spawn(move || run_validation_loop(&app, &task_id));
}

fn run_validation_loop(app: &AppHandle, task_id: &str) {
    let context = {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let Ok(connection) = state.db.lock() else {
            return;
        };
        (|| -> Result<_, String> {
            let task = db::get_task(&connection, task_id)?
                .ok_or_else(|| format!("task not found: {task_id}"))?;
            let workspace_path = task
                .workspace_path
                .clone()
                .ok_or_else(|| "task has no workspace".to_string())?;
            let config = db::get_validation_config(&connection, &task.repository_id)?;
            let repair_enabled = db::repair_on_validation_failure(&connection);
            let max_attempts = db::max_repair_attempts(&connection);
            let plan = match task.approved_plan_version {
                Some(version) => db::get_plan_version(&connection, task_id, version)?
                    .map(|version| version.content),
                None => None,
            };
            Ok((task, workspace_path, config, repair_enabled, max_attempts, plan))
        })()
    };

    let (task, workspace_path, config, repair_enabled, max_attempts, plan) = match context {
        Ok(context) => context,
        Err(error) => return fail(app, task_id, &error),
    };

    let workspace = PathBuf::from(&workspace_path);
    let ticket = TicketInput {
        key: task.jira_issue_key.clone(),
        title: task.title.clone(),
        description: task.description.clone(),
        acceptance_criteria: None,
    };
    let steps = validation::build_validation_steps(&config);
    let attempts = if repair_enabled { max_attempts } else { 1 };

    let mut repairs = 0u32;
    for attempt in 1..=attempts {
        set_status(app, task_id, TaskStatus::Validating);
        let started = Instant::now();
        let mut results = Vec::new();
        for command in &steps {
            let (exit_code, output) = validation::run_command(&workspace, command);
            results.push(ValidationResult {
                command: command.clone(),
                passed: exit_code == 0,
                exit_code,
                output,
            });
        }
        let elapsed_ms = started.elapsed().as_millis() as f64;
        let passed = results.iter().all(|result| result.passed);
        crate::metrics::record(
            app,
            "test.duration_ms",
            elapsed_ms,
            Some(task_id),
            serde_json::json!({ "phase": "validation", "attempt": attempt, "passed": passed }),
        );
        let run = ValidationRun {
            id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            passed,
            results: results.clone(),
            created_at: now(),
        };
        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(connection) = state.db.lock() {
                let _ = db::insert_validation_run(&connection, &run);
            }
        }
        let _ = app.emit("validation://result", &run);

        if passed {
            crate::metrics::record(
                app,
                "repair.attempts",
                repairs as f64,
                Some(task_id),
                serde_json::json!({ "phase": "validation", "outcome": "passed" }),
            );
            set_status(app, task_id, TaskStatus::Committing);
            crate::commands::git::start_commit(app.clone(), task_id.to_string());
            return;
        }

        if !repair_enabled || attempt == attempts {
            let failing = results
                .iter()
                .filter(|result| !result.passed)
                .map(|result| result.command.clone())
                .collect::<Vec<_>>()
                .join(", ");
            crate::metrics::record(
                app,
                "repair.attempts",
                repairs as f64,
                Some(task_id),
                serde_json::json!({ "phase": "validation", "outcome": "failed" }),
            );
            fail(app, task_id, &format!("Validation failed: {failing}"));
            return;
        }

        set_status(app, task_id, TaskStatus::Repairing);
        let failing_output = results
            .iter()
            .filter(|result| !result.passed)
            .map(|result| format!("$ {}\n{}", result.command, result.output))
            .collect::<Vec<_>>()
            .join("\n\n");
        let plan_content = plan.clone().unwrap_or_else(empty_plan);
        let prompt = validation::build_repair_prompt(&ticket, &plan_content, &failing_output);
        if let Err(error) = send_repair(app, task_id, &workspace_path, prompt, None) {
            crate::metrics::record(
                app,
                "repair.attempts",
                repairs as f64,
                Some(task_id),
                serde_json::json!({ "phase": "validation", "outcome": "error" }),
            );
            fail(app, task_id, &error);
            return;
        }
        repairs += 1;
    }
}

#[tauri::command]
pub fn run_final_validation(app: AppHandle, task_id: String) -> Result<(), String> {
    start_final_validation(app, task_id);
    Ok(())
}

#[tauri::command]
pub fn list_validation_runs(
    state: State<AppState>,
    task_id: String,
) -> Result<Vec<ValidationRun>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::list_validation_runs(&connection, &task_id)
}

#[tauri::command]
pub fn get_repair_on_validation_failure(state: State<AppState>) -> Result<bool, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    Ok(db::repair_on_validation_failure(&connection))
}

#[tauri::command]
pub fn set_repair_on_validation_failure(
    state: State<AppState>,
    enabled: bool,
) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::set_setting(
        &connection,
        "repairOnValidationFailure",
        &serde_json::json!(enabled),
    )
}

