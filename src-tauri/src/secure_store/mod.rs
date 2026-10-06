use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use sha2::{Digest, Sha256};

const KEY_FILE: &str = "credentials.key";
const KEYRING_SERVICE: &str = "com.abhay.jira-agent-desktop";
const KEY_LEN: usize = 32;

/// Where the AES key itself is kept.
///
/// `OsKeyring` is the production path: the key lives in the platform credential
/// store (Windows Credential Manager / macOS Keychain / Secret Service), so a
/// copy of the database and its folder is not enough to decrypt secrets. `File`
/// is the fallback used when no keyring is available (and by tests, so they
/// never touch the real credential store).
#[derive(Debug, Clone, Copy)]
pub enum KeySource {
    OsKeyring,
    File,
}

/// Encrypts secrets at rest with AES-256-GCM. The encryption key is stored in
/// the OS credential store, falling back to a key file beside the database when
/// no keyring is available. Envelope format: `v1:<base64 nonce>:<base64
/// ciphertext+tag>`.
pub struct SecureStore {
    key: [u8; KEY_LEN],
}

impl SecureStore {
    /// Production loader: prefers the OS credential store and migrates any
    /// legacy `credentials.key` file into it.
    pub fn load(data_dir: &Path) -> Result<Self, String> {
        Self::load_with(data_dir, KeySource::OsKeyring)
    }

    pub fn load_with(data_dir: &Path, source: KeySource) -> Result<Self, String> {
        match source {
            KeySource::File => Self::load_file(data_dir),
            KeySource::OsKeyring => Self::load_keyring(data_dir),
        }
    }

    fn load_keyring(data_dir: &Path) -> Result<Self, String> {
        let entry = match keyring::Entry::new(KEYRING_SERVICE, &keyring_user(data_dir)) {
            Ok(entry) => entry,
            Err(error) => {
                crate::logging::warn(
                    "secure-store",
                    "keyring unavailable; using key file",
                    serde_json::json!({ "error": error.to_string() }),
                );
                return Self::load_with(data_dir, KeySource::File);
            }
        };

        match entry.get_password() {
            Ok(stored) => match decode_key(&stored) {
                Some(key) => return Ok(Self { key }),
                None => crate::logging::warn(
                    "secure-store",
                    "keyring entry is malformed; falling back to the key file",
                    serde_json::json!({}),
                ),
            },
            Err(keyring::Error::NoEntry) => {
                return Self::adopt_or_generate(data_dir, &entry);
            }
            Err(error) => {
                crate::logging::warn(
                    "secure-store",
                    "keyring unavailable; using key file",
                    serde_json::json!({ "error": error.to_string() }),
                );
                return Self::load_with(data_dir, KeySource::File);
            }
        }

        Self::load_with(data_dir, KeySource::File)
    }

    /// No keyring entry yet: adopt an existing key file (migrating it) or
    /// generate a fresh key.
    fn adopt_or_generate(data_dir: &Path, entry: &keyring::Entry) -> Result<Self, String> {
        let path = data_dir.join(KEY_FILE);
        let key = if path.exists() {
            read_key_file(&path)?
        } else {
            generate_key()
        };

        match entry.set_password(&B64.encode(key)) {
            Ok(()) => {
                if path.exists() {
                    let _ = fs::remove_file(&path);
                }
                Ok(Self { key })
            }
            Err(error) => {
                crate::logging::warn(
                    "secure-store",
                    "could not write keyring entry; using key file",
                    serde_json::json!({ "error": error.to_string() }),
                );
                Self::persist_file(data_dir, key)
            }
        }
    }

    fn load_file(data_dir: &Path) -> Result<Self, String> {
        let path = data_dir.join(KEY_FILE);
        if path.exists() {
            Ok(Self {
                key: read_key_file(&path)?,
            })
        } else {
            Self::persist_file(data_dir, generate_key())
        }
    }

    fn persist_file(data_dir: &Path, key: [u8; KEY_LEN]) -> Result<Self, String> {
        fs::write(data_dir.join(KEY_FILE), key)
            .map_err(|error| format!("write credentials key: {error}"))?;
        Ok(Self { key })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| "encryption failed".to_string())?;
        Ok(format!("v1:{}:{}", B64.encode(nonce), B64.encode(ciphertext)))
    }

    pub fn decrypt(&self, envelope: &str) -> Result<String, String> {
        let mut parts = envelope.splitn(3, ':');
        let version = parts.next().unwrap_or_default();
        if version != "v1" {
            return Err("unsupported credential format".to_string());
        }
        let nonce = parts.next().ok_or_else(|| "malformed credential".to_string())?;
        let body = parts.next().ok_or_else(|| "malformed credential".to_string())?;
        let nonce = B64
            .decode(nonce)
            .map_err(|_| "malformed credential nonce".to_string())?;
        let body = B64
            .decode(body)
            .map_err(|_| "malformed credential body".to_string())?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce), body.as_ref())
            .map_err(|_| "could not decrypt credentials".to_string())?;
        String::from_utf8(plaintext).map_err(|_| "credentials are not valid UTF-8".to_string())
    }
}

