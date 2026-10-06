use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tungstenite::client::{client, IntoClientRequest};
use tungstenite::http::header::AUTHORIZATION;
use tungstenite::{Message, WebSocket};
use url::Url;

/// How long a read may block before the socket is treated as dead. The server
/// pings every 30s, so a 90s gap means the connection is gone.
const READ_TIMEOUT: Duration = Duration::from_secs(90);

/// Stop flag plus the live TCP stream, kept in `AppState` so the desktop can
/// tear the socket down (and unblock its reader) on logout.
pub struct CloudSocketHandle {
    pub stop: Arc<AtomicBool>,
    pub stream: Arc<Mutex<Option<TcpStream>>>,
}

/// Converts an HTTP(S) cloud base URL into the WebSocket endpoint.
pub fn websocket_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    let with_scheme = if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
        trimmed.to_string()
    } else if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if trimmed.is_empty() {
        String::new()
    } else {
        format!("wss://{trimmed}")
    };
    if with_scheme.is_empty() {
        with_scheme
    } else {
        format!("{with_scheme}/ws")
    }
}

pub struct CloudSocket {
    socket: WebSocket<TcpStream>,
}

impl CloudSocket {
    /// Opens the socket and authenticates with the device token as a normal
    /// `Authorization` header during the handshake.
    pub fn connect(
        base_url: &str,
        device_token: &str,
    ) -> Result<(Self, TcpStream), String> {
        let ws_url = websocket_url(base_url);
        if ws_url.is_empty() {
            return Err("cloud base URL is required".to_string());
        }
        let mut request = ws_url
            .as_str()
            .into_client_request()
            .map_err(|error| error.to_string())?;
        let header = format!("Bearer {device_token}")
            .parse()
            .map_err(|error: tungstenite::http::header::InvalidHeaderValue| error.to_string())?;
        request.headers_mut().insert(AUTHORIZATION, header);

        let parsed = Url::parse(&ws_url).map_err(|error| error.to_string())?;
        let host = parsed
            .host_str()
            .ok_or_else(|| "cloud URL has no host".to_string())?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| "cloud URL has no port".to_string())?;

        let stream = TcpStream::connect((host, port))
            .map_err(|error| format!("cloud socket connect failed: {error}"))?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|error| error.to_string())?;
        let _ = stream.set_nodelay(true);
        let shutdown = stream.try_clone().map_err(|error| error.to_string())?;

        let (socket, _response) =
            client(request, stream).map_err(|error| format!("cloud socket handshake failed: {error}"))?;
        Ok((Self { socket }, shutdown))
    }

    /// Reads the next JSON frame, transparently answering pings. Returns
    /// `Ok(None)` when the server closes the connection cleanly.
    pub fn read_message(&mut self) -> Result<Option<Value>, String> {
        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    let value: Value =
                        serde_json::from_str(&text.to_string()).map_err(|error| error.to_string())?;
                    return Ok(Some(value));
                }
                Ok(Message::Ping(payload)) => {
                    let _ = self.socket.send(Message::Pong(payload));
                }
                Ok(Message::Close(_)) => return Ok(None),
                Ok(_) => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_websocket_urls() {
        assert_eq!(
            websocket_url("http://localhost:4000"),
            "ws://localhost:4000/ws"
        );
        assert_eq!(
            websocket_url("https://api.example.com/"),
            "wss://api.example.com/ws"
        );
        assert_eq!(
            websocket_url("api.example.com"),
            "wss://api.example.com/ws"
        );
        assert_eq!(
            websocket_url("ws://localhost:4000"),
            "ws://localhost:4000/ws"
        );
        assert_eq!(websocket_url(""), "");
    }

    #[test]
    fn connects_and_reads_events() {
        use std::net::TcpListener;
        use tungstenite::accept;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = accept(stream).unwrap();
            socket
                .send(Message::Text(
                    r#"{"event":"task.updated","jiraKey":"CC-1","status":"PLAN_READY"}"#.into(),
                ))
                .unwrap();
            // Keep the connection open until the client goes away.
            while socket.read().is_ok() {}
        });

        let (mut socket, shutdown) =
            CloudSocket::connect(&format!("http://127.0.0.1:{port}"), "device-token").unwrap();
        let value = socket.read_message().unwrap().unwrap();
        assert_eq!(value["event"], "task.updated");
        assert_eq!(value["status"], "PLAN_READY");

        drop(socket);
        drop(shutdown);
        let _ = server.join();
    }
}
