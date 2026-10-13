use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::cloud::CloudClient;
use crate::db;
use crate::domain::{
    JiraConnectionInfo, JiraIssue, JiraProjectRepo, JiraStatusMap, JiraSync, JiraTransition,
    TicketSource,
};
use crate::jira::{JiraClient, WEBHOOK_EVENTS};
use crate::oauth::{self, OAuthConfig};
use crate::secure_store::SecureStore;
use crate::state::AppState;

const KEY_TOKENS: &str = "jiraOAuthTokens";
const KEY_STATUS_MAP: &str = "jiraStatusMap";
const KEY_WEBHOOK_URL: &str = "jiraWebhookUrl";
const KEY_WEBHOOK_IDS: &str = "jiraWebhookIds";
const KEY_CLIENT_ID: &str = "jiraClientId";
const KEY_CLIENT_SECRET: &str = "jiraClientSecret";
const KEY_CLOUD_BASE_URL: &str = "cloudBaseUrl";
const KEY_CLOUD_TOKEN: &str = "cloudToken";
const OAUTH_FILE: &str = "jira-oauth.json";
const AUTH_TIMEOUT: Duration = Duration::from_secs(180);
const WEBHOOK_JQL: &str = "project IS NOT EMPTY";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredTokens {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    cloud_id: String,
    site_url: String,
    #[serde(default)]
    site_name: Option<String>,
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn oauth_config(state: &AppState) -> Result<OAuthConfig, String> {
    let env_client_id = env_nonempty("JIRA_CLIENT_ID");
    let env_client_secret = env_nonempty("JIRA_CLIENT_SECRET");
    let stored = stored_oauth_credentials(state);
    let file = read_oauth_file(&state.data_dir.join(OAUTH_FILE))?;
    match resolve_oauth_credentials(env_client_id, env_client_secret, stored, file) {
        Some((client_id, client_secret)) => Ok(OAuthConfig::new(client_id, client_secret)),
        None => Err(
            "JIRA OAuth is not configured. Enter your Atlassian app Client ID and secret, set \
             JIRA_CLIENT_ID (and JIRA_CLIENT_SECRET), or create ~/.ticketbridge/jira-oauth.json. \
             See docs/phase-20-jira-oauth.md."
                .to_string(),
        ),
    }
}

/// Client id/secret entered in-app and stored encrypted, if any.
fn stored_oauth_credentials(state: &AppState) -> (Option<String>, Option<String>) {
    let client_id = crate::secrets::get(state, KEY_CLIENT_ID).ok().flatten();
    let client_secret = crate::secrets::get(state, KEY_CLIENT_SECRET).ok().flatten();
    (client_id, client_secret)
}

/// Reads the developer config file. `Ok(None)` when the file is absent; an error
/// when it exists but cannot be parsed or is missing `clientId`.
fn read_oauth_file(path: &Path) -> Result<Option<(String, Option<String>)>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    let client_id = value
        .get("clientId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{} is missing clientId", path.display()))?
        .to_string();
    let client_secret = value
        .get("clientSecret")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    Ok(Some((client_id, client_secret)))
}

/// Picks the effective OAuth credentials. Precedence: an explicit environment
/// override, then credentials entered in-app (secure store), then the developer
/// config file.
fn resolve_oauth_credentials(
    env_client_id: Option<String>,
    env_client_secret: Option<String>,
    stored: (Option<String>, Option<String>),
    file: Option<(String, Option<String>)>,
) -> Option<(String, Option<String>)> {
    if let Some(client_id) = env_client_id {
        return Some((client_id, env_client_secret));
    }
    let (stored_id, stored_secret) = stored;
    if let Some(client_id) = stored_id {
        return Some((client_id, stored_secret));
    }
    file
}

/// Whether OAuth client credentials are available from any source.
pub fn oauth_configured(state: &AppState) -> bool {
    if env_nonempty("JIRA_CLIENT_ID").is_some() || stored_oauth_credentials(state).0.is_some() {
        return true;
    }
    state.data_dir.join(OAUTH_FILE).exists()
}

fn secure_store(state: &AppState) -> &SecureStore {
    &state.secure_store
}

fn setting_string(state: &AppState, key: &str) -> Result<Option<String>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    Ok(db::get_setting(&connection, key)
        .and_then(|value| value.as_str().map(str::to_string))
        .filter(|value| !value.trim().is_empty()))
}

