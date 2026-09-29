use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::domain::{
    AgentEvent, AgentRun, AgentTask, Plan, PlanApproval, PlanContent, PlanMessage, PlanVersion,
    Repository, TaskStatus, TestRun, ValidationConfig, ValidationResult, ValidationRun, Workspace,
};

const SCHEMA_VERSION: i32 = 3;

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
"#,
        )
        .map_err(|error| error.to_string())?;

    connection
        .execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn recover_interrupted(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "UPDATE tasks SET status = 'FAILED' WHERE status IN (
                'PLANNING', 'WORKSPACE_CREATING', 'IMPLEMENTING', 'TESTING',
                'REPAIRING', 'VALIDATING', 'COMMITTING', 'PR_CREATING'
            )",
            [],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE agent_runs SET status = 'FAILED' WHERE status IN ('PENDING', 'RUNNING')",
            [],
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
              approved_plan_version, workspace_path, branch_name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

pub fn list_tasks(connection: &Connection) -> Result<Vec<AgentTask>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, jira_issue_key, title, description, repository_id, status, plan_version,
                    approved_plan_version, workspace_path, branch_name, created_at, updated_at
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
                    approved_plan_version, workspace_path, branch_name, created_at, updated_at
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
                    approved_plan_version, workspace_path, branch_name, created_at, updated_at
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
        // first task marked failed; second untouched
        let tasks = list_tasks(&connection).unwrap();
        let statuses: Vec<TaskStatus> = tasks.iter().map(|task| task.status).collect();
        assert!(statuses.contains(&TaskStatus::Failed));
        assert!(statuses.contains(&TaskStatus::PlanReady));
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
}
