use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::db;
use crate::domain::{AgentEvent, AgentRunStatus, TodoItem};
use crate::state::AppState;

pub mod models;

pub const DEFAULT_COMMAND: &str = "opencode";

pub const READ_ONLY_PERMISSION: &str = r#"{
  "edit": "deny",
  "webfetch": "deny",
  "external_directory": "deny",
  "doom_loop": "deny",
  "bash": {
    "*": "deny",
    "git status*": "allow",
    "git log*": "allow",
    "git diff*": "allow",
    "git branch*": "allow",
    "git rev-parse*": "allow",
    "ls*": "allow",
    "pwd": "allow",
    "rg*": "allow",
    "grep*": "allow",
    "cat*": "allow"
  }
}"#;

const AGENT_EVENT: &str = "agent://event";
const AGENT_STATUS: &str = "agent://status";

pub const IMPLEMENT_PERMISSION: &str = r#"{
  "edit": "allow",
  "webfetch": "deny",
  "external_directory": "deny",
  "doom_loop": "deny",
  "bash": {
    "*": "allow",
    "git commit*": "deny",
    "git push*": "deny",
    "git remote*": "deny",
    "gh *": "deny",
    "rm -rf *": "deny"
  }
}"#;

pub fn run_blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    match std::thread::spawn(f).join() {
        Ok(result) => result,
        Err(_) => Err("background task panicked".to_string()),
    }
}

pub fn free_port() -> Result<u16, String> {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    Ok(port)
}

pub fn resolve_program(command: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(command);
    if candidate.is_file() {
        return Ok(candidate.to_path_buf());
    }
    if command.contains('/') || command.contains('\\') {
        return Err(format!("executable not found: {command}"));
    }

    let path_var = std::env::var_os("PATH")
        .ok_or_else(|| "PATH environment variable is not set".to_string())?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path_var).collect();

    for dir in &dirs {
        if cfg!(windows) {
            let exe = dir.join(format!("{command}.exe"));
            if exe.is_file() {
                return Ok(exe);
            }
            let shim = dir.join(format!("{command}.cmd"));
            if shim.is_file() {
                let exe = dir
                    .join("node_modules")
                    .join("opencode-ai")
                    .join("bin")
                    .join("opencode.exe");
                if exe.is_file() {
                    return Ok(exe);
                }
                if let Some(found) = parse_npm_shim(&shim) {
                    return Ok(found);
                }
            }
        } else {
            let plain = dir.join(command);
            if plain.is_file() {
                return Ok(plain);
            }
        }
    }

    Err(format!(
        "could not find '{command}' on PATH; set an explicit path in Settings"
    ))
}

