import { useEffect, useState } from "react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { PageHeader } from "@/components/layout/PageHeader";
import { api } from "@/api/tauri";
import { useAppStore } from "@/stores/appStore";
import type { Settings as SettingsModel } from "@/types";

const defaults: SettingsModel = {
  workspaceRoot: "~/.jira-agent/workspaces",
  maxRepairAttempts: 5,
  opencodeCommand: "opencode",
};

function formatAge(iso: string): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) {
    return "";
  }
  const seconds = Math.max(0, Math.round((Date.now() - then) / 1000));
  if (seconds < 60) {
    return `${seconds}s ago`;
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.round(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  return `${Math.round(hours / 24)}d ago`;
}

function formatMetric(name: string, value: number): string {
  if (name.endsWith("duration_ms")) {
    return `${(value / 1000).toFixed(1)}s`;
  }
  if (name === "files.changed") {
    return `${value} files`;
  }
  return Number.isInteger(value) ? String(value) : value.toFixed(1);
}

export function Settings() {
  const githubAccount = useAppStore((state) => state.githubAccount);
  const jiraAccount = useAppStore((state) => state.jiraAccount);
  const jiraConnection = useAppStore((state) => state.jiraConnection);
  const jiraStatusMap = useAppStore((state) => state.jiraStatusMap);
  const jiraProjectRepos = useAppStore((state) => state.jiraProjectRepos);
  const repositories = useAppStore((state) => state.repositories);
  const error = useAppStore((state) => state.error);
  const loadGithubAccount = useAppStore((state) => state.loadGithubAccount);
  const setGithubToken = useAppStore((state) => state.setGithubToken);
  const loadJira = useAppStore((state) => state.loadJira);
  const connectJira = useAppStore((state) => state.connectJira);
  const registerJiraWebhook = useAppStore((state) => state.registerJiraWebhook);
  const disconnectJira = useAppStore((state) => state.disconnectJira);
  const saveJiraStatusMap = useAppStore((state) => state.saveJiraStatusMap);
  const setJiraProjectRepo = useAppStore((state) => state.setJiraProjectRepo);
  const deleteJiraProjectRepo = useAppStore((state) => state.deleteJiraProjectRepo);
  const cloudAccount = useAppStore((state) => state.cloudAccount);
  const loadCloudAccount = useAppStore((state) => state.loadCloudAccount);
  const loginCloud = useAppStore((state) => state.loginCloud);
  const logoutCloud = useAppStore((state) => state.logoutCloud);
  const closeToTray = useAppStore((state) => state.closeToTray);
  const loadCloseToTray = useAppStore((state) => state.loadCloseToTray);
  const setCloseToTray = useAppStore((state) => state.setCloseToTray);
  const metrics = useAppStore((state) => state.metrics);
  const logsDir = useAppStore((state) => state.logsDir);
  const loadMetrics = useAppStore((state) => state.loadMetrics);
  const openLogsFolder = useAppStore((state) => state.openLogsFolder);
  const [token, setToken] = useState("");
  const [saved, setSaved] = useState(false);
  const [jiraBusy, setJiraBusy] = useState(false);
  const [webhookBusy, setWebhookBusy] = useState(false);
  const [projectKey, setProjectKey] = useState("");
  const [projectRepo, setProjectRepo] = useState("");
  const [statusOpened, setStatusOpened] = useState("");
  const [statusMerged, setStatusMerged] = useState("");
  const [statusClosed, setStatusClosed] = useState("");
  const [cloudBaseUrl, setCloudBaseUrl] = useState("http://localhost:4000");
  const [cloudEmail, setCloudEmail] = useState("");
  const [cloudPassword, setCloudPassword] = useState("");
  const [cloudBusy, setCloudBusy] = useState(false);

  useEffect(() => {
    void loadGithubAccount();
  }, [loadGithubAccount]);

  useEffect(() => {
    void loadJira();
  }, [loadJira]);

  useEffect(() => {
    void loadCloudAccount();
  }, [loadCloudAccount]);

  useEffect(() => {
    void loadCloseToTray();
  }, [loadCloseToTray]);

  useEffect(() => {
    void loadMetrics(undefined, 20);
  }, [loadMetrics]);

  useEffect(() => {
    setStatusOpened(jiraStatusMap.opened ?? "");
    setStatusMerged(jiraStatusMap.merged ?? "");
    setStatusClosed(jiraStatusMap.closed ?? "");
  }, [jiraStatusMap]);

  async function handleJiraConnect() {
    setJiraBusy(true);
    try {
      await connectJira();
    } catch {
      // Surfaced through the store.
    } finally {
      setJiraBusy(false);
    }
  }

  async function handleWebhookRegister() {
    setWebhookBusy(true);
    try {
      await registerJiraWebhook();
    } catch {
      // Surfaced through the store.
    } finally {
      setWebhookBusy(false);
    }
  }

  async function handleAddProjectRepo() {
    if (!projectKey.trim() || !projectRepo) {
      return;
    }
    try {
      await setJiraProjectRepo(projectKey.trim().toUpperCase(), projectRepo);
      setProjectKey("");
      setProjectRepo("");
    } catch {
      // Surfaced through the store.
    }
  }

  async function handleJiraStatusSave() {
    try {
      await saveJiraStatusMap({
        opened: statusOpened.trim() || undefined,
        merged: statusMerged.trim() || undefined,
        closed: statusClosed.trim() || undefined,
      });
    } catch {
      // Surfaced through the store.
    }
  }

  async function handleCloudLogin(register: boolean) {
    setCloudBusy(true);
    try {
      await loginCloud(
        cloudBaseUrl.trim(),
        cloudEmail.trim(),
        cloudPassword,
        register,
      );
      setCloudPassword("");
    } catch {
      // Surfaced through the store.
    } finally {
      setCloudBusy(false);
    }
  }

  async function handleCloseToTray(enabled: boolean) {
    try {
      await setCloseToTray(enabled);
    } catch {
      // Surfaced through the store.
    }
  }

  function handleQuit() {
    void api.quitApp().catch(() => {});
  }

  const rows: Array<[string, string]> = [
    ["Workspace root", defaults.workspaceRoot],
    ["Max repair attempts", String(defaults.maxRepairAttempts)],
    ["OpenCode command", defaults.opencodeCommand],
    ["OpenCode model", defaults.opencodeModel ?? "default"],
  ];

  async function handleSave() {
    try {
      await setGithubToken(token.trim() || undefined);
      setToken("");
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1500);
    } catch {
      // Surfaced through the store.
    }
  }

  async function handleClear() {
    try {
      await setGithubToken(undefined);
      setToken("");
    } catch {
      // Surfaced through the store.
    }
  }

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Settings"
        description="Configuration is persisted locally."
      />
      <div className="flex flex-col gap-4 p-6">
        <Card className="max-w-xl">
          <CardHeader>
            <CardTitle>Defaults</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            {rows.map(([label, value], index) => (
              <div key={label} className="flex flex-col gap-3">
                {index > 0 ? <Separator /> : null}
                <div className="flex items-center justify-between">
                  <span className="text-muted-foreground">{label}</span>
                  <span className="font-mono text-xs">{value}</span>
                </div>
              </div>
            ))}
          </CardContent>
        </Card>

        <Card className="max-w-xl">
          <CardHeader>
            <CardTitle>Desktop</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            <label className="flex items-center gap-2 text-muted-foreground">
              <input
                type="checkbox"
                checked={closeToTray}
                onChange={(event) => void handleCloseToTray(event.target.checked)}
              />
              <span>Closing the window keeps JIRA Agent in the system tray</span>
            </label>
            <Separator />
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">Quit JIRA Agent</span>
              <Button variant="outline" onClick={handleQuit}>
                Quit
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              Press Ctrl+K to open the command palette. The tray menu shows
              pending tickets and running agents.
            </p>
          </CardContent>
        </Card>

        <Card className="max-w-xl">
          <CardHeader>
            <CardTitle>Diagnostics</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">Structured logs</span>
              <Button variant="outline" onClick={() => void openLogsFolder()}>
                Open logs folder
              </Button>
            </div>
            {logsDir ? (
              <p className="break-all font-mono text-xs text-muted-foreground">
                {logsDir}
              </p>
            ) : null}
            <p className="text-xs text-muted-foreground">
              Durations, repair attempts, files changed and failures are
              recorded locally so tasks can be debugged after the fact.
            </p>
            <Separator />
            <span className="text-xs text-muted-foreground">Recent metrics</span>
            {metrics.length > 0 ? (
              <ul className="flex flex-col gap-1">
                {metrics.slice(0, 20).map((metric) => (
                  <li
                    key={metric.id}
                    className="flex items-center justify-between gap-2 text-xs"
                  >
                    <span className="font-mono">{metric.name}</span>
                    <span className="flex items-center gap-2 text-muted-foreground">
                      <span>{formatMetric(metric.name, metric.value)}</span>
                      <span>{formatAge(metric.createdAt)}</span>
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-xs text-muted-foreground">
                No metrics recorded yet.
              </p>
            )}
          </CardContent>
        </Card>

        <Card className="max-w-xl">
          <CardHeader>
            <CardDescription>
              {githubAccount
                ? `Connected as ${githubAccount}`
                : "Not connected"}
            </CardDescription>
            <CardTitle>GitHub</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            <label className="flex flex-col gap-1 text-xs text-muted-foreground">
              Personal access token
              <input
                type="password"
                className="h-8 rounded-lg border border-input bg-background px-2 font-mono text-xs text-foreground"
                value={token}
                onChange={(event) => setToken(event.target.value)}
                placeholder="ghp_..."
              />
            </label>
            <div className="flex gap-2">
              <Button onClick={handleSave} disabled={!token.trim()}>
                {saved ? "Saved" : "Save token"}
              </Button>
              <Button
                variant="outline"
                onClick={handleClear}
                disabled={!githubAccount}
              >
                Disconnect
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              Used to push branches and create pull requests. Stored locally,
              encrypted at rest with a key held in the OS credential store.
            </p>
            {error ? <p className="text-destructive">{error}</p> : null}
          </CardContent>
        </Card>

        <Card className="max-w-xl">
          <CardHeader>
            <CardDescription>
              {jiraConnection
                ? `${jiraConnection.siteName ?? "JIRA"} (${jiraConnection.siteUrl})`
                : "Not connected"}
            </CardDescription>
            <CardTitle>JIRA</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            <div className="flex gap-2">
              <Button onClick={handleJiraConnect} disabled={jiraBusy}>
                {jiraBusy
                  ? "Waiting for browser..."
                  : jiraConnection
                    ? "Reconnect"
                    : "Connect to JIRA"}
              </Button>
              <Button
                variant="outline"
                onClick={() => void disconnectJira()}
                disabled={!jiraConnection}
              >
                Disconnect
              </Button>
            </div>
            {jiraAccount ? (
              <p className="text-xs text-muted-foreground">
                Connected as {jiraAccount}
              </p>
            ) : null}
            <p className="text-xs text-muted-foreground">
              Connecting opens Atlassian in your browser. Tokens are encrypted at
              rest.
            </p>
            <Separator />
            <p className="text-xs text-muted-foreground">Inbound webhooks</p>
            <p className="break-all font-mono text-xs text-muted-foreground">
              {jiraConnection?.webhookUrl ?? "Not registered"}
            </p>
            <div>
              <Button
                variant="outline"
                onClick={handleWebhookRegister}
                disabled={webhookBusy || !jiraConnection}
              >
                {webhookBusy ? "Registering..." : "Register webhook"}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              Requires a signed-in cloud account and a publicly reachable backend
              URL.
            </p>
            <Separator />
            <p className="text-xs text-muted-foreground">
              Status transitions applied when a pull request is created.
            </p>
            <div className="grid grid-cols-3 gap-2">
              <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                PR opened
                <input
                  className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground"
                  value={statusOpened}
                  onChange={(event) => setStatusOpened(event.target.value)}
                  placeholder="In Development"
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                PR merged
                <input
                  className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground"
                  value={statusMerged}
                  onChange={(event) => setStatusMerged(event.target.value)}
                  placeholder="Done"
                />
              </label>
              <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                PR closed
                <input
                  className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground"
                  value={statusClosed}
                  onChange={(event) => setStatusClosed(event.target.value)}
                  placeholder="Closed"
                />
              </label>
            </div>
            <div>
              <Button variant="outline" onClick={handleJiraStatusSave}>
                Save status map
              </Button>
            </div>
            <Separator />
            <p className="text-xs text-muted-foreground">
              JIRA project → local repository
            </p>
            {jiraProjectRepos.length > 0 ? (
              <ul className="flex flex-col gap-1">
                {jiraProjectRepos.map((mapping) => {
                  const repository = repositories.find(
                    (candidate) => candidate.id === mapping.repositoryId,
                  );
                  return (
                    <li
                      key={mapping.id}
                      className="flex items-center justify-between gap-2 text-xs"
                    >
                      <span className="font-mono">{mapping.projectKey}</span>
                      <span className="text-muted-foreground">
                        {repository?.name ?? mapping.repositoryId}
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() =>
                          void deleteJiraProjectRepo(mapping.projectKey)
                        }
                      >
                        Remove
                      </Button>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="text-xs text-muted-foreground">No mappings yet.</p>
            )}
            <div className="flex gap-2">
              <input
                className="h-8 w-28 rounded-lg border border-input bg-background px-2 font-mono text-xs text-foreground"
                value={projectKey}
                onChange={(event) => setProjectKey(event.target.value)}
                placeholder="CC"
              />
              <select
                className="h-8 flex-1 rounded-lg border border-input bg-background px-2 text-xs text-foreground"
                value={projectRepo}
                onChange={(event) => setProjectRepo(event.target.value)}
              >
                <option value="">Select repository…</option>
                {repositories.map((repository) => (
                  <option key={repository.id} value={repository.id}>
                    {repository.name}
                  </option>
                ))}
              </select>
              <Button
                variant="outline"
                onClick={handleAddProjectRepo}
                disabled={!projectKey.trim() || !projectRepo}
              >
                Add
              </Button>
            </div>
          </CardContent>
        </Card>

        <Card className="max-w-xl">
          <CardHeader>
            <CardDescription>
              {cloudAccount
                ? `Connected to ${cloudAccount.baseUrl}`
                : "Not connected"}
            </CardDescription>
            <CardTitle>Cloud account</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3 text-sm">
            {cloudAccount ? (
              <>
                <p className="text-xs text-muted-foreground">
                  Signed in as {cloudAccount.email}
                </p>
                <p className="font-mono text-xs text-muted-foreground">
                  Device: {cloudAccount.deviceId}
                </p>
                <div>
                  <Button
                    variant="outline"
                    onClick={() => void logoutCloud()}
                  >
                    Sign out
                  </Button>
                </div>
              </>
            ) : (
              <>
                <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                  Backend URL
                  <input
                    className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground"
                    value={cloudBaseUrl}
                    onChange={(event) => setCloudBaseUrl(event.target.value)}
                    placeholder="http://localhost:4000"
                  />
                </label>
                <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                  Email
                  <input
                    className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground"
                    value={cloudEmail}
                    onChange={(event) => setCloudEmail(event.target.value)}
                    placeholder="you@example.com"
                  />
                </label>
                <label className="flex flex-col gap-1 text-xs text-muted-foreground">
                  Password
                  <input
                    type="password"
                    className="h-8 rounded-lg border border-input bg-background px-2 text-xs text-foreground"
                    value={cloudPassword}
                    onChange={(event) => setCloudPassword(event.target.value)}
                    placeholder="at least 8 characters"
                  />
                </label>
                <div className="flex gap-2">
                  <Button
                    onClick={() => void handleCloudLogin(true)}
                    disabled={
                      cloudBusy ||
                      !cloudBaseUrl.trim() ||
                      !cloudEmail.trim() ||
                      cloudPassword.length < 8
                    }
                  >
                    Create account
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => void handleCloudLogin(false)}
                    disabled={
                      cloudBusy ||
                      !cloudBaseUrl.trim() ||
                      !cloudEmail.trim() ||
                      !cloudPassword
                    }
                  >
                    Sign in
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  Registering also registers this device for notifications.
                </p>
              </>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
