#![allow(dead_code)]

use serde::{Deserialize, Serialize};#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Received,
    Notified,
    Skipped,
    Planning,
    PlanReady,
    Rejected,
    Approved,
    WorkspaceCreating,
    Implementing,
    Testing,
    Repairing,
    Validating,
    Committing,
    PrCreating,
    PrCreated,
    WaitingForReview,
    Failed,
}

impl TaskStatus {
    pub fn transitions(self) -> &'static [TaskStatus] {
        use TaskStatus::*;
        match self {
            Received => &[Notified, Failed],
            Notified => &[Skipped, Planning, Failed],
            Skipped => &[],
            Planning => &[PlanReady, Failed],
            PlanReady => &[Rejected, Approved, Failed],
            Rejected => &[],
            Approved => &[WorkspaceCreating, Failed],
            WorkspaceCreating => &[Implementing, Failed],
            Implementing => &[Testing, Failed],
            Testing => &[Repairing, Validating, Failed],
            Repairing => &[Testing, Validating, Failed],
            Validating => &[Committing, Repairing, Failed],
            Committing => &[PrCreating, Failed],
            PrCreating => &[PrCreated, Failed],
            PrCreated => &[WaitingForReview, Failed],
            WaitingForReview => &[],
            Failed => &[],
        }
    }

    pub fn can_transition_to(self, target: TaskStatus) -> bool {
        self.transitions().contains(&target)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Skipped
                | TaskStatus::Rejected
                | TaskStatus::WaitingForReview
                | TaskStatus::Failed
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub local_path: String,
    pub remote_url: String,
    pub default_branch: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStatus {
    pub repository_id: String,
    pub is_git_repository: bool,
    pub current_branch: Option<String>,
    pub remote_url: Option<String>,
    pub is_clean: bool,
    pub changed_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub jira_issue_key: String,
    pub title: String,
    pub description: String,
    pub repository_id: String,
    pub status: TaskStatus,
    pub plan_version: Option<u32>,
    pub approved_plan_version: Option<u32>,
    pub workspace_path: Option<String>,
    pub branch_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub task_id: String,
    pub current_version: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub order: u32,
    pub description: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanContent {
    pub ticket: String,
    pub summary: String,
    pub understanding: String,
    pub steps: Vec<PlanStep>,
    pub tests: Vec<String>,
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanVersion {
    pub id: String,
    pub plan_id: String,
    pub task_id: String,
    pub version: u32,
    pub content: PlanContent,
    pub created_at: String,
    pub created_by: String,
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanApproval {
    pub id: String,
    pub task_id: String,
    pub plan_version: u32,
    pub user_id: String,
    pub approved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanMessage {
    pub id: String,
    pub task_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub plan_version: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentRunStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRun {
    pub id: String,
    pub task_id: String,
    pub mode: String,
    pub agent: Option<String>,
    pub status: AgentRunStatus,
    pub session_id: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunView {
    pub run: AgentRun,
    pub events: Vec<AgentEvent>,
    pub output: String,
    pub cost: f64,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub content: String,
    pub status: String,
    pub priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Started {
        #[serde(rename = "runId")]
        run_id: String,
        timestamp: String,
    },
    Message {
        #[serde(rename = "runId")]
        run_id: String,
        content: String,
        timestamp: String,
    },
    Reasoning {
        #[serde(rename = "runId")]
        run_id: String,
        content: String,
        timestamp: String,
    },
    Tool {
        #[serde(rename = "runId")]
        run_id: String,
        name: String,
        title: String,
        status: String,
        timestamp: String,
    },
    Todo {
        #[serde(rename = "runId")]
        run_id: String,
        todos: Vec<TodoItem>,
        timestamp: String,
    },
    PermissionRequest {
        #[serde(rename = "runId")]
        run_id: String,
        title: String,
        timestamp: String,
    },
    FileRead {
        #[serde(rename = "runId")]
        run_id: String,
        path: String,
        timestamp: String,
    },
    FileChanged {
        #[serde(rename = "runId")]
        run_id: String,
        path: String,
        timestamp: String,
    },
    CommandStarted {
        #[serde(rename = "runId")]
        run_id: String,
        command: String,
        timestamp: String,
    },
    CommandFinished {
        #[serde(rename = "runId")]
        run_id: String,
        command: String,
        #[serde(rename = "exitCode")]
        exit_code: i32,
        output: Option<String>,
        timestamp: String,
    },
    TestResult {
        #[serde(rename = "runId")]
        run_id: String,
        passed: bool,
        timestamp: String,
    },
    Finished {
        #[serde(rename = "runId")]
        run_id: String,
        timestamp: String,
    },
    Failed {
        #[serde(rename = "runId")]
        run_id: String,
        error: String,
        timestamp: String,
    },
}

impl AgentEvent {
    pub fn run_id(&self) -> &str {
        match self {
            AgentEvent::Started { run_id, .. }
            | AgentEvent::Message { run_id, .. }
            | AgentEvent::Reasoning { run_id, .. }
            | AgentEvent::Tool { run_id, .. }
            | AgentEvent::Todo { run_id, .. }
            | AgentEvent::PermissionRequest { run_id, .. }
            | AgentEvent::FileRead { run_id, .. }
            | AgentEvent::FileChanged { run_id, .. }
            | AgentEvent::CommandStarted { run_id, .. }
            | AgentEvent::CommandFinished { run_id, .. }
            | AgentEvent::TestResult { run_id, .. }
            | AgentEvent::Finished { run_id, .. }
            | AgentEvent::Failed { run_id, .. } => run_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider_id: String,
    pub model_id: String,
    pub free: bool,
    pub cost_input: f64,
    pub cost_output: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub task_id: String,
    pub repository_id: String,
    pub path: String,
    pub branch_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceStatus {
    pub task_id: String,
    pub path: String,
    pub branch_name: String,
    pub exists: bool,
    pub is_clean: bool,
    pub changed_files: u32,
    pub current_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub id: String,
    pub task_id: String,
    pub provider: String,
    pub number: u32,
    pub url: String,
    pub branch: String,
    pub base_branch: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationConfig {
    pub test: Option<String>,
    pub lint: Option<String>,
    pub build: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub command: String,
    pub passed: bool,
    pub exit_code: i32,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRun {
    pub id: String,
    pub task_id: String,
    pub passed: bool,
    pub results: Vec<ValidationResult>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStat {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffStats {
    pub files_changed: u32,
    pub additions: u32,
    pub deletions: u32,
    pub files: Vec<FileStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub original: String,
    pub modified: String,
    pub binary: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRun {
    pub id: String,
    pub task_id: String,
    pub attempt: u32,
    pub command: String,
    pub passed: bool,
    pub exit_code: i32,
    pub output: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub tauri_version: String,
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub workspace_root: String,
    pub default_repository_id: Option<String>,
    pub max_repair_attempts: u32,
    pub opencode_command: String,
    pub opencode_model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_valid_transition() {
        assert!(TaskStatus::Received.can_transition_to(TaskStatus::Notified));
        assert!(TaskStatus::Testing.can_transition_to(TaskStatus::Repairing));
    }

    #[test]
    fn rejects_invalid_transition() {
        assert!(!TaskStatus::Received.can_transition_to(TaskStatus::Approved));
        assert!(!TaskStatus::Failed.can_transition_to(TaskStatus::Planning));
    }

    #[test]
    fn serializes_status_in_screaming_snake_case() {
        let value = serde_json::to_string(&TaskStatus::WorkspaceCreating).unwrap();
        assert_eq!(value, "\"WORKSPACE_CREATING\"");
    }
}
