use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

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

/// Exponential reconnect backoff, capped at `RECONNECT_MAX`. Reset back to
/// `RECONNECT_MIN` after every successful connection.
fn grow_backoff(backoff: Duration) -> Duration {
    (backoff * 2).min(RECONNECT_MAX)
}

fn emit_status<R: Runtime>(app: &AppHandle<R>, status: &str) {
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

fn gap_fill<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
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

fn handle_event<R: Runtime>(app: &AppHandle<R>, state: &AppState, value: Value) {
    match value
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
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

fn read_until_closed<R: Runtime>(
    app: &AppHandle<R>,
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

fn run_socket<R: Runtime>(
    app: AppHandle<R>,
    stop: Arc<AtomicBool>,
    stream_slot: Arc<Mutex<Option<TcpStream>>>,
) {
    let mut backoff = RECONNECT_MIN;

    while !stop.load(Ordering::SeqCst) {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };

        let credentials = match (setting_string(&state, KEY_BASE_URL), device_token(&state)) {
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
                backoff = grow_backoff(backoff);
            }
            Err(error) => {
                crate::logging::warn("cloud-socket", "connect failed", json!({ "error": error }));
                emit_status(&app, "offline");
                sleep_interruptible(&stop, backoff);
                backoff = grow_backoff(backoff);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentRun, AgentRunStatus};
    use crate::state::AppState;
    use tauri::Manager;

    #[test]
    fn reconnect_backoff_grows_then_caps() {
        let mut backoff = RECONNECT_MIN;
        assert_eq!(backoff, Duration::from_secs(1));
        for expected in [2u64, 4, 8, 16, 30] {
            backoff = grow_backoff(backoff);
            assert_eq!(backoff, Duration::from_secs(expected));
        }
        assert_eq!(grow_backoff(RECONNECT_MAX), RECONNECT_MAX);
        // A successful connection resets the backoff (see run_socket).
        backoff = RECONNECT_MIN;
        assert_eq!(backoff, Duration::from_secs(1));
    }

    #[test]
    fn cloud_handshake_400_only_warns_and_retries_in_the_socket_thread() {
        use std::io::Read;
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

        // A fake backend that answers every WebSocket upgrade with 400 — the
        // exact status the deployed backend currently returns. The listener is
        // shared so the test can stop the accept loop by setting `stop` and
        // making one throwaway connection (a blocked `accept` needs a wake-up).
        let listener = Arc::new(TcpListener::bind("127.0.0.1:0").unwrap());
        let port = listener.local_addr().unwrap().port();
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_server = attempts.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_server = stop.clone();
        let listener_server = listener.clone();
        let server = std::thread::spawn(move || {
            loop {
                let (mut stream, _) = match listener_server.accept() {
                    Ok(accepted) => accepted,
                    Err(_) => break, // listener closed: test finished
                };
                attempts_server.fetch_add(1, AtomicOrdering::SeqCst);
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = std::io::Write::write_all(
                    &mut stream,
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 11\r\n\r\nbad request",
                );
                if stop_server.load(AtomicOrdering::SeqCst) {
                    break;
                }
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let state = AppState::load_with_key_source(
            dir.path().to_path_buf(),
            crate::secure_store::KeySource::File,
        )
        .unwrap();
        {
            let connection = state.db.lock().unwrap();
            db::set_setting(
                &connection,
                KEY_BASE_URL,
                &Value::String(format!("ws://127.0.0.1:{port}")),
            )
            .unwrap();
            db::set_setting(
                &connection,
                "cloudDeviceToken",
                &Value::String("device-token".to_string()),
            )
            .unwrap();
        }

        // A local implementation run that is in-flight while the cloud socket
        // fails. The 400 must never touch it.
        let local_run = AgentRun {
            id: "local-run-1".to_string(),
            task_id: "task-1".to_string(),
            mode: "implementation".to_string(),
            agent: Some("build".to_string()),
            status: AgentRunStatus::Running,
            session_id: None,
            started_at: "2026-10-10T00:00:00Z".to_string(),
            completed_at: None,
        };
        crate::commands::runtime::register_run(
            &state,
            local_run,
            String::new(),
            0.0,
            None,
            crate::test_support::dummy_managed_server(),
        )
        .unwrap();

        let app = tauri::test::mock_app();
        app.manage(state);
        let stream_slot: Arc<Mutex<Option<TcpStream>>> = Arc::new(Mutex::new(None));
        let handle = app.handle().clone();
        let socket_stop = stop.clone();
        let socket_thread =
            std::thread::spawn(move || run_socket(handle, socket_stop, stream_slot));

        // First attempt is immediate; backoff is 1s then 2s. Two attempts by
        // ~2.6s proves the loop keeps retrying a rejected handshake.
        std::thread::sleep(Duration::from_millis(2600));
        let seen = attempts.load(AtomicOrdering::SeqCst);
        assert!(
            seen >= 2,
            "expected retries after HTTP 400, saw {seen} attempts"
        );

        // The local agent run stayed Running and untouched.
        let app_state = app.try_state::<AppState>().unwrap();
        let agents = app_state.agents.lock().unwrap();
        assert_eq!(agents["local-run-1"].run.status, AgentRunStatus::Running);
        assert!(agents["local-run-1"].run.completed_at.is_none());

        stop.store(true, AtomicOrdering::SeqCst);
        // Wake the blocked `accept` so the server thread can observe `stop` and
        // exit; the connection itself is immediately served and closed.
        let _ = std::net::TcpStream::connect(("127.0.0.1", port));
        let _ = socket_thread.join();
        let _ = server.join();
    }
}
