import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { PageHeader } from "@/components/layout/PageHeader";
import type { Settings as SettingsModel } from "@/types";

const defaults: SettingsModel = {
  workspaceRoot: "~/.jira-agent/workspaces",
  maxRepairAttempts: 5,
  opencodeCommand: "opencode",
};

export function Settings() {
  const rows: Array<[string, string]> = [
    ["Workspace root", defaults.workspaceRoot],
    ["Max repair attempts", String(defaults.maxRepairAttempts)],
    ["OpenCode command", defaults.opencodeCommand],
    ["OpenCode model", defaults.opencodeModel ?? "default"],
  ];

  return (
    <div className="flex flex-col">
      <PageHeader
        title="Settings"
        description="Configuration is persisted locally in a later phase."
      />
      <div className="p-6">
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
      </div>
    </div>
  );
}