fn parse_npm_shim(shim: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(shim).ok()?;
    let dir = shim.parent()?.to_string_lossy().to_string();
    for token in content.split('"') {
        let token = token.trim();
        if token.to_ascii_lowercase().ends_with(".exe") {
            let expanded = token
                .replace("%dp0%", &dir)
                .replace("%~dp0%", &dir)
                .replace("$basedir", &dir);
            let path = PathBuf::from(expanded);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

pub struct OpenCodeClient {
    base_url: String,
    password: String,
    http: reqwest::blocking::Client,
}

impl OpenCodeClient {
    pub fn new(base_url: String, password: String) -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        Self {
            base_url,
            password,
            http,
        }
    }

    fn get(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        self.http
            .get(format!("{}{}", self.base_url, path))
            .basic_auth("opencode", Some(&self.password))
    }

    fn post(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        self.http
            .post(format!("{}{}", self.base_url, path))
            .basic_auth("opencode", Some(&self.password))
    }

    pub fn health(&self) -> Result<String, String> {
        let response = self
            .get("/global/health")
            .send()
            .map_err(|error| format!("health request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("health returned {}", response.status()));
        }
        let value: Value = response
            .json()
            .map_err(|error| format!("invalid health response: {error}"))?;
        Ok(value
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string())
    }

    pub fn connected_providers(&self) -> Result<Vec<String>, String> {
        let value: Value = self
            .get("/provider")
            .send()
            .map_err(|error| format!("provider request failed: {error}"))?
            .json()
            .map_err(|error| format!("invalid provider response: {error}"))?;
        Ok(value
            .get("connected")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn create_session(&self, title: &str) -> Result<String, String> {
        let response = self
            .post("/session")
            .json(&json!({ "title": title }))
            .send()
            .map_err(|error| format!("session create failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "session create returned {}: {}",
                response.status(),
                response.text().unwrap_or_default()
            ));
        }
        let value: Value = response
            .json()
            .map_err(|error| format!("invalid session response: {error}"))?;
        value
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "session create returned no id".to_string())
    }

    pub fn prompt(
        &self,
        session_id: &str,
        text: &str,
        agent: Option<&str>,
        model: Option<&str>,
        format: Option<&Value>,
    ) -> Result<Value, String> {
        let response = self
            .post(&format!("/session/{session_id}/message"))
            .json(&build_prompt_body(text, agent, model, format))
            .send()
            .map_err(|error| format!("prompt failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "prompt returned {}: {}",
                response.status(),
                response.text().unwrap_or_default()
            ));
        }
        response
            .json()
            .map_err(|error| format!("invalid prompt response: {error}"))
    }

    pub fn abort(&self, session_id: &str) {
        let _ = self
            .post(&format!("/session/{session_id}/abort"))
            .send();
    }

    pub fn respond_permission(&self, session_id: &str, permission_id: &str, response: &str) {
        let _ = self
            .post(&format!(
                "/session/{session_id}/permissions/{permission_id}"
            ))
            .json(&json!({ "response": response }))
            .send();
    }

    pub fn event_response(&self) -> Result<reqwest::blocking::Response, String> {
        let http = reqwest::blocking::Client::builder()
            .build()
            .map_err(|error| error.to_string())?;
        http.get(format!("{}/event", self.base_url))
            .basic_auth("opencode", Some(&self.password))
            .send()
            .map_err(|error| format!("event stream failed: {error}"))
    }
}

pub struct ManagedServer {
    pub child: Child,
    pub base_url: String,
    pub password: String,
}

impl ManagedServer {
    pub fn client(&self) -> OpenCodeClient {
        OpenCodeClient::new(self.base_url.clone(), self.password.clone())
    }

    pub fn stop(&mut self, session_id: Option<&str>) {
        if let Some(session_id) = session_id {
            self.client().abort(session_id);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ManagedServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn start_server(
    program: &Path,
    dir: &Path,
    log_dir: &Path,
    permission: &str,
    inline_config: &str,
) -> Result<ManagedServer, String> {
    let port = free_port()?;
    let password = Uuid::new_v4().to_string();
    std::fs::create_dir_all(log_dir).map_err(|error| error.to_string())?;
    let log_path = log_dir.join(format!("opencode-{port}.log"));
    let log = std::fs::File::create(&log_path).map_err(|error| error.to_string())?;
    let stderr = log.try_clone().map_err(|error| error.to_string())?;

    let mut command = Command::new(program);
    command
        .args([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .current_dir(dir)
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .env("OPENCODE_PERMISSION", permission)
        .env("OPENCODE_CONFIG_CONTENT", inline_config)
        .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let child = command
        .spawn()
        .map_err(|error| format!("failed to start opencode server: {error}"))?;

    let base_url = format!("http://127.0.0.1:{port}");
    let client = OpenCodeClient::new(base_url.clone(), password.clone());

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if client.health().is_ok() {
            break;
        }
        if Instant::now() > deadline {
            let _ = std::fs::read_to_string(&log_path).map(|contents| {
                eprintln!("opencode server log:\n{contents}");
            });
            return Err(
                "timed out waiting for the opencode server to start; see ~/.jira-agent/logs"
                    .to_string(),
            );
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    Ok(ManagedServer {
        child,
        base_url,
        password,
    })
}

pub fn build_prompt_body(
    text: &str,
    agent: Option<&str>,
    model: Option<&str>,
    format: Option<&Value>,
) -> Value {
    let mut body = json!({
        "parts": [{ "type": "text", "text": text }],
    });
    if let Some(agent) = agent {
        body["agent"] = json!(agent);
    }
    if let Some(model) = model {
        if let Some((provider, model_id)) = model.split_once('/') {
            body["model"] = json!({ "providerID": provider, "modelID": model_id });
        }
    }
    if let Some(format) = format {
        body["format"] = json!({ "type": "json_schema", "schema": format });
    }
    body
}

pub fn extract_text(value: &Value) -> String {
    let mut output = String::new();
    if let Some(parts) = value.get("parts").and_then(Value::as_array) {
        for part in parts {
            if part.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if text.is_empty() {
                        continue;
                    }
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(text);
                }
            }
        }
    }
    output
}

pub struct EventMapper {
    run_id: String,
    part_text: HashMap<String, String>,
    tool_status: HashMap<String, String>,
}

impl EventMapper {
    pub fn new(run_id: String) -> Self {
        Self {
            run_id,
            part_text: HashMap::new(),
            tool_status: HashMap::new(),
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn feed(&mut self, value: &Value) -> Vec<AgentEvent> {
        let event_type = match value.get("type").and_then(Value::as_str) {
            Some(event_type) => event_type,
            None => return Vec::new(),
        };
        let properties = value.get("properties");
        match event_type {
            "message.part.updated" | "message.part.created" => match properties
                .and_then(|properties| properties.get("part"))
            {
                Some(part) => self.map_part(part),
                None => Vec::new(),
            },
            "file.edited" => properties
                .and_then(|properties| properties.get("file"))
                .and_then(Value::as_str)
                .map(|path| {
                    vec![AgentEvent::FileChanged {
                        run_id: self.run_id.clone(),
                        path: path.to_string(),
                        timestamp: now(),
                    }]
                })
                .unwrap_or_default(),
            "todo.updated" => vec![AgentEvent::Todo {
                run_id: self.run_id.clone(),
                todos: parse_todos(properties),
                timestamp: now(),
            }],
            "session.error" => {
                let error = properties
                    .and_then(|properties| properties.get("error"))
                    .and_then(|error| error.get("data"))
                    .and_then(|data| data.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("session error")
                    .to_string();
                vec![AgentEvent::Failed {
                    run_id: self.run_id.clone(),
                    error,
                    timestamp: now(),
                }]
            }
            _ => Vec::new(),
        }
    }

    fn map_part(&mut self, part: &Value) -> Vec<AgentEvent> {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => self.delta(part, false),
            Some("reasoning") => self.delta(part, true),
            Some("tool") => self.map_tool(part),
            Some("patch") => {
                let timestamp = now();
                part.get("files")
                    .and_then(Value::as_array)
                    .map(|files| {
                        files
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|path| AgentEvent::FileChanged {
                                run_id: self.run_id.clone(),
                                path: path.to_string(),
                                timestamp: timestamp.clone(),
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    fn delta(&mut self, part: &Value, reasoning: bool) -> Vec<AgentEvent> {
        let id = match part.get("id").and_then(Value::as_str) {
            Some(id) => id.to_string(),
            None => return Vec::new(),
        };
        let full = match part.get("text").and_then(Value::as_str) {
            Some(text) => text.to_string(),
            None => return Vec::new(),
        };
        let previous = self.part_text.get(&id).cloned().unwrap_or_default();
        let delta = if full.starts_with(&previous) {
            full[previous.len()..].to_string()
        } else {
            full.clone()
        };
        self.part_text.insert(id, full);
        if delta.is_empty() {
            return Vec::new();
        }
        let run_id = self.run_id.clone();
        let timestamp = now();
        if reasoning {
            vec![AgentEvent::Reasoning {
                run_id,
                content: delta,
                timestamp,
            }]
        } else {
            vec![AgentEvent::Message {
                run_id,
                content: delta,
                timestamp,
            }]
        }
    }

    fn map_tool(&mut self, part: &Value) -> Vec<AgentEvent> {
        let call_id = part
            .get("callID")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let tool = part
            .get("tool")
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string();
        let state = part.get("state");
        let status = state
            .and_then(|state| state.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("pending")
            .to_string();

        if !call_id.is_empty() && self.tool_status.get(&call_id) == Some(&status) {
            return Vec::new();
        }
        if !call_id.is_empty() {
            self.tool_status.insert(call_id, status.clone());
        }

        let timestamp = now();
        let input = state.and_then(|state| state.get("input"));
        let title = state
            .and_then(|state| state.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let run_id = self.run_id.clone();
        let finished = status == "completed" || status == "error";

        match tool.as_str() {
            "read" => input_path(input)
                .map(|path| {
                    vec![AgentEvent::FileRead {
                        run_id,
                        path,
                        timestamp,
                    }]
                })
                .unwrap_or_default(),
            "edit" | "write" | "patch" => input_path(input)
                .map(|path| {
                    vec![AgentEvent::FileChanged {
                        run_id,
                        path,
                        timestamp,
                    }]
                })
                .unwrap_or_default(),
            "bash" => {
                let command = input
                    .and_then(|input| input.get("command"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if finished {
                    let exit_code = state
                        .and_then(|state| state.get("metadata"))
                        .and_then(|metadata| metadata.get("exit").or_else(|| metadata.get("exitCode")))
                        .and_then(Value::as_i64)
                        .map(|value| value as i32)
                        .unwrap_or(if status == "error" { 1 } else { 0 });
                    let output = state
                        .and_then(|state| state.get("output"))
                        .and_then(Value::as_str)
                        .map(|output| truncate(output, 4000));
                    vec![AgentEvent::CommandFinished {
                        run_id,
                        command,
                        exit_code,
                        output,
                        timestamp,
                    }]
                } else {
                    vec![AgentEvent::CommandStarted {
                        run_id,
                        command,
                        timestamp,
                    }]
                }
            }
            _ => {
                let tool_title = if title.is_empty() {
                    tool.clone()
                } else {
                    title
                };
                vec![AgentEvent::Tool {
                    run_id,
                    name: tool,
                    title: tool_title,
                    status,
                    timestamp,
                }]
            }
        }
    }
}

fn input_path(input: Option<&Value>) -> Option<String> {
    let input = input?;
    for key in ["filePath", "path", "file", "filename"] {
        if let Some(value) = input.get(key).and_then(Value::as_str) {
            return Some(value.to_string());
        }
    }
    None
}

fn parse_todos(properties: Option<&Value>) -> Vec<TodoItem> {
    properties
        .and_then(|properties| properties.get("todos"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| TodoItem {
                    content: item
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    status: item
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("pending")
                        .to_string(),
                    priority: item
                        .get("priority")
                        .and_then(Value::as_str)
                        .unwrap_or("medium")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut truncated: String = value.chars().take(max).collect();
        truncated.push_str("\n…(truncated)");
        truncated
    }
}

pub fn spawn_event_listener(
    app: AppHandle,
    run_id: String,
    base_url: String,
    password: String,
    session_id: String,
) {
    std::thread::spawn(move || {
        let client = OpenCodeClient::new(base_url, password);
        let mut mapper = EventMapper::new(run_id);
        let response = match client.event_response() {
            Ok(response) => response,
            Err(_) => return,
        };
        let reader = BufReader::new(response);
        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(_) => break,
            };
            let data = match line.strip_prefix("data:") {
                Some(data) => data.trim(),
                None => continue,
            };
            if data.is_empty() {
                continue;
            }
            let value: Value = match serde_json::from_str(data) {
                Ok(value) => value,
                Err(_) => continue,
            };
            handle_event(&app, &client, &mut mapper, &session_id, &value);
        }
    });
}

fn handle_event(
    app: &AppHandle,
    client: &OpenCodeClient,
    mapper: &mut EventMapper,
    session_id: &str,
    value: &Value,
) {
    let properties = value.get("properties");
    let event_session = properties
        .and_then(|properties| properties.get("sessionID"))
        .and_then(Value::as_str)
        .or_else(|| {
            properties
                .and_then(|properties| properties.get("part"))
                .and_then(|part| part.get("sessionID"))
                .and_then(Value::as_str)
        });

    if let Some(event_session) = event_session {
        if event_session != session_id {
            return;
        }
    }

    match value.get("type").and_then(Value::as_str) {
        Some("permission.updated") | Some("permission.asked") => {
            if let Some(permission_id) = properties
                .and_then(|properties| properties.get("id"))
                .and_then(Value::as_str)
            {
                let target_session = properties
                    .and_then(|properties| properties.get("sessionID"))
                    .and_then(Value::as_str)
                    .unwrap_or(session_id);
                client.respond_permission(target_session, permission_id, "reject");
                let title = properties
                    .and_then(|properties| properties.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or("permission requested")
                    .to_string();
                push_event(
                    app,
                    AgentEvent::PermissionRequest {
                        run_id: mapper.run_id().to_string(),
                        title,
                        timestamp: now(),
                    },
                );
            }
        }
        _ => {
            for event in mapper.feed(value) {
                push_event(app, event);
            }
        }
    }
}

#[derive(Default)]
pub struct PromptCallbacks {
    pub on_success: Option<Box<dyn FnOnce(&AppHandle, &Value) + Send>>,
    pub on_failure: Option<Box<dyn FnOnce(&AppHandle, &str) + Send>>,
}

pub fn spawn_prompt(
    app: AppHandle,
    run_id: String,
    base_url: String,
    password: String,
    session_id: String,
    text: String,
    agent: Option<String>,
    model: Option<String>,
) {
    spawn_prompt_with(
        app,
        run_id,
        base_url,
        password,
        session_id,
        text,
        agent,
        model,
        None,
        PromptCallbacks::default(),
    );
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_prompt_with(
    app: AppHandle,
    run_id: String,
    base_url: String,
    password: String,
    session_id: String,
    text: String,
    agent: Option<String>,
    model: Option<String>,
    format: Option<Value>,
    callbacks: PromptCallbacks,
) {
    std::thread::spawn(move || {
        let client = OpenCodeClient::new(base_url, password);
        set_status(&app, &run_id, AgentRunStatus::Running);
        match client.prompt(
            &session_id,
            &text,
            agent.as_deref(),
            model.as_deref(),
            format.as_ref(),
        ) {
            Ok(value) => {
                if let Some(on_success) = callbacks.on_success {
                    on_success(&app, &value);
                }
                finish_success(&app, &run_id, &value);
            }
            Err(error) => {
                if let Some(on_failure) = callbacks.on_failure {
                    on_failure(&app, &error);
                }
                finish_failure(&app, &run_id, error);
            }
        }
    });
}

pub fn run_prompt_blocking(
    app: &AppHandle,
    run_id: &str,
    base_url: String,
    password: String,
    session_id: String,
    text: String,
    agent: Option<String>,
    model: Option<String>,
) -> Result<(), String> {
    let client = OpenCodeClient::new(base_url, password);
    set_status(app, run_id, AgentRunStatus::Running);
    match client.prompt(&session_id, &text, agent.as_deref(), model.as_deref(), None) {
        Ok(value) => {
            finish_success(app, run_id, &value);
            Ok(())
        }
        Err(error) => {
            finish_failure(app, run_id, error.clone());
            Err(error)
        }
    }
}

fn persist_run(app: &AppHandle, run_id: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let snapshot = {
        let Ok(agents) = state.agents.lock() else {
            return;
        };
        agents
            .get(run_id)
            .map(|runtime| {
                (
                    runtime.run.clone(),
                    runtime.output.clone(),
                    runtime.cost,
                    runtime.model.clone(),
                )
            })
    };
    if let Some((run, output, cost, model)) = snapshot {
        if let Ok(connection) = state.db.lock() {
            let _ = db::upsert_agent_run(&connection, &run, &output, cost, model.as_deref());
        }
    }
}

fn push_event(app: &AppHandle, event: AgentEvent) {
    if let Some(state) = app.try_state::<AppState>() {
        {
            if let Ok(mut agents) = state.agents.lock() {
                if let Some(runtime) = agents.get_mut(event.run_id()) {
                    runtime.events.push(event.clone());
                }
            }
        }
        if !matches!(
            event,
            AgentEvent::Message { .. } | AgentEvent::Reasoning { .. }
        ) {
            if let Ok(connection) = state.db.lock() {
                let _ = db::insert_agent_event(&connection, event.run_id(), &event, &now());
            }
        }
    }
    let _ = app.emit(AGENT_EVENT, &event);
}

fn set_status(app: &AppHandle, run_id: &str, status: AgentRunStatus) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut agents) = state.agents.lock() {
            if let Some(runtime) = agents.get_mut(run_id) {
                runtime.run.status = status;
            }
        }
        persist_run(app, run_id);
    }
    let _ = app.emit(
        AGENT_STATUS,
        json!({ "runId": run_id, "status": status }),
    );
}

fn finish_success(app: &AppHandle, run_id: &str, value: &Value) {
    let output = extract_text(value);
    let info = value.get("info");
    let cost = info
        .and_then(|info| info.get("cost"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let model = info.and_then(|info| {
        let provider = info.get("providerID").and_then(Value::as_str);
        let model = info.get("modelID").and_then(Value::as_str);
        match (provider, model) {
            (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
            _ => None,
        }
    });
    finalize(app, run_id, output, cost, model, true, None);
}

fn finish_failure(app: &AppHandle, run_id: &str, error: String) {
    finalize(app, run_id, String::new(), 0.0, None, false, Some(error));
}

fn finalize(
    app: &AppHandle,
    run_id: &str,
    output: String,
    cost: f64,
    model: Option<String>,
    success: bool,
    error: Option<String>,
) {
    let timestamp = now();
    let event = if success {
        AgentEvent::Finished {
            run_id: run_id.to_string(),
            timestamp: timestamp.clone(),
        }
    } else {
        AgentEvent::Failed {
            run_id: run_id.to_string(),
            error: error.unwrap_or_default(),
            timestamp: timestamp.clone(),
        }
    };

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut agents) = state.agents.lock() {
            if let Some(runtime) = agents.get_mut(run_id) {
                runtime.events.push(event.clone());
                runtime.run.completed_at = Some(timestamp);
                runtime.cost = cost;
                runtime.model = model;
                if success {
                    runtime.output = output;
                    runtime.run.status = AgentRunStatus::Succeeded;
                } else {
                    runtime.run.status = AgentRunStatus::Failed;
                }
            }
        }
        persist_run(app, run_id);
    }

    let _ = app.emit(AGENT_EVENT, &event);
    let status = if success {
        AgentRunStatus::Succeeded
    } else {
        AgentRunStatus::Failed
    };
    let _ = app.emit(
        AGENT_STATUS,
        json!({ "runId": run_id, "status": status }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_body_includes_agent_and_model() {
        let body = build_prompt_body("hello", Some("plan"), Some("deepseek/deepseek-chat"), None);
        assert_eq!(body["parts"][0]["text"], "hello");
        assert_eq!(body["agent"], "plan");
        assert_eq!(body["model"]["providerID"], "deepseek");
        assert_eq!(body["model"]["modelID"], "deepseek-chat");
    }

    #[test]
    fn build_prompt_body_without_options() {
        let body = build_prompt_body("hi", None, None, None);
        assert!(body.get("agent").is_none());
        assert!(body.get("model").is_none());
        assert!(body.get("format").is_none());
    }

    #[test]
    fn build_prompt_body_includes_json_schema_format() {
        let schema = json!({ "type": "object" });
        let body = build_prompt_body("hi", None, None, Some(&schema));
        assert_eq!(body["format"]["type"], "json_schema");
        assert_eq!(body["format"]["schema"]["type"], "object");
    }

    #[test]
    fn streams_text_deltas() {
        let mut mapper = EventMapper::new("run1".to_string());
        let first = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "p1", "type": "text", "text": "Hello" } }
        }));
        assert!(matches!(first.first(), Some(AgentEvent::Message { content, .. }) if content == "Hello"));
        let second = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "p1", "type": "text", "text": "Hello world" } }
        }));
        assert!(matches!(second.first(), Some(AgentEvent::Message { content, .. }) if content == " world"));
        let third = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "p1", "type": "text", "text": "Hello world" } }
        }));
        assert!(third.is_empty());
    }

    #[test]
    fn maps_reasoning_part() {
        let mut mapper = EventMapper::new("run1".to_string());
        let events = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "r1", "type": "reasoning", "text": "thinking" } }
        }));
        assert!(matches!(events.first(), Some(AgentEvent::Reasoning { .. })));
    }

    #[test]
    fn maps_read_tool_to_file_read() {
        let mut mapper = EventMapper::new("run1".to_string());
        let events = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "t1", "type": "tool", "callID": "c1", "tool": "read",
                "state": { "status": "running", "input": { "filePath": "src/app.ts" } } } }
        }));
        assert!(matches!(events.first(), Some(AgentEvent::FileRead { path, .. }) if path == "src/app.ts"));
    }

    #[test]
    fn maps_bash_tool_lifecycle() {
        let mut mapper = EventMapper::new("run1".to_string());
        let running = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "t1", "type": "tool", "callID": "c1", "tool": "bash",
                "state": { "status": "running", "input": { "command": "npm test" } } } }
        }));
        assert!(matches!(running.first(), Some(AgentEvent::CommandStarted { command, .. }) if command == "npm test"));
        let done = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "t1", "type": "tool", "callID": "c1", "tool": "bash",
                "state": { "status": "completed", "input": { "command": "npm test" }, "metadata": { "exit": 0 }, "output": "tests passed" } } }
        }));
        let finished = done.first().expect("command finished");
        match finished {
            AgentEvent::CommandFinished { exit_code, output, .. } => {
                assert_eq!(*exit_code, 0);
                assert_eq!(output.as_deref(), Some("tests passed"));
            }
            _ => panic!("expected command_finished"),
        }
        let repeat = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "t1", "type": "tool", "callID": "c1", "tool": "bash",
                "state": { "status": "completed", "input": { "command": "npm test" } } } }
        }));
        assert!(repeat.is_empty());
    }

    #[test]
    fn maps_unknown_tool_to_tool_event() {
        let mut mapper = EventMapper::new("run1".to_string());
        let events = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "t1", "type": "tool", "callID": "c1", "tool": "grep",
                "state": { "status": "running", "title": "Searching for auth", "input": {} } } }
        }));
        assert!(matches!(
            events.first(),
            Some(AgentEvent::Tool { name, title, .. }) if name == "grep" && title == "Searching for auth"
        ));
    }

    #[test]
    fn maps_todo_event() {
        let mut mapper = EventMapper::new("run1".to_string());
        let events = mapper.feed(&json!({
            "type": "todo.updated",
            "properties": { "sessionID": "s1", "todos": [
                { "content": "Do thing", "status": "pending", "priority": "high" }
            ] }
        }));
        match events.first() {
            Some(AgentEvent::Todo { todos, .. }) => assert_eq!(todos.len(), 1),
            _ => panic!("expected todo event"),
        }
    }

    #[test]
    fn extracts_assistant_text() {
        let value = json!({
            "parts": [
                { "type": "text", "text": "first" },
                { "type": "tool", "tool": "bash" },
                { "type": "text", "text": "second" }
            ]
        });
        assert_eq!(extract_text(&value), "first\nsecond");
    }

    #[test]
    fn free_port_is_nonzero() {
        assert!(free_port().unwrap() > 0);
    }

    #[test]
    #[ignore = "spawns a real opencode server and spends tokens; run with --ignored"]
    fn live_server_returns_response() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("repo");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "# Test repo\n").unwrap();
        let _ = std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&dir)
            .status();

        let program = resolve_program(DEFAULT_COMMAND).expect("opencode on PATH");
        let log_dir = temp.path().join("logs");
        let config = models::inline_config(models::DEFAULT_MODEL);
        let mut server =
            start_server(&program, &dir, &log_dir, READ_ONLY_PERMISSION, &config)
                .expect("start server");
        let client = server.client();
        let session = client
            .create_session("phase5-test")
            .expect("create session");
        let value = client
            .prompt(
                &session,
                "Reply with exactly the text: pong",
                None,
                Some(models::DEFAULT_MODEL),
                None,
            )
            .expect("prompt");
        let text = extract_text(&value);
        let cost = value
            .get("info")
            .and_then(|info| info.get("cost"))
            .and_then(Value::as_f64)
            .unwrap_or(-1.0);
        eprintln!("live response: {text} cost: {cost}");
        assert!(!text.trim().is_empty(), "expected a non-empty response");
        assert_eq!(cost, 0.0, "expected a free (zero-cost) run");
        server.stop(Some(&session));
    }
}
