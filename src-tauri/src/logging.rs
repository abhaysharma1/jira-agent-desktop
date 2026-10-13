use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// One JSON object per line, written under `~/.ticketbridge/logs/`. This is the
/// local half of Phase 26: structured logs you can grep, plus a durable record
/// of the metric events emitted by `crate::metrics`. It is deliberately
/// dependency-free and best-effort — a logging failure must never break a task.
struct Logger {
    dir: PathBuf,
    date: String,
    file: File,
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

/// Initialises the file logger. Safe to call once; later calls are ignored.
/// Failures are swallowed (the app still runs, logs just go to stderr).
pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let date = today();
    let Some(file) = open_file(dir, &date) else {
        return;
    };
    let _ = LOGGER.set(Mutex::new(Logger {
        dir: dir.to_path_buf(),
        date,
        file,
    }));
}

pub fn info(target: &str, message: &str, fields: Value) {
    write("info", target, message, fields);
}

pub fn warn(target: &str, message: &str, fields: Value) {
    write("warn", target, message, fields);
}

pub fn error(target: &str, message: &str, fields: Value) {
    write("error", target, message, fields);
}

fn write(level: &str, target: &str, message: &str, fields: Value) {
    let line = format_line(
        level,
        target,
        message,
        fields,
        &chrono::Utc::now().to_rfc3339(),
    );
    if level == "error" {
        eprintln!("{line}");
    }
    match LOGGER.get() {
        Some(lock) => match lock.lock() {
            Ok(mut logger) => logger.write_line(&line),
            Err(_) => eprintln!("{line}"),
        },
        None => eprintln!("{line}"),
    }
}

impl Logger {
    /// Appends a line, rolling over to a new daily file when the date changes.
    fn write_line(&mut self, line: &str) {
        let today = today();
        if today != self.date {
            if let Some(file) = open_file(&self.dir, &today) {
                self.file = file;
                self.date = today;
            }
        }
        if writeln!(self.file, "{line}").is_err() {
            eprintln!("{line}");
        }
    }
}

fn open_file(dir: &Path, date: &str) -> Option<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path(dir, date))
        .ok()
}

/// Public so the Settings "Diagnostics" card can name the active file.
pub fn log_file_name(date: &str) -> String {
    format!("app-{date}.log")
}

