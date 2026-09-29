import { ClipboardList } from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { PageHeader } from "@/components/layout/PageHeader";
import { useAppStore } from "@/stores/appStore";

const flow = [
  "Ticket",
  "Plan",
  "Approve",
  "Worktree",
  "Implement",
  "Test",
  "Diff",
  "PR",
];

export function Task() {
  const activeTask = useAppStore((state) => state.activeTask);

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Task"
        description="A single JIRA ticket moving through the workflow."
      />
      <div className="p-6">
        <Card>
          <CardHeader>
            <CardDescription>
              {activeTask ? activeTask.jiraIssueKey : "No active task"}
            </CardDescription>
            <CardTitle className="flex items-center gap-2">
              <ClipboardList className="size-4" />
              {activeTask ? activeTask.title : "Ticket workflow"}
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            {flow.map((step) => (
              <span
                key={step}
                className="rounded-md border bg-muted px-2 py-1 text-xs"
              >
                {step}
              </span>
            ))}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
