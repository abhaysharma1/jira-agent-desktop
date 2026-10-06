import { useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import {
  AlertTriangle,
  CheckCircle2,
  ClipboardList,
  GitBranch,
  Plus,
  RefreshCw,
  Sparkles,
  Trash2,
  XCircle,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { PageHeader } from "@/components/layout/PageHeader";
import { useAppStore } from "@/stores/appStore";
import { api } from "@/api/tauri";
import type { AgentTask, TaskStatus } from "@/types";

const DEFAULT_MODEL = "opencode/nemotron-3-ultra-free";

/** Terminal buckets shown on the History tab. */
const HISTORY_GROUPS: {
  label: string;
  icon: typeof ClipboardList;
  statuses: TaskStatus[];
}[] = [
  {
    label: "Completed",
    icon: CheckCircle2,
    statuses: ["PR_CREATED", "WAITING_FOR_REVIEW"],
  },
  { label: "Failed", icon: AlertTriangle, statuses: ["FAILED"] },
  { label: "Closed", icon: XCircle, statuses: ["SKIPPED", "REJECTED"] },
];

const HISTORY_STATUSES = new Set<TaskStatus>(
  HISTORY_GROUPS.flatMap((group) => group.statuses),
);

function TaskStatusBadge({ status }: { status: TaskStatus }) {
  const variant =
    status === "FAILED"
      ? "destructive"
      : status === "PLAN_READY" ||
          status === "APPROVED" ||
          status === "INTERRUPTED"
        ? "outline"
        : "secondary";
  return (
    <Badge
      variant={variant}
      className={
        status === "INTERRUPTED"
          ? "border-amber-500 text-amber-600"
          : undefined
      }
    >
      {status}
    </Badge>
  );
}

export function Task() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const ticketParam = searchParams.get("ticket");
  const repositories = useAppStore((state) => state.repositories);
  const models = useAppStore((state) => state.models);
  const tasks = useAppStore((state) => state.tasks);
  const activeTask = useAppStore((state) => state.activeTask);
  const planVersions = useAppStore((state) => state.planVersions);
  const workspaces = useAppStore((state) => state.workspaces);
  const workspaceStatuses = useAppStore((state) => state.workspaceStatuses);
  const error = useAppStore((state) => state.error);

  const loadRepositories = useAppStore((state) => state.loadRepositories);
  const loadModels = useAppStore((state) => state.loadModels);
  const loadTasks = useAppStore((state) => state.loadTasks);
  const loadPlan = useAppStore((state) => state.loadPlan);
  const startPlanning = useAppStore((state) => state.startPlanning);
  const setActiveTask = useAppStore((state) => state.setActiveTask);
  const loadWorkspaces = useAppStore((state) => state.loadWorkspaces);
  const createWorkspace = useAppStore((state) => state.createWorkspace);
  const destroyWorkspace = useAppStore((state) => state.destroyWorkspace);
  const refreshWorkspaceStatus = useAppStore(
    (state) => state.refreshWorkspaceStatus,
  );

  const [repositoryId, setRepositoryId] = useState("");
  const [key, setKey] = useState("CC-142");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [acceptance, setAcceptance] = useState("");
  const [model, setModel] = useState(DEFAULT_MODEL);
  const [planning, setPlanning] = useState(false);
  const [taskTab, setTaskTab] = useState<"active" | "history">(
    searchParams.get("tab") === "history" ? "history" : "active",
  );

  const [wsRepositoryId, setWsRepositoryId] = useState("");
  const [wsTaskId, setWsTaskId] = useState("CC-142");
  const [wsSlug, setWsSlug] = useState("");
  const [wsBusy, setWsBusy] = useState(false);

  useEffect(() => {
    void loadRepositories();
    void loadModels();
    void loadTasks();
    void loadWorkspaces();
  }, [loadRepositories, loadModels, loadTasks, loadWorkspaces]);

  useEffect(() => {
    if (!repositoryId && repositories.length > 0) {
      setRepositoryId(repositories[0].id);
      setWsRepositoryId(repositories[0].id);
    }
  }, [repositories, repositoryId]);

  useEffect(() => {
    if (activeTask && !planVersions[activeTask.id]) {
      void loadPlan(activeTask.id);
    }
  }, [activeTask, planVersions, loadPlan]);

  useEffect(() => {
    for (const workspace of workspaces) {
      if (!workspaceStatuses[workspace.taskId]) {
        void refreshWorkspaceStatus(workspace.taskId);
      }
    }
  }, [workspaces, workspaceStatuses, refreshWorkspaceStatus]);

  useEffect(() => {
    if (!ticketParam) {
      return;
    }
    setKey(ticketParam);
    void api
      .getJiraIssue(ticketParam)
      .then((issue) => {
        setTitle(issue.summary);
        setDescription(issue.description);
      })
      .catch(() => {
        // The ticket could not be loaded; the user can still fill it in.
      });
  }, [ticketParam]);

  const activePlan = activeTask ? planVersions[activeTask.id] : undefined;

  async function handleCreatePlan() {
    if (!repositoryId || !key.trim() || !title.trim()) {
      return;
    }
    setPlanning(true);
    try {
      await startPlanning(
        repositoryId,
        key.trim(),
        title.trim(),
        description.trim(),
        acceptance.trim() || undefined,
        model || undefined,
      );
    } catch {
      // Surfaced through the store.
    } finally {
      setPlanning(false);
    }
  }

  async function handleCreateWorkspace() {
    if (!wsRepositoryId || !wsTaskId.trim()) {
      return;
    }
    setWsBusy(true);
    try {
      await createWorkspace(
        wsRepositoryId,
        wsTaskId.trim(),
        wsSlug.trim() || undefined,
      );
    } catch {
      // Surfaced through the store.
    } finally {
      setWsBusy(false);
    }
  }

  const activeTasks = tasks.filter((task) => !HISTORY_STATUSES.has(task.status));
  const historyTasks = tasks.filter((task) => HISTORY_STATUSES.has(task.status));

  function renderTaskRow(task: AgentTask) {
    return (
      <button
        key={task.id}
        type="button"
        onClick={() => {
          setActiveTask(task);
          navigate(`/tasks/${task.id}`);
        }}
        className={`flex items-center gap-2 rounded-lg border p-2 text-left text-sm ${
          task.id === activeTask?.id ? "border-ring bg-muted" : "border-input"
        }`}
      >
        <TaskStatusBadge status={task.status} />
        <span className="font-mono text-xs">{task.jiraIssueKey}</span>
        <span className="truncate">{task.title}</span>
      </button>
    );
  }

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Task"
        description="A single JIRA ticket moving through the workflow."
      />
      <div className="flex flex-col gap-4 p-6">
        <Card>
          <CardHeader>
            <CardDescription>Planning (read-only)</CardDescription>
            <CardTitle className="flex items-center gap-2">
              <ClipboardList className="size-4" />
              Create a plan from a ticket
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <div className="flex flex-wrap gap-2">
              <label className="flex flex-1 flex-col gap-1 text-xs text-muted-foreground">
                Repository
                <select
                  className="h-8 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                  value={repositoryId}
                  onChange={(event) => setRepositoryId(event.target.value)}
                >
                  {repositories.length === 0 ? (
                    <option value="">No repositories</option>
                  ) : (
                    repositories.map((repository) => (
                      <option key={repository.id} value={repository.id}>
                        {repository.name}
                      </option>
                    ))
                  )}
                </select>
              </label>
              <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                Ticket key
                <input
                  className="h-8 w-32 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                  value={key}
                  onChange={(event) => setKey(event.target.value)}
                  placeholder="CC-142"
                />
              </label>
              <label className="flex flex-[2] flex-col gap-1 text-xs text-muted-foreground">
                Model
                <select
                  className="h-8 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                  value={model}
                  onChange={(event) => setModel(event.target.value)}
                >
                  {models.length === 0 ? (
                    <option value={DEFAULT_MODEL}>{DEFAULT_MODEL}</option>
                  ) : (
                    models.map((item) => (
                      <option key={item.id} value={item.id}>
                        {item.id} {item.free ? "(free)" : "(paid)"}
                      </option>
                    ))
                  )}
                </select>
              </label>
            </div>
            <label className="flex flex-col gap-1 text-xs text-muted-foreground">
              Title
              <input
                className="h-8 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                placeholder="Add pagination to Problems API"
              />
            </label>
            <div className="flex gap-2">
              <label className="flex flex-1 flex-col gap-1 text-xs text-muted-foreground">
                Description
                <textarea
                  className="min-h-20 rounded-lg border border-input bg-background p-2 text-sm text-foreground"
                  value={description}
                  onChange={(event) => setDescription(event.target.value)}
                />
              </label>
              <label className="flex flex-1 flex-col gap-1 text-xs text-muted-foreground">
                Acceptance criteria (optional)
                <textarea
                  className="min-h-20 rounded-lg border border-input bg-background p-2 text-sm text-foreground"
                  value={acceptance}
                  onChange={(event) => setAcceptance(event.target.value)}
                />
              </label>
            </div>
            <div>
              <Button
                onClick={handleCreatePlan}
                disabled={
                  planning || !repositoryId || !key.trim() || !title.trim()
                }
              >
                <Sparkles className="size-4" />
                {planning ? "Planning..." : "Create Plan"}
              </Button>
            </div>
            {error ? (
              <p className="text-sm text-destructive">{error}</p>
            ) : null}
          </CardContent>
        </Card>

        <div className="grid gap-4 lg:grid-cols-2">
          <Card>
            <CardHeader>
              <CardDescription>Tasks</CardDescription>
              <CardTitle className="flex items-center justify-between gap-2">
                <span>{tasks.length} total</span>
                <span className="flex gap-1">
                  <Button
                    variant={taskTab === "active" ? "secondary" : "ghost"}
                    size="sm"
                    onClick={() => setTaskTab("active")}
                  >
                    Active
                  </Button>
                  <Button
                    variant={taskTab === "history" ? "secondary" : "ghost"}
                    size="sm"
                    onClick={() => setTaskTab("history")}
                  >
                    History
                  </Button>
                </span>
              </CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-2">
              {taskTab === "active" ? (
                activeTasks.length === 0 ? (
                  <p className="text-sm text-muted-foreground">
                    No active tasks.
                  </p>
                ) : (
                  activeTasks.map(renderTaskRow)
                )
              ) : historyTasks.length === 0 ? (
                <p className="text-sm text-muted-foreground">
                  Nothing finished yet.
                </p>
              ) : (
                HISTORY_GROUPS.map((group) => {
                  const groupTasks = tasks.filter((task) =>
                    group.statuses.includes(task.status),
                  );
                  if (groupTasks.length === 0) {
                    return null;
                  }
                  const Icon = group.icon;
                  return (
                    <div key={group.label} className="flex flex-col gap-2">
                      <p className="flex items-center gap-1.5 pt-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                        <Icon className="size-3.5" />
                        {group.label}
                      </p>
                      {groupTasks.map(renderTaskRow)}
                    </div>
                  );
                })
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardDescription>
                {activeTask ? activeTask.jiraIssueKey : "No active task"}
              </CardDescription>
              <CardTitle className="flex items-center gap-2">
                <ClipboardList className="size-4" />
                Implementation plan
              </CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-3 text-sm">
              {!activeTask ? (
                <p className="text-muted-foreground">
                  Create a plan to see it here.
                </p>
              ) : (
                <>
                  <div className="flex items-center gap-2">
                    <TaskStatusBadge status={activeTask.status} />
                    {activePlan ? (
                      <Badge variant="secondary">
                        plan v{activePlan.version}
                      </Badge>
                    ) : null}
                  </div>
                  <p className="text-muted-foreground">
                    {activePlan
                      ? activePlan.content.summary
                      : activeTask.status === "PLANNING"
                        ? "Planning in progress..."
                        : "No plan available."}
                  </p>
                  <div>
                    <Button
                      variant="outline"
                      onClick={() => navigate(`/tasks/${activeTask.id}`)}
                    >
                      Review plan
                    </Button>
                  </div>
                </>
              )}
            </CardContent>
          </Card>
        </div>

        <Card>
          <CardHeader>
            <CardDescription>Isolated Git worktree</CardDescription>
            <CardTitle className="flex items-center gap-2">
              <GitBranch className="size-4" />
              Workspaces
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <div className="flex flex-wrap items-end gap-2">
              <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                Repository
                <select
                  className="h-8 min-w-44 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                  value={wsRepositoryId}
                  onChange={(event) => setWsRepositoryId(event.target.value)}
                >
                  {repositories.length === 0 ? (
                    <option value="">No repositories</option>
                  ) : (
                    repositories.map((repository) => (
                      <option key={repository.id} value={repository.id}>
                        {repository.name}
                      </option>
                    ))
                  )}
                </select>
              </label>
              <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                Task id
                <input
                  className="h-8 w-32 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                  value={wsTaskId}
                  onChange={(event) => setWsTaskId(event.target.value)}
                  placeholder="CC-142"
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                Branch slug (optional)
                <input
                  className="h-8 w-56 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                  value={wsSlug}
                  onChange={(event) => setWsSlug(event.target.value)}
                  placeholder="add pagination"
                />
              </label>
              <Button
                onClick={handleCreateWorkspace}
                disabled={wsBusy || !wsRepositoryId || !wsTaskId.trim()}
              >
                <Plus className="size-4" />
                {wsBusy ? "Creating..." : "Create workspace"}
              </Button>
            </div>

            {workspaces.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                No workspaces yet. Creating one adds a git worktree under{" "}
                <span className="font-mono text-xs">
                  ~/.jira-agent/workspaces/
                </span>
                .
              </p>
            ) : (
              <div className="grid gap-2">
                {workspaces.map((workspace) => {
                  const status = workspaceStatuses[workspace.taskId];
                  return (
                    <div
                      key={workspace.id}
                      className="flex flex-wrap items-center gap-2 rounded-lg border p-3 text-sm"
                    >
                      <Badge variant="secondary">{workspace.taskId}</Badge>
                      <span className="font-mono text-xs">
                        {workspace.branchName}
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {workspace.path}
                      </span>
                      {status ? (
                        <Badge
                          variant={status.isClean ? "outline" : "destructive"}
                        >
                          {status.exists
                            ? status.isClean
                              ? "clean"
                              : `${status.changedFiles} changed`
                            : "missing"}
                        </Badge>
                      ) : null}
                      <span className="ml-auto flex items-center gap-1">
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          title="Refresh"
                          onClick={() =>
                            void refreshWorkspaceStatus(workspace.taskId)
                          }
                        >
                          <RefreshCw className="size-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          title="Destroy workspace"
                          onClick={() => {
                            void destroyWorkspace(workspace.taskId);
                          }}
                        >
                          <Trash2 className="size-4" />
                        </Button>
                      </span>
                    </div>
                  );
                })}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
