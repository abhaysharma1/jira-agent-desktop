use std::path::PathBuf;

use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::commands::runtime;
use crate::db;
use crate::domain::{AgentRun, AgentRunStatus, AgentRunView, ModelInfo};
use crate::opencode;
use crate::state::AppState;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn title_from(prompt: &str) -> String {
    let line = prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Agent run")
        .trim();
    let title: String = line.chars().take(60).collect();
    if title.is_empty() {
        "Agent run".to_string()
    } else {
        title
    }
}

#[tauri::command]
pub fn start_agent(
    app: AppHandle,
    state: State<AppState>,
    target_dir: String,
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
) -> Result<AgentRun, String> {
    let dir = PathBuf::from(&target_dir);
    if !dir.is_dir() {
        return Err(format!("target directory does not exist: {target_dir}"));
    }

    let program = opencode::resolve_program(opencode::DEFAULT_COMMAND)?;
    let permission = opencode::READ_ONLY_PERMISSION.to_string();
    let log_dir = state.data_dir.join("logs");
    let title = title_from(&prompt);
    let dir_for_thread = dir.clone();
    let selected_model = model
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| opencode::models::DEFAULT_MODEL.to_string());
    let config = opencode::models::inline_config(&selected_model);

    let (server, session_id) = opencode::run_blocking(move || {
        let server =
            opencode::start_server(&program, &dir_for_thread, &log_dir, &permission, &config)?;
        let client = server.client();
        let connected = client.connected_providers().unwrap_or_default();
        if connected.is_empty() {
            return Err(
                "No authenticated provider found. Run `opencode auth login` and try again."
                    .to_string(),
            );
        }
        let session_id = client.create_session(&title)?;
        Ok((server, session_id))
    })?;

    let run_id = Uuid::new_v4().to_string();
    let run = AgentRun {
        id: run_id.clone(),
        task_id: String::new(),
        mode: agent.clone().unwrap_or_else(|| "agent".to_string()),
        agent: agent.clone(),
        status: AgentRunStatus::Running,
        session_id: Some(session_id.clone()),
        started_at: now(),
        completed_at: None,
    };

    let base_url = server.base_url.clone();
    let password = server.password.clone();

    runtime::register_run(
        &state,
        run.clone(),
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
    opencode::spawn_prompt(
        app,
        run_id.clone(),
        base_url,
        password,
        session_id,
        prompt,
        agent,
        Some(selected_model),
    );

    Ok(run)
}

#[tauri::command]
pub fn list_models() -> Result<Vec<ModelInfo>, String> {
    let program = opencode::resolve_program(opencode::DEFAULT_COMMAND)?;
    opencode::run_blocking(move || Ok(opencode::models::discover_models(&program)))
}

#[tauri::command]
pub fn send_agent_message(
    app: AppHandle,
    state: State<AppState>,
    run_id: String,
    content: String,
) -> Result<(), String> {
    let (base_url, password, session_id, agent) = {
        let agents = state.agents.lock().map_err(|error| error.to_string())?;
        let runtime = agents
            .get(&run_id)
            .ok_or_else(|| format!("agent run not found: {run_id}"))?;
        let session_id = runtime
            .run
            .session_id
            .clone()
            .ok_or_else(|| "agent run has no session".to_string())?;
        (
            runtime.server.base_url.clone(),
            runtime.server.password.clone(),
            session_id,
            runtime.run.agent.clone(),
        )
    };

    opencode::spawn_prompt(app, run_id, base_url, password, session_id, content, agent, None);
    Ok(())
}

#[tauri::command]
pub fn stop_agent(
    app: AppHandle,
    state: State<AppState>,
    run_id: String,
) -> Result<(), String> {
    let runtime = state
        .agents
        .lock()
        .map_err(|error| error.to_string())?
        .remove(&run_id);

    match runtime {
        Some(mut runtime) => {
            let session_id = runtime.run.session_id.clone();
            runtime.server.stop(session_id.as_deref());
            runtime.run.status = AgentRunStatus::Cancelled;
            runtime.run.completed_at = Some(now());
            if let Ok(connection) = state.db.lock() {
                let _ = db::upsert_agent_run(
                    &connection,
                    &runtime.run,
                    &runtime.output,
                    runtime.cost,
                    runtime.model.as_deref(),
                );
            }
            let _ = app.emit(
                "agent://status",
                serde_json::json!({ "runId": runtime.run.id, "status": runtime.run.status }),
            );
            Ok(())
        }
        None => Err(format!("agent run not found: {run_id}")),
    }
}

#[tauri::command]
pub fn get_agent_status(
    state: State<AppState>,
    run_id: String,
) -> Result<AgentRunView, String> {
    let agents = state.agents.lock().map_err(|error| error.to_string())?;
    let runtime = agents
        .get(&run_id)
        .ok_or_else(|| format!("agent run not found: {run_id}"))?;
    Ok(AgentRunView {
        run: runtime.run.clone(),
        events: runtime.events.clone(),
        output: runtime.output.clone(),
        cost: runtime.cost,
        model: runtime.model.clone(),
    })
}

#[tauri::command]
pub fn list_agents(state: State<AppState>) -> Result<Vec<AgentRun>, String> {
    let agents = state.agents.lock().map_err(|error| error.to_string())?;
    Ok(agents.values().map(|runtime| runtime.run.clone()).collect())
}
