import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentEvent,
  AgentRun,
  AgentRunStatus,
  AgentRunView,
  AgentTask,
  AppInfo,
  DiffStats,
  FileDiff,
  ModelInfo,
  Plan,
  PlanApproval,
  PlanMessage,
  PlanVersion,
  Repository,
  RepositoryStatus,
  TestRun,
  ValidationConfig,
  ValidationRun,
  Workspace,
  WorkspaceStatus,
} from "@/types";

export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),

  listRepositories: () => invoke<Repository[]>("list_repositories"),
  addRepository: (localPath: string) =>
    invoke<Repository>("add_repository", { localPath }),
  getRepositoryStatus: (repositoryId: string) =>
    invoke<RepositoryStatus>("get_repository_status", { repositoryId }),
  removeRepository: (repositoryId: string) =>
    invoke<void>("remove_repository", { repositoryId }),

  listWorkspaces: () => invoke<Workspace[]>("list_workspaces"),
  createWorkspace: (repositoryId: string, taskId: string, branchSlug?: string) =>
    invoke<Workspace>("create_workspace", {
      repositoryId,
      taskId,
      branchSlug: branchSlug ?? null,
    }),
  deleteWorkspace: (taskId: string) =>
    invoke<void>("delete_workspace", { taskId }),
  getWorkspaceStatus: (taskId: string) =>
    invoke<WorkspaceStatus>("get_workspace_status", { taskId }),

  getWorkspaceStats: (taskId: string) =>
    invoke<DiffStats>("get_workspace_stats", { taskId }),
  getWorkspaceDiffs: (taskId: string) =>
    invoke<FileDiff[]>("get_workspace_diffs", { taskId }),
  commitChanges: (workspacePath: string, message: string) =>
    invoke<string>("commit_changes", { workspacePath, message }),
  pushBranch: (workspacePath: string) =>
    invoke<void>("push_branch", { workspacePath }),
  runFinalValidation: (taskId: string) =>
    invoke<void>("run_final_validation", { taskId }),
  listValidationRuns: (taskId: string) =>
    invoke<ValidationRun[]>("list_validation_runs", { taskId }),
  getRepairOnValidationFailure: () =>
    invoke<boolean>("get_repair_on_validation_failure"),
  setRepairOnValidationFailure: (enabled: boolean) =>
    invoke<void>("set_repair_on_validation_failure", { enabled }),

  getValidationConfig: (repositoryId: string) =>
    invoke<ValidationConfig>("get_validation_config", { repositoryId }),
  setValidationConfig: (repositoryId: string, config: ValidationConfig) =>
    invoke<void>("set_validation_config", { repositoryId, config }),
  listTestRuns: (taskId: string) =>
    invoke<TestRun[]>("list_test_runs", { taskId }),
  runTests: (taskId: string) => invoke<void>("run_tests", { taskId }),

  startAgent: (
    targetDir: string,
    prompt: string,
    agent?: string,
    model?: string,
  ) =>
    invoke<AgentRun>("start_agent", {
      targetDir,
      prompt,
      agent: agent ?? null,
      model: model ?? null,
    }),
  sendAgentMessage: (runId: string, content: string) =>
    invoke<void>("send_agent_message", { runId, content }),
  stopAgent: (runId: string) => invoke<void>("stop_agent", { runId }),
  getAgentStatus: (runId: string) =>
    invoke<AgentRunView>("get_agent_status", { runId }),
  listAgents: () => invoke<AgentRun[]>("list_agents"),
  listModels: () => invoke<ModelInfo[]>("list_models"),

  startPlanning: (
    repositoryId: string,
    key: string,
    title: string,
    description: string,
    acceptanceCriteria?: string,
    model?: string,
  ) =>
    invoke<AgentTask>("start_planning", {
      repositoryId,
      key,
      title,
      description,
      acceptanceCriteria: acceptanceCriteria ?? null,
      model: model ?? null,
    }),
  listTasks: () => invoke<AgentTask[]>("list_tasks"),
  getTask: (taskId: string) => invoke<AgentTask>("get_task", { taskId }),
  getPlan: (taskId: string) =>
    invoke<PlanVersion | null>("get_plan", { taskId }),
  approvePlan: (taskId: string, planVersion?: number, userId?: string) =>
    invoke<PlanApproval>("approve_plan", {
      taskId,
      planVersion: planVersion ?? null,
      userId: userId ?? null,
    }),
  rejectPlan: (taskId: string) =>
    invoke<AgentTask>("reject_plan", { taskId }),
  getPlanApproval: (taskId: string) =>
    invoke<PlanApproval | null>("get_plan_approval", { taskId }),
  revisePlan: (taskId: string, feedback: string, model?: string) =>
    invoke<PlanMessage>("revise_plan", {
      taskId,
      feedback,
      model: model ?? null,
    }),
  listPlanVersions: (taskId: string) =>
    invoke<PlanVersion[]>("list_plan_versions", { taskId }),
  listPlanMessages: (taskId: string) =>
    invoke<PlanMessage[]>("list_plan_messages", { taskId }),

  startImplementation: (taskId: string, model?: string) =>
    invoke<void>("start_implementation", { taskId, model: model ?? null }),

  onAgentEvent: (handler: (event: AgentEvent) => void): Promise<UnlistenFn> =>
    listen<AgentEvent>("agent://event", (event) => handler(event.payload)),
  onAgentStatus: (
    handler: (payload: { runId: string; status: AgentRunStatus }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ runId: string; status: AgentRunStatus }>(
      "agent://status",
      (event) => handler(event.payload),
    ),

  onPlanReady: (
    handler: (payload: {
      taskId: string;
      plan: Plan;
      version: PlanVersion;
    }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; plan: Plan; version: PlanVersion }>(
      "plan://ready",
      (event) => handler(event.payload),
    ),
  onPlanError: (
    handler: (payload: { taskId: string; error: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; error: string }>("plan://error", (event) =>
      handler(event.payload),
    ),
  onPlanApproved: (
    handler: (payload: { taskId: string; approval: PlanApproval }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; approval: PlanApproval }>(
      "plan://approved",
      (event) => handler(event.payload),
    ),
  onTaskUpdated: (handler: (task: AgentTask) => void): Promise<UnlistenFn> =>
    listen<AgentTask>("task://updated", (event) => handler(event.payload)),
  onPlanMessage: (handler: (message: PlanMessage) => void): Promise<UnlistenFn> =>
    listen<PlanMessage>("plan://message", (event) => handler(event.payload)),
  onImplementationError: (
    handler: (payload: { taskId: string; error: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; error: string }>("implementation://error", (event) =>
      handler(event.payload),
    ),
  onTestResult: (handler: (run: TestRun) => void): Promise<UnlistenFn> =>
    listen<TestRun>("test://result", (event) => handler(event.payload)),
  onValidationError: (
    handler: (payload: { taskId: string; error: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; error: string }>("validation://error", (event) =>
      handler(event.payload),
    ),
  onValidationResult: (
    handler: (run: ValidationRun) => void,
  ): Promise<UnlistenFn> =>
    listen<ValidationRun>("validation://result", (event) => handler(event.payload)),
};