fn load_tokens(state: &AppState) -> Result<StoredTokens, String> {
    let envelope =
        setting_string(state, KEY_TOKENS)?.ok_or_else(|| "JIRA is not connected".to_string())?;
    let json = secure_store(state).decrypt(&envelope)?;
    serde_json::from_str(&json).map_err(|error| format!("invalid stored JIRA credentials: {error}"))
}

fn save_tokens(state: &AppState, tokens: &StoredTokens) -> Result<(), String> {
    let json = serde_json::to_string(tokens).map_err(|error| error.to_string())?;
    let envelope = secure_store(state).encrypt(&json)?;
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::set_setting(&connection, KEY_TOKENS, &Value::String(envelope))
}

fn connection_info(state: &AppState) -> Result<Option<JiraConnectionInfo>, String> {
    let tokens = match load_tokens(state) {
        Ok(tokens) => tokens,
        Err(_) => return Ok(None),
    };
    let webhook_url = setting_string(state, KEY_WEBHOOK_URL)?;
    Ok(Some(JiraConnectionInfo {
        account: tokens.account,
        cloud_id: tokens.cloud_id,
        site_url: tokens.site_url,
        site_name: tokens.site_name,
        webhook_url,
    }))
}

fn expires_at_from(expires_in: Option<i64>) -> Option<String> {
    expires_in.map(|seconds| (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339())
}

fn token_expired(tokens: &StoredTokens) -> bool {
    match tokens
        .expires_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
    {
        Some(expires_at) => {
            chrono::Utc::now() + chrono::Duration::seconds(60)
                >= expires_at.with_timezone(&chrono::Utc)
        }
        None => false,
    }
}

/// Returns live credentials, refreshing the access token when it is close to
/// expiring.
fn access_tokens(state: &AppState) -> Result<(StoredTokens, OAuthConfig), String> {
    let config = oauth_config(state)?;
    let mut tokens = load_tokens(state)?;
    if token_expired(&tokens) {
        let refresh = tokens
            .refresh_token
            .clone()
            .ok_or_else(|| "JIRA session expired; reconnect JIRA".to_string())?;
        let refreshed = config.refresh(&refresh)?;
        tokens.access_token = refreshed.access_token;
        if refreshed.refresh_token.is_some() {
            tokens.refresh_token = refreshed.refresh_token;
        }
        if refreshed.scope.is_some() {
            tokens.scope = refreshed.scope;
        }
        tokens.expires_at = expires_at_from(refreshed.expires_in);
        save_tokens(state, &tokens)?;
    }
    Ok((tokens, config))
}

pub(crate) fn jira_client(state: &AppState) -> Result<JiraClient, String> {
    let (tokens, _config) = access_tokens(state)?;
    Ok(
        JiraClient::for_cloud(&tokens.cloud_id, tokens.access_token)?
            .with_site_url(tokens.site_url),
    )
}

pub fn is_connected(state: &AppState) -> bool {
    load_tokens(state).is_ok()
}

fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url.to_string(), None::<&str>)
        .map_err(|error| error.to_string())
}

fn status_map(state: &AppState) -> JiraStatusMap {
    state
        .db
        .lock()
        .ok()
        .and_then(|connection| db::get_setting(&connection, KEY_STATUS_MAP))
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn start_jira_oauth(
    app: AppHandle,
    state: State<AppState>,
) -> Result<JiraConnectionInfo, String> {
    let config = oauth_config(&state)?;
    let verifier = oauth::generate_verifier();
    let challenge = oauth::challenge_for(&verifier);
    let expected_state = oauth::generate_state();
    let listener = oauth::CallbackListener::bind()?;
    let redirect_uri = listener.redirect_uri();
    let authorize = oauth::authorize_url(&config, &redirect_uri, &expected_state, &challenge);

    open_url(&app, &authorize)?;
    let code = listener.wait(&expected_state, AUTH_TIMEOUT)?;
    let token = config.exchange_code(&redirect_uri, &code, &verifier)?;
    let resource = config
        .accessible_resources(&token.access_token)?
        .into_iter()
        .next()
        .ok_or_else(|| "this account has no accessible JIRA sites".to_string())?;

    let account = JiraClient::for_cloud(&resource.id, token.access_token.clone())?
        .with_site_url(resource.url.clone())
        .account()
        .ok();

    let stored = StoredTokens {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: expires_at_from(token.expires_in),
        cloud_id: resource.id,
        site_url: resource.url,
        site_name: resource.name,
        account,
        scope: token.scope,
    };
    save_tokens(&state, &stored)?;

    // Registering the backend webhook is best-effort; connecting still works
    // when the cloud account is not signed in.
    if let Err(error) = register_webhook(&state) {
        crate::logging::warn(
            "jira",
            "webhook registration skipped",
            serde_json::json!({ "error": error }),
        );
    }

    connection_info(&state)?.ok_or_else(|| "JIRA connection was not saved".to_string())
}

#[tauri::command]
pub fn jira_oauth_configured(state: State<AppState>) -> bool {
    oauth_configured(&state)
}

/// Stores the Atlassian app Client ID/secret in the encrypted settings, so the
/// user can paste them in-app instead of editing an environment or JSON file.
#[tauri::command]
pub fn set_jira_oauth_credentials(
    state: State<AppState>,
    client_id: String,
    client_secret: String,
) -> Result<(), String> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err("a JIRA client ID is required".to_string());
    }
    crate::secrets::set(&state, KEY_CLIENT_ID, client_id)?;

    let client_secret = client_secret.trim();
    if client_secret.is_empty() {
        crate::secrets::delete(&state, KEY_CLIENT_SECRET)?;
    } else {
        crate::secrets::set(&state, KEY_CLIENT_SECRET, client_secret)?;
    }
    Ok(())
}

