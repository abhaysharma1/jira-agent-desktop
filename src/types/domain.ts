export type TaskStatus =
  | "RECEIVED"
  | "NOTIFIED"
  | "SKIPPED"
  | "PLANNING"
  | "PLAN_READY"
  | "REJECTED"
  | "APPROVED"
  | "WORKSPACE_CREATING"
  | "IMPLEMENTING"
  | "TESTING"
  | "REPAIRING"
  | "VALIDATING"
  | "COMMITTING"
  | "PR_CREATING"
  | "PR_CREATED"
  | "WAITING_FOR_REVIEW"
  | "FAILED";

export const TASK_TRANSITIONS: Record<TaskStatus, readonly TaskStatus[]> = {
  RECEIVED: ["NOTIFIED", "FAILED"],
  NOTIFIED: ["SKIPPED", "PLANNING", "FAILED"],
  SKIPPED: [],
  PLANNING: ["PLAN_READY", "FAILED"],
  PLAN_READY: ["REJECTED", "APPROVED", "FAILED"],
  REJECTED: [],
  APPROVED: ["WORKSPACE_CREATING", "FAILED"],
  WORKSPACE_CREATING: ["IMPLEMENTING", "FAILED"],
  IMPLEMENTING: ["TESTING", "FAILED"],
  TESTING: ["REPAIRING", "VALIDATING", "FAILED"],
  REPAIRING: ["TESTING", "FAILED"],
  VALIDATING: ["COMMITTING", "FAILED"],
  COMMITTING: ["PR_CREATING", "FAILED"],
  PR_CREATING: ["PR_CREATED", "FAILED"],
  PR_CREATED: ["WAITING_FOR_REVIEW", "FAILED"],
  WAITING_FOR_REVIEW: [],
  FAILED: [],
};

export const TERMINAL_TASK_STATUSES: readonly TaskStatus[] = [
  "SKIPPED",
  "REJECTED",
  "WAITING_FOR_REVIEW",
  "FAILED",
];

export function canTransition(from: TaskStatus, to: TaskStatus): boolean {
  return TASK_TRANSITIONS[from].includes(to);
}

export function isTerminal(status: TaskStatus): boolean {
  return TERMINAL_TASK_STATUSES.includes(status);
}

export interface Repository {
  id: string;
  name: string;
  localPath: string;
  remoteUrl: string;
  defaultBranch: string;
  createdAt: string;
  updatedAt: string;
}

export interface RepositoryStatus {
  repositoryId: string;
  isGitRepository: boolean;
  currentBranch: string | null;
  remoteUrl: string | null;
  isClean: boolean;
  changedFiles: number;
}

export interface AgentTask {
  id: string;
  jiraIssueKey: string;
  title: string;
  description: string;

  repositoryId: string;

  status: TaskStatus;

  planVersion?: number;
  approvedPlanVersion?: number;

  workspacePath?: string;
  branchName?: string;

  createdAt: string;
  updatedAt: string;
}

export interface PlanStep {
  order: number;
  description: string;
  files: string[];
}

export interface PlanContent {
  ticket: string;
  summary: string;
  understanding: string;
  steps: PlanStep[];
  tests: string[];
  risks: string[];
}

export interface Plan {
  id: string;
  taskId: string;
  currentVersion: number;
  createdAt: string;
  updatedAt: string;
}

export interface PlanVersion {
  id: string;
  planId: string;
  taskId: string;
  version: number;
  content: PlanContent;
  createdAt: string;
  createdBy: "agent" | "user";
  approved: boolean;
}

export interface PlanApproval {
  id: string;
  taskId: string;
  planVersion: number;
  userId: string;
  approvedAt: string;
}

export type AgentRunStatus =
  | "PENDING"
  | "RUNNING"
  | "SUCCEEDED"
  | "FAILED"
  | "CANCELLED";

export interface AgentRun {
  id: string;
  taskId: string;
  mode: "planning" | "implementation" | "repair";
  status: AgentRunStatus;
  sessionId?: string;
  startedAt: string;
  completedAt?: string;
}

export type AgentEvent =
  | { type: "started"; runId: string; timestamp: string }
  | { type: "message"; runId: string; content: string; timestamp: string }
  | { type: "file_read"; runId: string; path: string; timestamp: string }
  | { type: "file_changed"; runId: string; path: string; timestamp: string }
  | { type: "command_started"; runId: string; command: string; timestamp: string }
  | {
      type: "command_finished";
      runId: string;
      command: string;
      exitCode: number;
      timestamp: string;
    }
  | { type: "test_result"; runId: string; passed: boolean; timestamp: string }
  | { type: "finished"; runId: string; timestamp: string }
  | { type: "failed"; runId: string; error: string; timestamp: string };

export interface Workspace {
  id: string;
  taskId: string;
  repositoryId: string;
  path: string;
  branchName: string;
  createdAt: string;
}

export interface PullRequest {
  id: string;
  taskId: string;
  provider: "github";
  number: number;
  url: string;
  branch: string;
  baseBranch: string;
  status: "open" | "closed" | "merged";
  createdAt: string;
}

export interface ValidationConfig {
  test?: string;
  lint?: string;
  build?: string;
}

export interface ValidationResult {
  command: string;
  passed: boolean;
  exitCode: number;
  output: string;
}

export interface AppInfo {
  name: string;
  version: string;
  tauriVersion: string;
  platform: string;
}

export interface Settings {
  workspaceRoot: string;
  defaultRepositoryId?: string;
  maxRepairAttempts: number;
  opencodeCommand: string;
  opencodeModel?: string;
}
