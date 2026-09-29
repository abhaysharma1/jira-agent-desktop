import { useEffect } from "react";
import { FolderGit2, Plus } from "lucide-react";
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
  const loading = useAppStore((state) => state.loading);
  const error = useAppStore((state) => state.error);
  const loadRepositories = useAppStore((state) => state.loadRepositories);

  useEffect(() => {
    void loadRepositories();
  }, [loadRepositories]);

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
        <Button disabled>
          <Plus className="size-4" />
          Add Repository
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
                Repository registration lands in Phase 2
              </CardTitle>
            </CardHeader>
            <CardContent className="text-sm text-muted-foreground">
              Adding a repository will detect Git, read the remote URL, current
              branch, and clean/dirty state.
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-3">
            {repositories.map((repository) => (
              <Card key={repository.id}>
                <CardHeader>
                  <CardTitle>{repository.name}</CardTitle>
                  <CardDescription>{repository.localPath}</CardDescription>
                </CardHeader>
              </Card>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