#[tauri::command]
pub fn clear_jira_oauth_credentials(state: State<AppState>) -> Result<(), String> {
    crate::secrets::delete(&state, KEY_CLIENT_ID)?;
    crate::secrets::delete(&state, KEY_CLIENT_SECRET)?;
    Ok(())
}

#[tauri::command]
pub fn get_jira_connection_info(
    state: State<AppState>,
) -> Result<Option<JiraConnectionInfo>, String> {
    connection_info(&state)
}

#[tauri::command]
pub fn get_jira_account(state: State<AppState>) -> Result<Option<String>, String> {
    Ok(connection_info(&state)?.and_then(|info| info.account))
}

/// Ensures the cloud backend has a JIRA connection and registers its webhook
/// URL on the JIRA site.
pub fn register_webhook(state: &AppState) -> Result<JiraConnectionInfo, String> {
    let base_url = setting_string(state, KEY_CLOUD_BASE_URL)?
        .ok_or_else(|| "sign in to the cloud account to register JIRA webhooks".to_string())?;
    let cloud_token = crate::secrets::get(state, KEY_CLOUD_TOKEN)?
        .ok_or_else(|| "sign in to the cloud account to register JIRA webhooks".to_string())?;
    let cloud = CloudClient::new(base_url)?;

    let (tokens, _config) = access_tokens(state)?;
    let webhook_url = match cloud
        .jira_connections(&cloud_token)?
        .first()
        .and_then(|connection| connection.get("webhookUrl"))
        .and_then(Value::as_str)
    {
        Some(url) => url.to_string(),
        None => cloud
            .create_jira_connection(&cloud_token, &tokens.site_url, "")
            .and_then(|connection| {
                connection
                    .get("webhookUrl")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| "cloud response missing webhookUrl".to_string())
            })?,
    };

    let ids = jira_client(state)?.register_webhook(&webhook_url, WEBHOOK_EVENTS, WEBHOOK_JQL)?;
    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::set_setting(&connection, KEY_WEBHOOK_URL, &Value::String(webhook_url))?;
        db::set_setting(&connection, KEY_WEBHOOK_IDS, &json!(ids))?;
    }
    connection_info(state)?.ok_or_else(|| "JIRA is not connected".to_string())
}

#[tauri::command]
pub fn register_jira_webhook(state: State<AppState>) -> Result<JiraConnectionInfo, String> {
    register_webhook(&state)
}