fn generate_key() -> [u8; KEY_LEN] {
    let generated = Aes256Gcm::generate_key(&mut OsRng);
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(generated.as_slice());
    key
}

fn read_key_file(path: &Path) -> Result<[u8; KEY_LEN], String> {
    let bytes = fs::read(path).map_err(|error| format!("read credentials key: {error}"))?;
    if bytes.len() != KEY_LEN {
        return Err("credentials key has an invalid length".to_string());
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn decode_key(encoded: &str) -> Option<[u8; KEY_LEN]> {
    let bytes = B64.decode(encoded.trim()).ok()?;
    if bytes.len() != KEY_LEN {
        return None;
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Some(key)
}

/// Credential-store user name, unique per data directory so tests (which use
/// temporary dirs) cannot collide with each other or with a real install.
fn keyring_user(data_dir: &Path) -> String {
    let canonical: PathBuf = fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    format!("credentials-key-{}", B64.encode(&digest[..8]))
}

/// Removes the keyring entry for a data directory. Used by tests to avoid
/// leaving entries behind in the real credential store.
#[cfg(test)]
fn delete_keyring_entry(data_dir: &Path) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &keyring_user(data_dir)) {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (SecureStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = SecureStore::load_with(dir.path(), KeySource::File).unwrap();
        (store, dir)
    }

    #[test]
    fn round_trips_secrets() {
        let (store, _dir) = store();
        let envelope = store.encrypt("access-token-123").unwrap();
        assert!(envelope.starts_with("v1:"));
        assert_ne!(envelope, "access-token-123");
        assert_eq!(store.decrypt(&envelope).unwrap(), "access-token-123");
    }

    #[test]
    fn rejects_tampered_ciphertext() {
        let (store, _dir) = store();
        let envelope = store.encrypt("secret").unwrap();
        let mut bytes: Vec<char> = envelope.chars().collect();
        let last = bytes.len() - 2;
        bytes[last] = if bytes[last] == 'A' { 'B' } else { 'A' };
        let tampered: String = bytes.into_iter().collect();
        assert!(store.decrypt(&tampered).is_err());
    }

    #[test]
    fn rejects_malformed_envelope() {
        let (store, _dir) = store();
        assert!(store.decrypt("not-an-envelope").is_err());
        assert!(store.decrypt("v2:abc:def").is_err());
    }

    #[test]
    fn reuses_the_same_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = SecureStore::load_with(dir.path(), KeySource::File).unwrap();
        let envelope = first.encrypt("persisted").unwrap();
        let second = SecureStore::load_with(dir.path(), KeySource::File).unwrap();
        assert_eq!(second.decrypt(&envelope).unwrap(), "persisted");
    }

    #[test]
    fn keyring_round_trips_and_migrates_the_key_file() {
        let dir = tempfile::tempdir().unwrap();
        // Simulate a pre-keyring install: a key file, no credential-store entry.
        let existing = SecureStore::load_with(dir.path(), KeySource::File).unwrap();
        let envelope = existing.encrypt("legacy-secret").unwrap();
        assert!(dir.path().join(KEY_FILE).exists());

        // Loading through the keyring migrates the key and removes the file.
        let migrated = SecureStore::load(dir.path()).unwrap();
        assert_eq!(migrated.decrypt(&envelope).unwrap(), "legacy-secret");
        assert!(!dir.path().join(KEY_FILE).exists());

        // A second load reads the key back from the credential store.
        let reloaded = SecureStore::load(dir.path()).unwrap();
        assert_eq!(reloaded.decrypt(&envelope).unwrap(), "legacy-secret");

        delete_keyring_entry(dir.path());
    }

    #[test]
    fn keyring_generates_and_reuses_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let first = SecureStore::load(dir.path()).unwrap();
        let envelope = first.encrypt("fresh").unwrap();
        let second = SecureStore::load(dir.path()).unwrap();
        assert_eq!(second.decrypt(&envelope).unwrap(), "fresh");
        delete_keyring_entry(dir.path());
    }
}
