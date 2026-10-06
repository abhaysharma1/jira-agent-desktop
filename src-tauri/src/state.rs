use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::cloud::socket::CloudSocketHandle;
use crate::domain::{AgentEvent, AgentRun};
use crate::opencode::ManagedServer;
use crate::secure_store::{KeySource, SecureStore};

pub struct AgentRuntime {
    pub run: AgentRun,
    pub events: Vec<AgentEvent>,
    pub output: String,
    pub cost: f64,
    pub model: Option<String>,
    pub server: ManagedServer,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub db: Mutex<Connection>,
    pub agents: Mutex<HashMap<String, AgentRuntime>>,
    /// Stop flag and live stream for the background cloud socket, if running.
    pub cloud_socket: Mutex<Option<CloudSocketHandle>>,
    /// Encrypts/decrypts credential settings; key held in the OS credential store.
    pub secure_store: SecureStore,
}

impl AppState {
    pub fn load(data_dir: PathBuf) -> Result<Self, String> {
        Self::load_with_key_source(data_dir, KeySource::OsKeyring)
    }

    /// Test/fallback entry point that lets the caller choose where the
    /// encryption key lives (tests use the file source so they never touch the
    /// real credential store).
    pub fn load_with_key_source(data_dir: PathBuf, source: KeySource) -> Result<Self, String> {
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        let connection = crate::db::open(&data_dir.join("jira-agent.db"))?;
        let _ = crate::db::import_json_if_empty(&connection, &data_dir);
        crate::db::recover_interrupted(&connection)?;
        let secure_store = match source {
            KeySource::OsKeyring => SecureStore::load(&data_dir)?,
            KeySource::File => SecureStore::load_with(&data_dir, KeySource::File)?,
        };
        let state = Self {
            data_dir,
            db: Mutex::new(connection),
            agents: Mutex::new(HashMap::new()),
            cloud_socket: Mutex::new(None),
            secure_store,
        };
        // Encrypt any secrets left in plaintext by earlier releases.
        crate::secrets::migrate_plaintext(&state)?;
        Ok(state)
    }
}
