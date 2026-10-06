import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentEvent,
  AgentRun,
  AgentRunStatus,
  AgentRunView,
  AgentTask,
  AppInfo,
  CloudAccount,
  CloudEvent,
  CloudNotification,
  CloudSocketStatus,
  DiffStats,
  FileDiff,
  JiraConnectionInfo,
  JiraIssue,
  JiraProjectRepo,
  JiraStatusMap,
  JiraSync,
  JiraTransition,
  ModelInfo,
  Metric,
  Plan,
  PlanApproval,
  PlanMessage,
  PlanVersion,
  PullRequest,
  Repository,
  RepositoryStatus,
  SearchHit,
  TestRun,
  TicketIntake,
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
  commitChanges: (taskId: string, message?: string) =>
    invoke<string>("commit_changes", { taskId, message: message ?? null }),
  pushBranch: (taskId: string) =>
    invoke<void>("push_branch", { taskId }),
  createPullRequest: (taskId: string, title?: string, body?: string) =>
    invoke<void>("create_pull_request", {
      taskId,
      title: title ?? null,
      body: body ?? null,
    }),
  getPullRequest: (taskId: string) =>
    invoke<PullRequest | null>("get_pull_request", { taskId }),
  setGithubToken: (token?: string) =>
    invoke<void>("set_github_token", { token: token ?? null }),
  getGithubAccount: () => invoke<string | null>("get_github_account"),

  startJiraOAuth: () => invoke<JiraConnectionInfo>("start_jira_oauth"),
  getJiraConnectionInfo: () =>
    invoke<JiraConnectionInfo | null>("get_jira_connection_info"),
  registerJiraWebhook: () =>
    invoke<JiraConnectionInfo>("register_jira_webhook"),
  disconnectJira: () => invoke<void>("disconnect_jira"),
  getJiraAccount: () => invoke<string | null>("get_jira_account"),
  listJiraProjectRepos: () =>
    invoke<JiraProjectRepo[]>("list_jira_project_repos"),
  setJiraProjectRepo: (projectKey: string, repositoryId: string) =>
    invoke<JiraProjectRepo>("set_jira_project_repo", {
      projectKey,
      repositoryId,
    }),
  deleteJiraProjectRepo: (projectKey: string) =>
    invoke<void>("delete_jira_project_repo", { projectKey }),

  loginCloud: (
    baseUrl: string,
    email: string,
    password: string,
    register: boolean,
  ) =>
    invoke<CloudAccount>("login_cloud", {
      baseUrl,
      email,
      password,
      register,
    }),
  getCloudAccount: () => invoke<CloudAccount | null>("get_cloud_account"),
  logoutCloud: () => invoke<void>("logout_cloud"),
  startCloudSync: () => invoke<void>("start_cloud_sync"),
  stopCloudSync: () => invoke<void>("stop_cloud_sync"),
  markNotificationRead: (id: string) =>
    invoke<void>("mark_notification_read", { id }),
  skipTicket: (jiraKey: string, title?: string, notificationId?: string) =>
    invoke<AgentTask>("skip_ticket", {
      jiraKey,
      title: title ?? null,
      notificationId: notificationId ?? null,
    }),
  prepareTicketIntake: (jiraKey: string, notificationId?: string) =>
    invoke<TicketIntake>("prepare_ticket_intake", {
      jiraKey,
      notificationId: notificationId ?? null,
    }),
  getJiraStatusMap: () => invoke<JiraStatusMap>("get_jira_status_map"),
  setJiraStatusMap: (map: JiraStatusMap) =>
    invoke<void>("set_jira_status_map", { map }),
  getJiraIssue: (key: string) =>
    invoke<JiraIssue>("get_jira_issue", { key }),
  listJiraTransitions: (key: string) =>
    invoke<JiraTransition[]>("list_jira_transitions", { key }),
  getJiraSync: (taskId: string) =>
    invoke<JiraSync | null>("get_jira_sync", { taskId }),
  syncTaskJira: (taskId: string) =>
    invoke<JiraSync>("sync_task_jira", { taskId }),
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

  search: (query: string, limit?: number) =>
    invoke<SearchHit[]>("search", { query, limit: limit ?? null }),

  setTrayStats: (pending: number, running: number) =>
    invoke<void>("set_tray_stats", { pending, running }),
  showMainWindow: () => invoke<void>("show_main_window"),
  quitApp: () => invoke<void>("quit_app"),
  getCloseToTray: () => invoke<boolean>("get_close_to_tray"),
  setCloseToTray: (enabled: boolean) =>
    invoke<void>("set_close_to_tray", { enabled }),

  listMetrics: (name?: string, limit?: number) =>
    invoke<Metric[]>("list_metrics", {
      name: name ?? null,
      limit: limit ?? null,
    }),
  getLogsDir: () => invoke<string>("get_logs_dir"),
  openLogsFolder: () => invoke<void>("open_logs_folder"),

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

  resumeTask: (taskId: string) => invoke<void>("resume_task", { taskId }),

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
  onCommitCreated: (
    handler: (payload: { taskId: string; commitHash: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; commitHash: string }>("commit://created", (event) =>
      handler(event.payload),
    ),
  onCommitError: (
    handler: (payload: { taskId: string; error: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; error: string }>("commit://error", (event) =>
      handler(event.payload),
    ),
  onPrCreated: (handler: (pr: PullRequest) => void): Promise<UnlistenFn> =>
    listen<PullRequest>("pr://created", (event) => handler(event.payload)),
  onPrError: (
    handler: (payload: { taskId: string; error: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; error: string }>("pr://error", (event) =>
      handler(event.payload),
    ),
  onJiraSynced: (handler: (sync: JiraSync) => void): Promise<UnlistenFn> =>
    listen<JiraSync>("jira://synced", (event) => handler(event.payload)),
  onJiraError: (
    handler: (payload: { taskId: string; error: string }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ taskId: string; error: string }>("jira://error", (event) =>
      handler(event.payload),
    ),
  onNotification: (
    handler: (notification: CloudNotification) => void,
  ): Promise<UnlistenFn> =>
    listen<CloudNotification>("notification://received", (event) =>
      handler(event.payload),
    ),
  onCloudStatus: (
    handler: (payload: { status: CloudSocketStatus }) => void,
  ): Promise<UnlistenFn> =>
    listen<{ status: CloudSocketStatus }>("cloud://status", (event) =>
      handler(event.payload),
    ),
  onCloudEvent: (handler: (event: CloudEvent) => void): Promise<UnlistenFn> =>
    listen<CloudEvent>("cloud://event", (event) => handler(event.payload)),

  /** The tray menu asked the frontend to navigate (e.g. to Settings). */
  onTrayNavigate: (handler: (route: string) => void): Promise<UnlistenFn> =>
    listen<string>("tray://navigate", (event) => handler(event.payload)),
};
