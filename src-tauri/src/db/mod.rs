use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::domain::{
    AgentEvent, AgentRun, AgentTask, JiraProjectRepo, JiraSync, Metric, OpencodeServer, Plan,
    PlanApproval, PlanContent, PlanMessage, PlanVersion, PullRequest, Repository, SearchHit,
    TaskStatus, TestRun, ValidationConfig, ValidationResult, ValidationRun, Workspace,
};

const SCHEMA_VERSION: i32 = 8;

pub fn open(path: &Path) -> Result<Connection, String> {
    let connection = Connection::open(path).map_err(|error| error.to_string())?;
    connection
        .execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
        .map_err(|error| error.to_string())?;
    migrate(&connection)?;
    Ok(connection)
}

fn migrate(connection: &Connection) -> Result<(), String> {
    let version: i32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }

    connection
        .execute_batch(
            r#"
CREATE TABLE IF NOT EXISTS repositories (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    local_path TEXT NOT NULL,
    remote_url TEXT NOT NULL,
    default_branch TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    path TEXT NOT NULL,
    branch_name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    jira_issue_key TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    status TEXT NOT NULL,
    plan_version INTEGER,
    approved_plan_version INTEGER,
    workspace_path TEXT,
    branch_name TEXT,
    commit_hash TEXT,
    interrupted_from TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS plans (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    current_version INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS plan_versions (
    id TEXT PRIMARY KEY,
    plan_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    content_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    created_by TEXT NOT NULL,
    approved INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_plan_versions_task ON plan_versions(task_id);

CREATE TABLE IF NOT EXISTS plan_approvals (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    plan_version INTEGER NOT NULL,
    user_id TEXT NOT NULL,
    approved_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS plan_messages (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    plan_version INTEGER
);

CREATE TABLE IF NOT EXISTS agent_runs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    mode TEXT NOT NULL,
    agent TEXT,
    status TEXT NOT NULL,
    session_id TEXT,
    model TEXT,
    cost REAL NOT NULL DEFAULT 0,
    output TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE TABLE IF NOT EXISTS agent_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_events_run ON agent_events(run_id);

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pull_requests (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    number INTEGER,
    url TEXT NOT NULL,
    branch TEXT NOT NULL,
    base_branch TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS validation_configs (
    repository_id TEXT PRIMARY KEY,
    test TEXT,
    lint TEXT,
    build TEXT
);

CREATE TABLE IF NOT EXISTS test_runs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    command TEXT NOT NULL,
    passed INTEGER NOT NULL,
    exit_code INTEGER NOT NULL,
    output TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_test_runs_task ON test_runs(task_id);

CREATE TABLE IF NOT EXISTS validation_runs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    passed INTEGER NOT NULL,
    results_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_validation_runs_task ON validation_runs(task_id);

CREATE TABLE IF NOT EXISTS jira_issue_sync (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL UNIQUE,
    jira_issue_key TEXT NOT NULL,
    jira_issue_id TEXT,
    pr_number INTEGER,
    last_action TEXT,
    last_synced_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS jira_project_repos (
    id TEXT PRIMARY KEY,
    project_key TEXT NOT NULL UNIQUE,
    repository_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS opencode_servers (
    run_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    pid INTEGER NOT NULL,
    port INTEGER NOT NULL,
    password TEXT NOT NULL,
    started_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS metrics (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    value REAL NOT NULL,
    task_id TEXT,
    dims_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_metrics_name ON metrics(name, created_at);
"#,
        )
        .map_err(|error| error.to_string())?;

    // Phase 20 replaced API-token auth with OAuth; drop the legacy connection.
    connection
        .execute("DELETE FROM settings WHERE key = 'jiraConnection'", [])
        .map_err(|error| error.to_string())?;

    if !column_exists(&connection, "tasks", "commit_hash")? {
        connection
            .execute("ALTER TABLE tasks ADD COLUMN commit_hash TEXT", [])
            .map_err(|error| error.to_string())?;
    }

    if !column_exists(&connection, "tasks", "interrupted_from")? {
        connection
            .execute("ALTER TABLE tasks ADD COLUMN interrupted_from TEXT", [])
            .map_err(|error| error.to_string())?;
    }

    connection
        .execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let name: String = row.get(1).map_err(|error| error.to_string())?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Recovers tasks that were mid-run when the app last exited (cleanly or not).
/// They become `INTERRUPTED` with the stage recorded in `interrupted_from`, so
/// the UI can offer a Resume that restarts exactly that stage. Running agent
/// runs are closed out as failed.
pub fn recover_interrupted(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE tasks
             SET interrupted_from = status, status = 'INTERRUPTED', updated_at = ?1
             WHERE status IN (
                'PLANNING', 'APPROVED', 'WORKSPACE_CREATING', 'IMPLEMENTING', 'TESTING',
                'REPAIRING', 'VALIDATING', 'COMMITTING', 'PR_CREATING'
             )",
            params![now()],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE agent_runs SET status = 'FAILED', completed_at = ?1
             WHERE status IN ('PENDING', 'RUNNING')",
            params![now()],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn import_json_if_empty(connection: &Connection, data_dir: &Path) -> Result<(), String> {
    import_table::<Repository>(connection, "repositories", data_dir, "repositories.json")?;
    import_table::<Workspace>(connection, "workspaces", data_dir, "workspaces.json")?;
    import_table::<AgentTask>(connection, "tasks", data_dir, "tasks.json")?;
    import_table::<Plan>(connection, "plans", data_dir, "plans.json")?;
    import_table::<PlanVersion>(connection, "plan_versions", data_dir, "plan_versions.json")?;
    import_table::<PlanApproval>(connection, "plan_approvals", data_dir, "plan_approvals.json")?;
    import_table::<PlanMessage>(connection, "plan_messages", data_dir, "plan_messages.json")?;
    Ok(())
}

fn import_table<T: DeserializeOwned + Importable>(
    connection: &Connection,
    table: &str,
    data_dir: &Path,
    file: &str,
) -> Result<(), String> {
    let count: i64 = connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if count > 0 {
        return Ok(());
    }
    let path = data_dir.join(file);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let Ok(items) = serde_json::from_str::<Vec<T>>(&raw) else {
        return Ok(());
    };
    for item in &items {
        item.insert(connection)?;
    }
    Ok(())
}

trait Importable {
    fn insert(&self, connection: &Connection) -> Result<(), String>;
}

fn enum_to_string<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn enum_from_str<T: DeserializeOwned>(value: &str) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(value.to_string()))
        .map_err(|error| error.to_string())
}

fn json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

fn from_json<T: DeserializeOwned>(value: &str) -> Result<T, String> {
    serde_json::from_str(value).map_err(|error| error.to_string())
}

// ---------------------------------------------------------------- repositories

impl Importable for Repository {
    fn insert(&self, connection: &Connection) -> Result<(), String> {
        insert_repository(connection, self)
    }
}

pub fn insert_repository(connection: &Connection, repository: &Repository) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO repositories
             (id, name, local_path, remote_url, default_branch, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                repository.id,
                repository.name,
                repository.local_path,
                repository.remote_url,
                repository.default_branch,
                repository.created_at,
                repository.updated_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_repository(row: &Row) -> rusqlite::Result<Repository> {
    Ok(Repository {
        id: row.get(0)?,
        name: row.get(1)?,
        local_path: row.get(2)?,
        remote_url: row.get(3)?,
        default_branch: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn list_repositories(connection: &Connection) -> Result<Vec<Repository>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, name, local_path, remote_url, default_branch, created_at, updated_at
             FROM repositories ORDER BY created_at",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], row_to_repository)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

pub fn get_repository(connection: &Connection, id: &str) -> Result<Option<Repository>, String> {
    connection
        .query_row(
            "SELECT id, name, local_path, remote_url, default_branch, created_at, updated_at
             FROM repositories WHERE id = ?1",
            params![id],
            row_to_repository,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn remove_repository(connection: &Connection, id: &str) -> Result<(), String> {
    connection
        .execute("DELETE FROM repositories WHERE id = ?1", params![id])
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- workspaces

impl Importable for Workspace {
    fn insert(&self, connection: &Connection) -> Result<(), String> {
        insert_workspace(connection, self)
    }
}

pub fn insert_workspace(connection: &Connection, workspace: &Workspace) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO workspaces
             (id, task_id, repository_id, path, branch_name, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                workspace.id,
                workspace.task_id,
                workspace.repository_id,
                workspace.path,
                workspace.branch_name,
                workspace.created_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_workspace(row: &Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        task_id: row.get(1)?,
        repository_id: row.get(2)?,
        path: row.get(3)?,
        branch_name: row.get(4)?,
        created_at: row.get(5)?,
    })
}

pub fn list_workspaces(connection: &Connection) -> Result<Vec<Workspace>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, task_id, repository_id, path, branch_name, created_at
             FROM workspaces ORDER BY created_at",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], row_to_workspace)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

pub fn get_workspace_by_task(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<Workspace>, String> {
    connection
        .query_row(
            "SELECT id, task_id, repository_id, path, branch_name, created_at
             FROM workspaces WHERE task_id = ?1",
            params![task_id],
            row_to_workspace,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn remove_workspace_by_task(connection: &Connection, task_id: &str) -> Result<(), String> {
    connection
        .execute("DELETE FROM workspaces WHERE task_id = ?1", params![task_id])
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- tasks

impl Importable for AgentTask {
    fn insert(&self, connection: &Connection) -> Result<(), String> {
        insert_task(connection, self)
    }
}

pub fn insert_task(connection: &Connection, task: &AgentTask) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO tasks
             (id, jira_issue_key, title, description, repository_id, status, plan_version,
              approved_plan_version, workspace_path, branch_name, commit_hash, interrupted_from,
              created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                task.id,
                task.jira_issue_key,
                task.title,
                task.description,
                task.repository_id,
                enum_to_string(&task.status),
                task.plan_version,
                task.approved_plan_version,
                task.workspace_path,
                task.branch_name,
                task.commit_hash,
                task.interrupted_from,
                task.created_at,
                task.updated_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_task(row: &Row) -> rusqlite::Result<AgentTask> {
    let status: String = row.get(5)?;
    Ok(AgentTask {
        id: row.get(0)?,
        jira_issue_key: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        repository_id: row.get(4)?,
        status: enum_from_str(&status).map_err(|_| rusqlite::Error::InvalidQuery)?,
        plan_version: row.get(6)?,
        approved_plan_version: row.get(7)?,
        workspace_path: row.get(8)?,
        branch_name: row.get(9)?,
        commit_hash: row.get(10)?,
        interrupted_from: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

pub fn list_tasks(connection: &Connection) -> Result<Vec<AgentTask>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, jira_issue_key, title, description, repository_id, status, plan_version,
                    approved_plan_version, workspace_path, branch_name, commit_hash, interrupted_from,
                    created_at, updated_at
             FROM tasks ORDER BY created_at DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], row_to_task)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

pub fn get_task(connection: &Connection, id: &str) -> Result<Option<AgentTask>, String> {
    connection
        .query_row(
            "SELECT id, jira_issue_key, title, description, repository_id, status, plan_version,
                    approved_plan_version, workspace_path, branch_name, commit_hash, interrupted_from,
                    created_at, updated_at
             FROM tasks WHERE id = ?1",
            params![id],
            row_to_task,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn update_task_status(
    connection: &Connection,
    task_id: &str,
    status: TaskStatus,
    updated_at: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![enum_to_string(&status), updated_at, task_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Moves a task to `status` and clears the crash marker. Used by resume.
pub fn set_task_running_state(
    connection: &Connection,
    task_id: &str,
    status: TaskStatus,
    updated_at: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE tasks SET status = ?1, interrupted_from = NULL, updated_at = ?2 WHERE id = ?3",
            params![enum_to_string(&status), updated_at, task_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn set_task_workspace(
    connection: &Connection,
    task_id: &str,
    path: &str,
    branch: &str,
    updated_at: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE tasks SET workspace_path = ?1, branch_name = ?2, updated_at = ?3 WHERE id = ?4",
            params![path, branch, updated_at, task_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn set_task_commit(
    connection: &Connection,
    task_id: &str,
    commit_hash: &str,
    updated_at: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE tasks SET commit_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![commit_hash, updated_at, task_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- plans & versions

impl Importable for Plan {
    fn insert(&self, connection: &Connection) -> Result<(), String> {
        connection
            .execute(
                "INSERT OR REPLACE INTO plans (id, task_id, current_version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    self.id,
                    self.task_id,
                    self.current_version,
                    self.created_at,
                    self.updated_at
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

impl Importable for PlanVersion {
    fn insert(&self, connection: &Connection) -> Result<(), String> {
        insert_plan_version(connection, self)
    }
}

pub fn insert_plan_version(
    connection: &Connection,
    version: &PlanVersion,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO plan_versions
             (id, plan_id, task_id, version, content_json, created_at, created_by, approved)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                version.id,
                version.plan_id,
                version.task_id,
                version.version,
                json(&version.content)?,
                version.created_at,
                version.created_by,
                version.approved as i64,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_plan_version(row: &Row) -> rusqlite::Result<PlanVersion> {
    let content: String = row.get(4)?;
    Ok(PlanVersion {
        id: row.get(0)?,
        plan_id: row.get(1)?,
        task_id: row.get(2)?,
        version: row.get(3)?,
        content: from_json(&content).map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(5)?,
        created_by: row.get(6)?,
        approved: row.get::<_, i64>(7)? != 0,
    })
}

pub fn list_plan_versions(
    connection: &Connection,
    task_id: &str,
) -> Result<Vec<PlanVersion>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, plan_id, task_id, version, content_json, created_at, created_by, approved
             FROM plan_versions WHERE task_id = ?1 ORDER BY version",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![task_id], row_to_plan_version)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

#[allow(dead_code)]
pub fn get_plan_version(
    connection: &Connection,
    task_id: &str,
    version: u32,
) -> Result<Option<PlanVersion>, String> {
    connection
        .query_row(
            "SELECT id, plan_id, task_id, version, content_json, created_at, created_by, approved
             FROM plan_versions WHERE task_id = ?1 AND version = ?2",
            params![task_id, version],
            row_to_plan_version,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn latest_plan_version(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<PlanVersion>, String> {
    connection
        .query_row(
            "SELECT id, plan_id, task_id, version, content_json, created_at, created_by, approved
             FROM plan_versions WHERE task_id = ?1 ORDER BY version DESC LIMIT 1",
            params![task_id],
            row_to_plan_version,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn latest_plan_content(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<(u32, PlanContent)>, String> {
    Ok(latest_plan_version(connection, task_id)?.map(|version| (version.version, version.content)))
}

pub fn store_plan_version(
    connection: &Connection,
    task_id: &str,
    content: PlanContent,
    created_by: &str,
    timestamp: &str,
) -> Result<(Plan, PlanVersion), String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;

    let next: u32 = transaction
        .query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM plan_versions WHERE task_id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;

    let plan = match transaction
        .query_row(
            "SELECT id, task_id, current_version, created_at, updated_at
             FROM plans WHERE task_id = ?1",
            params![task_id],
            |row| {
                Ok(Plan {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    current_version: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
    {
        Some(mut plan) => {
            plan.current_version = next;
            plan.updated_at = timestamp.to_string();
            plan
        }
        None => Plan {
            id: uuid::Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            current_version: next,
            created_at: timestamp.to_string(),
            updated_at: timestamp.to_string(),
        },
    };

    transaction
        .execute(
            "INSERT OR REPLACE INTO plans (id, task_id, current_version, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                plan.id,
                plan.task_id,
                plan.current_version,
                plan.created_at,
                plan.updated_at
            ],
        )
        .map_err(|error| error.to_string())?;

    let version = PlanVersion {
        id: uuid::Uuid::new_v4().to_string(),
        plan_id: plan.id.clone(),
        task_id: task_id.to_string(),
        version: next,
        content,
        created_at: timestamp.to_string(),
        created_by: created_by.to_string(),
        approved: false,
    };
    insert_plan_version(&transaction, &version)?;

    transaction
        .execute(
            "UPDATE tasks SET plan_version = ?1, updated_at = ?2 WHERE id = ?3",
            params![next, timestamp, task_id],
        )
        .map_err(|error| error.to_string())?;

    transaction.commit().map_err(|error| error.to_string())?;
    Ok((plan, version))
}

// ---------------------------------------------------------------- approvals

impl Importable for PlanApproval {
    fn insert(&self, connection: &Connection) -> Result<(), String> {
        insert_plan_approval(connection, self)
    }
}

pub fn insert_plan_approval(
    connection: &Connection,
    approval: &PlanApproval,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO plan_approvals (id, task_id, plan_version, user_id, approved_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                approval.id,
                approval.task_id,
                approval.plan_version,
                approval.user_id,
                approval.approved_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_approval(row: &Row) -> rusqlite::Result<PlanApproval> {
    Ok(PlanApproval {
        id: row.get(0)?,
        task_id: row.get(1)?,
        plan_version: row.get(2)?,
        user_id: row.get(3)?,
        approved_at: row.get(4)?,
    })
}

pub fn latest_plan_approval(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<PlanApproval>, String> {
    connection
        .query_row(
            "SELECT id, task_id, plan_version, user_id, approved_at
             FROM plan_approvals WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
            params![task_id],
            row_to_approval,
        )
        .optional()
        .map_err(|error| error.to_string())
}

pub fn approve_plan(
    connection: &Connection,
    task_id: &str,
    plan_version: Option<u32>,
    user_id: &str,
    timestamp: &str,
) -> Result<PlanApproval, String> {
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;

    let task = transaction
        .query_row(
            "SELECT id, jira_issue_key, title, description, repository_id, status, plan_version,
                    approved_plan_version, workspace_path, branch_name, commit_hash, interrupted_from,
                    created_at, updated_at
             FROM tasks WHERE id = ?1",
            params![task_id],
            row_to_task,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("task not found: {task_id}"))?;

    if !task.status.can_transition_to(TaskStatus::Approved) {
        return Err(format!(
            "task {task_id} cannot be approved from status {:?}",
            task.status
        ));
    }

    let version_number = plan_version
        .or(task.plan_version)
        .ok_or_else(|| "task has no plan version to approve".to_string())?;

    let exists: Option<i64> = transaction
        .query_row(
            "SELECT 1 FROM plan_versions WHERE task_id = ?1 AND version = ?2",
            params![task_id, version_number],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if exists.is_none() {
        return Err(format!("plan version {version_number} not found"));
    }

    transaction
        .execute(
            "UPDATE plan_versions SET approved = 1 WHERE task_id = ?1 AND version = ?2",
            params![task_id, version_number],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE tasks SET status = 'APPROVED', approved_plan_version = ?1, updated_at = ?2
             WHERE id = ?3",
            params![version_number, timestamp, task_id],
        )
        .map_err(|error| error.to_string())?;

    let approval = PlanApproval {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: task_id.to_string(),
        plan_version: version_number,
        user_id: user_id.to_string(),
        approved_at: timestamp.to_string(),
    };
    insert_plan_approval(&transaction, &approval)?;

    transaction.commit().map_err(|error| error.to_string())?;
    Ok(approval)
}

pub fn reject_plan(
    connection: &Connection,
    task_id: &str,
    timestamp: &str,
) -> Result<AgentTask, String> {
    let task = get_task(connection, task_id)?
        .ok_or_else(|| format!("task not found: {task_id}"))?;

    if !task.status.can_transition_to(TaskStatus::Rejected) {
        return Err(format!(
            "task {task_id} cannot be rejected from status {:?}",
            task.status
        ));
    }

    update_task_status(connection, task_id, TaskStatus::Rejected, timestamp)?;
    get_task(connection, task_id)?.ok_or_else(|| format!("task not found: {task_id}"))
}

// ---------------------------------------------------------------- messages

impl Importable for PlanMessage {
    fn insert(&self, connection: &Connection) -> Result<(), String> {
        insert_plan_message(connection, self)
    }
}

pub fn insert_plan_message(
    connection: &Connection,
    message: &PlanMessage,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO plan_messages (id, task_id, role, content, created_at, plan_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                message.id,
                message.task_id,
                message.role,
                message.content,
                message.created_at,
                message.plan_version,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_message(row: &Row) -> rusqlite::Result<PlanMessage> {
    Ok(PlanMessage {
        id: row.get(0)?,
        task_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        created_at: row.get(4)?,
        plan_version: row.get(5)?,
    })
}

pub fn list_plan_messages(
    connection: &Connection,
    task_id: &str,
) -> Result<Vec<PlanMessage>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, task_id, role, content, created_at, plan_version
             FROM plan_messages WHERE task_id = ?1 ORDER BY rowid",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![task_id], row_to_message)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------- agent runs

pub fn upsert_agent_run(
    connection: &Connection,
    run: &AgentRun,
    output: &str,
    cost: f64,
    model: Option<&str>,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO agent_runs
             (id, task_id, mode, agent, status, session_id, model, cost, output, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                run.id,
                run.task_id,
                run.mode,
                run.agent,
                enum_to_string(&run.status),
                run.session_id,
                model,
                cost,
                output,
                run.started_at,
                run.completed_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn insert_agent_event(
    connection: &Connection,
    run_id: &str,
    event: &AgentEvent,
    timestamp: &str,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO agent_events (run_id, payload_json, created_at) VALUES (?1, ?2, ?3)",
            params![run_id, json(event)?, timestamp],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- validation & tests

pub fn get_validation_config(
    connection: &Connection,
    repository_id: &str,
) -> Result<ValidationConfig, String> {
    connection
        .query_row(
            "SELECT test, lint, build FROM validation_configs WHERE repository_id = ?1",
            params![repository_id],
            |row| {
                Ok(ValidationConfig {
                    test: row.get(0)?,
                    lint: row.get(1)?,
                    build: row.get(2)?,
                })
            },
        )
        .optional()
        .map(|config| config.unwrap_or_default())
        .map_err(|error| error.to_string())
}

pub fn set_validation_config(
    connection: &Connection,
    repository_id: &str,
    config: &ValidationConfig,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO validation_configs (repository_id, test, lint, build)
             VALUES (?1, ?2, ?3, ?4)",
            params![repository_id, config.test, config.lint, config.build],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn max_repair_attempts(connection: &Connection) -> u32 {
    connection
        .query_row(
            "SELECT value_json FROM settings WHERE key = 'maxRepairAttempts'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| value.as_u64())
        .map(|value| value as u32)
        .filter(|value| *value > 0)
        .unwrap_or(5)
}

pub fn insert_test_run(connection: &Connection, run: &TestRun) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO test_runs
             (id, task_id, attempt, command, passed, exit_code, output, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run.id,
                run.task_id,
                run.attempt,
                run.command,
                run.passed as i64,
                run.exit_code,
                run.output,
                run.created_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_test_run(row: &Row) -> rusqlite::Result<TestRun> {
    Ok(TestRun {
        id: row.get(0)?,
        task_id: row.get(1)?,
        attempt: row.get(2)?,
        command: row.get(3)?,
        passed: row.get::<_, i64>(4)? != 0,
        exit_code: row.get(5)?,
        output: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub fn list_test_runs(connection: &Connection, task_id: &str) -> Result<Vec<TestRun>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, task_id, attempt, command, passed, exit_code, output, created_at
             FROM test_runs WHERE task_id = ?1 ORDER BY attempt, created_at",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![task_id], row_to_test_run)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

pub fn insert_validation_run(
    connection: &Connection,
    run: &ValidationRun,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO validation_runs (id, task_id, passed, results_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                run.id,
                run.task_id,
                run.passed as i64,
                json(&run.results)?,
                run.created_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_validation_run(row: &Row) -> rusqlite::Result<ValidationRun> {
    let results: String = row.get(3)?;
    Ok(ValidationRun {
        id: row.get(0)?,
        task_id: row.get(1)?,
        passed: row.get::<_, i64>(2)? != 0,
        results: from_json::<Vec<ValidationResult>>(&results)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(4)?,
    })
}

pub fn list_validation_runs(
    connection: &Connection,
    task_id: &str,
) -> Result<Vec<ValidationRun>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, task_id, passed, results_json, created_at
             FROM validation_runs WHERE task_id = ?1 ORDER BY created_at",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![task_id], row_to_validation_run)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

fn row_to_jira_project_repo(row: &Row) -> rusqlite::Result<JiraProjectRepo> {
    Ok(JiraProjectRepo {
        id: row.get(0)?,
        project_key: row.get(1)?,
        repository_id: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

pub fn upsert_jira_project_repo(
    connection: &Connection,
    project_key: &str,
    repository_id: &str,
) -> Result<JiraProjectRepo, String> {
    let timestamp = chrono::Utc::now().to_rfc3339();
    let existing: Option<(String, String)> = connection
        .query_row(
            "SELECT id, created_at FROM jira_project_repos WHERE project_key = ?1",
            params![project_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let (id, created_at) = existing.unwrap_or_else(|| {
        (
            uuid::Uuid::new_v4().to_string(),
            timestamp.clone(),
        )
    });
    connection
        .execute(
            "INSERT OR REPLACE INTO jira_project_repos
             (id, project_key, repository_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, project_key, repository_id, created_at, timestamp],
        )
        .map_err(|error| error.to_string())?;
    Ok(JiraProjectRepo {
        id,
        project_key: project_key.to_string(),
        repository_id: repository_id.to_string(),
        created_at,
        updated_at: timestamp,
    })
}

pub fn list_jira_project_repos(connection: &Connection) -> Result<Vec<JiraProjectRepo>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, project_key, repository_id, created_at, updated_at
             FROM jira_project_repos ORDER BY project_key",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], row_to_jira_project_repo)
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

pub fn delete_jira_project_repo(connection: &Connection, project_key: &str) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM jira_project_repos WHERE project_key = ?1",
            params![project_key],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[allow(dead_code)] // used by ticket intake in a later phase
pub fn find_repository_for_project(connection: &Connection, project_key: &str) -> Option<String> {
    connection
        .query_row(
            "SELECT repository_id FROM jira_project_repos WHERE project_key = ?1",
            params![project_key],
            |row| row.get::<_, String>(0),
        )
        .ok()
}

pub fn get_setting(connection: &Connection, key: &str) -> Option<serde_json::Value> {
    connection
        .query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

pub fn set_setting(
    connection: &Connection,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO settings (key, value_json) VALUES (?1, ?2)",
            params![key, json(value)?],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn repair_on_validation_failure(connection: &Connection) -> bool {
    get_setting(connection, "repairOnValidationFailure")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Whether the window's close button hides to the tray instead of quitting.
/// Defaults to true so the tray is reachable the first time the app runs.
pub fn close_to_tray(connection: &Connection) -> bool {
    get_setting(connection, "closeToTray")
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

// ------------------------------------------------------------ opencode servers

pub fn record_opencode_server(
    connection: &Connection,
    server: &OpencodeServer,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO opencode_servers
             (run_id, task_id, pid, port, password, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                server.run_id,
                server.task_id,
                server.pid,
                server.port,
                server.password,
                server.started_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn delete_opencode_server(connection: &Connection, run_id: &str) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM opencode_servers WHERE run_id = ?1",
            params![run_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Rewrites the stored server password (used to encrypt legacy plaintext rows).
pub fn update_opencode_server_password(
    connection: &Connection,
    run_id: &str,
    password: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE opencode_servers SET password = ?1 WHERE run_id = ?2",
            params![password, run_id],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn list_opencode_servers(connection: &Connection) -> Result<Vec<OpencodeServer>, String> {
    let mut statement = connection
        .prepare("SELECT run_id, task_id, pid, port, password, started_at FROM opencode_servers")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(OpencodeServer {
                run_id: row.get(0)?,
                task_id: row.get(1)?,
                pid: row.get(2)?,
                port: row.get(3)?,
                password: row.get(4)?,
                started_at: row.get(5)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

pub fn insert_pull_request(connection: &Connection, pull_request: &PullRequest) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO pull_requests
             (id, task_id, provider, number, url, branch, base_branch, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                pull_request.id,
                pull_request.task_id,
                pull_request.provider,
                pull_request.number,
                pull_request.url,
                pull_request.branch,
                pull_request.base_branch,
                pull_request.status,
                pull_request.created_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_pull_request(row: &Row) -> rusqlite::Result<PullRequest> {
    Ok(PullRequest {
        id: row.get(0)?,
        task_id: row.get(1)?,
        provider: row.get(2)?,
        number: row.get(3)?,
        url: row.get(4)?,
        branch: row.get(5)?,
        base_branch: row.get(6)?,
        status: row.get(7)?,
        created_at: row.get(8)?,
    })
}

pub fn get_pull_request(
    connection: &Connection,
    task_id: &str,
) -> Result<Option<PullRequest>, String> {
    connection
        .query_row(
            "SELECT id, task_id, provider, number, url, branch, base_branch, status, created_at
             FROM pull_requests WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
            params![task_id],
            row_to_pull_request,
        )
        .optional()
        .map_err(|error| error.to_string())
}

// ------------------------------------------------------------------- metrics

pub fn insert_metric(connection: &Connection, metric: &Metric) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO metrics (id, name, value, task_id, dims_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                metric.id,
                metric.name,
                metric.value,
                metric.task_id,
                json(&metric.dims)?,
                metric.created_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_metric(row: &Row) -> rusqlite::Result<Metric> {
    let dims: String = row.get(4)?;
    Ok(Metric {
        id: row.get(0)?,
        name: row.get(1)?,
        value: row.get(2)?,
        task_id: row.get(3)?,
        dims: from_json::<serde_json::Value>(&dims).unwrap_or(serde_json::Value::Null),
        created_at: row.get(5)?,
    })
}

/// Newest first. `name` filters to a single metric; `None` lists everything.
pub fn list_metrics(
    connection: &Connection,
    name: Option<&str>,
    limit: usize,
) -> Result<Vec<Metric>, String> {
    let limit = limit as i64;
    let sql = match name {
        Some(_) => {
            "SELECT id, name, value, task_id, dims_json, created_at FROM metrics
             WHERE name = ?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2"
        }
        None => {
            "SELECT id, name, value, task_id, dims_json, created_at FROM metrics
             ORDER BY created_at DESC, rowid DESC LIMIT ?1"
        }
    };
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let map_row = |row: &Row| row_to_metric(row);
    let rows = match name {
        Some(name) => statement
            .query_map(params![name, limit], map_row)
            .map_err(|error| error.to_string())?,
        None => statement
            .query_map(params![limit], map_row)
            .map_err(|error| error.to_string())?,
    };
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

/// Deletes everything but the newest `keep` rows, oldest first.
pub fn prune_metrics(connection: &Connection, keep: i64) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM metrics WHERE id NOT IN (
                SELECT id FROM metrics ORDER BY created_at DESC, rowid DESC LIMIT ?1
             )",
            params![keep.max(0)],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

// --------------------------------------------------------------------- search

/// Case-insensitive (ASCII) substring search across everything the command
/// palette can open: tickets, plans, pull requests and agent runs. Each source
/// is limited independently so one noisy category cannot crowd out the rest.
pub fn search(
    connection: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, String> {
    let query = query.trim();
    if query.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let like = format!("%{query}%");
    let limit = limit as i64;
    let mut hits = Vec::new();

    // Tickets — an exact issue-key match ranks above a fuzzy title match.
    {
        let mut statement = connection
            .prepare(
                "SELECT id, jira_issue_key, title FROM tasks
                 WHERE jira_issue_key LIKE ?1 OR title LIKE ?1 OR description LIKE ?1
                 ORDER BY CASE WHEN jira_issue_key = ?2 THEN 0 ELSE 1 END, updated_at DESC
                 LIMIT ?3",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![like, query, limit], |row| {
                let id: String = row.get(0)?;
                let key: String = row.get(1)?;
                let title: String = row.get(2)?;
                Ok(SearchHit {
                    kind: "ticket".to_string(),
                    title: format!("{key}: {title}"),
                    subtitle: "Ticket".to_string(),
                    route: format!("/tasks/{id}"),
                    task_id: id,
                })
            })
            .map_err(|error| error.to_string())?;
        hits.extend(
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| error.to_string())?,
        );
    }

    // Plans — the newest version per task, summarised from its content JSON.
    {
        let mut statement = connection
            .prepare(
                "SELECT pv.task_id, t.jira_issue_key, pv.content_json, pv.version
                 FROM plan_versions pv
                 LEFT JOIN tasks t ON t.id = pv.task_id
                 WHERE pv.content_json LIKE ?1
                 GROUP BY pv.task_id
                 ORDER BY pv.created_at DESC
                 LIMIT ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![like, limit], |row| {
                let task_id: String = row.get(0)?;
                let key: Option<String> = row.get(1)?;
                let content: String = row.get(2)?;
                let version: u32 = row.get(3)?;
                let summary = serde_json::from_str::<PlanContent>(&content)
                    .map(|plan| plan.summary)
                    .unwrap_or_else(|_| content.chars().take(60).collect());
                Ok(SearchHit {
                    kind: "plan".to_string(),
                    title: format!(
                        "{}: {summary}",
                        key.unwrap_or_else(|| "Plan".to_string())
                    ),
                    subtitle: format!("Plan v{version}"),
                    route: format!("/tasks/{task_id}"),
                    task_id,
                })
            })
            .map_err(|error| error.to_string())?;
        hits.extend(
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| error.to_string())?,
        );
    }

    // Pull requests.
    {
        let mut statement = connection
            .prepare(
                "SELECT pr.task_id, t.jira_issue_key, pr.number, pr.url, pr.status
                 FROM pull_requests pr
                 LEFT JOIN tasks t ON t.id = pr.task_id
                 WHERE t.jira_issue_key LIKE ?1 OR pr.url LIKE ?1 OR pr.branch LIKE ?1
                    OR CAST(pr.number AS TEXT) LIKE ?1
                 ORDER BY pr.created_at DESC
                 LIMIT ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![like, limit], |row| {
                let task_id: String = row.get(0)?;
                let key: Option<String> = row.get(1)?;
                let number: Option<i64> = row.get(2)?;
                let url: String = row.get(3)?;
                let status: String = row.get(4)?;
                let label = number
                    .map(|number| format!("PR #{number}"))
                    .unwrap_or_else(|| url.clone());
                Ok(SearchHit {
                    kind: "pull_request".to_string(),
                    title: format!(
                        "{label} · {}",
                        key.unwrap_or_else(|| "pull request".to_string())
                    ),
                    subtitle: format!("Pull request · {status}"),
                    route: format!("/tasks/{task_id}"),
                    task_id,
                })
            })
            .map_err(|error| error.to_string())?;
        hits.extend(
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| error.to_string())?,
        );
    }

    // Agent runs.
    {
        let mut statement = connection
            .prepare(
                "SELECT ar.task_id, t.jira_issue_key, ar.mode, ar.status
                 FROM agent_runs ar
                 LEFT JOIN tasks t ON t.id = ar.task_id
                 WHERE ar.output LIKE ?1 OR ar.model LIKE ?1 OR ar.mode LIKE ?1
                    OR t.jira_issue_key LIKE ?1
                 ORDER BY ar.started_at DESC
                 LIMIT ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![like, limit], |row| {
                let task_id: String = row.get(0)?;
                let key: Option<String> = row.get(1)?;
                let mode: String = row.get(2)?;
                let status: String = row.get(3)?;
                Ok(SearchHit {
                    kind: "agent_run".to_string(),
                    title: format!(
                        "{mode} · {}",
                        key.unwrap_or_else(|| "agent run".to_string())
                    ),
                    subtitle: format!("Agent run · {status}"),
                    route: format!("/tasks/{task_id}/run"),
                    task_id,
                })
            })
            .map_err(|error| error.to_string())?;
        hits.extend(
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| error.to_string())?,
        );
    }

    Ok(hits)
}

pub fn upsert_jira_sync(connection: &Connection, sync: &JiraSync) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO jira_issue_sync
             (id, task_id, jira_issue_key, jira_issue_id, pr_number, last_action, last_synced_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(task_id) DO UPDATE SET
                 jira_issue_key = excluded.jira_issue_key,
                 jira_issue_id = excluded.jira_issue_id,
                 pr_number = excluded.pr_number,
                 last_action = excluded.last_action,
                 last_synced_at = excluded.last_synced_at,
                 updated_at = excluded.updated_at",
            params![
                sync.id,
                sync.task_id,
                sync.jira_issue_key,
                sync.jira_issue_id,
                sync.pr_number,
                sync.last_action,
                sync.last_synced_at,
                sync.created_at,
                sync.updated_at,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn row_to_jira_sync(row: &Row) -> rusqlite::Result<JiraSync> {
    Ok(JiraSync {
        id: row.get(0)?,
        task_id: row.get(1)?,
        jira_issue_key: row.get(2)?,
        jira_issue_id: row.get(3)?,
        pr_number: row.get(4)?,
        last_action: row.get(5)?,
        last_synced_at: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub fn get_jira_sync(connection: &Connection, task_id: &str) -> Result<Option<JiraSync>, String> {
    connection
        .query_row(
            "SELECT id, task_id, jira_issue_key, jira_issue_id, pr_number, last_action, last_synced_at, created_at, updated_at
             FROM jira_issue_sync WHERE task_id = ?1",
            params![task_id],
            row_to_jira_sync,
        )
        .optional()
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PlanContent, PlanStep};

    fn memory() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        migrate(&connection).unwrap();
        connection
    }

    fn sample_task(id: &str, status: TaskStatus) -> AgentTask {
        AgentTask {
            id: id.to_string(),
            jira_issue_key: "CC-1".to_string(),
            title: "t".to_string(),
            description: "d".to_string(),
            repository_id: "repo-1".to_string(),
            status,
            plan_version: None,
            approved_plan_version: None,
            workspace_path: None,
            branch_name: None,
            commit_hash: None,
            interrupted_from: None,
            created_at: "t0".to_string(),
            updated_at: "t0".to_string(),
        }
    }

    fn sample_content() -> PlanContent {
        PlanContent {
            ticket: "CC-1".to_string(),
            summary: "s".to_string(),
            understanding: "u".to_string(),
            steps: vec![PlanStep {
                order: 1,
                description: "d".to_string(),
                files: vec!["a.ts".to_string()],
            }],
            tests: vec!["t".to_string()],
            risks: vec![],
        }
    }

    #[test]
    fn searches_across_tickets_plans_pull_requests_and_runs() {
        let connection = memory();
        insert_task(&connection, &sample_task("task-1", TaskStatus::PlanReady)).unwrap();
        // A distinctive token only the plan summary carries.
        let mut content = sample_content();
        content.summary = "Add pagination to the widget list".to_string();
        store_plan_version(&connection, "task-1", content, "agent", "t1").unwrap();
        insert_pull_request(
            &connection,
            &crate::domain::PullRequest {
                id: "pr-1".to_string(),
                task_id: "task-1".to_string(),
                provider: "github".to_string(),
                number: 42,
                url: "https://github.com/acme/demo/pull/42".to_string(),
                branch: "CC-1-widget-list".to_string(),
                base_branch: "main".to_string(),
                status: "open".to_string(),
                created_at: "t2".to_string(),
            },
        )
        .unwrap();
        upsert_agent_run(
            &connection,
            &crate::domain::AgentRun {
                id: "run-1".to_string(),
                task_id: "task-1".to_string(),
                mode: "plan".to_string(),
                agent: None,
                status: crate::domain::AgentRunStatus::Succeeded,
                session_id: None,
                started_at: "t3".to_string(),
                completed_at: None,
            },
            "finished planning the widget list",
            0.0,
            Some("deepseek/deepseek-chat"),
        )
        .unwrap();

        let kinds = |query: &str| {
            let mut kinds: Vec<String> = search(&connection, query, 10)
                .unwrap()
                .into_iter()
                .map(|hit| hit.kind)
                .collect();
            kinds.sort();
            kinds
        };

        // Every source has a route back to its task.
        for hit in search(&connection, "CC-1", 10).unwrap() {
            assert!(hit.route.starts_with("/tasks/task-1"));
        }

        assert_eq!(
            kinds("CC-1"),
            vec!["agent_run", "plan", "pull_request", "ticket"]
        );
        assert_eq!(kinds("pagination"), vec!["plan"]);
        assert_eq!(kinds("42"), vec!["pull_request"]);
        assert_eq!(kinds("finished planning"), vec!["agent_run"]);
        // Case-insensitive, and matches reduce to the sources that carry it.
        assert_eq!(kinds("WIDGET"), vec!["agent_run", "plan", "pull_request"]);

        assert!(search(&connection, "zzz-nope", 10).unwrap().is_empty());
        assert!(search(&connection, "   ", 10).unwrap().is_empty());

        // The limit is per source, so four categories still each contribute one.
        assert_eq!(search(&connection, "CC-1", 1).unwrap().len(), 4);
    }

    #[test]
    fn close_to_tray_defaults_on_and_persists() {
        let connection = memory();
        assert!(close_to_tray(&connection));
        set_setting(&connection, "closeToTray", &serde_json::json!(false)).unwrap();
        assert!(!close_to_tray(&connection));
    }

    #[test]
    fn stores_and_reads_repository() {
        let connection = memory();
        let repository = Repository {
            id: "repo-1".to_string(),
            name: "demo".to_string(),
            local_path: "C:/demo".to_string(),
            remote_url: String::new(),
            default_branch: "main".to_string(),
            created_at: "t0".to_string(),
            updated_at: "t0".to_string(),
        };
        insert_repository(&connection, &repository).unwrap();
        assert_eq!(list_repositories(&connection).unwrap().len(), 1);
        assert_eq!(get_repository(&connection, "repo-1").unwrap().unwrap().name, "demo");
        remove_repository(&connection, "repo-1").unwrap();
        assert!(list_repositories(&connection).unwrap().is_empty());
    }

    #[test]
    fn creates_incrementing_plan_versions() {
        let connection = memory();
        insert_task(&connection, &sample_task("task-1", TaskStatus::Planning)).unwrap();

        let (plan, v1) =
            store_plan_version(&connection, "task-1", sample_content(), "agent", "t1").unwrap();
        assert_eq!(v1.version, 1);
        assert_eq!(plan.current_version, 1);

        let (_, v2) =
            store_plan_version(&connection, "task-1", sample_content(), "agent", "t2").unwrap();
        assert_eq!(v2.version, 2);
        assert_eq!(latest_plan_version(&connection, "task-1").unwrap().unwrap().version, 2);
        assert_eq!(list_plan_versions(&connection, "task-1").unwrap().len(), 2);
        assert_eq!(get_task(&connection, "task-1").unwrap().unwrap().plan_version, Some(2));
    }

    #[test]
    fn approves_plan_transactionally() {
        let connection = memory();
        insert_task(&connection, &sample_task("task-1", TaskStatus::PlanReady)).unwrap();
        store_plan_version(&connection, "task-1", sample_content(), "agent", "t1").unwrap();

        let approval = approve_plan(&connection, "task-1", None, "local-user", "t2").unwrap();
        assert_eq!(approval.plan_version, 1);
        assert_eq!(get_task(&connection, "task-1").unwrap().unwrap().status, TaskStatus::Approved);
        assert!(get_plan_version(&connection, "task-1", 1).unwrap().unwrap().approved);
        assert_eq!(latest_plan_approval(&connection, "task-1").unwrap().unwrap().user_id, "local-user");
    }

    #[test]
    fn refuses_approval_from_wrong_state() {
        let connection = memory();
        insert_task(&connection, &sample_task("task-1", TaskStatus::Planning)).unwrap();
        store_plan_version(&connection, "task-1", sample_content(), "agent", "t1").unwrap();
        assert!(approve_plan(&connection, "task-1", None, "local-user", "t2").is_err());
    }

    #[test]
    fn recovers_interrupted_runs() {
        let connection = memory();
        insert_task(&connection, &sample_task("task-a", TaskStatus::Implementing)).unwrap();
        insert_task(&connection, &sample_task("task-b", TaskStatus::PlanReady)).unwrap();
        recover_interrupted(&connection).unwrap();
        let tasks = list_tasks(&connection).unwrap();
        let recovered = tasks.iter().find(|task| task.id == "task-a").unwrap();
        assert_eq!(recovered.status, TaskStatus::Interrupted);
        assert_eq!(recovered.interrupted_from.as_deref(), Some("IMPLEMENTING"));
        // a task with a stable status is untouched
        let untouched = tasks.iter().find(|task| task.id == "task-b").unwrap();
        assert_eq!(untouched.status, TaskStatus::PlanReady);
        assert!(untouched.interrupted_from.is_none());
    }

    #[test]
    fn clears_the_interrupt_marker_on_resume() {
        let connection = memory();
        insert_task(&connection, &sample_task("task-a", TaskStatus::Implementing)).unwrap();
        recover_interrupted(&connection).unwrap();
        set_task_running_state(&connection, "task-a", TaskStatus::Approved, "t1").unwrap();
        let task = get_task(&connection, "task-a").unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Approved);
        assert!(task.interrupted_from.is_none());
    }

    #[test]
    fn round_trips_opencode_servers() {
        let connection = memory();
        let server = OpencodeServer {
            run_id: "run-1".to_string(),
            task_id: "task-1".to_string(),
            pid: 4242,
            port: 5132,
            password: "secret".to_string(),
            started_at: "t0".to_string(),
        };
        record_opencode_server(&connection, &server).unwrap();
        let servers = list_opencode_servers(&connection).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].pid, 4242);
        assert_eq!(servers[0].port, 5132);
        delete_opencode_server(&connection, "run-1").unwrap();
        assert!(list_opencode_servers(&connection).unwrap().is_empty());
    }

    #[test]
    fn app_state_load_marks_in_flight_tasks_interrupted() {
        let temp = tempfile::tempdir().unwrap();
        {
            let connection = open(&temp.path().join("jira-agent.db")).unwrap();
            insert_task(&connection, &sample_task("task-a", TaskStatus::Implementing)).unwrap();
        }
        let state = crate::state::AppState::load_with_key_source(
            temp.path().to_path_buf(),
            crate::secure_store::KeySource::File,
        )
        .unwrap();
        let connection = state.db.lock().unwrap();
        let task = get_task(&connection, "task-a").unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Interrupted);
        assert_eq!(task.interrupted_from.as_deref(), Some("IMPLEMENTING"));
    }

    #[test]
    fn stores_workspace_on_task() {
        let connection = memory();
        insert_task(&connection, &sample_task("task-1", TaskStatus::Implementing)).unwrap();
        set_task_workspace(&connection, "task-1", "C:/ws/CC-1", "agent/CC-1", "t1").unwrap();
        let task = get_task(&connection, "task-1").unwrap().unwrap();
        assert_eq!(task.workspace_path.as_deref(), Some("C:/ws/CC-1"));
        assert_eq!(task.branch_name.as_deref(), Some("agent/CC-1"));
    }

    #[test]
    fn stores_validation_config_and_test_runs() {
        let connection = memory();
        let config = ValidationConfig {
            test: Some("npm test".to_string()),
            lint: None,
            build: None,
        };
        set_validation_config(&connection, "repo-1", &config).unwrap();
        assert_eq!(
            get_validation_config(&connection, "repo-1").unwrap().test.as_deref(),
            Some("npm test")
        );
        assert_eq!(get_validation_config(&connection, "missing").unwrap().test, None);
        assert_eq!(max_repair_attempts(&connection), 5);

        insert_task(&connection, &sample_task("task-1", TaskStatus::Testing)).unwrap();
        insert_test_run(
            &connection,
            &TestRun {
                id: "tr1".to_string(),
                task_id: "task-1".to_string(),
                attempt: 1,
                command: "npm test".to_string(),
                passed: false,
                exit_code: 1,
                output: "fail".to_string(),
                created_at: "t1".to_string(),
            },
        )
        .unwrap();
        let runs = list_test_runs(&connection, "task-1").unwrap();
        assert_eq!(runs.len(), 1);
        assert!(!runs[0].passed);
    }

    #[test]
    fn stores_validation_runs_and_settings() {
        let connection = memory();
        insert_task(&connection, &sample_task("task-1", TaskStatus::Validating)).unwrap();
        let run = ValidationRun {
            id: "vr1".to_string(),
            task_id: "task-1".to_string(),
            passed: true,
            results: vec![ValidationResult {
                command: "git diff --check".to_string(),
                passed: true,
                exit_code: 0,
                output: String::new(),
            }],
            created_at: "t1".to_string(),
        };
        insert_validation_run(&connection, &run).unwrap();
        let runs = list_validation_runs(&connection, "task-1").unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].results.len(), 1);

        assert!(!repair_on_validation_failure(&connection));
        set_setting(
            &connection,
            "repairOnValidationFailure",
            &serde_json::json!(true),
        )
        .unwrap();
        assert!(repair_on_validation_failure(&connection));
    }

    #[test]
    fn stores_lists_and_prunes_metrics() {
        let connection = memory();
        for index in 0..5 {
            insert_metric(
                &connection,
                &Metric {
                    id: format!("m-{index}"),
                    name: "test.duration_ms".to_string(),
                    value: index as f64,
                    task_id: Some("task-1".to_string()),
                    dims: serde_json::json!({ "attempt": index + 1 }),
                    created_at: format!("2026-10-07T00:00:0{index}Z"),
                },
            )
            .unwrap();
        }
        insert_metric(
            &connection,
            &Metric {
                id: "m-other".to_string(),
                name: "files.changed".to_string(),
                value: 3.0,
                task_id: None,
                dims: serde_json::Value::Null,
                created_at: "2026-10-07T00:01:00Z".to_string(),
            },
        )
        .unwrap();

        // Newest first, name-filtered.
        let filtered = list_metrics(&connection, Some("test.duration_ms"), 10).unwrap();
        assert_eq!(filtered.len(), 5);
        assert_eq!(filtered[0].value, 4.0);
        assert_eq!(filtered[0].dims["attempt"], 5);
        assert_eq!(list_metrics(&connection, None, 10).unwrap().len(), 6);

        // Pruning keeps only the newest rows.
        prune_metrics(&connection, 2).unwrap();
        let remaining = list_metrics(&connection, None, 10).unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].name, "files.changed");
    }
}
