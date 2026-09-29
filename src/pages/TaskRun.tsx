import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  CheckCircle2,
  Circle,
  FileCode,
  FolderOpen,
  GitBranch,
  Loader2,
  Play,
  RefreshCw,
  Terminal,
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
import { AgentTimeline } from "@/components/agent/AgentTimeline";
import { PageHeader } from "@/components/layout/PageHeader";
import { useAppStore } from "@/stores/appStore";
import type { AgentEvent, AgentTask, TaskStatus } from "@/types";

interface Milestone {
  label: string;
  done: boolean;
  active: boolean;
}

const POST_APPROVAL: TaskStatus[] = [
  "APPROVED",
  "WORKSPACE_CREATING",
  "IMPLEMENTING",
  "TESTING",
  "REPAIRING",
  "VALIDATING",
  "COMMITTING",
  "PR_CREATING",
  "PR_CREATED",
  "WAITING_FOR_REVIEW",
];

function buildMilestones(task: AgentTask, events: AgentEvent[]): Milestone[] {
  const has = (type: AgentEvent["type"]) => events.some((event) => event.type === type);
  const status = task.status;

  const raw: Array<{ label: string; done: boolean }> = [
    {
      label: "Plan approved",
      done: task.approvedPlanVersion != null || POST_APPROVAL.includes(status),
    },
    { label: "Workspace created", done: task.workspacePath != null },
    { label: "Branch created", done: task.branchName != null },
    { label: "Repository analyzed", done: has("file_read") || has("tool") },
    { label: "Files modified", done: has("file_changed") },
    {
      label: "Running tests",
      done: status === "TESTING" || status === "VALIDATING" || has("test_result"),
    },
    { label: "Validation", done: has("finished") && status === "VALIDATING" },
    { label: "Commit", done: status === "COMMITTING" || status === "PR_CREATING" },
    { label: "Pull request", done: status === "PR_CREATED" || status === "WAITING_FOR_REVIEW" },
  ];

  const firstPending = raw.findIndex((item) => !item.done);
  return raw.map((item, index) => ({
    label: item.label,
    done: item.done,
    active: !item.done && index === firstPending,
  }));
}

