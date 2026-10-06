use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::db;
use crate::opencode::OpenCodeClient;
use crate::state::AppState;

/// Best-effort cleanup of `opencode serve` processes left behind by a previous
/// app instance that exited mid-run. Each recorded server is health-checked
/// first: a response authenticated with the recorded password proves the
/// process is ours, so terminating it is safe. Unresponsive records are simply
/// dropped so pid reuse can never cause us to kill an unrelated process.
pub fn sweep(state: &AppState) -> Result<usize, String> {
    let servers = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::list_opencode_servers(&connection)?
    };

    let mut killed = 0;
    for server in servers {
        let password = match crate::secrets::decrypt_value(&state.secure_store, &server.password) {
            Ok(password) => password,
            Err(error) => {
                crate::logging::warn(
                    "crash-recovery",
                    "could not decrypt a recorded server password",
                    serde_json::json!({ "error": error }),
                );
                if let Ok(connection) = state.db.lock() {
                    let _ = db::delete_opencode_server(&connection, &server.run_id);
                }
                continue;
            }
        };

        let alive = port_open(server.port)
            && OpenCodeClient::new(format!("http://127.0.0.1:{}", server.port), password)
                .health()
                .is_ok();

        if alive && is_opencode_process(server.pid) {
            kill_process(server.pid);
            killed += 1;
        }

        if let Ok(connection) = state.db.lock() {
            let _ = db::delete_opencode_server(&connection, &server.run_id);
        }
    }

    Ok(killed)
}

fn port_open(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok()
}

/// Guards against pid reuse: only kill when the pid still looks like an
/// opencode/node process.
#[cfg(windows)]
fn is_opencode_process(pid: u32) -> bool {
    let text = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).to_lowercase())
        .unwrap_or_default();
    text.contains("opencode") || text.contains("node")
}

#[cfg(not(windows))]
fn is_opencode_process(pid: u32) -> bool {
    let text = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).to_lowercase())
        .unwrap_or_default();
    text.contains("opencode") || text.contains("node")
}

#[cfg(windows)]
fn kill_process(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

#[cfg(not(windows))]
fn kill_process(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .output();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_open_ports() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_open(port));
        drop(listener);
        assert!(!port_open(port));
    }

    #[test]
    fn sweep_drops_dead_records() {
        let temp = tempfile::tempdir().unwrap();
        let state = crate::state::AppState::load_with_key_source(
            temp.path().to_path_buf(),
            crate::secure_store::KeySource::File,
        )
        .unwrap();
        {
            let connection = state.db.lock().unwrap();
            crate::db::record_opencode_server(
                &connection,
                &crate::domain::OpencodeServer {
                    run_id: "run-1".to_string(),
                    task_id: "task-1".to_string(),
                    pid: 4_000_000,
                    port: 1,
                    password: "secret".to_string(),
                    started_at: "t0".to_string(),
                },
            )
            .unwrap();
        }

        let killed = sweep(&state).unwrap();
        assert_eq!(killed, 0);
        let connection = state.db.lock().unwrap();
        assert!(crate::db::list_opencode_servers(&connection).unwrap().is_empty());
    }
}
