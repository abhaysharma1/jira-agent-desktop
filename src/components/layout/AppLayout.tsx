import { useEffect, useState } from "react";
import { Link, NavLink, Outlet, useNavigate } from "react-router-dom";
import {
  AlertTriangle,
  Bot,
  ClipboardList,
  FolderGit2,
  LayoutDashboard,
  Settings,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  NotificationBell,
  TicketNotificationCards,
} from "@/components/notifications/NotificationCenter";
import { CommandPalette } from "@/components/palette/CommandPalette";
import { api } from "@/api/tauri";
import { useAppStore } from "@/stores/appStore";
import type { CloudSocketStatus, TaskStatus } from "@/types";

const navItems = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, end: true },
  { to: "/repositories", label: "Repositories", icon: FolderGit2, end: false },
  { to: "/tasks", label: "Task", icon: ClipboardList, end: false },
  { to: "/agent", label: "Agent", icon: Bot, end: false },
  { to: "/settings", label: "Settings", icon: Settings, end: false },
];

const cloudStatusMeta: Record<
  CloudSocketStatus,
  { label: string; className: string }
> = {
  signedOut: { label: "Signed out", className: "bg-muted-foreground" },
  connecting: { label: "Connecting", className: "bg-amber-500" },
  online: { label: "Live", className: "bg-emerald-500" },
  offline: { label: "Offline", className: "bg-destructive" },
};

/**
 * Statuses that still need work or attention. The tray shows this as "pending
 * tickets"; terminal and already-shipped states are excluded.
 */
const PENDING_STATUSES = new Set<TaskStatus>([
  "RECEIVED",
  "NOTIFIED",
  "PLANNING",
  "PLAN_READY",
  "APPROVED",
  "WORKSPACE_CREATING",
  "IMPLEMENTING",
  "TESTING",
  "REPAIRING",
  "VALIDATING",
  "COMMITTING",
  "PR_CREATING",
  "INTERRUPTED",
]);

export function AppLayout() {
  const navigate = useNavigate();
  const startCloudSync = useAppStore((state) => state.startCloudSync);
  const cloudSocketStatus = useAppStore((state) => state.cloudSocketStatus);
  const tasks = useAppStore((state) => state.tasks);
  const loadTasks = useAppStore((state) => state.loadTasks);
  const agentRuns = useAppStore((state) => state.agentRuns);
  const loadAgents = useAppStore((state) => state.loadAgents);
  const status = cloudStatusMeta[cloudSocketStatus];
  const [dismissedRecovery, setDismissedRecovery] = useState(false);

  useEffect(() => {
    void startCloudSync();
    void loadTasks();
    void loadAgents();
  }, [startCloudSync, loadAgents, loadTasks]);

  // The tray's "Settings" item asks the frontend to navigate.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void api
      .onTrayNavigate((route) => navigate(route))
      .then((stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [navigate]);

  const pendingCount = tasks.filter((task) =>
    PENDING_STATUSES.has(task.status),
  ).length;
  const runningCount = agentRuns.filter((run) => run.status === "RUNNING").length;

  // Keep the tray labels in sync, debounced so bursts of task updates coalesce.
  useEffect(() => {
    const handle = window.setTimeout(() => {
      void api.setTrayStats(pendingCount, runningCount).catch(() => {});
    }, 300);
    return () => window.clearTimeout(handle);
  }, [pendingCount, runningCount]);

  const interrupted = tasks.filter((task) => task.status === "INTERRUPTED");

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <aside className="flex w-56 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
        <div className="flex h-14 items-center border-b px-4">
          <span className="text-sm font-semibold tracking-tight">
            JIRA Agent
          </span>
        </div>
        <nav className="flex flex-1 flex-col gap-1 p-2">
          {navItems.map(({ to, label, icon: Icon, end }) => (
            <NavLink
              key={to}
              to={to}
              end={end}
              className={({ isActive }) =>
                cn(
                  "flex items-center gap-2 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                  isActive
                    ? "bg-sidebar-accent text-sidebar-accent-foreground"
                    : "text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
                )
              }
            >
              <Icon className="size-4" />
              {label}
            </NavLink>
          ))}
        </nav>
        <div className="border-t p-3 text-xs text-muted-foreground">
          Local MVP
        </div>
      </aside>
      <main className="flex flex-1 flex-col overflow-hidden">
        <header className="flex h-14 shrink-0 items-center justify-end gap-3 border-b px-4">
          <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <span className={cn("size-2 rounded-full", status.className)} />
            {status.label}
          </span>
          <NotificationBell />
        </header>
        {interrupted.length > 0 && !dismissedRecovery ? (
          <div className="flex shrink-0 items-center gap-3 border-b bg-amber-500/10 px-4 py-2 text-sm text-amber-700">
            <AlertTriangle className="size-4 shrink-0" />
            <span>
              {interrupted.length} task
              {interrupted.length === 1 ? " was" : "s were"} interrupted by a
              previous shutdown.
            </span>
            <Link to="/tasks" className="font-medium underline">
              Review and resume
            </Link>
            <button
              type="button"
              onClick={() => setDismissedRecovery(true)}
              className="ml-auto text-muted-foreground"
              aria-label="Dismiss recovery notice"
            >
              <X className="size-4" />
            </button>
          </div>
        ) : null}
        <div className="flex-1 overflow-auto">
          <Outlet />
        </div>
      </main>
      <TicketNotificationCards />
      <CommandPalette />
    </div>
  );
}