export function TaskRun() {
  const { taskId = "" } = useParams<{ taskId: string }>();

  const tasks = useAppStore((state) => state.tasks);
  const repositories = useAppStore((state) => state.repositories);
  const agentRuns = useAppStore((state) => state.agentRuns);
  const agentEvents = useAppStore((state) => state.agentEvents);
  const workspaceStats = useAppStore((state) => state.workspaceStats);
  const testRuns = useAppStore((state) => state.testRuns);
  const validationRuns = useAppStore((state) => state.validationRuns);
  const repairOnValidationFailure = useAppStore(
    (state) => state.repairOnValidationFailure,
  );
  const error = useAppStore((state) => state.error);
  const loadTasks = useAppStore((state) => state.loadTasks);
  const loadRepositories = useAppStore((state) => state.loadRepositories);
  const loadAgents = useAppStore((state) => state.loadAgents);
  const loadWorkspaceStats = useAppStore((state) => state.loadWorkspaceStats);
  const loadTestRuns = useAppStore((state) => state.loadTestRuns);
  const runTests = useAppStore((state) => state.runTests);
  const loadValidationRuns = useAppStore((state) => state.loadValidationRuns);
  const runFinalValidation = useAppStore((state) => state.runFinalValidation);
  const loadRepairOnValidationFailure = useAppStore(
    (state) => state.loadRepairOnValidationFailure,
  );
  const setRepairOnValidationFailure = useAppStore(
    (state) => state.setRepairOnValidationFailure,
  );
  const initAgentListeners = useAppStore((state) => state.initAgentListeners);

  const [refreshing, setRefreshing] = useState(false);
  const [testing, setTesting] = useState(false);
  const [validating, setValidating] = useState(false);

  const task = tasks.find((item) => item.id === taskId);
  const repository = task
    ? repositories.find((item) => item.id === task.repositoryId)
    : undefined;

  const run = useMemo(() => {
    const taskRuns = agentRuns
      .filter((item) => item.taskId === taskId)
      .sort((a, b) => (a.startedAt < b.startedAt ? 1 : -1));
    return (
      taskRuns.find((item) => item.mode === "implementation") ??
      taskRuns[0]
    );
  }, [agentRuns, taskId]);

  const events = run ? agentEvents[run.id] ?? [] : [];
  const stats = workspaceStats[taskId];
  const runs = testRuns[taskId] ?? [];
  const lastRun = runs[runs.length - 1];
  const validationList = validationRuns[taskId] ?? [];
  const lastValidation = validationList[validationList.length - 1];
  const finished = events.some((event) => event.type === "finished");

  useEffect(() => {
    void initAgentListeners();
    void loadRepositories();
    void loadAgents();
    if (tasks.length === 0) {
      void loadTasks();
    }
  }, [initAgentListeners, loadRepositories, loadAgents, loadTasks, tasks.length]);

  useEffect(() => {
    if (task?.workspacePath) {
      void loadWorkspaceStats(taskId);
    }
  }, [task?.id, task?.workspacePath, loadWorkspaceStats, taskId]);

  useEffect(() => {
    if (finished && task?.workspacePath) {
      void loadWorkspaceStats(taskId);
    }
  }, [finished, task?.workspacePath, loadWorkspaceStats, taskId]);

  useEffect(() => {
    if (task) {
      void loadTestRuns(taskId);
    }
  }, [task?.id, task?.status, loadTestRuns, taskId]);

  useEffect(() => {
    if (task) {
      void loadValidationRuns(taskId);
    }
  }, [task?.id, task?.status, loadValidationRuns, taskId]);

  useEffect(() => {
    void loadRepairOnValidationFailure();
  }, [loadRepairOnValidationFailure]);

  const commands = useMemo(
    () =>
      events.filter(
        (event) =>
          event.type === "command_started" ||
          event.type === "command_finished",
      ),
    [events],
  );

  async function handleRefresh() {
    setRefreshing(true);
    try {
      await loadWorkspaceStats(taskId);
    } finally {
      setRefreshing(false);
    }
  }

  async function handleRunTests() {
    setTesting(true);
    try {
      await runTests(taskId);
    } catch {
      // Surfaced through the store.
    } finally {
      setTesting(false);
    }
  }

  async function handleValidate() {
    setValidating(true);
    try {
      await runFinalValidation(taskId);
    } catch {
      // Surfaced through the store.
    } finally {
      setValidating(false);
    }
  }

  async function handleToggleRepair(enabled: boolean) {
    try {
      await setRepairOnValidationFailure(enabled);
    } catch {
      // Surfaced through the store.
    }
  }

  if (!task) {
    return (
      <div className="flex flex-col">
        <PageHeader title="Execution" description="Agent progress for a task." />
        <div className="p-6">
          <p className="text-sm text-muted-foreground">Task not found.</p>
          <Link
            to="/tasks"
            className="mt-2 inline-flex items-center gap-1 text-sm text-primary"
          >
            ← Back to tasks
          </Link>
        </div>
      </div>
    );
  }

  const milestones = buildMilestones(task, events);

  return (
    <div className="flex flex-col">
      <PageHeader
        title={`${task.jiraIssueKey} - ${task.title}`}
        description={repository ? repository.name : task.repositoryId}
      />
      <div className="flex flex-col gap-4 p-6">
        <div className="flex flex-wrap items-center gap-3 text-sm">
          <Link to={`/tasks/${taskId}`} className="text-primary">
            ← Plan review
          </Link>
          <Badge variant="secondary">{task.status}</Badge>
          {task.branchName ? (
            <span className="flex items-center gap-1 font-mono text-xs text-muted-foreground">
              <GitBranch className="size-3.5" />
              {task.branchName}
            </span>
          ) : null}
          {task.workspacePath ? (
            <span className="font-mono text-xs text-muted-foreground">
              {task.workspacePath}
            </span>
          ) : null}
          <Link to={`/tasks/${taskId}/diff`} className="text-primary">
            View diff →
          </Link>
        </div>

        <div className="grid gap-4 lg:grid-cols-3">
          <Card className="lg:col-span-2">
            <CardHeader>
              <CardDescription>Progress</CardDescription>
              <CardTitle>Milestones</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-2 text-sm">
              {milestones.map((milestone) => (
                <div key={milestone.label} className="flex items-center gap-2">
                  {milestone.done ? (
                    <CheckCircle2 className="size-4 text-green-600" />
                  ) : milestone.active ? (
                    <Loader2 className="size-4 animate-spin text-primary" />
                  ) : (
                    <Circle className="size-4 text-muted-foreground" />
                  )}
                  <span
                    className={
                      milestone.done
                        ? ""
                        : milestone.active
                          ? "font-medium"
                          : "text-muted-foreground"
                    }
                  >
                    {milestone.label}
                  </span>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardDescription>Changes</CardDescription>
              <CardTitle className="flex items-center justify-between">
                Workspace diff
                <Button
                  variant="ghost"
                  size="icon-sm"
                  title="Refresh stats"
                  onClick={handleRefresh}
                  disabled={refreshing || !task.workspacePath}
                >
                  <RefreshCw className="size-4" />
                </Button>
              </CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-3 text-sm">
              <div className="grid grid-cols-3 gap-2 text-center">
                <div className="rounded-lg border p-2">
                  <div className="text-lg font-semibold">
                    {stats?.filesChanged ?? 0}
                  </div>
                  <div className="text-xs text-muted-foreground">files</div>
                </div>
                <div className="rounded-lg border p-2">
                  <div className="text-lg font-semibold text-green-600">
                    +{stats?.additions ?? 0}
                  </div>
                  <div className="text-xs text-muted-foreground">added</div>
                </div>
                <div className="rounded-lg border p-2">
                  <div className="text-lg font-semibold text-red-600">
                    -{stats?.deletions ?? 0}
                  </div>
                  <div className="text-xs text-muted-foreground">removed</div>
                </div>
              </div>
              <div className="flex max-h-48 flex-col gap-1 overflow-auto">
                {(stats?.files ?? []).map((file) => (
                  <div
                    key={file.path}
                    className="flex items-center gap-2 text-xs"
                  >
                    <FileCode className="size-3.5 shrink-0 text-muted-foreground" />
                    <span className="truncate font-mono">{file.path}</span>
                    <span className="ml-auto shrink-0 text-green-600">
                      +{file.additions}
                    </span>
                    <span className="shrink-0 text-red-600">
                      -{file.deletions}
                    </span>
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>
        </div>

        <Card>
          <CardHeader>
            <CardDescription>Testing &amp; repair</CardDescription>
            <CardTitle className="flex items-center justify-between">
              Test runs
              <Button
                onClick={handleRunTests}
                disabled={testing || !task.workspacePath}
              >
                {testing ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Play className="size-4" />
                )}
                Run tests
              </Button>
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            {runs.length === 0 ? (
              <p className="text-muted-foreground">No test runs yet.</p>
            ) : (
              <div className="flex flex-col gap-2">
                {runs.map((testRun) => (
                  <div key={testRun.id} className="rounded-lg border p-3">
                    <div className="flex flex-wrap items-center gap-2 text-xs">
                      <Badge
                        variant={testRun.passed ? "outline" : "destructive"}
                      >
                        {testRun.passed ? (
                          <CheckCircle2 className="size-3" />
                        ) : (
                          <XCircle className="size-3" />
                        )}
                        attempt {testRun.attempt}{" "}
                        {testRun.passed ? "passed" : "failed"}
                      </Badge>
                      <span className="font-mono text-muted-foreground">
                        {testRun.command}
                      </span>
                      <span className="ml-auto text-muted-foreground">
                        exit {testRun.exitCode}
                      </span>
                    </div>
                    {testRun.output ? (
                      <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded bg-muted p-2 text-xs">
                        {testRun.output}
                      </pre>
                    ) : null}
                  </div>
                ))}
              </div>
            )}
            {lastRun && !lastRun.passed ? (
              <div className="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 p-2 text-xs text-destructive">
                <AlertTriangle className="size-3.5" />
                Tests are failing.
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    task.workspacePath && void openPath(task.workspacePath)
                  }
                >
                  <FolderOpen className="size-3.5" />
                  Open workspace
                </Button>
              </div>
            ) : null}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardDescription>Independent validation</CardDescription>
            <CardTitle className="flex items-center justify-between">
              Validation
              <Button
                onClick={handleValidate}
                disabled={validating || !task.workspacePath}
              >
                {validating ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Play className="size-4" />
                )}
                Validate
              </Button>
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              <input
                type="checkbox"
                checked={repairOnValidationFailure}
                onChange={(event) =>
                  void handleToggleRepair(event.target.checked)
                }
              />
              Repair automatically on validation failure (loop up to the attempt
              cap)
            </label>
            {validationList.length === 0 ? (
              <p className="text-muted-foreground">No validation runs yet.</p>
            ) : (
              <div className="flex flex-col gap-2">
                {validationList.map((validation) => (
                  <div key={validation.id} className="rounded-lg border p-3">
                    <div className="mb-2 flex items-center gap-2 text-xs">
                      <Badge
                        variant={validation.passed ? "outline" : "destructive"}
                      >
                        {validation.passed ? (
                          <CheckCircle2 className="size-3" />
                        ) : (
                          <XCircle className="size-3" />
                        )}
                        {validation.passed ? "passed" : "failed"}
                      </Badge>
                      <span className="text-muted-foreground">
                        {new Date(validation.createdAt).toLocaleTimeString()}
                      </span>
                    </div>
                    <div className="flex flex-col gap-1">
                      {validation.results.map((result) => (
                        <div
                          key={result.command}
                          className="flex items-center gap-2 text-xs"
                        >
                          {result.passed ? (
                            <CheckCircle2 className="size-3.5 text-green-600" />
                          ) : (
                            <XCircle className="size-3.5 text-red-600" />
                          )}
                          <span className="font-mono">{result.command}</span>
                          <span className="ml-auto text-muted-foreground">
                            exit {result.exitCode}
                          </span>
                        </div>
                      ))}
                    </div>
                    {!validation.passed ? (
                      <details className="mt-2 text-xs">
                        <summary className="cursor-pointer text-muted-foreground">
                          Show output
                        </summary>
                        {validation.results
                          .filter((result) => !result.passed)
                          .map((result) => (
                            <pre
                              key={result.command}
                              className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded bg-muted p-2"
                            >
                              {result.output}
                            </pre>
                          ))}
                      </details>
                    ) : null}
                  </div>
                ))}
              </div>
            )}
            {lastValidation && !lastValidation.passed ? (
              <div className="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/40 p-2 text-xs text-destructive">
                <AlertTriangle className="size-3.5" />
                Validation failed.
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() =>
                    task.workspacePath && void openPath(task.workspacePath)
                  }
                >
                  <FolderOpen className="size-3.5" />
                  Open workspace
                </Button>
              </div>
            ) : null}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardDescription>Terminal</CardDescription>
            <CardTitle className="flex items-center gap-2">
              <Terminal className="size-4" />
              Commands
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="max-h-72 overflow-auto rounded-lg bg-neutral-950 p-3 font-mono text-xs text-neutral-100">
              {commands.length === 0 ? (
                <p className="text-neutral-500">No commands yet.</p>
              ) : (
                commands.map((event, index) =>
                  event.type === "command_started" ? (
                    <div key={index} className="text-neutral-300">
                      $ {event.command}
                    </div>
                  ) : event.type === "command_finished" ? (
                    <div key={index} className="mb-2">
                      <div
                        className={
                          event.exitCode === 0
                            ? "text-green-400"
                            : "text-red-400"
                        }
                      >
                        $ {event.command} (exit {event.exitCode})
                      </div>
                      {event.output ? (
                        <pre className="mt-1 whitespace-pre-wrap text-neutral-400">
                          {event.output}
                        </pre>
                      ) : null}
                    </div>
                  ) : null,
                )
              )}
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardDescription>
              {run ? `Run ${run.id.slice(0, 8)} (${run.mode})` : "No run yet"}
            </CardDescription>
            <CardTitle>Live activity</CardTitle>
          </CardHeader>
          <CardContent>
            <AgentTimeline events={events} className="max-h-72" />
          </CardContent>
        </Card>

        {error ? <p className="text-sm text-destructive">{error}</p> : null}
      </div>
    </div>
  );
}
