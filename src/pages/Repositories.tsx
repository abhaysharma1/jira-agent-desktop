import { useEffect, useState } from "react";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { FolderGit2, Plus, RefreshCw, Trash2 } from "lucide-react";
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

export function Repositories() {
  const repositories = useAppStore((state) => state.repositories);
  const statuses = useAppStore((state) => state.repositoryStatuses);
  const loading = useAppStore((state) => state.loading);
  const error = useAppStore((state) => state.error);
  const loadRepositories = useAppStore((state) => state.loadRepositories);
  const addRepository = useAppStore((state) => state.addRepository);
  const removeRepository = useAppStore((state) => state.removeRepository);
  const refreshRepositoryStatus = useAppStore(
    (state) => state.refreshRepositoryStatus,
  );
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void loadRepositories();
  }, [loadRepositories]);

  async function handleAdd() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Select a Git repository",
    });
    if (!selected || Array.isArray(selected)) {
      return;
    }
    setBusy(true);
    try {
      await addRepository(selected);
    } catch {
      // Error is surfaced through the store.
    } finally {
      setBusy(false);
    }
  }

  async function handleRemove(repositoryId: string, name: string) {
    const confirmed = await ask(
      `Remove ${name}? This does not delete any files on disk.`,
      { title: "Remove repository", kind: "warning" },
    );
    if (!confirmed) {
      return;
    }
    try {
      await removeRepository(repositoryId);
    } catch {
      // Error is surfaced through the store.
    }
  }

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Repositories"
        description="Local repositories the agent can work in."
      />
      <div className="flex items-center justify-between px-6 py-4">
        <p className="text-sm text-muted-foreground">
          {repositories.length} registered
        </p>
        <Button onClick={handleAdd} disabled={busy}>
          <Plus className="size-4" />
          {busy ? "Adding..." : "Add Repository"}
        </Button>
      </div>
      {error ? (
        <p className="px-6 pb-4 text-sm text-destructive">{error}</p>
      ) : null}
      <div className="px-6 pb-6">
        {loading ? (
          <p className="text-sm text-muted-foreground">Loading...</p>
        ) : repositories.length === 0 ? (
          <Card>
            <CardHeader>
              <CardDescription>No repositories yet</CardDescription>
              <CardTitle className="flex items-center gap-2">
                <FolderGit2 className="size-4" />
                Add a local Git repository to get started
              </CardTitle>
            </CardHeader>
            <CardContent className="text-sm text-muted-foreground">
              The app detects the Git root, reads the remote URL and default
              branch, and reports clean/dirty state.
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-3">
            {repositories.map((repository) => {
              const status = statuses[repository.id];
              return (
                <Card key={repository.id}>
                  <CardHeader>
                    <CardTitle className="flex items-center gap-2">
                      <FolderGit2 className="size-4" />
                      {repository.name}
                    </CardTitle>
                    <CardDescription>{repository.localPath}</CardDescription>
                  </CardHeader>
                  <CardContent className="flex flex-wrap items-center gap-2 text-xs">
                    {status ? (
                      status.isGitRepository ? (
                        <>
                          <Badge variant="secondary">
                            {status.currentBranch ?? "detached"}
                          </Badge>
                          <Badge
                            variant={
                              status.isClean ? "outline" : "destructive"
                            }
                          >
                            {status.isClean
                              ? "clean"
                              : `${status.changedFiles} changed`}
                          </Badge>
                        </>
                      ) : (
                        <Badge variant="destructive">not a git repository</Badge>
                      )
                    ) : (
                      <span className="text-muted-foreground">checking...</span>
                    )}
                    {repository.remoteUrl ? (
                      <span className="text-muted-foreground">
                        {repository.remoteUrl}
                      </span>
                    ) : null}
                    <span className="ml-auto flex items-center gap-1">
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        title="Refresh status"
                        onClick={() =>
                          void refreshRepositoryStatus(repository.id)
                        }
                      >
                        <RefreshCw className="size-4" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        title="Remove"
                        onClick={() =>
                          void handleRemove(repository.id, repository.name)
                        }
                      >
                        <Trash2 className="size-4" />
                      </Button>
                    </span>
                    <TestCommandEditor repositoryId={repository.id} />
                  </CardContent>
                </Card>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

function TestCommandEditor({ repositoryId }: { repositoryId: string }) {
  const config = useAppStore((state) => state.validationConfigs[repositoryId]);
  const loadValidationConfig = useAppStore(
    (state) => state.loadValidationConfig,
  );
  const saveValidationConfig = useAppStore(
    (state) => state.saveValidationConfig,
  );
  const [value, setValue] = useState("");
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    void loadValidationConfig(repositoryId);
  }, [repositoryId, loadValidationConfig]);

  useEffect(() => {
    setValue(config?.test ?? "");
  }, [config?.test]);

  async function handleSave() {
    try {
      await saveValidationConfig(repositoryId, {
        ...config,
        test: value.trim() || undefined,
      });
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1500);
    } catch {
      // Surfaced through the store.
    }
  }

  return (
    <div className="mt-1 flex w-full items-center gap-2">
      <span className="text-muted-foreground">Test command</span>
      <input
        className="h-7 flex-1 rounded-lg border border-input bg-background px-2 font-mono text-xs text-foreground"
        value={value}
        placeholder="npm test"
        onChange={(event) => setValue(event.target.value)}
      />
      <Button variant="outline" size="sm" onClick={handleSave}>
        {saved ? "Saved" : "Save"}
      </Button>
    </div>
  );
}
