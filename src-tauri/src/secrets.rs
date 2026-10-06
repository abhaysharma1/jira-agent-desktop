//! Credential settings stored in the `settings` table (and the OpenCode server
//! registry) are kept as AES-GCM envelopes produced by [`SecureStore`]. This
//! module centralises encrypt-on-write / decrypt-on-read and the one-time
//! migration of values written by earlier releases in plaintext.

use serde_json::Value;

use crate::db;
use crate::secure_store::SecureStore;
use crate::state::AppState;

/// Settings keys that hold credentials rather than ordinary configuration.
pub const SECRET_KEYS: &[&str] = &["githubToken", "cloudToken", "cloudDeviceToken"];

pub fn is_envelope(value: &str) -> bool {
    value.starts_with("v1:")
}

/// Encrypts a secret for storage. Values that are already envelopes are
/// returned unchanged, which makes migration idempotent.
pub fn encrypt_value(store: &SecureStore, value: &str) -> Result<String, String> {
    if is_envelope(value) {
        return Ok(value.to_string());
    }
    store.encrypt(value)
}

/// Decrypts a stored secret, tolerating legacy plaintext values.
pub fn decrypt_value(store: &SecureStore, stored: &str) -> Result<String, String> {
    if is_envelope(stored) {
        store.decrypt(stored)
    } else {
        Ok(stored.to_string())
    }
}

/// Reads a secret setting, decrypting it. Returns `None` when unset or blank.
pub fn get(state: &AppState, key: &str) -> Result<Option<String>, String> {
    let raw = {
        let connection = state.db.lock().map_err(|error| error.to_string())?;
        db::get_setting(&connection, key).and_then(|value| value.as_str().map(str::to_string))
    };
    match raw.filter(|value| !value.trim().is_empty()) {
        Some(raw) => Ok(Some(decrypt_value(&state.secure_store, &raw)?)),
        None => Ok(None),
    }
}

/// Writes a secret setting, encrypting it.
pub fn set(state: &AppState, key: &str, value: &str) -> Result<(), String> {
    let envelope = encrypt_value(&state.secure_store, value)?;
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    db::set_setting(&connection, key, &Value::String(envelope))
}

pub fn delete(state: &AppState, key: &str) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;
    connection
        .execute("DELETE FROM settings WHERE key = ?1", [key])
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// One-time migration: encrypts any plaintext secret still in the database.
/// Idempotent — encrypted values are left untouched.
pub fn migrate_plaintext(state: &AppState) -> Result<(), String> {
    let connection = state.db.lock().map_err(|error| error.to_string())?;

    for key in SECRET_KEYS {
        let current = db::get_setting(&connection, key)
            .and_then(|value| value.as_str().map(str::to_string));
        if let Some(value) = current.filter(|value| !value.trim().is_empty() && !is_envelope(value)) {
            let envelope = state.secure_store.encrypt(&value)?;
            db::set_setting(&connection, key, &Value::String(envelope))?;
        }
    }

    for server in db::list_opencode_servers(&connection)? {
        if !is_envelope(&server.password) {
            let envelope = state.secure_store.encrypt(&server.password)?;
            db::update_opencode_server_password(&connection, &server.run_id, &envelope)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure_store::KeySource;

    fn state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::load_with_key_source(dir.path().to_path_buf(), KeySource::File).unwrap();
        (state, dir)
    }

    #[test]
    fn round_trips_secret_settings() {
        let (state, _dir) = state();
        set(&state, "githubToken", "ghp_example").unwrap();
        assert_eq!(get(&state, "githubToken").unwrap().as_deref(), Some("ghp_example"));

        // the raw value on disk is an envelope, not the token
        let connection = state.db.lock().unwrap();
        let raw = db::get_setting(&connection, "githubToken").unwrap();
        assert!(raw.as_str().unwrap().starts_with("v1:"));
    }

    #[test]
    fn tolerates_legacy_plaintext_values() {
        let (state, _dir) = state();
        {
            let connection = state.db.lock().unwrap();
            db::set_setting(&connection, "cloudToken", &Value::String("legacy".to_string())).unwrap();
        }
        assert_eq!(get(&state, "cloudToken").unwrap().as_deref(), Some("legacy"));
    }

    #[test]
    fn migrates_plaintext_settings_and_server_passwords() {
        let (state, _dir) = state();
        {
            let connection = state.db.lock().unwrap();
            db::set_setting(
                &connection,
                "cloudDeviceToken",
                &Value::String("device-secret".to_string()),
            )
            .unwrap();
            db::record_opencode_server(
                &connection,
                &crate::domain::OpencodeServer {
                    run_id: "run-1".to_string(),
                    task_id: "task-1".to_string(),
                    pid: 1,
                    port: 1,
                    password: "server-secret".to_string(),
                    started_at: "t0".to_string(),
                },
            )
            .unwrap();
        }

        migrate_plaintext(&state).unwrap();

        {
            let connection = state.db.lock().unwrap();
            let token = db::get_setting(&connection, "cloudDeviceToken").unwrap();
            assert!(token.as_str().unwrap().starts_with("v1:"));
            let servers = db::list_opencode_servers(&connection).unwrap();
            assert!(servers[0].password.starts_with("v1:"));
        }
        assert_eq!(
            get(&state, "cloudDeviceToken").unwrap().as_deref(),
            Some("device-secret")
        );
    }

    #[test]
    fn delete_removes_the_setting() {
        let (state, _dir) = state();
        set(&state, "cloudToken", "jwt").unwrap();
        delete(&state, "cloudToken").unwrap();
        assert!(get(&state, "cloudToken").unwrap().is_none());
    }
}
