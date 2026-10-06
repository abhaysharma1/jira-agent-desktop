import { create } from "zustand";
import { api } from "@/api/tauri";
import { notify } from "@/lib/desktopNotify";
import { TASK_TRANSITIONS } from "@/types";
import type {
  AgentEvent,
  AgentRun,
  AgentTask,
  AppInfo,
  CloudAccount,
  CloudNotification,
  CloudSocketStatus,
  DiffStats,
  FileDiff,
  JiraConnectionInfo,
  JiraProjectRepo,
  JiraStatusMap,
  JiraSync,
  ModelInfo,
  Metric,
  PlanApproval,
  PlanMessage,
  PlanVersion,
  PullRequest,
  Repository,
  RepositoryStatus,
  TaskStatus,
  TestRun,
  TicketIntake,
  ValidationConfig,
  ValidationRun,
  Workspace,
  WorkspaceStatus,
} from "@/types";

/** The JIRA issue key carried by a notification, or parsed from its title. */
export function notificationJiraKey(notification: CloudNotification): string {
  const fromPayload = notification.payload?.jiraKey;
  if (typeof fromPayload === "string" && fromPayload.trim()) {
    return fromPayload;
  }
  const [prefix] = notification.title.split(":");
  return prefix.trim();
}

/** The notification's event type, falling back to its stored type. */
export function notificationEventType(
  notification: CloudNotification,
): string {
  const fromPayload = notification.payload?.eventType;
  if (typeof fromPayload === "string" && fromPayload.trim()) {
    return fromPayload;
  }
  return notification.type;
}

/** The ticket summary from the `KEY: summary` notification title. */
export function notificationSummary(notification: CloudNotification): string {
  const index = notification.title.indexOf(":");
  return index >= 0 ? notification.title.slice(index + 1).trim() : notification.title;
}

/** Whether a pushed task status maps to one of the desktop's local statuses. */
export function isTaskStatus(value: string): value is TaskStatus {
  return value in TASK_TRANSITIONS;
}

interface AppState {
  appInfo: AppInfo | null;
  repositories: Repository[];
  repositoryStatuses: Record<string, RepositoryStatus>;
  tasks: AgentTask[];
  activeTask: AgentTask | null;
  planVersions: Record<string, PlanVersion>;
  planVersionList: Record<string, PlanVersion[]>;
  planMessages: Record<string, PlanMessage[]>;
  planApprovals: Record<string, PlanApproval>;
  workspaceStats: Record<string, DiffStats>;
  workspaceDiffs: Record<string, FileDiff[]>;
  testRuns: Record<string, TestRun[]>;
  validationConfigs: Record<string, ValidationConfig>;
  validationRuns: Record<string, ValidationRun[]>;
  repairOnValidationFailure: boolean;
  closeToTray: boolean;
  metrics: Metric[];
  logsDir: string;
  githubAccount: string | null;
  pullRequests: Record<string, PullRequest>;
  jiraAccount: string | null;
  jiraConnection: JiraConnectionInfo | null;
  jiraStatusMap: JiraStatusMap;
  jiraSyncs: Record<string, JiraSync>;
  jiraProjectRepos: JiraProjectRepo[];
  cloudAccount: CloudAccount | null;
  notifications: CloudNotification[];
  cloudSocketStatus: CloudSocketStatus;
  events: AgentEvent[];
  workspaces: Workspace[];
  workspaceStatuses: Record<string, WorkspaceStatus>;
  agentRuns: AgentRun[];
  agentEvents: Record<string, AgentEvent[]>;
  agentOutput: Record<string, string>;
  agentCost: Record<string, number>;
  agentModel: Record<string, string | null>;
  models: ModelInfo[];
  activeRunId: string | null;
  listenersReady: boolean;
  loading: boolean;
  error: string | null;