pub fn log_file_path(dir: &Path, date: &str) -> PathBuf {
    dir.join(log_file_name(date))
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// Masks credential-looking text inside a snippet intended for logs or error
/// fields: bearer tokens and the values of sensitive JSON keys
/// (`token`, `secret`, `password`, `authorization`, `apiKey`, `jwt`, …).
/// Response bodies from our own and third-party servers are not trusted input,
/// so treat anything that looks like a credential as one.
pub fn redact_secrets(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let is_sensitive_key = |key: &str| {
        let lower = key.to_ascii_lowercase();
        [
            "token",
            "secret",
            "password",
            "authorization",
            "apikey",
            "api_key",
            "api-key",
            "credential",
            "jwt",
            "bearer",
        ]
        .iter()
        .any(|hint| lower.contains(hint))
    };

    while i < chars.len() {
        // `bearer <token>` in any case, anywhere in the text.
        if chars[i] == 'b' || chars[i] == 'B' {
            let word: String = chars[i..].iter().take(7).collect();
            if word.eq_ignore_ascii_case("bearer ") {
                out.push_str("bearer ");
                i += 7;
                while i < chars.len() && is_token_char(chars[i]) {
                    i += 1;
                }
                out.push_str("<redacted>");
                continue;
            }
        }
        // `"key": "value"` where the key looks sensitive.
        if chars[i] == '"' {
            let key_start = i;
            let mut end_key = i + 1;
            while end_key < chars.len() && chars[end_key] != '"' {
                end_key += 1;
            }
            if end_key < chars.len() {
                let key: String = chars[key_start + 1..end_key].iter().collect();
                let mut colon = end_key + 1;
                while colon < chars.len() && chars[colon].is_whitespace() {
                    colon += 1;
                }
                if colon < chars.len() && chars[colon] == ':' {
                    let mut value_open = colon + 1;
                    while value_open < chars.len() && chars[value_open].is_whitespace() {
                        value_open += 1;
                    }
                    if value_open < chars.len() && chars[value_open] == '"' {
                        let mut value_end = value_open + 1;
                        while value_end < chars.len() {
                            if chars[value_end] == '\\' {
                                value_end += 2;
                                continue;
                            }
                            if chars[value_end] == '"' {
                                break;
                            }
                            value_end += 1;
                        }
                        if is_sensitive_key(&key) {
                            for ch in &chars[key_start..=end_key] {
                                out.push(*ch);
                            }
                            for ch in &chars[end_key + 1..value_open] {
                                out.push(*ch);
                            }
                            out.push_str("\"<redacted>\"");
                            i = if value_end < chars.len() {
                                value_end + 1
                            } else {
                                chars.len()
                            };
                            continue;
                        }
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "._~+/=-".contains(c)
}

/// Builds a single JSON log line. `fields` is merged into the top level so
/// events stay queryable, but the envelope keys always win.
fn format_line(level: &str, target: &str, message: &str, fields: Value, timestamp: &str) -> String {
    let mut object = serde_json::Map::new();
    object.insert("ts".to_string(), Value::String(timestamp.to_string()));
    object.insert("level".to_string(), Value::String(level.to_string()));
    object.insert("target".to_string(), Value::String(target.to_string()));
    object.insert("msg".to_string(), Value::String(message.to_string()));
    if let Value::Object(map) = fields {
        for (key, value) in map {
            if !matches!(key.as_str(), "ts" | "level" | "target" | "msg") {
                object.insert(key, value);
            }
        }
    }
    serde_json::to_string(&Value::Object(object)).unwrap_or_else(|_| {
        r#"{"level":"error","target":"logging","msg":"failed to serialize log line"}"#.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_json_with_merged_fields() {
        let line = format_line(
            "info",
            "metrics",
            "planning.duration_ms",
            json!({ "metric": "planning.duration_ms", "value": 1234.0, "taskId": "t1" }),
            "2026-10-07T00:00:00Z",
        );
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["level"], "info");
        assert_eq!(parsed["target"], "metrics");
        assert_eq!(parsed["msg"], "planning.duration_ms");
        assert_eq!(parsed["ts"], "2026-10-07T00:00:00Z");
        assert_eq!(parsed["value"], 1234.0);
        assert_eq!(parsed["taskId"], "t1");
    }

    #[test]
    fn fields_cannot_clobber_the_envelope() {
        let line = format_line("warn", "app", "hello", json!({ "level": "fake" }), "t");
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["level"], "warn");
    }

    #[test]
    fn names_files_by_date() {
        assert_eq!(log_file_name("2026-10-07"), "app-2026-10-07.log");
        assert_eq!(
            log_file_path(Path::new("C:/logs"), "2026-10-07"),
            Path::new("C:/logs").join("app-2026-10-07.log")
        );
    }

    #[test]
    fn rotates_to_a_new_file_when_the_date_changes() {
        let dir = tempfile::tempdir().unwrap();
        let stale = open_file(dir.path(), "2000-01-01").unwrap();
        let mut logger = Logger {
            dir: dir.path().to_path_buf(),
            date: "2000-01-01".to_string(),
            file: stale,
        };
        logger.write_line("first");
        // The write rolled over to today's file.
        let today = today();
        let contents = std::fs::read_to_string(log_file_path(dir.path(), &today)).unwrap();
        assert_eq!(contents, "first\n");
    }

    #[test]
    fn redacts_bearer_tokens_and_sensitive_json_values() {
        let out = redact_secrets(
            r#"{"error":"unauthorized","accessToken":"tok-123","jwt":"abc.def.ghi","note":"Authorization: Bearer kv-yyyy"}"#,
        );
        assert!(!out.contains("tok-123"), "token value leaked: {out}");
        assert!(!out.contains("abc.def.ghi"), "jwt value leaked: {out}");
        assert!(!out.contains("kv-yyyy"), "bearer token leaked: {out}");
        assert_eq!(out.matches("<redacted>").count(), 3, "output: {out}");
        assert!(out.contains("\"accessToken\":\"<redacted>\""));
    }

    #[test]
    fn redact_leaves_ordinary_text_alone() {
        let out = redact_secrets(r#"{"error":"plan review needed","message":"todo list updated"}"#);
        assert!(!out.contains("<redacted>"));
        assert!(out.contains("todo list updated"));
    }
}
