import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  ArrowLeft,
  Bot,
  CheckCircle2,
  ClipboardList,
  FileText,
  FlaskConical,
  ListChecks,
  Loader2,
  Play,
  RotateCcw,
  Send,
  User,
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
import type { TaskStatus } from "@/types";

function statusVariant(
  status: TaskStatus,
): "secondary" | "outline" | "destructive" {
  if (status === "FAILED" || status === "REJECTED") {
    return "destructive";
  }
  if (
    status === "APPROVED" ||
    status === "PLAN_READY" ||
    status === "INTERRUPTED"
  ) {
    return "outline";
  }
  return "secondary";
}

export function PlanReview() {
  const { taskId = "" } = useParams<{ taskId: string }>();

  const tasks = useAppStore((state) => state.tasks);
  const repositories = useAppStore((state) => state.repositories);
  const planVersions = useAppStore((state) => state.planVersions);
  const planVersionList = useAppStore((state) => state.planVersionList);
  const planMessages = useAppStore((state) => state.planMessages);
  const planApprovals = useAppStore((state) => state.planApprovals);
  const error = useAppStore((state) => state.error);
  const loadTasks = useAppStore((state) => state.loadTasks);
  const loadRepositories = useAppStore((state) => state.loadRepositories);
  const loadPlan = useAppStore((state) => state.loadPlan);
  const loadPlanVersions = useAppStore((state) => state.loadPlanVersions);
  const loadPlanMessages = useAppStore((state) => state.loadPlanMessages);
  const loadPlanApproval = useAppStore((state) => state.loadPlanApproval);
  const initAgentListeners = useAppStore((state) => state.initAgentListeners);
  const approvePlan = useAppStore((state) => state.approvePlan);
  const rejectPlan = useAppStore((state) => state.rejectPlan);
  const revisePlan = useAppStore((state) => state.revisePlan);
  const startImplementation = useAppStore((state) => state.startImplementation);
  const resumeTask = useAppStore((state) => state.resumeTask);

  const [busy, setBusy] = useState(false);
  const [revising, setRevising] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [selected, setSelected] = useState<number | null>(null);

  const task = tasks.find((item) => item.id === taskId);
  const approval = planApprovals[taskId];
  const messages = planMessages[taskId] ?? [];
  const repository = task
    ? repositories.find((item) => item.id === task.repositoryId)
    : undefined;

  const versions = useMemo(() => {
    const list = planVersionList[taskId];
    if (list && list.length > 0) {
      return list;
    }
    const latest = planVersions[taskId];
    return latest ? [latest] : [];
  }, [planVersionList, planVersions, taskId]);

  const latestVersion = versions.length
    ? versions[versions.length - 1].version
    : undefined;

  useEffect(() => {
    if (latestVersion != null) {
      setSelected(latestVersion);
    }
  }, [latestVersion]);

  const selectedVersion =
    versions.find((item) => item.version === selected) ??
    versions[versions.length - 1];

  useEffect(() => {
    void initAgentListeners();
    void loadRepositories();
    if (tasks.length === 0) {
      void loadTasks();
    }
  }, [initAgentListeners, loadRepositories, loadTasks, tasks.length]);

  useEffect(() => {
    if (!taskId) {
      return;
    }
    void loadPlanVersions(taskId);
    void loadPlanMessages(taskId);
    if (!planVersions[taskId]) {
      void loadPlan(taskId);
    }
  }, [
    taskId,
    loadPlanVersions,
    loadPlanMessages,
    loadPlan,
    planVersions,
  ]);

  useEffect(() => {
    if (task?.status === "APPROVED" && !planApprovals[taskId]) {
      void loadPlanApproval(taskId);
    }
  }, [task, taskId, planApprovals, loadPlanApproval]);

  const canDecide = task?.status === "PLAN_READY";

  async function handleApprove() {
    if (!task || !selectedVersion) {
      return;
    }
    setBusy(true);
    try {
      await approvePlan(task.id, selectedVersion.version);
    } catch {
      // Surfaced through the store.
    } finally {
      setBusy(false);
    }
  }

  async function handleReject() {
    if (!task) {
      return;
    }
    const confirmed = await ask(
      `Reject the plan for ${task.jiraIssueKey}?`,
      { title: "Reject plan", kind: "warning" },
    );
    if (!confirmed) {
      return;
    }
    setBusy(true);
    try {
      await rejectPlan(task.id);
    } catch {
      // Surfaced through the store.
    } finally {
      setBusy(false);
    }
  }

  async function handleSend() {
    if (!task || !feedback.trim() || !canDecide) {
      return;
    }
    setRevising(true);
    try {
      await revisePlan(task.id, feedback.trim());
      setFeedback("");
    } catch {
      // Surfaced through the store.
    } finally {
      setRevising(false);
    }
  }

  async function handleStartImplementation() {
    if (!task) {
      return;
    }
    setBusy(true);
    try {
      await startImplementation(task.id);
    } catch {
      // Surfaced through the store.
    } finally {
      setBusy(false);
    }
  }

  async function handleResume() {
    if (!task) {
      return;
    }
    setBusy(true);
    try {
      await resumeTask(task.id);
    } catch {
      // Surfaced through the store.
    } finally {
      setBusy(false);
    }
  }

  if (!task) {
    return (
      <div className="flex flex-col">
        <PageHeader title="Plan" description="Review an implementation plan." />
        <div className="p-6">
          <p className="text-sm text-muted-foreground">Task not found.</p>
          <Link
            to="/tasks"
            className="mt-2 inline-flex items-center gap-1 text-sm text-primary"
          >
            <ArrowLeft className="size-4" /> Back to tasks
          </Link>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      <PageHeader
        title={`${task.jiraIssueKey} - ${task.title}`}
        description={repository ? repository.name : task.repositoryId}
      />
      <div className="flex flex-col gap-4 p-6">
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <Link
            to="/tasks"
            className="inline-flex items-center gap-1 text-primary"
          >
            <ArrowLeft className="size-4" /> Tasks
          </Link>
          <Badge variant={statusVariant(task.status)}>{task.status}</Badge>
          {approval ? (
            <span className="text-xs text-muted-foreground">
              approved by {approval.userId} (v{approval.planVersion}) at{" "}
              {new Date(approval.approvedAt).toLocaleString()}
            </span>
          ) : null}
          {task.workspacePath ? (
            <span className="font-mono text-xs text-muted-foreground">
              {task.branchName} - {task.workspacePath}
            </span>
          ) : null}
        </div>

        {versions.length > 0 ? (
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs text-muted-foreground">Versions</span>
            {versions.map((version) => (
              <button
                key={version.id}
                type="button"
                onClick={() => setSelected(version.version)}
                className={`rounded-md border px-2 py-1 text-xs ${
                  version.version === selectedVersion?.version
                    ? "border-ring bg-muted font-medium"
                    : "border-input"
                }`}
              >
                v{version.version}
                {version.approved ? " ✓" : ""}
              </button>
            ))}
          </div>
        ) : null}

        {!selectedVersion ? (
          <Card>
            <CardContent className="pt-6 text-sm text-muted-foreground">
              {task.status === "PLANNING"
                ? "Planning in progress..."
                : "No plan available for this task."}
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-4 lg:grid-cols-3">
            <Card className="lg:col-span-2">
              <CardHeader>
                <CardDescription>AI Understanding</CardDescription>
                <CardTitle>Summary</CardTitle>
              </CardHeader>
              <CardContent className="flex flex-col gap-4 text-sm">
                <p className="font-medium">
                  {selectedVersion.content.summary}
                </p>
                <p className="whitespace-pre-wrap text-muted-foreground">
                  {selectedVersion.content.understanding}
                </p>
              </CardContent>
            </Card>

            <Card>
              <CardHeader>
                <CardDescription>Metadata</CardDescription>
                <CardTitle>Plan</CardTitle>
              </CardHeader>
              <CardContent className="flex flex-col gap-2 text-sm">
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Ticket</span>
                  <span className="font-mono">
                    {selectedVersion.content.ticket}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Version</span>
                  <span>{selectedVersion.version}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Created by</span>
                  <span>{selectedVersion.createdBy}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Approved</span>
                  <span>{selectedVersion.approved ? "yes" : "no"}</span>
                </div>
              </CardContent>
            </Card>

            <Card className="lg:col-span-2">
              <CardHeader>
                <CardDescription>Implementation Plan</CardDescription>
                <CardTitle className="flex items-center gap-2">
                  <ListChecks className="size-4" />
                  Steps
                </CardTitle>
              </CardHeader>
              <CardContent>
                <ol className="flex flex-col gap-2">
                  {selectedVersion.content.steps.map((step) => (
                    <li key={step.order} className="rounded-lg border p-3 text-sm">
                      <p className="font-medium">
                        {step.order}. {step.description}
                      </p>
                      {step.files.length > 0 ? (
                        <div className="mt-2 flex flex-wrap gap-1">
                          {step.files.map((file) => (
                            <Badge key={file} variant="secondary">
                              <FileText className="size-3" />
                              {file}
                            </Badge>
                          ))}
                        </div>
                      ) : null}
                    </li>
                  ))}
                </ol>
              </CardContent>
            </Card>

            <div className="flex flex-col gap-4">
              <Card>
                <CardHeader>
                  <CardDescription>Tests</CardDescription>
                  <CardTitle className="flex items-center gap-2">
                    <FlaskConical className="size-4" />
                    {selectedVersion.content.tests.length}
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  {selectedVersion.content.tests.length === 0 ? (
                    <p className="text-sm text-muted-foreground">None listed.</p>
                  ) : (
                    <ul className="list-disc pl-5 text-sm">
                      {selectedVersion.content.tests.map((test) => (
                        <li key={test}>{test}</li>
                      ))}
                    </ul>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardHeader>
                  <CardDescription>Risks</CardDescription>
                  <CardTitle className="flex items-center gap-2">
                    <AlertTriangle className="size-4" />
                    {selectedVersion.content.risks.length}
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  {selectedVersion.content.risks.length === 0 ? (
                    <p className="text-sm text-muted-foreground">None listed.</p>
                  ) : (
                    <ul className="list-disc pl-5 text-sm">
                      {selectedVersion.content.risks.map((risk) => (
                        <li key={risk}>{risk}</li>
                      ))}
                    </ul>
                  )}
                </CardContent>
              </Card>
            </div>
          </div>
        )}

        <Card>
          <CardHeader>
            <CardDescription>Plan conversation</CardDescription>
            <CardTitle className="flex items-center gap-2">
              {revising ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Bot className="size-4" />
              )}
              Discuss before approving
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <div className="flex max-h-72 flex-col gap-2 overflow-auto rounded-lg border p-3">
              {messages.length === 0 ? (
                <p className="text-sm text-muted-foreground">
                  No feedback yet. Ask OpenCode to change the plan.
                </p>
              ) : (
                messages.map((message) => (
                  <div
                    key={message.id}
                    className={`flex items-start gap-2 text-sm ${
                      message.role === "user"
                        ? "text-foreground"
                        : "text-muted-foreground"
                    }`}
                  >
                    {message.role === "user" ? (
                      <User className="mt-0.5 size-3.5 shrink-0" />
                    ) : (
                      <Bot className="mt-0.5 size-3.5 shrink-0" />
                    )}
                    <span className="whitespace-pre-wrap break-words">
                      {message.content}
                    </span>
                  </div>
                ))
              )}
              {revising ? (
                <p className="flex items-center gap-2 text-xs text-muted-foreground">
                  <Loader2 className="size-3 animate-spin" /> Revising plan...
                </p>
              ) : null}
            </div>
            <div className="flex gap-2">
              <textarea
                className="min-h-16 flex-1 rounded-lg border border-input bg-background p-2 text-sm text-foreground"
                value={feedback}
                onChange={(event) => setFeedback(event.target.value)}
                placeholder="e.g. Don't change the existing response format."
                disabled={!canDecide || revising}
              />
              <Button
                onClick={handleSend}
                disabled={!canDecide || revising || !feedback.trim()}
              >
                <Send className="size-4" />
                Send
              </Button>
            </div>
            {!canDecide ? (
              <p className="text-xs text-muted-foreground">
                Feedback is available while the task is PLAN_READY.
              </p>
            ) : null}
          </CardContent>
        </Card>

        {error ? <p className="text-sm text-destructive">{error}</p> : null}

        <Card>
          <CardContent className="flex flex-wrap items-center gap-3 pt-6">
            {canDecide ? (
              <>
                <Button variant="outline" onClick={handleReject} disabled={busy}>
                  <XCircle className="size-4" />
                  Reject
                </Button>
                <Button
                  onClick={handleApprove}
                  disabled={busy || !selectedVersion}
                >
                  <CheckCircle2 className="size-4" />
                  {busy
                    ? "Working..."
                    : `Approve Plan v${selectedVersion?.version ?? ""}`}
                </Button>
              </>
            ) : task.status === "APPROVED" ? (
              <div className="flex flex-wrap items-center gap-3">
                <p className="flex items-center gap-2 text-sm text-muted-foreground">
                  <CheckCircle2 className="size-4" />
                  Plan approved (v
                  {approval?.planVersion ?? task.approvedPlanVersion}).
                </p>
                <Button onClick={handleStartImplementation} disabled={busy}>
                  <Play className="size-4" />
                  Start implementation
                </Button>
              </div>
            ) : task.status === "WORKSPACE_CREATING" ||
              task.status === "IMPLEMENTING" ||
              task.status === "TESTING" ? (
              <div className="flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                <span>Implementation in progress ({task.status}).</span>
                <Link to={`/tasks/${task.id}/run`} className="text-primary">
                  Open execution
                </Link>
              </div>
            ) : task.status === "INTERRUPTED" ? (
              <div className="flex flex-wrap items-center gap-3">
                <p className="flex items-center gap-2 text-sm text-amber-600">
                  <AlertTriangle className="size-4" />
                  Interrupted during {task.interruptedFrom ?? "a previous run"} by an
                  app restart.
                </p>
                <Button onClick={handleResume} disabled={busy}>
                  {busy ? (
                    <Loader2 className="size-4 animate-spin" />
                  ) : (
                    <RotateCcw className="size-4" />
                  )}
                  Resume
                </Button>
              </div>
            ) : task.status === "REJECTED" ? (
              <p className="flex items-center gap-2 text-sm text-muted-foreground">
                <XCircle className="size-4" />
                Plan rejected.
              </p>
            ) : (
              <p className="flex items-center gap-2 text-sm text-muted-foreground">
                <ClipboardList className="size-4" />
                No decision available for status {task.status}.
              </p>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
