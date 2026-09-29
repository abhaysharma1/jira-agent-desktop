use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::domain::{AgentEvent, AgentRun};
use crate::opencode::ManagedServer;

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
}

impl AppState {
    pub fn load(data_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        let connection = crate::db::open(&data_dir.join("jira-agent.db"))?;
        let _ = crate::db::import_json_if_empty(&connection, &data_dir);
        crate::db::recover_interrupted(&connection)?;
        Ok(Self {
            data_dir,
            db: Mutex::new(connection),
            agents: Mutex::new(HashMap::new()),
        })
    }
}
