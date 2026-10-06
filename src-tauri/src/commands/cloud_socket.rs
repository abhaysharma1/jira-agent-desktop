use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::cloud::socket::{CloudSocket, CloudSocketHandle};
use crate::cloud::CloudClient;
use crate::commands::notifications::{
    advance_cursor, device_token, fetch_new_notifications, setting_string, KEY_BASE_URL,
};
use crate::db;
use crate::domain::{AgentTask, CloudNotification};
use crate::state::AppState;

const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);
const SIGNED_OUT_RECHECK: Duration = Duration::from_secs(5);

fn emit_status(app: &AppHandle, status: &str) {
    let _ = app.emit("cloud://status", json!({ "status": status }));
}

fn sleep_interruptible(stop: &AtomicBool, duration: Duration) {
    let step = Duration::from_millis(250);
    let mut waited = Duration::ZERO;
    while waited < duration {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(step);
        waited += step;
    }
}

/// Emits `task://updated` and best-effort pushes the snapshot to the cloud so
/// the user's other devices see the change immediately.
pub fn emit_task_updated(app: &AppHandle, state: &AppState, task_id: &str) {
    let loaded = {
        let Ok(connection) = state.db.lock() else {
            return;
        };
        let Ok(Some(task)) = db::get_task(&connection, task_id) else {
            return;
        };
        let pr = db::get_pull_request(&connection, task_id).ok().flatten();
        (task, pr)
    };
    let (task, pr) = loaded;
    let _ = app.emit("task://updated", &task);
    push_task_snapshot(state, &task, pr.as_ref());
}

fn push_task_snapshot(state: &AppState, task: &AgentTask, pr: Option<&crate::domain::PullRequest>) {
    let Ok(Some(base_url)) = setting_string(state, KEY_BASE_URL) else {
        return;
    };
    let Ok(Some(device_token)) = device_token(state) else {
        return;
    };

    let status = serde_json::to_value(task.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let snapshot = json!({
        "jiraKey": task.jira_issue_key,
        "title": task.title,
        "status": status,
        "prNumber": pr.map(|pull| pull.number),
        "prUrl": pr.map(|pull| pull.url.clone()),
        "branch": task.branch_name.clone(),
        "updatedAt": task.updated_at,
    });

    std::thread::spawn(move || {
        if let Ok(client) = CloudClient::new(base_url) {
            if let Err(error) = client.push_tasks(&device_token, &json!([snapshot])) {
                crate::logging::warn(
                    "cloud-socket",
                    "task snapshot push failed",
                    json!({ "error": error }),
                );
            }
        }
    });
}

fn gap_fill(app: &AppHandle, state: &AppState) {
    match fetch_new_notifications(state) {
        Ok(notifications) => {
            for notification in notifications {
                let _ = app.emit("notification://received", notification);
            }
        }
        Err(error) => crate::logging::warn(
            "cloud-socket",
            "notification gap fill failed",
            json!({ "error": error }),
        ),
    }
}

fn handle_event(app: &AppHandle, state: &AppState, value: Value) {
    match value.get("event").and_then(Value::as_str).unwrap_or_default() {
        "connected" => {}
        "ticket.created" | "ticket.updated" => {
            if let Some(notification) = value.get("notification") {
                if let Some(created_at) = notification.get("createdAt").and_then(Value::as_str) {
                    let _ = advance_cursor(state, created_at);
                }
                if let Ok(parsed) =
                    serde_json::from_value::<CloudNotification>(notification.clone())
                {
                    let _ = app.emit("notification://received", parsed);
                }
            }
        }
        _ => {
            let _ = app.emit("cloud://event", value);
        }
    }
}

fn read_until_closed(
    app: &AppHandle,
    state: &AppState,
    socket: &mut CloudSocket,
    stop: &AtomicBool,
) -> Result<(), String> {
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        match socket.read_message()? {
            Some(value) => handle_event(app, state, value),
            None => return Ok(()),
        }
    }
}

fn run_socket(app: AppHandle, stop: Arc<AtomicBool>, stream_slot: Arc<Mutex<Option<TcpStream>>>) {
    let mut backoff = RECONNECT_MIN;

    while !stop.load(Ordering::SeqCst) {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };

        let credentials = match (
            setting_string(&state, KEY_BASE_URL),
            device_token(&state),
        ) {
            (Ok(Some(base_url)), Ok(Some(device_token))) => Some((base_url, device_token)),
            _ => None,
        };

        let Some((base_url, device_token)) = credentials else {
            emit_status(&app, "signedOut");
            sleep_interruptible(&stop, SIGNED_OUT_RECHECK);
            continue;
        };

        emit_status(&app, "connecting");
        match CloudSocket::connect(&base_url, &device_token) {
            Ok((mut socket, stream)) => {
                if let Ok(mut slot) = stream_slot.lock() {
                    *slot = Some(stream);
                }
                emit_status(&app, "online");
                backoff = RECONNECT_MIN;
                gap_fill(&app, &state);

                let outcome = read_until_closed(&app, &state, &mut socket, &stop);

                if let Ok(mut slot) = stream_slot.lock() {
                    *slot = None;
                }
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                if let Err(error) = outcome {
                    crate::logging::warn(
                        "cloud-socket",
                        "socket closed",
                        json!({ "error": error }),
                    );
                }
                emit_status(&app, "offline");
                sleep_interruptible(&stop, backoff);
                backoff = (backoff * 2).min(RECONNECT_MAX);
            }
            Err(error) => {
                crate::logging::warn(
                    "cloud-socket",
                    "connect failed",
                    json!({ "error": error }),
                );
                emit_status(&app, "offline");
                sleep_interruptible(&stop, backoff);
                backoff = (backoff * 2).min(RECONNECT_MAX);
            }
        }
    }
}

#[tauri::command]
pub fn start_cloud_sync(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    {
        let guard = state
            .cloud_socket
            .lock()
            .map_err(|error| error.to_string())?;
        if guard.is_some() {
            return Ok(());
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stream_slot: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
    {
        let mut guard = state
            .cloud_socket
            .lock()
            .map_err(|error| error.to_string())?;
        *guard = Some(CloudSocketHandle {
            stop: stop.clone(),
            stream: stream_slot.clone(),
        });
    }

    std::thread::spawn(move || run_socket(app, stop, stream_slot));
    Ok(())
}

#[tauri::command]
pub fn stop_cloud_sync(state: State<AppState>) -> Result<(), String> {
    let mut guard = state
        .cloud_socket
        .lock()
        .map_err(|error| error.to_string())?;
    if let Some(handle) = guard.take() {
        handle.stop.store(true, Ordering::SeqCst);
        if let Ok(stream) = handle.stream.lock() {
            if let Some(stream) = stream.as_ref() {
                let _ = stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }
    Ok(())
}
