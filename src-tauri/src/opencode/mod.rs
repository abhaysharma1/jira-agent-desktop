use std::collections::HashMap;
use std::error::Error;
use std::ffi::OsString;
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
pub mod orphans;

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
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    drop(listener);
    Ok(port)
}

/// Environment variables stripped before spawning `opencode serve`: this app's
/// JIRA integration and the backend's database/JWT secrets. Provider keys
/// (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `NPM_TOKEN`, `AWS_*`, …) are
/// deliberately preserved because that is often how OpenCode is authenticated.
fn is_sensitive_env(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.starts_with("JIRA_")
        || upper.contains("DATABASE_URL")
        || matches!(
            upper.as_str(),
            "JWT_SECRET" | "GH_TOKEN" | "GITHUB_TOKEN" | "GITHUB_PAT" | "PUBLIC_BASE_URL"
        )
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

/// Total time allowed for short control-plane calls (health, provider list,
/// session create, abort, permission replies). Agent prompts deliberately do
/// not use this value.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(600);

/// If the agent emits no events for this long it is considered stuck and the
/// session is aborted. Generous on purpose: a long-running test/build command
/// is quiet while it executes and must not be cut off.
const STALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Running the exact same shell command this many times in a row is treated as
/// a doom loop (e.g. re-running a failing test command without converging).
const LOOP_REPEAT_LIMIT: u32 = 6;

pub struct OpenCodeClient {
    base_url: String,
    password: String,
    http: reqwest::blocking::Client,
    /// Dedicated client for `prompt`. It is deliberately built with NO total
    /// request timeout: a single agent turn can run far longer than a control
    /// call while it implements changes and runs the repository's test suite.
    /// reqwest's client builder silently defaults to a 30-second total timeout
    /// (`Timeout::default()` in reqwest 0.12), which is what actually killed
    /// planning/implementation prompts at ~30s; `timeout(None)` removes that
    /// cap. Progress arrives over the event stream, and a stuck turn is aborted
    /// by the stall/loop watchdog rather than a clock.
    prompt_http: reqwest::blocking::Client,
}

impl OpenCodeClient {
    pub fn new(base_url: String, password: String) -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(CONTROL_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        // The prompt client must never carry reqwest's default 30s total
        // timeout (see the field docs). The connect-timeout only bounds the
        // TCP handshake so a dead/black-holed server fails fast; it does not
        // cap the prompt duration.
        let prompt_http = reqwest::blocking::Client::builder()
            .timeout(None)
            .connect_timeout(Some(Duration::from_secs(10)))
            .build()
            .unwrap_or_else(|_| {
                // The build above only fails when no usable connector exists
                // (e.g. a missing TLS backend). Dropping the connect-timeout
                // still yields a fully timeout-free client, which is the point.
                reqwest::blocking::Client::builder()
                    .timeout(None)
                    .build()
                    .expect("timeout(None) client build cannot fail")
            });
        Self {
            base_url,
            password,
            http,
            prompt_http,
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
            .prompt_http
            .post(format!("{}/session/{session_id}/message", self.base_url))
            .basic_auth("opencode", Some(&self.password))
            .json(&build_prompt_body(text, agent, model, format))
            .send()
            .map_err(|error| describe_prompt_error(&error))?;
        if !response.status().is_success() {
            return Err(format!(
                "prompt returned {}: {}",
                response.status(),
                bounded_response_body(response)
            ));
        }
        response
            .json()
            .map_err(|error| describe_prompt_error(&error))
    }

    pub fn abort(&self, session_id: &str) {
        let _ = self.post(&format!("/session/{session_id}/abort")).send();
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
            // Same silent 30s default as the prompt client: a quiet tool
            // execution (a long build or test run) emits no SSE frames, and a
            // read that blocks for 30s must not kill the event stream.
            .timeout(None)
            .connect_timeout(Some(Duration::from_secs(10)))
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
    pub port: u16,
}

impl ManagedServer {
    pub fn client(&self) -> OpenCodeClient {
        OpenCodeClient::new(self.base_url.clone(), self.password.clone())
    }

    /// OS process id of the `opencode serve` child, for crash recovery.
    pub fn pid(&self) -> u32 {
        self.child.id()
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

    // Never hand the agent this app's own credentials: pass the parent
    // environment minus our integration and database secrets.
    let scrubbed: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(key, _)| !is_sensitive_env(&key.to_string_lossy()))
        .collect();

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
        .env_clear()
        .envs(scrubbed)
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
            let contents = std::fs::read_to_string(&log_path).unwrap_or_default();
            crate::logging::error(
                "opencode",
                "server start timed out",
                json!({
                    "port": port,
                    "log": log_path.to_string_lossy(),
                    "output": contents,
                }),
            );
            return Err(
                "timed out waiting for the opencode server to start; see ~/.ticketbridge/logs"
                    .to_string(),
            );
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    let pid = child.id();
    crate::logging::info(
        "opencode",
        "server started",
        json!({ "port": port, "pid": pid }),
    );

    Ok(ManagedServer {
        child,
        base_url,
        password,
        port,
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
    last_bash_command: Option<String>,
    bash_repeat_count: u32,
}

impl EventMapper {
    pub fn new(run_id: String) -> Self {
        Self {
            run_id,
            part_text: HashMap::new(),
            tool_status: HashMap::new(),
            last_bash_command: None,
            bash_repeat_count: 0,
        }
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// A human-readable reason the agent is considered stuck in a loop, if the
    /// same shell command has just been repeated enough times in a row.
    pub fn loop_reason(&self) -> Option<String> {
        if self.bash_repeat_count >= LOOP_REPEAT_LIMIT {
            Some(format!(
                "the agent ran `{}` {} times in a row and appears stuck in a loop",
                self.last_bash_command.as_deref().unwrap_or(""),
                self.bash_repeat_count
            ))
        } else {
            None
        }
    }

    pub fn feed(&mut self, value: &Value) -> Vec<AgentEvent> {
        let event_type = match value.get("type").and_then(Value::as_str) {
            Some(event_type) => event_type,
            None => return Vec::new(),
        };
        let properties = value.get("properties");
        match event_type {
            "message.part.updated" | "message.part.created" => {
                match properties.and_then(|properties| properties.get("part")) {
                    Some(part) => self.map_part(part),
                    None => Vec::new(),
                }
            }
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
                if finished && !command.is_empty() {
                    if self.last_bash_command.as_deref() == Some(command.as_str()) {
                        self.bash_repeat_count += 1;
                    } else {
                        self.last_bash_command = Some(command.clone());
                        self.bash_repeat_count = 1;
                    }
                }
                if finished {
                    let exit_code = state
                        .and_then(|state| state.get("metadata"))
                        .and_then(|metadata| {
                            metadata.get("exit").or_else(|| metadata.get("exitCode"))
                        })
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

/// How much of an HTTP error-response body is kept for the DB/UI error field.
/// Error pages can be huge, and the response is not trusted input.
const MAX_ERROR_BODY: usize = 2048;

/// Builds an actionable, redacted one-line error for a failed prompt request
/// (connection refused / reset / request timeout / response timeout / premature
/// termination / server exit / HTTP error / provider error / cancellation is
/// handled by the stall watchdog, not reqwest). Request bodies, credentials and
/// auth headers are never included; the underlying error chain is bounded.
fn describe_prompt_error(error: &reqwest::Error) -> String {
    let url = error
        .url()
        .map(|url| format!(" for url ({url})"))
        .unwrap_or_default();
    let chains = error_chain(error);
    let category = classify_prompt_failure(
        error.is_connect(),
        error.is_timeout(),
        error.is_body(),
        error.is_decode(),
        &chains,
    );
    format!(
        "prompt failed: {category}{url} [{}]",
        summarize_chain(&chains)
    )
}

/// The immediate and underlying error messages for a reqwest failure, in order.
fn error_chain(error: &reqwest::Error) -> Vec<String> {
    let mut lines = Vec::new();
    let mut next = error.source();
    while let Some(source) = next {
        lines.push(source.to_string());
        next = source.source();
    }
    lines
}

/// Picks a single human-readable category for a failed request based on
/// reqwest's error predicates and the underlying error chain. Pure so it can be
/// unit-tested with representative chains from every platform.
fn classify_prompt_failure(
    is_connect: bool,
    is_timeout: bool,
    is_body: bool,
    is_decode: bool,
    chains: &[String],
) -> &'static str {
    let mentions = |needles: &[&str]| {
        chains.iter().any(|line| {
            let lower = line.to_ascii_lowercase();
            needles.iter().any(|needle| lower.contains(needle))
        })
    };

    if is_timeout {
        if mentions(&["connect"]) {
            "timed out while connecting to the opencode server"
        } else {
            "timed out waiting for the opencode response"
        }
    } else if is_connect {
        if mentions(&["refused"]) {
            "connection refused by the opencode server"
        } else if mentions(&["reset", "forcibly closed"]) {
            "connection reset while connecting to the opencode server"
        } else {
            "could not connect to the opencode server"
        }
    } else if mentions(&[
        "forcibly closed",
        "reset by peer",
        "connection reset",
        "broken pipe",
        "unexpected eof",
        "unexpected end of file",
        "end of file before message",
        "closed before message completed",
        "channel closed",
        "producer dropped",
    ]) {
        "the opencode server terminated the connection before the response completed (the server process likely crashed or was killed)"
    } else if is_body {
        "the response body could not be read from the opencode server"
    } else if is_decode {
        "the opencode server returned an unparseable response body"
    } else {
        "the opencode request failed"
    }
}

/// At most three chain entries, each truncated, so DB/UI error fields stay small.
fn summarize_chain(chains: &[String]) -> String {
    if chains.is_empty() {
        return "no underlying detail".to_string();
    }
    chains
        .iter()
        .take(3)
        .map(|line| {
            let mut line = line.clone();
            if line.chars().count() > 200 {
                line = line.chars().take(200).collect::<String>() + "…";
            }
            line
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Reads at most `MAX_ERROR_BODY` bytes of an HTTP error response body and
/// redacts anything that looks like a credential before it reaches the error
/// string stored in the database.
fn bounded_response_body(mut response: reqwest::blocking::Response) -> String {
    use std::io::Read;
    let mut buffer = [0u8; 1024];
    let mut body = String::new();
    loop {
        match response.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                body.push_str(&String::from_utf8_lossy(&buffer[..read]));
                if body.len() >= MAX_ERROR_BODY {
                    body.truncate(MAX_ERROR_BODY);
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let truncated = body.len() >= MAX_ERROR_BODY;
    let body = crate::logging::redact_secrets(&body);
    if body.is_empty() {
        return String::new();
    }
    if truncated {
        format!("{body}\n…(body truncated to {MAX_ERROR_BODY} bytes)")
    } else {
        body
    }
}

pub fn spawn_event_listener(
    app: AppHandle,
    run_id: String,
    base_url: String,
    password: String,
    session_id: String,
) {
    spawn_stall_watchdog(
        app.clone(),
        run_id.clone(),
        base_url.clone(),
        password.clone(),
        session_id.clone(),
    );
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

/// Watches a run for total inactivity. If the agent emits no events for
/// `STALL_TIMEOUT`, the session is aborted and the run failed. A long-running
/// shell command is quiet while it executes, so the timeout is deliberately
/// generous.
fn spawn_stall_watchdog(
    app: AppHandle,
    run_id: String,
    base_url: String,
    password: String,
    session_id: String,
) {
    std::thread::spawn(move || {
        let client = OpenCodeClient::new(base_url, password);
        loop {
            std::thread::sleep(Duration::from_secs(15));
            let stalled = {
                let Some(state) = app.try_state::<AppState>() else {
                    return;
                };
                let Ok(agents) = state.agents.lock() else {
                    return;
                };
                let Some(runtime) = agents.get(&run_id) else {
                    return;
                };
                if runtime.run.completed_at.is_some() {
                    return;
                }
                runtime.last_activity.elapsed() >= STALL_TIMEOUT
            };
            if stalled {
                let reason = format!(
                    "no agent activity for {} minutes; aborting a stuck turn",
                    STALL_TIMEOUT.as_secs() / 60
                );
                stuck_abort(&app, &client, &run_id, &session_id, &reason);
                return;
            }
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

    if let Some(reason) = mapper.loop_reason() {
        stuck_abort(app, client, mapper.run_id(), session_id, &reason);
    }
}

#[derive(Default)]
pub struct PromptCallbacks {
    pub on_success: Option<Box<dyn Fn(&AppHandle, &Value) + Send + Sync>>,
    pub on_failure: Option<Box<dyn Fn(&AppHandle, &str) + Send + Sync>>,
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
    let callbacks = std::sync::Arc::new(callbacks);
    // Hand the failure handler to the event-listener thread so a stuck/looping
    // agent can be aborted and the owning task marked failed with a clear
    // message, even if the blocking prompt below is still waiting on the server.
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut agents) = state.agents.lock() {
            if let Some(runtime) = agents.get_mut(&run_id) {
                runtime.failure_handler = callbacks.clone();
            }
        }
    }
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
                // Only run the success callback if this call actually finalized
                // the run; the event-listener loop guard may have failed it
                // first.
                if finish_success(&app, &run_id, &value) {
                    if let Some(on_success) = callbacks.on_success.as_ref() {
                        on_success(&app, &value);
                    }
                }
            }
            Err(error) => {
                if finish_failure(&app, &run_id, error.clone()) {
                    if let Some(on_failure) = callbacks.on_failure.as_ref() {
                        on_failure(&app, &error);
                    }
                }
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
        agents.get(run_id).map(|runtime| {
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
                    runtime.last_activity = std::time::Instant::now();
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
    let _ = app.emit(AGENT_STATUS, json!({ "runId": run_id, "status": status }));
}

/// Output text, cost, and model id extracted from a successful prompt response.
fn success_details(value: &Value) -> (String, f64, Option<String>) {
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
    (output, cost, model)
}

fn finish_success(app: &AppHandle, run_id: &str, value: &Value) -> bool {
    let (output, cost, model) = success_details(value);
    finalize(app, run_id, output, cost, model, true, None)
}

fn finish_failure(app: &AppHandle, run_id: &str, error: String) -> bool {
    finalize(app, run_id, String::new(), 0.0, None, false, Some(error))
}

fn run_finalized(app: &AppHandle, run_id: &str) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return true;
    };
    let agents = match state.agents.lock() {
        Ok(agents) => agents,
        Err(_) => return true,
    };
    agents
        .get(run_id)
        .map(|runtime| runtime.run.completed_at.is_some())
        .unwrap_or(true)
}

/// Aborts a session the watchdog has flagged as stuck or looping, fails the run
/// with a clear message, and — only if this call actually finalized the run —
/// marks the owning task failed via the stored failure handler. Called from the
/// event-listener and stall-watchdog threads.
fn stuck_abort(
    app: &AppHandle,
    client: &OpenCodeClient,
    run_id: &str,
    session_id: &str,
    reason: &str,
) {
    if run_finalized(app, run_id) {
        return;
    }
    let handler = {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let Ok(agents) = state.agents.lock() else {
            return;
        };
        let Some(runtime) = agents.get(run_id) else {
            return;
        };
        runtime.failure_handler.clone()
    };

    crate::logging::warn(
        "opencode",
        "agent appeared stuck; aborting",
        json!({ "runId": run_id, "reason": reason }),
    );

    client.abort(session_id);

    if finish_failure(app, run_id, reason.to_string()) {
        if let Some(on_failure) = handler.on_failure.as_ref() {
            on_failure(app, reason);
        }
    }
}

/// Applies the exactly-once run-finalization state mutation. Returns false when
/// the run has already been finalized — the event-listener loop guard may
/// finalize an aborted/stuck run before the blocking prompt returns, and
/// whichever thread gets there first wins. Stays state-level so the semantics
/// are testable without a Tauri app handle.
fn apply_finalize(
    state: &AppState,
    run_id: &str,
    event: &AgentEvent,
    timestamp: &str,
    output: String,
    cost: f64,
    model: Option<String>,
    success: bool,
) -> bool {
    let Ok(mut agents) = state.agents.lock() else {
        return true;
    };
    let Some(runtime) = agents.get_mut(run_id) else {
        return true;
    };
    if runtime.run.completed_at.is_some() {
        return false;
    }
    runtime.events.push(event.clone());
    runtime.run.completed_at = Some(timestamp.to_string());
    runtime.cost = cost;
    runtime.model = model;
    if success {
        runtime.output = output;
        runtime.run.status = AgentRunStatus::Succeeded;
    } else {
        runtime.run.status = AgentRunStatus::Failed;
    }
    true
}

fn finalize(
    app: &AppHandle,
    run_id: &str,
    output: String,
    cost: f64,
    model: Option<String>,
    success: bool,
    error: Option<String>,
) -> bool {
    let timestamp = now();
    let error_message = error.unwrap_or_default();
    let event = if success {
        AgentEvent::Finished {
            run_id: run_id.to_string(),
            timestamp: timestamp.clone(),
        }
    } else {
        AgentEvent::Failed {
            run_id: run_id.to_string(),
            error: error_message.clone(),
            timestamp: timestamp.clone(),
        }
    };

    if let Some(state) = app.try_state::<AppState>() {
        if !apply_finalize(
            &state, run_id, &event, &timestamp, output, cost, model, success,
        ) {
            return false;
        }
        persist_run(app, run_id);
    }

    // Phase 26: durations and failures are tracked centrally here, the single
    // completion point for every agent run (planning, implementation, repair).
    let finished_run = app.try_state::<AppState>().and_then(|state| {
        state
            .agents
            .lock()
            .ok()
            .and_then(|agents| agents.get(run_id).map(|runtime| runtime.run.clone()))
    });
    if let Some(run) = finished_run {
        crate::metrics::record_run(
            app,
            &run,
            success,
            (!success).then_some(error_message.as_str()),
        );
    }

    let _ = app.emit(AGENT_EVENT, &event);
    let status = if success {
        AgentRunStatus::Succeeded
    } else {
        AgentRunStatus::Failed
    };
    let _ = app.emit(AGENT_STATUS, json!({ "runId": run_id, "status": status }));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_sensitive_environment_variables() {
        for key in [
            "JIRA_CLIENT_ID",
            "JIRA_CLIENT_SECRET",
            "JWT_SECRET",
            "DATABASE_URL",
            "TEST_DATABASE_URL",
            "PRISMA_DATABASE_URL",
            "GITHUB_TOKEN",
            "GH_TOKEN",
            "GITHUB_PAT",
        ] {
            assert!(is_sensitive_env(key), "{key} should be scrubbed");
        }

        // Provider/CI keys the user may rely on are kept.
        for key in [
            "PATH",
            "HOME",
            "USERPROFILE",
            "SYSTEMROOT",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "NPM_TOKEN",
            "AWS_ACCESS_KEY_ID",
        ] {
            assert!(!is_sensitive_env(key), "{key} should be preserved");
        }
    }

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
        assert!(
            matches!(first.first(), Some(AgentEvent::Message { content, .. }) if content == "Hello")
        );
        let second = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "p1", "type": "text", "text": "Hello world" } }
        }));
        assert!(
            matches!(second.first(), Some(AgentEvent::Message { content, .. }) if content == " world")
        );
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
        assert!(
            matches!(events.first(), Some(AgentEvent::FileRead { path, .. }) if path == "src/app.ts")
        );
    }

    #[test]
    fn maps_bash_tool_lifecycle() {
        let mut mapper = EventMapper::new("run1".to_string());
        let running = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "t1", "type": "tool", "callID": "c1", "tool": "bash",
                "state": { "status": "running", "input": { "command": "npm test" } } } }
        }));
        assert!(
            matches!(running.first(), Some(AgentEvent::CommandStarted { command, .. }) if command == "npm test")
        );
        let done = mapper.feed(&json!({
            "type": "message.part.updated",
            "properties": { "part": { "id": "t1", "type": "tool", "callID": "c1", "tool": "bash",
                "state": { "status": "completed", "input": { "command": "npm test" }, "metadata": { "exit": 0 }, "output": "tests passed" } } }
        }));
        let finished = done.first().expect("command finished");
        match finished {
            AgentEvent::CommandFinished {
                exit_code, output, ..
            } => {
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
    fn detects_repeated_command_loop() {
        let mut mapper = EventMapper::new("run1".to_string());
        for i in 0..LOOP_REPEAT_LIMIT {
            mapper.feed(&json!({
                "type": "message.part.updated",
                "properties": { "part": { "id": format!("t{i}"), "type": "tool", "callID": format!("c{i}"),
                    "tool": "bash", "state": { "status": "completed",
                        "input": { "command": "npm test" }, "metadata": { "exit": 1 } } } }
            }));
        }
        assert!(mapper.loop_reason().is_some());
    }

    #[test]
    fn different_commands_reset_loop_counter() {
        let mut mapper = EventMapper::new("run1".to_string());
        for (i, command) in ["npm test", "npm run lint", "npm test"].iter().enumerate() {
            mapper.feed(&json!({
                "type": "message.part.updated",
                "properties": { "part": { "id": format!("t{i}"), "type": "tool", "callID": format!("c{i}"),
                    "tool": "bash", "state": { "status": "completed",
                        "input": { "command": command } } } }
            }));
        }
        assert!(mapper.loop_reason().is_none());
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
        let mut server = start_server(&program, &dir, &log_dir, READ_ONLY_PERMISSION, &config)
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

    // ------------------------------------------------------------------
    // Mock-server scenarios for the prompt timeout / diagnostics fixes.
    // A real opencode server or paid provider call is never involved.
    // ------------------------------------------------------------------

    use crate::commands::runtime::register_run;
    use crate::state::AppState;
    use crate::test_support::MockServer;

    fn temp_state() -> (tempfile::TempDir, AppState) {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::load_with_key_source(
            temp.path().to_path_buf(),
            crate::secure_store::KeySource::File,
        )
        .unwrap();
        (temp, state)
    }

    fn insert_run(state: &AppState, run_id: &str, status: AgentRunStatus) {
        let run = crate::domain::AgentRun {
            id: run_id.to_string(),
            task_id: "task-1".to_string(),
            mode: "implementation".to_string(),
            agent: Some("build".to_string()),
            status,
            session_id: None,
            started_at: "2026-10-10T00:00:00Z".to_string(),
            completed_at: None,
        };
        register_run(
            state,
            run,
            String::new(),
            0.0,
            None,
            crate::test_support::dummy_managed_server(),
        )
        .unwrap();
    }

    #[test]
    fn health_reports_version_and_rejects_errors() {
        let server = MockServer::start(|request, _body| match request {
            "GET /global/health" => (200, r#"{"version":"1.2.3"}"#.to_string()),
            _ => (500, "boom".to_string()),
        });
        let client = OpenCodeClient::new(server.base_url(), "pw".to_string());
        assert_eq!(client.health().unwrap(), "1.2.3");
    }

    #[test]
    fn health_fails_clearly_when_server_unreachable() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener); // nothing is listening any more: not ready
        let client = OpenCodeClient::new(format!("http://{addr}"), "pw".to_string());
        let error = client.health().unwrap_err();
        assert!(error.contains("health request failed"), "got: {error}");
    }

    #[test]
    fn prompt_success_records_output_cost_and_model() {
        let server = MockServer::start(|request, _body| {
            match request {
            "POST /session/ses_1/message" => (
                200,
                r#"{"parts":[{"type":"text","text":"all done"}],"info":{"cost":0.5,"providerID":"opencode","modelID":"big-pickle"}}"#
                    .to_string(),
            ),
            _ => (404, "{}".to_string()),
        }
        });
        let client = OpenCodeClient::new(server.base_url(), "pw".to_string());
        let value = client
            .prompt("ses_1", "implement it", None, None, None)
            .unwrap();

        let (output, cost, model) = success_details(&value);
        let (_temp, state) = temp_state();
        insert_run(&state, "run-1", AgentRunStatus::Running);

        let event = AgentEvent::Finished {
            run_id: "run-1".to_string(),
            timestamp: "2026-10-10T01:00:00Z".to_string(),
        };
        assert!(apply_finalize(
            &state,
            "run-1",
            &event,
            "2026-10-10T01:00:00Z",
            output.clone(),
            cost,
            model.clone(),
            true
        ));

        let agents = state.agents.lock().unwrap();
        let runtime = &agents["run-1"];
        assert_eq!(runtime.run.status, AgentRunStatus::Succeeded);
        assert_eq!(runtime.output, "all done");
        assert_eq!(runtime.cost, 0.5);
        assert_eq!(runtime.model.as_deref(), Some("opencode/big-pickle"));
        assert!(runtime.run.completed_at.is_some());
        assert!(runtime
            .events
            .iter()
            .any(|event| matches!(event, AgentEvent::Finished { .. })));
    }

    #[test]
    fn finalize_runs_exactly_once() {
        let (_temp, state) = temp_state();
        insert_run(&state, "run-1", AgentRunStatus::Running);

        let success_event = AgentEvent::Finished {
            run_id: "run-1".to_string(),
            timestamp: "2026-10-10T01:00:00Z".to_string(),
        };
        assert!(apply_finalize(
            &state,
            "run-1",
            &success_event,
            "2026-10-10T01:00:00Z",
            "out".to_string(),
            1.0,
            None,
            true
        ));
        // A second finalize (e.g. the blocking prompt thread returning after the
        // event listener already finalized) must be a no-op.
        assert!(!apply_finalize(
            &state,
            "run-1",
            &success_event,
            "2026-10-10T01:00:00Z",
            "second".to_string(),
            2.0,
            None,
            true
        ));

        let agents = state.agents.lock().unwrap();
        let runtime = &agents["run-1"];
        assert_eq!(
            runtime.output, "out",
            "late finalize must not overwrite the run"
        );
        assert_eq!(runtime.cost, 1.0);
        assert_eq!(
            runtime
                .events
                .iter()
                .filter(|event| matches!(event, AgentEvent::Finished { .. }))
                .count(),
            1,
            "exactly one Finished event"
        );
    }

    #[test]
    fn async_completion_failure_wins_over_late_success() {
        let (_temp, state) = temp_state();
        insert_run(&state, "run-1", AgentRunStatus::Running);

        // The event-listener thread aborts a stuck turn first…
        let failed_event = AgentEvent::Failed {
            run_id: "run-1".to_string(),
            error: "no agent activity for 15 minutes; aborting a stuck turn".to_string(),
            timestamp: "2026-10-10T01:00:00Z".to_string(),
        };
        assert!(apply_finalize(
            &state,
            "run-1",
            &failed_event,
            "2026-10-10T01:00:00Z",
            String::new(),
            0.0,
            None,
            false
        ));

        // …then the blocking prompt finally returns success. It must not
        // override the failure (async completion guard).
        let success_event = AgentEvent::Finished {
            run_id: "run-1".to_string(),
            timestamp: "2026-10-10T01:00:01Z".to_string(),
        };
        assert!(!apply_finalize(
            &state,
            "run-1",
            &success_event,
            "2026-10-10T01:00:01Z",
            "late output".to_string(),
            9.0,
            None,
            true
        ));

        let agents = state.agents.lock().unwrap();
        let runtime = &agents["run-1"];
        assert_eq!(runtime.run.status, AgentRunStatus::Failed);
        assert_eq!(runtime.output, "");
        assert_eq!(runtime.cost, 0.0);
        assert!(runtime.events.iter().any(
            |event| matches!(event, AgentEvent::Failed { error, .. } if error.contains("stuck"))
        ));
    }

    #[test]
    fn classifies_every_failure_shape() {
        let s = |line: &str| line.to_string();
        let refused = classify_prompt_failure(
            true,
            false,
            false,
            false,
            &[s("tcp connect error: Connection refused (os error 10061)")],
        );
        assert!(refused.contains("refused"), "{refused}");
        let reset =
            classify_prompt_failure(true, false, false, false, &[s("connection reset by peer")]);
        assert!(reset.contains("reset"), "{reset}");
        let connect_timeout = classify_prompt_failure(
            false,
            true,
            false,
            false,
            &[s("error trying to connect: timed out")],
        );
        assert!(connect_timeout.contains("connecting"), "{connect_timeout}");
        let response_timeout =
            classify_prompt_failure(false, true, false, false, &[s("operation timed out")]);
        assert!(
            response_timeout.contains("timed out waiting"),
            "{response_timeout}"
        );
        let exited = classify_prompt_failure(
            false,
            false,
            false,
            false,
            &[s("connection closed before message completed")],
        );
        assert!(exited.contains("terminated the connection"), "{exited}");
        let garbage = classify_prompt_failure(
            false,
            false,
            false,
            true,
            &[s("expected value at line 1 column 1")],
        );
        assert!(garbage.contains("unparseable"), "{garbage}");
    }

    #[test]
    fn prompt_reports_connection_refused_diagnostic() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let client = OpenCodeClient::new(format!("http://{addr}"), "pw".to_string());
        let error = client.prompt("ses_1", "hi", None, None, None).unwrap_err();
        assert!(
            error.contains("connection refused")
                || error.contains("could not connect")
                || error.contains("timed out while connecting"),
            "got: {error}"
        );
        assert!(error.starts_with("prompt failed:"), "got: {error}");
    }

    #[test]
    fn prompt_diagnoses_unexpected_process_exit() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Read the request, start a response claiming 100 bytes, then vanish
            // without delivering them — like a crashed/killed server process.
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n");
            let _ = stream.flush();
        });
        let client = OpenCodeClient::new(format!("http://{addr}"), "pw".to_string());
        let error = client.prompt("ses_1", "hi", None, None, None).unwrap_err();
        let _ = server.join();
        assert!(error.starts_with("prompt failed:"), "got: {error}");
        assert!(
            error.contains("terminated the connection")
                || error.contains("unreadable")
                || error.contains("closed before message completed")
                || error.contains("reset")
                || error.contains("forcibly closed")
                || error.contains("eof"),
            "no termination diagnostic in: {error}"
        );
    }

    #[test]
    fn prompt_reports_http_error_status_with_redacted_body() {
        let server = MockServer::start(|request, _body| match request {
            "POST /session/ses_1/message" => (
                401,
                r#"{"error":"unauthorized","accessToken":"super-secret-token-123"}"#.to_string(),
            ),
            _ => (404, "{}".to_string()),
        });
        let client = OpenCodeClient::new(server.base_url(), "pw".to_string());
        let error = client.prompt("ses_1", "hi", None, None, None).unwrap_err();
        assert!(error.contains("401 Unauthorized"), "got: {error}");
        assert!(error.contains("unauthorized"), "got: {error}");
        assert!(
            !error.contains("super-secret-token-123"),
            "credential leaked: {error}"
        );
        assert!(error.contains("<redacted>"), "expected masking in: {error}");
    }

    #[test]
    fn http_error_body_read_is_bounded() {
        let large_body = "x".repeat(64 * 1024);
        let body = large_body.clone();
        let server = MockServer::start(move |request, _| match request {
            "POST /session/ses_1/message" => (500, body.clone()),
            _ => (404, "{}".to_string()),
        });
        let client = OpenCodeClient::new(server.base_url(), "pw".to_string());
        let error = client.prompt("ses_1", "hi", None, None, None).unwrap_err();
        assert!(
            error.len() < 3000,
            "error body was not bounded ({} bytes): {}",
            error.len(),
            &error[..error.len().min(200)]
        );
        assert!(error.contains("500 Internal Server Error"));
    }

    #[test]
    #[ignore = "slow: holds a connection 31s, past reqwest's 30s default; run with --ignored"]
    fn prompt_succeeds_when_server_takes_longer_than_default_timeout() {
        // Regression test for the root cause: reqwest's blocking builder
        // defaults to a 30s total request timeout, which killed planning and
        // implementation prompts at ~30s. With `.timeout(None)` the same
        // request completes at ~31s.
        let server = MockServer::start(|_request, _body| {
            std::thread::sleep(Duration::from_secs(31));
            (
                200,
                r#"{"parts":[{"type":"text","text":"finished"}]}"#.to_string(),
            )
        });
        let client = OpenCodeClient::new(server.base_url(), "pw".to_string());
        let value = client
            .prompt("ses_1", "do the long work", None, None, None)
            .expect("31s prompt must not be cut off by a 30s default");
        assert_eq!(extract_text(&value), "finished");
    }

    #[test]
    fn cannot_double_start_an_active_implementation() {
        // `start_implementation_inner` guards on this exact transition: a task
        // already Implementing (a run is in flight) can never transition back
        // to WorkspaceCreating, so no second agent run can be spawned for it.
        assert!(!crate::domain::TaskStatus::Implementing
            .can_transition_to(crate::domain::TaskStatus::WorkspaceCreating));
        assert!(crate::domain::TaskStatus::Approved
            .can_transition_to(crate::domain::TaskStatus::WorkspaceCreating));
    }
}