#[tauri::command]
pub fn disconnect_jira(state: State<AppState>) -> Result<(), String> {
    if let Ok(Some(raw)) = setting_string(&state, KEY_WEBHOOK_IDS) {
        let ids: Vec<i64> = serde_json::from_str(&raw).unwrap_or_default();
        if !ids.is_empty() {
            if let Ok(client) = jira_client(&state) {
                let _ = client.delete_webhooks(&ids);
            }
        }
    }

    let connection = state.db.lock().map_err(|error| error.to_string())?;
    for key in [KEY_TOKENS, KEY_WEBHOOK_URL, KEY_WEBHOOK_IDS] {
        connection
            .execute("DELETE FROM settings WHERE key = ?1", [key])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_jira_status_map(state: State<AppState>) -> Result<JiraStatusMap, String> {
    Ok(status_map(&state))
}

#[tauri::command]
pub fn set_jira_status_map(state: State<AppState>, map: JiraStatusMap) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::set_setting(
        &connection,
        KEY_STATUS_MAP,
        &serde_json::to_value(map).map_err(|error| error.to_string())?,
    )
}

#[tauri::command]
pub fn get_jira_issue(state: State<AppState>, key: String) -> Result<JiraIssue, String> {
    jira_client(&state)?.get_issue(&key)
}

/// JQL for the Task page ticket picker. With no query it lists the current
/// user's issues (most recently updated first); a non-empty query does a
/// free-text match across issues the user can see.
fn build_search_jql(query: &str) -> String {
    let query = query.trim();
    if query.is_empty() {
        "assignee = currentUser() ORDER BY updated DESC".to_string()
    } else {
        let escaped = query.replace('\\', "\\\\").replace('"', "\\\"");
        format!("text ~ \"{escaped}\" ORDER BY updated DESC")
    }
}

/// Searches JIRA so the user can pick a ticket by name. Results carry the key
/// and summary (title); the UI renders them as `KEY - Title`.
#[tauri::command]
pub fn search_jira_issues(
    state: State<AppState>,
    query: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<JiraIssue>, String> {
    let jql = build_search_jql(query.as_deref().unwrap_or_default());
    let limit = limit.unwrap_or(20);
    jira_client(&state)?.search_issues(&jql, limit)
}

#[tauri::command]
pub fn list_jira_transitions(
    state: State<AppState>,
    key: String,
) -> Result<Vec<JiraTransition>, String> {
    jira_client(&state)?.transitions(&key)
}

#[tauri::command]
pub fn get_jira_sync(state: State<AppState>, task_id: String) -> Result<Option<JiraSync>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::get_jira_sync(&connection, &task_id)
}

#[tauri::command]
pub fn sync_task_jira(app: AppHandle, task_id: String) -> Result<JiraSync, String> {
    sync_task(&app, &task_id)
}

pub fn sync_task(app: &AppHandle, task_id: &str) -> Result<JiraSync, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "state unavailable".to_string())?;

    let (task, existing, pr) = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        let task = db::get_task(&connection, task_id)?
            .ok_or_else(|| format!("task not found: {task_id}"))?;
        let existing = db::get_jira_sync(&connection, task_id)?;
        let pr = db::get_pull_request(&connection, task_id)?;
        (task, existing, pr)
    };

    if task.ticket_source == TicketSource::Custom {
        return Err("this task was created without a JIRA ticket".to_string());
    }

    let key = task.jira_issue_key.clone();
    if key.trim().is_empty() {
        return Err("task has no JIRA issue key".to_string());
    }

    let (tokens, _config) = access_tokens(&state)?;
    let cloud_id = tokens.cloud_id.clone();
    let access = tokens.access_token.clone();
    let site_url = tokens.site_url.clone();
    let map = status_map(&state);

    let issue = crate::opencode::run_blocking({
        let cloud_id = cloud_id.clone();
        let access = access.clone();
        let site_url = site_url.clone();
        let key = key.clone();
        move || {
            JiraClient::for_cloud(&cloud_id, access)?
                .with_site_url(site_url)
                .get_issue(&key)
        }
    })?;

    let mut actions: Vec<String> = Vec::new();

    if let Some(target) = map.opened.clone().filter(|value| !value.trim().is_empty()) {
        if !issue.status.eq_ignore_ascii_case(&target) {
            let (cloud_id, access, site_url) = (cloud_id.clone(), access.clone(), site_url.clone());
            let key = key.clone();
            let target_move = target.clone();
            let moved = crate::opencode::run_blocking(move || {
                JiraClient::for_cloud(&cloud_id, access)?
                    .with_site_url(site_url)
                    .transition_to_status(&key, &target_move)
            })?;
            match moved {
                Some(status) => actions.push(format!("transitioned to {status}")),
                None => actions.push(format!("no transition available to '{target}'")),
            }
        }
    }

    let comment = match &pr {
        Some(pr) => format!(
            "TicketBridge opened pull request #{}: {}",
            pr.number, pr.url
        ),
        None => "TicketBridge completed the implementation.".to_string(),
    };
    {
        let (cloud_id, access, site_url) = (cloud_id.clone(), access.clone(), site_url.clone());
        let key = key.clone();
        let comment_move = comment.clone();
        crate::opencode::run_blocking(move || {
            JiraClient::for_cloud(&cloud_id, access)?
                .with_site_url(site_url)
                .add_comment(&key, &comment_move)
        })?;
        actions.push("posted comment".to_string());
    }

    let timestamp = chrono::Utc::now().to_rfc3339();
    let sync = JiraSync {
        id: existing
            .as_ref()
            .map(|sync| sync.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        task_id: task_id.to_string(),
        jira_issue_key: key,
        jira_issue_id: Some(issue.id),
        pr_number: pr.as_ref().map(|pr| pr.number),
        last_action: Some(actions.join(", ")),
        last_synced_at: Some(timestamp.clone()),
        created_at: existing
            .map(|sync| sync.created_at)
            .unwrap_or_else(|| timestamp.clone()),
        updated_at: timestamp,
    };

    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::upsert_jira_sync(&connection, &sync)?;
    }
    let _ = app.emit("jira://synced", &sync);
    Ok(sync)
}

