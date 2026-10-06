use serde_json::{json, Value};

use crate::domain::{JiraIssue, JiraTransition};

pub const DEFAULT_API_BASE: &str = "https://api.atlassian.com";

pub const WEBHOOK_EVENTS: &[&str] = &[
    "jira:issue_created",
    "jira:issue_updated",
    "jira:issue_deleted",
    "comment_created",
    "comment_updated",
];

pub fn cloud_api_base(cloud_id: &str) -> String {
    format!(
        "{}/ex/jira/{}",
        DEFAULT_API_BASE.trim_end_matches('/'),
        cloud_id
    )
}

/// Flattens an Atlassian Document Format node into plain text suitable for an
/// implementation prompt. Non-ADF values (plain strings, null) pass through.
pub fn adf_to_text(node: &Value) -> String {
    match node {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(adf_to_text).collect(),
        Value::Object(map) => {
            let node_type = map.get("type").and_then(Value::as_str).unwrap_or_default();
            match node_type {
                "text" => map
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                "hardBreak" => "\n".to_string(),
                "listItem" => {
                    let inner = map.get("content").map(adf_to_text).unwrap_or_default();
                    format!("- {}\n", inner.trim())
                }
                "paragraph" | "heading" | "codeBlock" | "blockquote" => {
                    let inner = map.get("content").map(adf_to_text).unwrap_or_default();
                    format!("{inner}\n")
                }
                _ => map.get("content").map(adf_to_text).unwrap_or_default(),
            }
        }
        _ => String::new(),
    }
}

pub struct JiraClient {
    api_base: String,
    token: String,
    http: reqwest::blocking::Client,
    site_url: Option<String>,
}

