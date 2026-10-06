use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use crate::cloud::CloudClient;
use crate::db;
use crate::domain::CloudAccount;
use crate::state::AppState;

const KEY_BASE_URL: &str = "cloudBaseUrl";
const KEY_TOKEN: &str = "cloudToken";
const KEY_EMAIL: &str = "cloudEmail";
const KEY_DEVICE_ID: &str = "cloudDeviceId";
const KEY_DEVICE_TOKEN: &str = "cloudDeviceToken";

fn setting_string(state: &AppState, key: &str) -> Result<Option<String>, String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    Ok(db::get_setting(&connection, key)
        .and_then(|value| value.as_str().map(str::to_string))
        .filter(|value| !value.trim().is_empty()))
}

fn ensure_device_id(state: &AppState) -> Result<String, String> {
    if let Some(existing) = setting_string(state, KEY_DEVICE_ID)? {
        return Ok(existing);
    }
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    let device_id = Uuid::new_v4().to_string();
    db::set_setting(&connection, KEY_DEVICE_ID, &Value::String(device_id.clone()))?;
    Ok(device_id)
}

#[tauri::command]
pub fn login_cloud(
    state: State<AppState>,
    base_url: String,
    email: String,
    password: String,
    register: bool,
) -> Result<CloudAccount, String> {
    let device_id = ensure_device_id(&state)?;
    let device_name = "JIRA Agent Desktop".to_string();
    let platform = std::env::consts::OS.to_string();
    let email = email.trim().to_string();

    let (base_url, token, email, device_token) = crate::opencode::run_blocking({
        let device_id = device_id.clone();
        move || {
            let client = CloudClient::new(base_url)?;
            let (token, email) = if register {
                client.register(&email, &password)?
            } else {
                client.login(&email, &password)?
            };
            let device_token =
                client.register_device(&token, &device_id, &device_name, &platform)?;
            Ok((client.base_url().to_string(), token, email, device_token))
        }
    })?;

    {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::set_setting(&connection, KEY_BASE_URL, &Value::String(base_url.clone()))?;
        db::set_setting(&connection, KEY_EMAIL, &Value::String(email.clone()))?;
    }
    crate::secrets::set(&state, KEY_TOKEN, &token)?;
    crate::secrets::set(&state, KEY_DEVICE_TOKEN, &device_token)?;

    Ok(CloudAccount {
        base_url,
        email,
        device_id: Some(device_id),
    })
}

#[tauri::command]
pub fn get_cloud_account(state: State<AppState>) -> Result<Option<CloudAccount>, String> {
    if crate::secrets::get(&state, KEY_TOKEN)?.is_none() {
        return Ok(None);
    }
    let base_url = setting_string(&state, KEY_BASE_URL)?.unwrap_or_default();
    let email = setting_string(&state, KEY_EMAIL)?.unwrap_or_default();
    let device_id = setting_string(&state, KEY_DEVICE_ID)?;
    Ok(Some(CloudAccount {
        base_url,
        email,
        device_id,
    }))
}

#[tauri::command]
pub fn logout_cloud(state: State<AppState>) -> Result<(), String> {
    crate::secrets::delete(&state, KEY_TOKEN)?;
    crate::secrets::delete(&state, KEY_DEVICE_TOKEN)?;
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    for key in [KEY_EMAIL, KEY_BASE_URL] {
        connection
            .execute("DELETE FROM settings WHERE key = ?1", [key])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
