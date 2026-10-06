//! A tiny in-process HTTP server used by tests to stand in for Atlassian and
//! the cloud backend. Not compiled outside `#[cfg(test)]`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct MockServer {
    addr: String,
    shutdown: Arc<AtomicBool>,
}

impl MockServer {
    /// `handler` receives `"<METHOD> <path>"` and the request body, and returns
    /// `(status, body)`.
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(&str, &str) -> (u16, String) + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("mock addr").to_string();
        let shutdown = Arc::new(AtomicBool::new(false));
        let flag = shutdown.clone();
        let handler = Arc::new(handler);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                let mut stream = match stream {
                    Ok(stream) => stream,
                    Err(_) => continue,
                };
                let handler = handler.clone();
                std::thread::spawn(move || {
                    let request = read_request(&mut stream);
                    let first = request.lines().next().unwrap_or_default().to_string();
                    let mut parts = first.split_whitespace();
                    let method = parts.next().unwrap_or_default();
                    let path = parts.next().unwrap_or_default();
                    let body = request
                        .split("\r\n\r\n")
                        .nth(1)
                        .unwrap_or_default()
                        .to_string();
                    let (status, response) = handler(&format!("{method} {path}"), &body);
                    let reason = if status < 400 { "OK" } else { "ERROR" };
                    let reply = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                        response.len()
                    );
                    let _ = stream.write_all(reply.as_bytes());
                    let _ = stream.flush();
                });
            }
        });
        Self { addr, shutdown }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr);
    }
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                data.extend_from_slice(&buffer[..read]);
                if request_complete(&data) {
                    break;
                }
            }
            Err(_) => break,
        }
        if data.len() > 1_000_000 {
            break;
        }
    }
    String::from_utf8_lossy(&data).to_string()
}

fn request_complete(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(data);
    let Some(index) = text.find("\r\n\r\n") else {
        return false;
    };
    let content_length = text[..index]
        .lines()
        .find_map(|line| {
            let lower = line.to_ascii_lowercase();
            lower
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    data.len() >= index + 4 + content_length
}
