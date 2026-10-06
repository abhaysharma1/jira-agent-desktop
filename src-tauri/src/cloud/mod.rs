use serde_json::{json, Value};

pub mod socket;

pub fn normalize_base_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else if trimmed.is_empty() {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

fn error_message(value: &Value, status: reqwest::StatusCode, context: &str) -> String {
    value
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{context} failed: {status}"))
}

pub struct CloudClient {
    base_url: String,
    http: reqwest::blocking::Client,
}

impl CloudClient {
    pub fn new(base_url: String) -> Result<Self, String> {
        let base_url = normalize_base_url(&base_url);
        if base_url.is_empty() {
            return Err("cloud base URL is required".to_string());
        }
        let http = reqwest::blocking::Client::builder()
            .user_agent("jira-agent-desktop")
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { base_url, http })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn authenticate(
        &self,
        path: &str,
        email: &str,
        password: &str,
    ) -> Result<(String, String), String> {
        let response = self
            .http
            .post(format!("{}{}", self.base_url, path))
            .json(&json!({ "email": email, "password": password }))
            .send()
            .map_err(|error| format!("cloud request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(error_message(&value, status, "cloud auth"));
        }
        let token = value
            .get("token")
            .and_then(Value::as_str)
            .ok_or_else(|| "cloud auth response missing token".to_string())?
            .to_string();
        let email = value
            .get("user")
            .and_then(|user| user.get("email"))
            .and_then(Value::as_str)
            .unwrap_or(email)
            .to_string();
        Ok((token, email))
    }

    pub fn register(&self, email: &str, password: &str) -> Result<(String, String), String> {
        self.authenticate("/auth/register", email, password)
    }

    pub fn login(&self, email: &str, password: &str) -> Result<(String, String), String> {
        self.authenticate("/auth/login", email, password)
    }

    pub fn register_device(
        &self,
        token: &str,
        device_id: &str,
        name: &str,
        platform: &str,
    ) -> Result<String, String> {
        let response = self
            .http
            .post(format!("{}/devices", self.base_url))
            .bearer_auth(token)
            .json(&json!({ "deviceId": device_id, "name": name, "platform": platform }))
            .send()
            .map_err(|error| format!("cloud request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(error_message(&value, status, "device registration"));
        }
        value
            .get("deviceToken")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "device registration response missing deviceToken".to_string())
    }

    pub fn jira_connections(&self, token: &str) -> Result<Vec<Value>, String> {
        let response = self
            .http
            .get(format!("{}/jira/connections", self.base_url))
            .bearer_auth(token)
            .header("Accept", "application/json")
            .send()
            .map_err(|error| format!("cloud request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(error_message(&value, status, "list JIRA connections"));
        }
        Ok(value
            .get("connections")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub fn create_jira_connection(
        &self,
        token: &str,
        site_url: &str,
        email: &str,
    ) -> Result<Value, String> {
        let response = self
            .http
            .post(format!("{}/jira/connections", self.base_url))
            .bearer_auth(token)
            .json(&json!({ "siteUrl": site_url, "email": email }))
            .send()
            .map_err(|error| format!("cloud request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(error_message(&value, status, "create JIRA connection"));
        }
        value
            .get("connection")
            .cloned()
            .ok_or_else(|| "create JIRA connection response missing connection".to_string())
    }

    pub fn list_notifications(
        &self,
        device_token: &str,
        after: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        let mut request = self
            .http
            .get(format!("{}/notifications", self.base_url))
            .bearer_auth(device_token)
            .header("Accept", "application/json");
        if let Some(after) = after.filter(|value| !value.trim().is_empty()) {
            request = request.query(&[("after", after)]);
        }
        let response = request
            .send()
            .map_err(|error| format!("cloud request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(error_message(&value, status, "list notifications"));
        }
        Ok(value
            .get("notifications")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    pub fn mark_notification_read(&self, device_token: &str, id: &str) -> Result<Value, String> {
        let response = self
            .http
            .post(format!("{}/notifications/{id}/read", self.base_url))
            .bearer_auth(device_token)
            .header("Accept", "application/json")
            .send()
            .map_err(|error| format!("cloud request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(error_message(&value, status, "mark notification read"));
        }
        Ok(value.get("notification").cloned().unwrap_or(Value::Null))
    }

    /// Pushes task snapshots to `POST /tasks/sync`. The server broadcasts the
    /// resulting `task.updated` / `pr.created` / `plan.ready` events to the
    /// user's other devices.
    pub fn push_tasks(&self, device_token: &str, tasks: &Value) -> Result<Value, String> {
        let response = self
            .http
            .post(format!("{}/tasks/sync", self.base_url))
            .bearer_auth(device_token)
            .header("Accept", "application/json")
            .json(&json!({ "tasks": tasks }))
            .send()
            .map_err(|error| format!("cloud request failed: {error}"))?;
        let status = response.status();
        let value: Value = response.json().unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(error_message(&value, status, "task sync"));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockServer;

    #[test]
    fn normalizes_base_urls() {
        assert_eq!(normalize_base_url("api.example.com"), "https://api.example.com");
        assert_eq!(
            normalize_base_url("https://api.example.com/"),
            "https://api.example.com"
        );
        assert_eq!(
            normalize_base_url("  http://localhost:4000  "),
            "http://localhost:4000"
        );
    }

    #[test]
    fn lists_and_reads_notifications() {
        let server = MockServer::start(|request, _body| match request {
            "GET /notifications?after=2026-01-01T00%3A00%3A00Z" => (
                200,
                r#"{"notifications":[{"id":"n1","type":"jira:issue_created","title":"CC-1: Add pagination","createdAt":"2026-01-02T00:00:00Z"}]}"#
                    .to_string(),
            ),
            "GET /notifications" => (
                200,
                r#"{"notifications":[{"id":"n1","type":"jira:issue_created","title":"CC-1: Add pagination","createdAt":"2026-01-02T00:00:00Z"}]}"#
                    .to_string(),
            ),
            "POST /notifications/n1/read" => (
                200,
                r#"{"notification":{"id":"n1","readAt":"2026-01-03T00:00:00Z"}}"#.to_string(),
            ),
            _ => (404, r#"{"error":"unexpected"}"#.to_string()),
        });

        let client = CloudClient::new(server.base_url()).unwrap();
        let items = client.list_notifications("device-token", None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["id"], "n1");

        let items = client
            .list_notifications("device-token", Some("2026-01-01T00:00:00Z"))
            .unwrap();
        assert_eq!(items.len(), 1);

        let updated = client
            .mark_notification_read("device-token", "n1")
            .unwrap();
        assert_eq!(updated["readAt"], "2026-01-03T00:00:00Z");
    }

    #[test]
    fn pushes_task_snapshots() {
        let server = MockServer::start(|request, _body| match request {
            "POST /tasks/sync" => (200, r#"{"applied":1,"tasks":[]}"#.to_string()),
            _ => (404, r#"{"error":"unexpected"}"#.to_string()),
        });

        let client = CloudClient::new(server.base_url()).unwrap();
        let value = client
            .push_tasks(
                "device-token",
                &json!([{ "jiraKey": "CC-1", "status": "PLAN_READY" }]),
            )
            .unwrap();
        assert_eq!(value["applied"], 1);
    }
}
