import { useEffect, useMemo, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  Bot,
  Play,
  Square,
} from "lucide-react";
import { AgentTimeline } from "@/components/agent/AgentTimeline";
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
import type { AgentEvent, AgentRunStatus } from "@/types";

const DEFAULT_MODEL = "opencode/nemotron-3-ultra-free";

const DEFAULT_PROMPT =
  "Inspect this repository and explain its architecture. Do not modify anything.";

const AGENTS = ["plan", "build", "explore", "general"];

function statusVariant(
  status: AgentRunStatus,
): "secondary" | "outline" | "destructive" {
  switch (status) {
    case "RUNNING":
    case "PENDING":
      return "secondary";
    case "FAILED":
    case "CANCELLED":
      return "destructive";
    default:
      return "outline";
  }
}

export function Agent() {
  const repositories = useAppStore((state) => state.repositories);
  const agentRuns = useAppStore((state) => state.agentRuns);
  const agentEvents = useAppStore((state) => state.agentEvents);
  const agentOutput = useAppStore((state) => state.agentOutput);
  const agentCost = useAppStore((state) => state.agentCost);
  const agentModel = useAppStore((state) => state.agentModel);
  const models = useAppStore((state) => state.models);
  const activeRunId = useAppStore((state) => state.activeRunId);
  const error = useAppStore((state) => state.error);
  const loadRepositories = useAppStore((state) => state.loadRepositories);
  const loadAgents = useAppStore((state) => state.loadAgents);
  const loadModels = useAppStore((state) => state.loadModels);
  const initAgentListeners = useAppStore((state) => state.initAgentListeners);
  const startAgent = useAppStore((state) => state.startAgent);
  const stopAgent = useAppStore((state) => state.stopAgent);
  const setActiveRun = useAppStore((state) => state.setActiveRun);

  const [targetDir, setTargetDir] = useState("");
  const [agent, setAgent] = useState("plan");
  const [model, setModel] = useState(DEFAULT_MODEL);
  const [prompt, setPrompt] = useState(DEFAULT_PROMPT);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void initAgentListeners();
    void loadRepositories();
    void loadAgents();
    void loadModels();
  }, [initAgentListeners, loadRepositories, loadAgents, loadModels]);

  useEffect(() => {
    if (!targetDir && repositories.length > 0) {
      setTargetDir(repositories[0].localPath);
    }
  }, [repositories, targetDir]);

  const groupedModels = useMemo(() => {
    const groups = new Map<string, typeof models>();
    for (const item of models) {
      const list = groups.get(item.providerId) ?? [];
      list.push(item);
      groups.set(item.providerId, list);
    }
    return Array.from(groups.entries());
  }, [models]);

  const activeRun = useMemo(
    () => agentRuns.find((run) => run.id === activeRunId) ?? agentRuns[0],
    [agentRuns, activeRunId],
  );

  const events = activeRun ? agentEvents[activeRun.id] ?? [] : [];
  const streamText = useMemo(
    () =>
      events
        .filter(
          (event): event is Extract<AgentEvent, { type: "message" }> =>
            event.type === "message",
        )
        .map((event) => event.content)
        .join(""),
    [events],
  );
  const output = activeRun ? agentOutput[activeRun.id] ?? "" : "";
  const running =
    activeRun?.status === "RUNNING" || activeRun?.status === "PENDING";
  const selectedModel = models.find((item) => item.id === model);

  async function handleRun() {
    if (!targetDir.trim() || !prompt.trim()) {
      return;
    }
    if (selectedModel && !selectedModel.free) {
      const confirmed = await ask(
        `${selectedModel.name} is a paid model. Continue and incur token costs?`,
        { title: "Paid model", kind: "warning" },
      );
      if (!confirmed) {
        return;
      }
    }
    setBusy(true);
    try {
      await startAgent(
        targetDir.trim(),
        prompt,
        agent || undefined,
        model || undefined,
      );
    } catch {
      // Surfaced through the store.
    } finally {
      setBusy(false);
    }
  }

  async function handleStop() {
    if (!activeRun) {
      return;
    }
    try {
      await stopAgent(activeRun.id);
    } catch {
      // Surfaced through the store.
    }
  }

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Agent"
        description="Run OpenCode against a repository and watch it work live."
      />
      <div className="grid gap-4 p-6 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardDescription>New run (read-only)</CardDescription>
            <CardTitle className="flex items-center gap-2">
              <Bot className="size-4" />
              OpenCode prompt
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <label className="flex flex-col gap-1 text-xs text-muted-foreground">
              Repository
              <select
                className="h-8 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                value={
                  repositories.some((repo) => repo.localPath === targetDir)
                    ? targetDir
                    : ""
                }
                onChange={(event) => setTargetDir(event.target.value)}
              >
                <option value="">Custom path...</option>
                {repositories.map((repository) => (
                  <option key={repository.id} value={repository.localPath}>
                    {repository.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex flex-col gap-1 text-xs text-muted-foreground">
              Target directory
              <input
                className="h-8 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                value={targetDir}
                onChange={(event) => setTargetDir(event.target.value)}
                placeholder="C:\Projects\MyRepo"
              />
            </label>
            <div className="flex gap-2">
              <label className="flex flex-1 flex-col gap-1 text-xs text-muted-foreground">
                Agent
                <select
                  className="h-8 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                  value={agent}
                  onChange={(event) => setAgent(event.target.value)}
                >
                  {AGENTS.map((name) => (
                    <option key={name} value={name}>
                      {name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="flex flex-[2] flex-col gap-1 text-xs text-muted-foreground">
                Model
                <select
                  className="h-8 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
                  value={model}
                  onChange={(event) => setModel(event.target.value)}
                >
                  {groupedModels.length === 0 ? (
                    <option value={DEFAULT_MODEL}>{DEFAULT_MODEL}</option>
                  ) : (
                    groupedModels.map(([providerId, items]) => (
                      <optgroup key={providerId} label={providerId}>
                        {items.map((item) => (
                          <option key={item.id} value={item.id}>
                            {item.name} {item.free ? "(free)" : "(paid)"}
                          </option>
                        ))}
                      </optgroup>
                    ))
                  )}
                </select>
              </label>
            </div>
            <div className="flex items-center gap-2 text-xs">
              <Badge variant={selectedModel?.free ? "outline" : "destructive"}>
                {selectedModel
                  ? selectedModel.free
                    ? "free model"
                    : "paid model"
                  : "default model"}
              </Badge>
              <span className="text-muted-foreground">{model}</span>
            </div>
            <label className="flex flex-col gap-1 text-xs text-muted-foreground">
              Prompt
              <textarea
                className="min-h-24 rounded-lg border border-input bg-background p-2 text-sm text-foreground"
                value={prompt}
                onChange={(event) => setPrompt(event.target.value)}
              />
            </label>
            <div className="flex gap-2">
              <Button onClick={handleRun} disabled={busy || running}>
                <Play className="size-4" />
                {busy ? "Starting..." : "Run"}
              </Button>
              <Button variant="outline" onClick={handleStop} disabled={!running}>
                <Square className="size-4" />
                Stop
              </Button>
            </div>
            {error ? <p className="text-sm text-destructive">{error}</p> : null}
          </CardContent>
        </Card>

        <div className="flex flex-col gap-4">
          <Card>
            <CardHeader>
              <CardDescription>Runs</CardDescription>
              <CardTitle>{agentRuns.length} total</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-2">
              {agentRuns.length === 0 ? (
                <p className="text-sm text-muted-foreground">No runs yet.</p>
              ) : (
                agentRuns.map((run) => (
                  <button
                    key={run.id}
                    type="button"
                    onClick={() => setActiveRun(run.id)}
                    className={`flex items-center gap-2 rounded-lg border p-2 text-left text-sm ${
                      run.id === activeRun?.id
                        ? "border-ring bg-muted"
                        : "border-input"
                    }`}
                  >
                    <Badge variant={statusVariant(run.status)}>
                      {run.status}
                    </Badge>
                    <span className="font-medium">{run.agent ?? run.mode}</span>
                    <span className="ml-auto font-mono text-xs text-muted-foreground">
                      {run.id.slice(0, 8)}
                    </span>
                  </button>
                ))
              )}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardDescription>
                {activeRun ? `Run ${activeRun.id.slice(0, 8)}` : "Activity"}
              </CardDescription>
              <CardTitle>Live activity</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-3">
              <AgentTimeline events={events} className="max-h-56" />
              <pre className="max-h-72 overflow-auto rounded-lg border bg-muted p-3 text-xs whitespace-pre-wrap">
                {output || streamText || "Waiting for the agent response..."}
              </pre>
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                {activeRun ? (
                  <>
                    <span>
                      model: {agentModel[activeRun.id] ?? "unknown"}
                    </span>
                    <span>
                      cost: ${(agentCost[activeRun.id] ?? 0).toFixed(4)}
                    </span>
                  </>
                ) : null}
              </div>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
