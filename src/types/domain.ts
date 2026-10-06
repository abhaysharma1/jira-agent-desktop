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
  | "INTERRUPTED"
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
  REPAIRING: ["TESTING", "VALIDATING", "FAILED"],
  VALIDATING: ["COMMITTING", "REPAIRING", "FAILED"],
  COMMITTING: ["PR_CREATING", "FAILED"],
  PR_CREATING: ["PR_CREATED", "FAILED"],
  PR_CREATED: ["WAITING_FOR_REVIEW", "FAILED"],
  WAITING_FOR_REVIEW: [],
  // Recovery clears INTERRUPTED by resetting to the recorded stage's entry
  // point, then restarting that stage.
  INTERRUPTED: [
    "PLANNING",
    "APPROVED",
    "TESTING",
    "VALIDATING",
    "COMMITTING",
    "PR_CREATING",
  ],
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
  commitHash?: string | null;

  interruptedFrom?: string | null;

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

export interface PlanMessage {
  id: string;
  taskId: string;
  role: "user" | "agent";
  content: string;
  createdAt: string;
  planVersion?: number | null;
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
  mode: string;
  agent?: string | null;
  status: AgentRunStatus;
  sessionId?: string;
  startedAt: string;
  completedAt?: string;
}

export interface AgentRunView {
  run: AgentRun;
  events: AgentEvent[];
  output: string;
  cost: number;
  model?: string | null;
}

export interface TodoItem {
  content: string;
  status: string;
  priority: string;
}

export interface ModelInfo {
  id: string;
  name: string;
  providerId: string;
  modelId: string;
  free: boolean;
  costInput: number;
  costOutput: number;
}

export type AgentEvent =
  | { type: "started"; runId: string; timestamp: string }
  | { type: "message"; runId: string; content: string; timestamp: string }
  | { type: "reasoning"; runId: string; content: string; timestamp: string }
  | {
      type: "tool";
      runId: string;
      name: string;
      title: string;
      status: string;
      timestamp: string;
    }
  | { type: "todo"; runId: string; todos: TodoItem[]; timestamp: string }
  | {
      type: "permission_request";
      runId: string;
      title: string;
      timestamp: string;
    }
  | { type: "file_read"; runId: string; path: string; timestamp: string }
  | { type: "file_changed"; runId: string; path: string; timestamp: string }
  | { type: "command_started"; runId: string; command: string; timestamp: string }
  | {
      type: "command_finished";
      runId: string;
      command: string;
      exitCode: number;
      output: string | null;
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

export interface WorkspaceStatus {
  taskId: string;
  path: string;
  branchName: string;
  exists: boolean;
  isClean: boolean;
  changedFiles: number;
  currentBranch: string | null;
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

export interface ValidationRun {
  id: string;
  taskId: string;
  passed: boolean;
  results: ValidationResult[];
  createdAt: string;
}

export interface FileStat {
  path: string;
  additions: number;
  deletions: number;
  status: string;
}

export interface DiffStats {
  filesChanged: number;
  additions: number;
  deletions: number;
  files: FileStat[];
}

export interface FileDiff {
  path: string;
  status: string;
  additions: number;
  deletions: number;
  original: string;
  modified: string;
  binary: boolean;
  truncated: boolean;
}

export interface TestRun {
  id: string;
  taskId: string;
  attempt: number;
  command: string;
  passed: boolean;
  exitCode: number;
  output: string;
  createdAt: string;
}

export interface AppInfo {
  name: string;
  version: string;
  tauriVersion: string;
  platform: string;
}

export interface CloudAccount {
  baseUrl: string;
  email: string;
  deviceId?: string | null;
}

export interface JiraIssue {
  key: string;
  id: string;
  summary: string;
  description: string;
  status: string;
  issueType: string;
  url: string;
}

export interface CloudNotification {
  id: string;
  type: string;
  title: string;
  body?: string | null;
  taskId?: string | null;
  payload?: Record<string, unknown> | null;
  createdAt: string;
  readAt?: string | null;
}

export interface TicketIntake {
  repositoryId?: string | null;
  issue: JiraIssue;
}

/** Where a command-palette search result came from. */
export type SearchKind = "ticket" | "plan" | "pull_request" | "agent_run";

export interface SearchHit {
  kind: SearchKind;
  title: string;
  subtitle: string;
  /** Hash route the palette navigates to when the hit is chosen. */
  route: string;
  taskId: string;
}

export type CloudSocketStatus = "signedOut" | "connecting" | "online" | "offline";

/** A server-pushed WebSocket event: `{ event, ...payload }`. */
export interface CloudEvent {
  event: string;
  [key: string]: unknown;
}

export interface JiraTransition {
  id: string;
  name: string;
  toStatus: string;
}

export interface JiraSync {
  id: string;
  taskId: string;
  jiraIssueKey: string;
  jiraIssueId?: string | null;
  prNumber?: number | null;
  lastAction?: string | null;
  lastSyncedAt?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface JiraStatusMap {
  opened?: string;
  merged?: string;
  closed?: string;
}

export interface JiraConnectionInfo {
  account?: string | null;
  cloudId: string;
  siteUrl: string;
  siteName?: string | null;
  webhookUrl?: string | null;
}

export interface JiraProjectRepo {
  id: string;
  projectKey: string;
  repositoryId: string;
  createdAt: string;
  updatedAt: string;
}

/** A single observability measurement (Phase 26). */
export interface Metric {
  id: string;
  name: string;
  value: number;
  taskId?: string | null;
  dims: Record<string, unknown>;
  createdAt: string;
}

export interface Settings {
  workspaceRoot: string;
  defaultRepositoryId?: string;
  maxRepairAttempts: number;
  opencodeCommand: string;
  opencodeModel?: string;
}
