use serde_json::Value;
use tauri::State;

use crate::cloud::CloudClient;
use crate::db;
use crate::domain::{AgentTask, CloudNotification, TaskStatus, TicketIntake};
use crate::state::AppState;

pub(crate) const KEY_BASE_URL: &str = "cloudBaseUrl";
pub(crate) const KEY_DEVICE_TOKEN: &str = "cloudDeviceToken";
const KEY_CURSOR: &str = "notificationsCursor";

pub(crate) fn setting_string(state: &AppState, key: &str) -> Result<Option<String>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    Ok(db::get_setting(&connection, key)
        .and_then(|value| value.as_str().map(str::to_string))
        .filter(|value| !value.trim().is_empty()))
}

/// Reads the cloud device token, decrypting it (it is a credential).
pub(crate) fn device_token(state: &AppState) -> Result<Option<String>, String> {
    crate::secrets::get(state, KEY_DEVICE_TOKEN)
}

fn save_cursor(state: &AppState, cursor: &str) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::set_setting(&connection, KEY_CURSOR, &Value::String(cursor.to_string()))
}

/// Moves the notification cursor forward, never backwards. Used by the socket
/// so reconnects do not re-deliver events that already arrived live.
pub(crate) fn advance_cursor(state: &AppState, created_at: &str) -> Result<(), String> {
    let current = setting_string(state, KEY_CURSOR)?;
    let should_advance = current.as_deref().map(|value| created_at > value).unwrap_or(true);
    if should_advance {
        save_cursor(state, created_at)?;
    }
    Ok(())
}

/// The JIRA project key for an issue key (`CC-142` -> `CC`).
fn project_key(issue_key: &str) -> String {
    issue_key
        .split('-')
        .next()
        .unwrap_or(issue_key)
        .to_uppercase()
}

/// Fetches notifications after the stored cursor and advances the cursor.
/// Returns an empty list when the cloud account is not signed in.
pub(crate) fn fetch_new_notifications(
    state: &AppState,
) -> Result<Vec<CloudNotification>, String> {
    let Some(base_url) = setting_string(state, KEY_BASE_URL)? else {
        return Ok(Vec::new());
    };
    let Some(device_token) = device_token(state)? else {
        return Ok(Vec::new());
    };
    let cursor = setting_string(state, KEY_CURSOR)?;

    let client = CloudClient::new(base_url)?;
    let items = client.list_notifications(&device_token, cursor.as_deref())?;
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let newest = items
        .iter()
        .filter_map(|item| item.get("createdAt").and_then(Value::as_str))
        .max()
        .map(str::to_string);
    let notifications: Vec<CloudNotification> = items
        .iter()
        .filter_map(|item| serde_json::from_value(item.clone()).ok())
        .collect();

    if let Some(newest) = newest {
        save_cursor(state, &newest)?;
    }

    Ok(notifications)
}

fn mark_read(state: &AppState, id: &str) -> Result<(), String> {
    let Some(base_url) = setting_string(state, KEY_BASE_URL)? else {
        return Ok(());
    };
    let Some(device_token) = device_token(state)? else {
        return Ok(());
    };
    let client = CloudClient::new(base_url)?;
    client.mark_notification_read(&device_token, id)?;
    Ok(())
}

#[tauri::command]
pub fn mark_notification_read(state: State<AppState>, id: String) -> Result<(), String> {
    mark_read(&state, &id)
}

#[tauri::command]
pub fn skip_ticket(
    state: State<AppState>,
    jira_key: String,
    title: Option<String>,
    notification_id: Option<String>,
) -> Result<AgentTask, String> {
    let key = jira_key.trim().to_uppercase();
    if key.is_empty() {
        return Err("a JIRA issue key is required".to_string());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let task = AgentTask {
        id: uuid::Uuid::new_v4().to_string(),
        jira_issue_key: key.clone(),
        title: title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| key.clone()),
        description: String::new(),
        repository_id: String::new(),
        status: TaskStatus::Skipped,
        plan_version: None,
        approved_plan_version: None,
        workspace_path: None,
        branch_name: None,
        commit_hash: None,
        interrupted_from: None,
        created_at: now.clone(),
        updated_at: now,
    };

    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::insert_task(&connection, &task)?;
    }

    if let Some(id) = notification_id.filter(|value| !value.trim().is_empty()) {
        let _ = mark_read(&state, &id);
    }
    Ok(task)
}

#[tauri::command]
pub fn prepare_ticket_intake(
    state: State<AppState>,
    jira_key: String,
    notification_id: Option<String>,
) -> Result<TicketIntake, String> {
    let key = jira_key.trim().to_uppercase();
    if key.is_empty() {
        return Err("a JIRA issue key is required".to_string());
    }

    let project = project_key(&key);
    let repository_id = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::find_repository_for_project(&connection, &project)
    };

    let issue = crate::commands::jira::jira_client(&state)?.get_issue(&key)?;

    if let Some(id) = notification_id.filter(|value| !value.trim().is_empty()) {
        let _ = mark_read(&state, &id);
    }

    Ok(TicketIntake {
        repository_id,
        issue,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MockServer;
    use serde_json::json;

    #[test]
    fn extracts_project_keys() {
        assert_eq!(project_key("CC-142"), "CC");
        assert_eq!(project_key("cc-142"), "CC");
        assert_eq!(project_key("PLATFORM-7"), "PLATFORM");
        assert_eq!(project_key("nohyphen"), "NOHYPHEN");
    }

    #[test]
    fn fetches_notifications_and_advances_the_cursor() {
        let server = MockServer::start(|request, _body| match request {
            "GET /notifications" => (
                200,
                r#"{"notifications":[{"id":"n1","type":"jira:issue_created","title":"CC-1: Add pagination","createdAt":"2026-01-02T00:00:00Z","payload":{"jiraKey":"CC-1","eventType":"jira:issue_created"}}]}"#
                    .to_string(),
            ),
            "GET /notifications?after=2026-01-02T00%3A00%3A00Z" => {
                (200, r#"{"notifications":[]}"#.to_string())
            }
            _ => (404, "{}".to_string()),
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
                &Value::String(server.base_url()),
            )
            .unwrap();
            db::set_setting(
                &connection,
                KEY_DEVICE_TOKEN,
                &Value::String("device-token".to_string()),
            )
            .unwrap();
        }

        let notifications = fetch_new_notifications(&state).unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].id, "n1");
        assert_eq!(notifications[0].kind, "jira:issue_created");
        assert_eq!(
            notifications[0].payload.as_ref().unwrap()["jiraKey"],
            json!("CC-1")
        );

        assert_eq!(
            setting_string(&state, KEY_CURSOR).unwrap().as_deref(),
            Some("2026-01-02T00:00:00Z")
        );

        // The second poll uses the advanced cursor and returns nothing.
        assert!(fetch_new_notifications(&state).unwrap().is_empty());
    }

    #[test]
    fn returns_nothing_when_signed_out() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::load_with_key_source(
            dir.path().to_path_buf(),
            crate::secure_store::KeySource::File,
        )
        .unwrap();
        assert!(fetch_new_notifications(&state).unwrap().is_empty());
    }
}