#[tauri::command]
pub fn list_jira_project_repos(state: State<AppState>) -> Result<Vec<JiraProjectRepo>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::list_jira_project_repos(&connection)
}

#[tauri::command]
pub fn set_jira_project_repo(
    state: State<AppState>,
    project_key: String,
    repository_id: String,
) -> Result<JiraProjectRepo, String> {
    let project_key = project_key.trim().to_uppercase();
    if project_key.is_empty() {
        return Err("a JIRA project key is required".to_string());
    }
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::upsert_jira_project_repo(&connection, &project_key, &repository_id)
}

#[tauri::command]
pub fn delete_jira_project_repo(state: State<AppState>, project_key: String) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::delete_jira_project_repo(&connection, project_key.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ticket_picker_jql() {
        assert_eq!(
            build_search_jql(""),
            "assignee = currentUser() ORDER BY updated DESC"
        );
        assert_eq!(
            build_search_jql("   "),
            "assignee = currentUser() ORDER BY updated DESC"
        );
        assert_eq!(
            build_search_jql("login bug"),
            "text ~ \"login bug\" ORDER BY updated DESC"
        );
        assert_eq!(
            build_search_jql("say \"hi\""),
            "text ~ \"say \\\"hi\\\"\" ORDER BY updated DESC"
        );
    }

    fn stored(id: &str, secret: &str) -> (Option<String>, Option<String>) {
        (Some(id.to_string()), Some(secret.to_string()))
    }

    fn file(id: &str, secret: &str) -> Option<(String, Option<String>)> {
        Some((id.to_string(), Some(secret.to_string())))
    }

    #[test]
    fn env_credentials_win_over_stored_and_file() {
        let resolved = resolve_oauth_credentials(
            Some("env-id".to_string()),
            Some("env-secret".to_string()),
            stored("stored-id", "stored-secret"),
            file("file-id", "file-secret"),
        );
        assert_eq!(
            resolved,
            Some(("env-id".to_string(), Some("env-secret".to_string())))
        );
    }

    #[test]
    fn stored_credentials_win_over_file() {
        let resolved = resolve_oauth_credentials(
            None,
            None,
            stored("stored-id", "stored-secret"),
            file("file-id", "file-secret"),
        );
        assert_eq!(
            resolved,
            Some(("stored-id".to_string(), Some("stored-secret".to_string())))
        );
    }

    #[test]
    fn file_is_the_last_resort() {
        let resolved = resolve_oauth_credentials(None, None, (None, None), file("file-id", "s"));
        assert_eq!(
            resolved,
            Some(("file-id".to_string(), Some("s".to_string())))
        );
    }

    #[test]
    fn no_sources_means_unconfigured() {
        assert_eq!(
            resolve_oauth_credentials(None, None, (None, None), None),
            None
        );
    }

    #[test]
    fn reads_and_rejects_the_oauth_file() {
        let dir = tempfile::tempdir().unwrap();

        let missing = dir.path().join("jira-oauth.json");
        assert_eq!(read_oauth_file(&missing).unwrap(), None);

        let valid = dir.path().join("valid.json");
        std::fs::write(&valid, r#"{"clientId":"id-1","clientSecret":"secret-1"}"#).unwrap();
        assert_eq!(
            read_oauth_file(&valid).unwrap(),
            Some(("id-1".to_string(), Some("secret-1".to_string())))
        );

        let id_only = dir.path().join("id-only.json");
        std::fs::write(&id_only, r#"{"clientId":"id-2"}"#).unwrap();
        assert_eq!(
            read_oauth_file(&id_only).unwrap(),
            Some(("id-2".to_string(), None))
        );

        let malformed = dir.path().join("malformed.json");
        std::fs::write(&malformed, "not json").unwrap();
        assert!(read_oauth_file(&malformed).is_err());

        let no_id = dir.path().join("no-id.json");
        std::fs::write(&no_id, r#"{"clientSecret":"secret"}"#).unwrap();
        assert!(read_oauth_file(&no_id).is_err());
    }
}
