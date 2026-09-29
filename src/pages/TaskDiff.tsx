import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { DiffEditor } from "@monaco-editor/react";
import { ChevronDown, ChevronRight, FileText } from "lucide-react";
import "@/lib/monaco";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { PageHeader } from "@/components/layout/PageHeader";
import { useAppStore } from "@/stores/appStore";

const LANGUAGE_BY_EXT: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  css: "css",
  scss: "scss",
  less: "less",
  html: "html",
  htm: "html",
  md: "markdown",
  py: "python",
  rs: "rust",
  go: "go",
  java: "java",
  rb: "ruby",
  php: "php",
  c: "c",
  h: "c",
  cpp: "cpp",
  cs: "csharp",
  yml: "yaml",
  yaml: "yaml",
  toml: "ini",
  sh: "shell",
  sql: "sql",
  xml: "xml",
};

function languageFor(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return LANGUAGE_BY_EXT[ext] ?? "plaintext";
}

function statusVariant(
  status: string,
): "secondary" | "outline" | "destructive" {
  if (status === "deleted") {
    return "destructive";
  }
  if (status === "added") {
    return "secondary";
  }
  return "outline";
}

export function TaskDiff() {
  const { taskId = "" } = useParams<{ taskId: string }>();

  const tasks = useAppStore((state) => state.tasks);
  const diffs = useAppStore((state) => state.workspaceDiffs[taskId]);
  const error = useAppStore((state) => state.error);
  const loadTasks = useAppStore((state) => state.loadTasks);
  const loadWorkspaceDiffs = useAppStore((state) => state.loadWorkspaceDiffs);

  const [open, setOpen] = useState<Record<string, boolean>>({});

  const task = tasks.find((item) => item.id === taskId);
  const files = diffs ?? [];

  useEffect(() => {
    if (tasks.length === 0) {
      void loadTasks();
    }
  }, [loadTasks, tasks.length]);

  useEffect(() => {
    if (taskId) {
      void loadWorkspaceDiffs(taskId);
    }
  }, [taskId, loadWorkspaceDiffs]);

  useEffect(() => {
    if (files.length > 0) {
      setOpen((current) =>
        Object.keys(current).length === 0
          ? { [files[0].path]: true }
          : current,
      );
    }
  }, [files]);

  const totals = useMemo(
    () =>
      files.reduce(
        (accumulator, file) => ({
          additions: accumulator.additions + file.additions,
          deletions: accumulator.deletions + file.deletions,
        }),
        { additions: 0, deletions: 0 },
      ),
    [files],
  );

  if (!task) {
    return (
      <div className="flex flex-col">
        <PageHeader title="Diff" description="Inspect the agent's changes." />
        <div className="p-6 text-sm text-muted-foreground">Task not found.</div>
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      <PageHeader
        title={`${task.jiraIssueKey} - Changes`}
        description={task.branchName ?? task.repositoryId}
      />
      <div className="flex flex-col gap-4 p-6">
        <div className="flex flex-wrap items-center gap-3 text-sm">
          <Link to={`/tasks/${taskId}/run`} className="text-primary">
            ← Execution
          </Link>
          <Badge variant="secondary">{files.length} files changed</Badge>
          <span className="font-mono text-xs text-green-600">
            +{totals.additions}
          </span>
          <span className="font-mono text-xs text-red-600">
            -{totals.deletions}
          </span>
        </div>

        {error ? <p className="text-sm text-destructive">{error}</p> : null}

        {files.length === 0 ? (
          <Card>
            <CardContent className="pt-6 text-sm text-muted-foreground">
              No changes detected in the workspace.
            </CardContent>
          </Card>
        ) : (
          files.map((file) => {
            const isOpen = open[file.path] ?? false;
            return (
              <Card key={file.path}>
                <CardContent className="flex flex-col gap-3 pt-4">
                  <div className="flex flex-wrap items-center gap-2 text-sm">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={() =>
                        setOpen((current) => ({
                          ...current,
                          [file.path]: !isOpen,
                        }))
                      }
                    >
                      {isOpen ? (
                        <ChevronDown className="size-4" />
                      ) : (
                        <ChevronRight className="size-4" />
                      )}
                    </Button>
                    <FileText className="size-4 text-muted-foreground" />
                    <span className="font-mono">{file.path}</span>
                    <Badge variant={statusVariant(file.status)}>
                      {file.status}
                    </Badge>
                    <span className="text-xs text-green-600">
                      +{file.additions}
                    </span>
                    <span className="text-xs text-red-600">
                      -{file.deletions}
                    </span>
                  </div>

                  {isOpen ? (
                    file.binary ? (
                      <p className="text-xs text-muted-foreground">
                        Binary file - not shown.
                      </p>
                    ) : (
                      <>
                        {file.truncated ? (
                          <p className="text-xs text-muted-foreground">
                            File is large; content truncated.
                          </p>
                        ) : null}
                        <div className="overflow-hidden rounded-lg border">
                          <DiffEditor
                            original={file.original}
                            modified={file.modified}
                            language={languageFor(file.path)}
                            theme="vs-dark"
                            height="360px"
                            options={{
                              readOnly: true,
                              originalEditable: false,
                              renderSideBySide: true,
                              minimap: { enabled: false },
                              renderOverviewRuler: false,
                              scrollBeyondLastLine: false,
                              automaticLayout: true,
                              fontSize: 12,
                            }}
                          />
                        </div>
                      </>
                    )
                  ) : null}
                </CardContent>
              </Card>
            );
          })
        )}
      </div>
    </div>
  );
}