  loadAppInfo: () => Promise<void>;
  loadRepositories: () => Promise<void>;
  addRepository: (localPath: string) => Promise<Repository>;
  removeRepository: (repositoryId: string) => Promise<void>;
  refreshRepositoryStatus: (repositoryId: string) => Promise<void>;
  refreshRepositoryStatuses: () => Promise<void>;
  loadWorkspaces: () => Promise<void>;
  createWorkspace: (
    repositoryId: string,
    taskId: string,
    branchSlug?: string,
  ) => Promise<Workspace>;
  destroyWorkspace: (taskId: string) => Promise<void>;
  refreshWorkspaceStatus: (taskId: string) => Promise<void>;
  initAgentListeners: () => Promise<void>;
  loadAgents: () => Promise<void>;
  loadModels: () => Promise<void>;
  startAgent: (
    targetDir: string,
    prompt: string,
    agent?: string,
    model?: string,
  ) => Promise<AgentRun>;
  stopAgent: (runId: string) => Promise<void>;
  refreshAgentStatus: (runId: string) => Promise<void>;
  setActiveRun: (runId: string | null) => void;
  setActiveTask: (task: AgentTask | null) => void;
  loadTasks: () => Promise<void>;
  loadPlan: (taskId: string) => Promise<void>;
  loadPlanVersions: (taskId: string) => Promise<void>;
  loadPlanMessages: (taskId: string) => Promise<void>;
  loadPlanApproval: (taskId: string) => Promise<void>;
  approvePlan: (
    taskId: string,
    planVersion?: number,
    userId?: string,
  ) => Promise<PlanApproval>;
  rejectPlan: (taskId: string) => Promise<void>;
  startImplementation: (taskId: string, model?: string) => Promise<void>;
  resumeTask: (taskId: string) => Promise<void>;
  loadWorkspaceStats: (taskId: string) => Promise<void>;
  loadWorkspaceDiffs: (taskId: string) => Promise<void>;
  loadTestRuns: (taskId: string) => Promise<void>;
  runTests: (taskId: string) => Promise<void>;
  loadValidationConfig: (repositoryId: string) => Promise<void>;
  saveValidationConfig: (
    repositoryId: string,
    config: ValidationConfig,
  ) => Promise<void>;
  loadValidationRuns: (taskId: string) => Promise<void>;
  runFinalValidation: (taskId: string) => Promise<void>;
  commitChanges: (taskId: string, message?: string) => Promise<string>;
  loadGithubAccount: () => Promise<void>;
  setGithubToken: (token?: string) => Promise<void>;
  createPullRequest: (
    taskId: string,
    title?: string,
    body?: string,
  ) => Promise<void>;
  loadPullRequest: (taskId: string) => Promise<void>;
  loadJira: () => Promise<void>;
  connectJira: () => Promise<void>;
  registerJiraWebhook: () => Promise<void>;
  disconnectJira: () => Promise<void>;
  saveJiraStatusMap: (map: JiraStatusMap) => Promise<void>;
  loadJiraProjectRepos: () => Promise<void>;
  setJiraProjectRepo: (
    projectKey: string,
    repositoryId: string,
  ) => Promise<void>;
  deleteJiraProjectRepo: (projectKey: string) => Promise<void>;
  loadJiraSync: (taskId: string) => Promise<void>;
  syncTaskJira: (taskId: string) => Promise<void>;
  loadCloudAccount: () => Promise<void>;
  loginCloud: (
    baseUrl: string,
    email: string,
    password: string,
    register: boolean,
  ) => Promise<void>;
  logoutCloud: () => Promise<void>;
  startCloudSync: () => Promise<void>;
  stopCloudSync: () => Promise<void>;
  markNotificationRead: (id: string) => Promise<void>;
  skipTicket: (notification: CloudNotification) => Promise<void>;
  prepareTicketIntake: (
    jiraKey: string,
    notificationId?: string,
  ) => Promise<TicketIntake>;
  loadRepairOnValidationFailure: () => Promise<void>;
  setRepairOnValidationFailure: (enabled: boolean) => Promise<void>;
  loadCloseToTray: () => Promise<void>;
  setCloseToTray: (enabled: boolean) => Promise<void>;
  loadMetrics: (name?: string, limit?: number) => Promise<void>;
  openLogsFolder: () => Promise<void>;
  revisePlan: (
    taskId: string,
    feedback: string,
    model?: string,
  ) => Promise<PlanMessage>;
  startPlanning: (
    repositoryId: string,
    key: string,
    title: string,
    description: string,
    acceptanceCriteria?: string,
    model?: string,
  ) => Promise<AgentTask>;
  pushEvent: (event: AgentEvent) => void;
  setError: (error: string | null) => void;
}

