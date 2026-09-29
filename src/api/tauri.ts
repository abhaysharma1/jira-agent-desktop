import { invoke } from "@tauri-apps/api/core";
import type {
  AgentEvent,
  AppInfo,
  Repository,
  RepositoryStatus,
  ValidationResult,
  Workspace,
} from "@/types";

export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),

  listRepositories: () => invoke<Repository[]>("list_repositories"),
  addRepository: (localPath: string) =>
    invoke<Repository>("add_repository", { localPath }),
  getRepositoryStatus: (repositoryId: string) =>
    invoke<RepositoryStatus>("get_repository_status", { repositoryId }),

  createWorkspace: (repositoryId: string, taskId: string) =>
    invoke<Workspace>("create_workspace", { repositoryId, taskId }),
  deleteWorkspace: (workspacePath: string) =>
    invoke<void>("delete_workspace", { workspacePath }),
  getWorkspaceStatus: (workspacePath: string) =>
    invoke<Workspace>("get_workspace_status", { workspacePath }),

  getChangedFiles: (workspacePath: string) =>
    invoke<string[]>("get_changed_files", { workspacePath }),
  getDiff: (workspacePath: string) =>
    invoke<string>("get_diff", { workspacePath }),
  commitChanges: (workspacePath: string, message: string) =>
    invoke<string>("commit_changes", { workspacePath, message }),
  pushBranch: (workspacePath: string) =>
    invoke<void>("push_branch", { workspacePath }),
  runValidation: (workspacePath: string, command: string) =>
    invoke<ValidationResult>("run_validation", { workspacePath, command }),

  startPlanning: (taskId: string) =>
    invoke<string>("start_planning", { taskId }),
  startImplementation: (taskId: string, approvedPlanVersion: number) =>
    invoke<string>("start_implementation", { taskId, approvedPlanVersion }),
  sendAgentMessage: (runId: string, content: string) =>
    invoke<void>("send_agent_message", { runId, content }),
  stopAgent: (runId: string) => invoke<void>("stop_agent", { runId }),
  getAgentStatus: (runId: string) =>
    invoke<AgentEvent[]>("get_agent_status", { runId }),
};
