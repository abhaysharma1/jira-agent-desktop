import { useEffect } from "react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { PageHeader } from "@/components/layout/PageHeader";
import { useAppStore } from "@/stores/appStore";

const workflow = [
  "RECEIVED",
  "PLANNING",
  "PLAN_READY",
  "APPROVED",
  "WORKSPACE_CREATING",
  "IMPLEMENTING",
  "TESTING",
  "VALIDATING",
  "COMMITTING",
  "PR_CREATED",
];

export function Dashboard() {
  const appInfo = useAppStore((state) => state.appInfo);
  const loadAppInfo = useAppStore((state) => state.loadAppInfo);

  useEffect(() => {
    void loadAppInfo();
  }, [loadAppInfo]);

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Dashboard"
        description="Local shell for the JIRA to PR workflow."
      />
      <div className="grid gap-4 p-6 md:grid-cols-3">
        <Card>
          <CardHeader>
            <CardDescription>Application</CardDescription>
            <CardTitle>{appInfo?.name ?? "Loading..."}</CardTitle>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground">
            Version {appInfo?.version ?? "-"}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardDescription>Tauri core</CardDescription>
            <CardTitle>{appInfo?.tauriVersion ?? "-"}</CardTitle>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground">
            Runtime bridge verified
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardDescription>Platform</CardDescription>
            <CardTitle className="capitalize">
              {appInfo?.platform ?? "-"}
            </CardTitle>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground">
            Local desktop environment
          </CardContent>
        </Card>
      </div>
      <div className="px-6 pb-6">
        <Card>
          <CardHeader>
            <CardDescription>Task state machine</CardDescription>
            <CardTitle>Contract defined in Phase 0</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            {workflow.map((status) => (
              <span
                key={status}
                className="rounded-md border bg-muted px-2 py-1 font-mono text-xs"
              >
                {status}
              </span>
            ))}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