export const useAppStore = create<AppState>((set, get) => ({
  appInfo: null,
  repositories: [],
  repositoryStatuses: {},
  tasks: [],
  activeTask: null,
  planVersions: {},
  planVersionList: {},
  planMessages: {},
  planApprovals: {},
  workspaceStats: {},
  workspaceDiffs: {},
  testRuns: {},
  validationConfigs: {},
  validationRuns: {},
  repairOnValidationFailure: false,
  closeToTray: true,
  metrics: [],
  logsDir: "",
  githubAccount: null,
  pullRequests: {},
  jiraAccount: null,
  jiraConnection: null,
  jiraStatusMap: {},
  jiraSyncs: {},
  jiraProjectRepos: [],
  cloudAccount: null,
  notifications: [],
  cloudSocketStatus: "signedOut",
  events: [],
  workspaces: [],
  workspaceStatuses: {},
  agentRuns: [],
  agentEvents: {},
  agentOutput: {},
  agentCost: {},
  agentModel: {},
  models: [],
  activeRunId: null,
  listenersReady: false,
  loading: false,
  error: null,

  loadAppInfo: async () => {
    try {
      const appInfo = await api.getAppInfo();
      set({ appInfo });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadRepositories: async () => {
    set({ loading: true, error: null });
    try {
      const repositories = await api.listRepositories();
      set({ repositories, loading: false });
      await get().refreshRepositoryStatuses();
    } catch (error) {
      set({ error: String(error), loading: false });
    }
  },

  addRepository: async (localPath) => {
    set({ error: null });
    try {
      const repository = await api.addRepository(localPath);
      set((state) => ({ repositories: [...state.repositories, repository] }));
      await get().refreshRepositoryStatus(repository.id);
      return repository;
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  removeRepository: async (repositoryId) => {
    set({ error: null });
    try {
      await api.removeRepository(repositoryId);
      set((state) => {
        const repositoryStatuses = { ...state.repositoryStatuses };
        delete repositoryStatuses[repositoryId];
        return {
          repositories: state.repositories.filter(
            (repository) => repository.id !== repositoryId,
          ),
          repositoryStatuses,
        };
      });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  refreshRepositoryStatus: async (repositoryId) => {
    try {
      const status = await api.getRepositoryStatus(repositoryId);
      set((state) => ({
        repositoryStatuses: {
          ...state.repositoryStatuses,
          [repositoryId]: status,
        },
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  refreshRepositoryStatuses: async () => {
    const { repositories, refreshRepositoryStatus } = get();
    await Promise.all(
      repositories.map((repository) => refreshRepositoryStatus(repository.id)),
    );
  },

  loadWorkspaces: async () => {
    try {
      const workspaces = await api.listWorkspaces();
      set({ workspaces });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  createWorkspace: async (repositoryId, taskId, branchSlug) => {
    set({ error: null });
    try {
      const workspace = await api.createWorkspace(
        repositoryId,
        taskId,
        branchSlug,
      );
      set((state) => ({ workspaces: [...state.workspaces, workspace] }));
      await get().refreshWorkspaceStatus(taskId);
      return workspace;
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  destroyWorkspace: async (taskId) => {
    set({ error: null });
    try {
      await api.deleteWorkspace(taskId);
      set((state) => {
        const workspaceStatuses = { ...state.workspaceStatuses };
        delete workspaceStatuses[taskId];
        return {
          workspaces: state.workspaces.filter(
            (workspace) => workspace.taskId !== taskId,
          ),
          workspaceStatuses,
        };
      });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  refreshWorkspaceStatus: async (taskId) => {
    try {
      const status = await api.getWorkspaceStatus(taskId);
      set((state) => ({
        workspaceStatuses: { ...state.workspaceStatuses, [taskId]: status },
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  initAgentListeners: async () => {
    if (get().listenersReady) {
      return;
    }
    set({ listenersReady: true });
    await api.onAgentEvent((event) => {
      set((state) => ({
        agentEvents: {
          ...state.agentEvents,
          [event.runId]: [...(state.agentEvents[event.runId] ?? []), event],
        },
      }));
      if (event.type === "finished" || event.type === "failed") {
        void get().refreshAgentStatus(event.runId);
      }
    });
    await api.onAgentStatus((payload) => {
      set((state) => ({
        agentRuns: state.agentRuns.map((run) =>
          run.id === payload.runId ? { ...run, status: payload.status } : run,
        ),
      }));
    });
    await api.onPlanReady((payload) => {
      set((state) => ({
        planVersions: {
          ...state.planVersions,
          [payload.taskId]: payload.version,
        },
        planVersionList: {
          ...state.planVersionList,
          [payload.taskId]: [
            ...(state.planVersionList[payload.taskId] ?? []).filter(
              (version) => version.version !== payload.version.version,
            ),
            payload.version,
          ].sort((a, b) => a.version - b.version),
        },
        tasks: state.tasks.map((task) =>
          task.id === payload.taskId
            ? {
                ...task,
                status: "PLAN_READY",
                planVersion: payload.version.version,
              }
            : task,
        ),
        activeTask:
          state.activeTask && state.activeTask.id === payload.taskId
            ? {
                ...state.activeTask,
                status: "PLAN_READY",
                planVersion: payload.version.version,
              }
            : state.activeTask,
      }));
    });
    await api.onPlanError((payload) => {
      set({ error: payload.error });
      set((state) => ({
        tasks: state.tasks.map((task) =>
          task.id === payload.taskId ? { ...task, status: "FAILED" } : task,
        ),
      }));
    });
    await api.onTaskUpdated((task) => {
      set((state) => ({
        tasks: state.tasks.some((existing) => existing.id === task.id)
          ? state.tasks.map((existing) =>
              existing.id === task.id ? task : existing,
            )
          : [task, ...state.tasks],
        activeTask: state.activeTask?.id === task.id ? task : state.activeTask,
      }));
    });
    await api.onPlanMessage((message) => {
      set((state) => ({
        planMessages: {
          ...state.planMessages,
          [message.taskId]: [
            ...(state.planMessages[message.taskId] ?? []),
            message,
          ],
        },
      }));
    });
    await api.onImplementationError((payload) => {
      set({ error: payload.error });
    });
    await api.onTestResult((run) => {
      set((state) => {
        const existing = state.testRuns[run.taskId] ?? [];
        if (existing.some((item) => item.id === run.id)) {
          return {};
        }
        return {
          testRuns: {
            ...state.testRuns,
            [run.taskId]: [...existing, run],
          },
        };
      });
    });
    await api.onValidationError((payload) => {
      set({ error: payload.error });
    });
    await api.onValidationResult((run) => {
      set((state) => {
        const existing = state.validationRuns[run.taskId] ?? [];
        if (existing.some((item) => item.id === run.id)) {
          return {};
        }
        return {
          validationRuns: {
            ...state.validationRuns,
            [run.taskId]: [...existing, run],
          },
        };
      });
    });
    await api.onCommitError((payload) => {
      set({ error: payload.error });
    });
    await api.onPrCreated((pr) => {
      set((state) => ({
        pullRequests: { ...state.pullRequests, [pr.taskId]: pr },
      }));
    });
    await api.onPrError((payload) => {
      set({ error: payload.error });
    });
    await api.onJiraSynced((sync) => {
      set((state) => ({
        jiraSyncs: { ...state.jiraSyncs, [sync.taskId]: sync },
      }));
    });
    await api.onJiraError((payload) => {
      set({ error: payload.error });
    });
    await api.onNotification((notification) => {
      set((state) => {
        if (state.notifications.some((item) => item.id === notification.id)) {
          return {};
        }
        return { notifications: [notification, ...state.notifications] };
      });
      if (notificationEventType(notification) === "jira:issue_created") {
        void notify(
          `New JIRA Ticket: ${notificationJiraKey(notification)}`,
          notificationSummary(notification),
        );
      }
    });
    await api.onCloudStatus((payload) => {
      set({ cloudSocketStatus: payload.status });
    });
    await api.onCloudEvent((event) => {
      if (event.event === "task.updated") {
        const jiraKey = typeof event.jiraKey === "string" ? event.jiraKey : null;
        const status = typeof event.status === "string" ? event.status : null;
        if (jiraKey && status && isTaskStatus(status)) {
          set((state) => ({
            tasks: state.tasks.map((task) =>
              task.jiraIssueKey === jiraKey ? { ...task, status } : task,
            ),
          }));
        }
      } else if (event.event === "plan.ready") {
        const jiraKey = typeof event.jiraKey === "string" ? event.jiraKey : null;
        const task = jiraKey
          ? get().tasks.find((item) => item.jiraIssueKey === jiraKey)
          : undefined;
        if (task) {
          set((state) => ({
            tasks: state.tasks.map((item) =>
              item.id === task.id ? { ...item, status: "PLAN_READY" } : item,
            ),
          }));
          void get().loadPlan(task.id);
        }
      } else if (event.event === "pr.created") {
        const jiraKey = typeof event.jiraKey === "string" ? event.jiraKey : null;
        const task = jiraKey
          ? get().tasks.find((item) => item.jiraIssueKey === jiraKey)
          : undefined;
        if (task) {
          const pullRequest: PullRequest = {
            id: `remote-${task.id}`,
            taskId: task.id,
            provider: "github",
            number: typeof event.prNumber === "number" ? event.prNumber : 0,
            url: typeof event.prUrl === "string" ? event.prUrl : "",
            branch: typeof event.branch === "string" ? event.branch : "",
            baseBranch: "",
            status: "open",
            createdAt: new Date().toISOString(),
          };
          set((state) => ({
            pullRequests: { ...state.pullRequests, [task.id]: pullRequest },
          }));
        }
      }
    });
    await api.onPlanApproved((payload) => {
      set((state) => ({
        planApprovals: {
          ...state.planApprovals,
          [payload.taskId]: payload.approval,
        },
        tasks: state.tasks.map((task) =>
          task.id === payload.taskId
            ? {
                ...task,
                status: "APPROVED",
                approvedPlanVersion: payload.approval.planVersion,
              }
            : task,
        ),
        activeTask:
          state.activeTask?.id === payload.taskId
            ? {
                ...state.activeTask,
                status: "APPROVED",
                approvedPlanVersion: payload.approval.planVersion,
              }
            : state.activeTask,
      }));
    });
  },

  loadAgents: async () => {
    try {
      const agentRuns = await api.listAgents();
      set({ agentRuns });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadModels: async () => {
    try {
      const models = await api.listModels();
      set({ models });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  startAgent: async (targetDir, prompt, agent, model) => {
    set({ error: null });
    try {
      const run = await api.startAgent(targetDir, prompt, agent, model);
      set((state) => ({
        agentRuns: [run, ...state.agentRuns],
        activeRunId: run.id,
        agentEvents: { ...state.agentEvents, [run.id]: [] },
        agentOutput: { ...state.agentOutput, [run.id]: "" },
      }));
      return run;
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  stopAgent: async (runId) => {
    set({ error: null });
    try {
      await api.stopAgent(runId);
      await get().refreshAgentStatus(runId);
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  refreshAgentStatus: async (runId) => {
    try {
      const view = await api.getAgentStatus(runId);
      set((state) => ({
        agentOutput: { ...state.agentOutput, [runId]: view.output },
        agentEvents: { ...state.agentEvents, [runId]: view.events },
        agentCost: { ...state.agentCost, [runId]: view.cost },
        agentModel: { ...state.agentModel, [runId]: view.model ?? null },
        agentRuns: state.agentRuns.map((run) =>
          run.id === runId ? view.run : run,
        ),
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  setActiveRun: (activeRunId) => set({ activeRunId }),
  setActiveTask: (activeTask) => set({ activeTask }),

  loadTasks: async () => {
    try {
      const tasks = await api.listTasks();
      set({ tasks });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadPlan: async (taskId) => {
    try {
      const version = await api.getPlan(taskId);
      if (version) {
        set((state) => ({
          planVersions: { ...state.planVersions, [taskId]: version },
        }));
      }
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadPlanVersions: async (taskId) => {
    try {
      const versions = await api.listPlanVersions(taskId);
      set((state) => ({
        planVersionList: { ...state.planVersionList, [taskId]: versions },
        planVersions: {
          ...state.planVersions,
          ...(versions.length > 0
            ? { [taskId]: versions[versions.length - 1] }
            : {}),
        },
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadPlanMessages: async (taskId) => {
    try {
      const messages = await api.listPlanMessages(taskId);
      set((state) => ({
        planMessages: { ...state.planMessages, [taskId]: messages },
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  revisePlan: async (taskId, feedback, model) => {
    set({ error: null });
    try {
      const message = await api.revisePlan(taskId, feedback, model);
      set((state) => {
        const existing = state.planMessages[taskId] ?? [];
        if (existing.some((item) => item.id === message.id)) {
          return {};
        }
        return {
          planMessages: {
            ...state.planMessages,
            [taskId]: [...existing, message],
          },
        };
      });
      return message;
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadPlanApproval: async (taskId) => {
    try {
      const approval = await api.getPlanApproval(taskId);
      if (approval) {
        set((state) => ({
          planApprovals: { ...state.planApprovals, [taskId]: approval },
        }));
      }
    } catch (error) {
      set({ error: String(error) });
    }
  },

  approvePlan: async (taskId, planVersion, userId) => {
    set({ error: null });
    try {
      const approval = await api.approvePlan(taskId, planVersion, userId);
      set((state) => ({
        planApprovals: { ...state.planApprovals, [taskId]: approval },
        tasks: state.tasks.map((task) =>
          task.id === taskId
            ? {
                ...task,
                status: "APPROVED",
                approvedPlanVersion: approval.planVersion,
              }
            : task,
        ),
        activeTask:
          state.activeTask?.id === taskId
            ? {
                ...state.activeTask,
                status: "APPROVED",
                approvedPlanVersion: approval.planVersion,
              }
            : state.activeTask,
      }));
      return approval;
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  rejectPlan: async (taskId) => {
    set({ error: null });
    try {
      const task = await api.rejectPlan(taskId);
      set((state) => ({
        tasks: state.tasks.map((existing) =>
          existing.id === taskId ? task : existing,
        ),
        activeTask: state.activeTask?.id === taskId ? task : state.activeTask,
      }));
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  startImplementation: async (taskId, model) => {
    set({ error: null });
    try {
      await api.startImplementation(taskId, model);
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  resumeTask: async (taskId) => {
    set({ error: null });
    try {
      await api.resumeTask(taskId);
      await get().loadTasks();
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadWorkspaceStats: async (taskId) => {
    try {
      const stats = await api.getWorkspaceStats(taskId);
      set((state) => ({
        workspaceStats: { ...state.workspaceStats, [taskId]: stats },
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadWorkspaceDiffs: async (taskId) => {
    try {
      const diffs = await api.getWorkspaceDiffs(taskId);
      set((state) => ({
        workspaceDiffs: { ...state.workspaceDiffs, [taskId]: diffs },
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadTestRuns: async (taskId) => {
    try {
      const runs = await api.listTestRuns(taskId);
      set((state) => ({ testRuns: { ...state.testRuns, [taskId]: runs } }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  runTests: async (taskId) => {
    set({ error: null });
    try {
      await api.runTests(taskId);
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadValidationConfig: async (repositoryId) => {
    try {
      const config = await api.getValidationConfig(repositoryId);
      set((state) => ({
        validationConfigs: { ...state.validationConfigs, [repositoryId]: config },
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  saveValidationConfig: async (repositoryId, config) => {
    set({ error: null });
    try {
      await api.setValidationConfig(repositoryId, config);
      set((state) => ({
        validationConfigs: { ...state.validationConfigs, [repositoryId]: config },
      }));
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadValidationRuns: async (taskId) => {
    try {
      const runs = await api.listValidationRuns(taskId);
      set((state) => ({
        validationRuns: { ...state.validationRuns, [taskId]: runs },
      }));
    } catch (error) {
      set({ error: String(error) });
    }
  },

  runFinalValidation: async (taskId) => {
    set({ error: null });
    try {
      await api.runFinalValidation(taskId);
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  commitChanges: async (taskId, message) => {
    set({ error: null });
    try {
      return await api.commitChanges(taskId, message);
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadGithubAccount: async () => {
    try {
      const account = await api.getGithubAccount();
      set({ githubAccount: account });
    } catch (error) {
      set({ githubAccount: null, error: String(error) });
    }
  },

  setGithubToken: async (token) => {
    set({ error: null });
    try {
      await api.setGithubToken(token);
      const account = await api.getGithubAccount();
      set({ githubAccount: account });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  createPullRequest: async (taskId, title, body) => {
    set({ error: null });
    try {
      await api.createPullRequest(taskId, title, body);
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadPullRequest: async (taskId) => {
    try {
      const pr = await api.getPullRequest(taskId);
      if (pr) {
        set((state) => ({
          pullRequests: { ...state.pullRequests, [taskId]: pr },
        }));
      }
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadJira: async () => {
    try {
      const [connection, account, jiraStatusMap, jiraProjectRepos] =
        await Promise.all([
          api.getJiraConnectionInfo(),
          api.getJiraAccount(),
          api.getJiraStatusMap(),
          api.listJiraProjectRepos(),
        ]);
      set({
        jiraConnection: connection,
        jiraAccount: account,
        jiraStatusMap,
        jiraProjectRepos,
      });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  connectJira: async () => {
    set({ error: null });
    try {
      const connection = await api.startJiraOAuth();
      set({ jiraConnection: connection, jiraAccount: connection.account ?? null });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  registerJiraWebhook: async () => {
    set({ error: null });
    try {
      const connection = await api.registerJiraWebhook();
      set({ jiraConnection: connection });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  disconnectJira: async () => {
    set({ error: null });
    try {
      await api.disconnectJira();
      set({ jiraAccount: null, jiraConnection: null });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  saveJiraStatusMap: async (map) => {
    set({ error: null });
    try {
      await api.setJiraStatusMap(map);
      set({ jiraStatusMap: map });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadJiraProjectRepos: async () => {
    try {
      const jiraProjectRepos = await api.listJiraProjectRepos();
      set({ jiraProjectRepos });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  setJiraProjectRepo: async (projectKey, repositoryId) => {
    set({ error: null });
    try {
      await api.setJiraProjectRepo(projectKey, repositoryId);
      await get().loadJiraProjectRepos();
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  deleteJiraProjectRepo: async (projectKey) => {
    set({ error: null });
    try {
      await api.deleteJiraProjectRepo(projectKey);
      await get().loadJiraProjectRepos();
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadJiraSync: async (taskId) => {
    try {
      const sync = await api.getJiraSync(taskId);
      if (sync) {
        set((state) => ({
          jiraSyncs: { ...state.jiraSyncs, [taskId]: sync },
        }));
      }
    } catch (error) {
      set({ error: String(error) });
    }
  },

  syncTaskJira: async (taskId) => {
    set({ error: null });
    try {
      const sync = await api.syncTaskJira(taskId);
      set((state) => ({
        jiraSyncs: { ...state.jiraSyncs, [taskId]: sync },
      }));
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadCloudAccount: async () => {
    try {
      const cloudAccount = await api.getCloudAccount();
      set({ cloudAccount });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loginCloud: async (baseUrl, email, password, register) => {
    set({ error: null });
    try {
      const cloudAccount = await api.loginCloud(
        baseUrl,
        email,
        password,
        register,
      );
      set({ cloudAccount });
      void get().startCloudSync();
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  logoutCloud: async () => {
    set({ error: null });
    try {
      await api.logoutCloud();
      await get().stopCloudSync();
      set({ cloudAccount: null, notifications: [], cloudSocketStatus: "signedOut" });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  startCloudSync: async () => {
    try {
      await api.startCloudSync();
    } catch (error) {
      set({ error: String(error) });
    }
  },

  stopCloudSync: async () => {
    try {
      await api.stopCloudSync();
    } catch {
      // Stopping the socket is best-effort.
    }
  },

  markNotificationRead: async (id) => {
    set((state) => ({
      notifications: state.notifications.map((notification) =>
        notification.id === id
          ? { ...notification, readAt: notification.readAt ?? new Date().toISOString() }
          : notification,
      ),
    }));
    try {
      await api.markNotificationRead(id);
    } catch {
      // Marking read on the server is best-effort.
    }
  },

  skipTicket: async (notification) => {
    set({ error: null });
    const jiraKey = notificationJiraKey(notification);
    const summary = notificationSummary(notification);
    const task = await api.skipTicket(jiraKey, summary, notification.id);
    set((state) => ({
      tasks: [task, ...state.tasks],
      notifications: state.notifications.map((item) =>
        item.id === notification.id
          ? { ...item, readAt: item.readAt ?? new Date().toISOString() }
          : item,
      ),
    }));
  },

  prepareTicketIntake: async (jiraKey, notificationId) => {
    const intake = await api.prepareTicketIntake(jiraKey, notificationId);
    if (notificationId) {
      set((state) => ({
        notifications: state.notifications.map((item) =>
          item.id === notificationId
            ? { ...item, readAt: item.readAt ?? new Date().toISOString() }
            : item,
        ),
      }));
    }
    return intake;
  },

  loadRepairOnValidationFailure: async () => {
    try {
      const enabled = await api.getRepairOnValidationFailure();
      set({ repairOnValidationFailure: enabled });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  setRepairOnValidationFailure: async (enabled) => {
    set({ error: null });
    try {
      await api.setRepairOnValidationFailure(enabled);
      set({ repairOnValidationFailure: enabled });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadCloseToTray: async () => {
    try {
      const enabled = await api.getCloseToTray();
      set({ closeToTray: enabled });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  setCloseToTray: async (enabled) => {
    set({ error: null });
    try {
      await api.setCloseToTray(enabled);
      set({ closeToTray: enabled });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  loadMetrics: async (name, limit) => {
    try {
      const metrics = await api.listMetrics(name, limit ?? 50);
      set({ metrics });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  openLogsFolder: async () => {
    set({ error: null });
    try {
      const logsDir = await api.getLogsDir();
      set({ logsDir });
      await api.openLogsFolder();
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  startPlanning: async (
    repositoryId,
    key,
    title,
    description,
    acceptanceCriteria,
    model,
  ) => {
    set({ error: null });
    try {
      const task = await api.startPlanning(
        repositoryId,
        key,
        title,
        description,
        acceptanceCriteria,
        model,
      );
      set((state) => ({
        tasks: [task, ...state.tasks],
        activeTask: task,
      }));
      return task;
    } catch (error) {
      set({ error: String(error) });
      throw error;
    }
  },

  pushEvent: (event) => set((state) => ({ events: [...state.events, event] })),
  setError: (error) => set({ error }),
}));