impl JiraClient {
    pub fn new(api_base: String, token: String) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .user_agent("jira-agent-desktop")
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            api_base: api_base.trim_end_matches('/').to_string(),
            token,
            http,
            site_url: None,
        })
    }

    pub fn with_site_url(mut self, site_url: impl Into<String>) -> Self {
        self.site_url = Some(site_url.into());
        self
    }

    pub fn for_cloud(cloud_id: &str, token: String) -> Result<Self, String> {
        Self::new(cloud_api_base(cloud_id), token)
    }

    fn get(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        self.http
            .get(format!("{}{}", self.api_base, path))
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
    }

    fn post(&self, path: &str) -> reqwest::blocking::RequestBuilder {
        self.http
            .post(format!("{}{}", self.api_base, path))
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
    }

    pub fn account(&self) -> Result<String, String> {
        let response = self
            .get("/rest/api/3/myself")
            .send()
            .map_err(|error| format!("jira request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("jira auth failed: {}", response.status()));
        }
        let value: Value = response.json().map_err(|error| error.to_string())?;
        value
            .get("displayName")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "jira user response missing displayName".to_string())
    }

    pub fn get_issue(&self, key: &str) -> Result<JiraIssue, String> {
        let response = self
            .get(&format!("/rest/api/3/issue/{key}"))
            .send()
            .map_err(|error| format!("jira request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "jira issue {key} lookup failed: {}",
                response.status()
            ));
        }
        let value: Value = response.json().map_err(|error| error.to_string())?;
        let fields = value.get("fields").cloned().unwrap_or(Value::Null);
        Ok(JiraIssue {
            key: key.to_string(),
            id: value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            summary: fields
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            description: fields
                .get("description")
                .map(adf_to_text)
                .unwrap_or_default(),
            status: fields
                .get("status")
                .and_then(|status| status.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            issue_type: fields
                .get("issuetype")
                .and_then(|issue_type| issue_type.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            url: self
                .site_url
                .as_ref()
                .map(|base| format!("{}/browse/{key}", base.trim_end_matches('/')))
                .unwrap_or_default(),
        })
    }

    pub fn transitions(&self, key: &str) -> Result<Vec<JiraTransition>, String> {
        let response = self
            .get(&format!("/rest/api/3/issue/{key}/transitions"))
            .send()
            .map_err(|error| format!("jira request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "jira transitions for {key} failed: {}",
                response.status()
            ));
        }
        let value: Value = response.json().map_err(|error| error.to_string())?;
        Ok(value
            .get("transitions")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        Some(JiraTransition {
                            id: item.get("id")?.as_str()?.to_string(),
                            name: item.get("name")?.as_str()?.to_string(),
                            to_status: item
                                .get("to")
                                .and_then(|to| to.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn transition(&self, key: &str, transition_id: &str) -> Result<(), String> {
        let response = self
            .post(&format!("/rest/api/3/issue/{key}/transitions"))
            .json(&json!({ "transition": { "id": transition_id } }))
            .send()
            .map_err(|error| format!("jira request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "jira transition failed: {} {}",
                response.status(),
                response.text().unwrap_or_default()
            ));
        }
        Ok(())
    }

    pub fn add_comment(&self, key: &str, body: &str) -> Result<(), String> {
        let payload = json!({
            "body": {
                "type": "doc",
                "version": 1,
                "content": [
                    { "type": "paragraph", "content": [{ "type": "text", "text": body }] }
                ]
            }
        });
        let response = self
            .post(&format!("/rest/api/3/issue/{key}/comment"))
            .json(&payload)
            .send()
            .map_err(|error| format!("jira request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "jira comment failed: {} {}",
                response.status(),
                response.text().unwrap_or_default()
            ));
        }
        Ok(())
    }

    pub fn transition_to_status(
        &self,
        key: &str,
        target_status: &str,
    ) -> Result<Option<String>, String> {
        let transitions = self.transitions(key)?;
        let target = target_status.trim();
        if let Some(found) = transitions.iter().find(|transition| {
            transition.to_status.eq_ignore_ascii_case(target)
                || transition.name.eq_ignore_ascii_case(target)
        }) {
            self.transition(key, &found.id)?;
            return Ok(Some(found.to_status.clone()));
        }
        Ok(None)
    }

    /// Registers a webhook on the JIRA site and returns the created webhook ids.
    pub fn register_webhook(
        &self,
        url: &str,
        events: &[&str],
        jql_filter: &str,
    ) -> Result<Vec<i64>, String> {
        let response = self
            .post("/rest/api/3/webhook")
            .json(&json!({
                "url": url,
                "webhooks": [ { "events": events, "jqlFilter": jql_filter } ]
            }))
            .send()
            .map_err(|error| format!("jira request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "jira webhook registration failed: {} {}",
                response.status(),
                response.text().unwrap_or_default()
            ));
        }
        let value: Value = response.json().map_err(|error| error.to_string())?;
        Ok(value
            .get("webhooks")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Value::as_i64))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn delete_webhooks(&self, ids: &[i64]) -> Result<(), String> {
        let response = self
            .http
            .delete(format!("{}/rest/api/3/webhook", self.api_base))
            .bearer_auth(&self.token)
            .json(&json!({ "webhookIds": ids }))
            .send()
            .map_err(|error| format!("jira request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "jira webhook removal failed: {} {}",
                response.status(),
                response.text().unwrap_or_default()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockServer;

    #[test]
    fn builds_cloud_api_base() {
        assert_eq!(
            cloud_api_base("cloud-1"),
            "https://api.atlassian.com/ex/jira/cloud-1"
        );
        assert_eq!(
            cloud_api_base("cloud-1/"),
            "https://api.atlassian.com/ex/jira/cloud-1/"
        );
    }

    #[test]
    fn flattens_adf_descriptions() {
        let doc = json!({
            "type": "doc",
            "content": [
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "First line" },
                    { "type": "hardBreak" }
                ]},
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [
                            { "type": "text", "text": "Do a thing" }
                        ]}
                    ]}
                ]}
            ]
        });
        let text = adf_to_text(&doc);
        assert!(text.contains("First line"));
        assert!(text.contains("- Do a thing"));
        assert_eq!(adf_to_text(&Value::Null), "");
        assert_eq!(adf_to_text(&json!("plain")), "plain");
    }

    #[test]
    fn calls_jira_through_the_cloud_proxy() {
        let server = MockServer::start(|request, body| match request {
            "GET /ex/jira/cloud-1/rest/api/3/myself" => {
                (200, r#"{"displayName":"Ada Lovelace"}"#.to_string())
            }
            "GET /ex/jira/cloud-1/rest/api/3/issue/CC-1" => (
                200,
                r#"{"id":"10001","fields":{"summary":"Pagination","description":{"type":"doc","version":1,"content":[{"type":"paragraph","content":[{"type":"text","text":"The API returns all rows."}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"Add limit"}]}]}]}]},"status":{"name":"To Do"},"issuetype":{"name":"Task"}}}"#
                    .to_string(),
            ),
            "POST /ex/jira/cloud-1/rest/api/3/webhook" => {
                assert!(body.contains("\"url\":\"https://backend.example/webhooks/jira/s3cret\""));
                assert!(body.contains("jira:issue_created"));
                (200, r#"{"webhooks":[{"id":10000}]}"#.to_string())
            }
            _ => (404, r#"{"error":"unexpected"}"#.to_string()),
        });
        let base = format!("{}/ex/jira/cloud-1", server.base_url());
        let client = JiraClient::new(base, "access-1".to_string()).unwrap();

        assert_eq!(client.account().unwrap(), "Ada Lovelace");
        assert_eq!(client.get_issue("CC-1").unwrap().summary, "Pagination");
        let issue = client.get_issue("CC-1").unwrap();
        assert!(issue.description.contains("The API returns all rows."));
        assert!(issue.description.contains("Add limit"));

        let ids = client
            .register_webhook(
                "https://backend.example/webhooks/jira/s3cret",
                WEBHOOK_EVENTS,
                "project IS NOT EMPTY",
            )
            .unwrap();
        assert_eq!(ids, vec![10000]);
    }
}
