use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::cloud::CloudClient;
use crate::db;
use crate::domain::{
    JiraConnectionInfo, JiraIssue, JiraProjectRepo, JiraStatusMap, JiraSync, JiraTransition,
};
use crate::jira::{JiraClient, WEBHOOK_EVENTS};
use crate::oauth::{self, OAuthConfig};
use crate::secure_store::SecureStore;
use crate::state::AppState;

const KEY_TOKENS: &str = "jiraOAuthTokens";
const KEY_STATUS_MAP: &str = "jiraStatusMap";
const KEY_WEBHOOK_URL: &str = "jiraWebhookUrl";
const KEY_WEBHOOK_IDS: &str = "jiraWebhookIds";
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
    std::env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn oauth_config(state: &AppState) -> Result<OAuthConfig, String> {
    if let Some(client_id) = env_nonempty("JIRA_CLIENT_ID") {
        return Ok(OAuthConfig::new(
            client_id,
            env_nonempty("JIRA_CLIENT_SECRET"),
        ));
    }
    let path = state.data_dir.join(OAUTH_FILE);
    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|error| format!("read {OAUTH_FILE}: {error}"))?;
        let value: Value = serde_json::from_str(&raw)
            .map_err(|error| format!("invalid {OAUTH_FILE}: {error}"))?;
        let client_id = value
            .get("clientId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("{OAUTH_FILE} is missing clientId"))?
            .to_string();
        let client_secret = value
            .get("clientSecret")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string);
        return Ok(OAuthConfig::new(client_id, client_secret));
    }
    Err(
        "JIRA OAuth is not configured. Set JIRA_CLIENT_ID (and JIRA_CLIENT_SECRET), or create \
         ~/.jira-agent/jira-oauth.json with {\"clientId\": \"...\", \"clientSecret\": \"...\"}. \
         See docs/phase-20-jira-oauth.md."
            .to_string(),
    )
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
    let envelope = setting_string(state, KEY_TOKENS)?
        .ok_or_else(|| "JIRA is not connected".to_string())?;
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
    expires_in.map(|seconds| {
        (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339()
    })
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
    Ok(JiraClient::for_cloud(&tokens.cloud_id, tokens.access_token)?.with_site_url(tokens.site_url))
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
            let (cloud_id, access, site_url) =
                (cloud_id.clone(), access.clone(), site_url.clone());
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
            "JIRA Agent opened pull request #{}: {}",
            pr.number, pr.url
        ),
        None => "JIRA Agent completed the implementation.".to_string(),
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
pub fn delete_jira_project_repo(
    state: State<AppState>,
    project_key: String,
) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::delete_jira_project_repo(&connection, project_key.trim())
}
