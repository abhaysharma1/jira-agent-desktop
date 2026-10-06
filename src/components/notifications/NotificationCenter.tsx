import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Bell, Check, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  notificationEventType,
  notificationJiraKey,
  notificationSummary,
  useAppStore,
} from "@/stores/appStore";
import type { CloudNotification } from "@/types";

export function NotificationBell() {
  const notifications = useAppStore((state) => state.notifications);
  const markNotificationRead = useAppStore(
    (state) => state.markNotificationRead,
  );
  const [open, setOpen] = useState(false);

  const unread = notifications.filter((item) => !item.readAt).length;

  return (
    <div className="relative">
      <Button
        variant="ghost"
        size="sm"
        aria-label="Notifications"
        onClick={() => setOpen((value) => !value)}
      >
        <Bell className="size-4" />
        {unread > 0 ? (
          <Badge variant="destructive" className="ml-1">
            {unread}
          </Badge>
        ) : null}
      </Button>
      {open ? (
        <div className="absolute right-0 top-11 z-50 w-80 rounded-lg border bg-popover p-2 text-popover-foreground shadow-md">
          {notifications.length === 0 ? (
            <p className="p-2 text-xs text-muted-foreground">
              No notifications yet.
            </p>
          ) : (
            <ul className="flex max-h-96 flex-col gap-1 overflow-auto">
              {notifications.slice(0, 20).map((notification) => (
                <li key={notification.id}>
                  <button
                    type="button"
                    className="flex w-full flex-col gap-1 rounded-md p-2 text-left text-xs hover:bg-muted"
                    onClick={() => {
                      void markNotificationRead(notification.id);
                      setOpen(false);
                    }}
                  >
                    <span className="flex items-center gap-2">
                      {!notification.readAt ? (
                        <span className="size-2 shrink-0 rounded-full bg-primary" />
                      ) : null}
                      <span className="font-medium">{notification.title}</span>
                    </span>
                    {notification.body ? (
                      <span className="text-muted-foreground">
                        {notification.body}
                      </span>
                    ) : null}
                    <span className="text-muted-foreground">
                      {notification.type} ·{" "}
                      {new Date(notification.createdAt).toLocaleString()}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  );
}

type CardAction = "skip" | "plan";

export function TicketNotificationCards() {
  const navigate = useNavigate();
  const notifications = useAppStore((state) => state.notifications);
  const prepareTicketIntake = useAppStore(
    (state) => state.prepareTicketIntake,
  );
  const startPlanning = useAppStore((state) => state.startPlanning);
  const skipTicket = useAppStore((state) => state.skipTicket);
  const setError = useAppStore((state) => state.setError);
  const [busy, setBusy] = useState<Record<string, CardAction | undefined>>({});

  const tickets = notifications.filter(
    (notification) =>
      !notification.readAt &&
      notificationEventType(notification) === "jira:issue_created",
  );

  if (tickets.length === 0) {
    return null;
  }

  async function handleCreatePlan(notification: CloudNotification) {
    const jiraKey = notificationJiraKey(notification);
    setBusy((state) => ({ ...state, [notification.id]: "plan" }));
    try {
      const intake = await prepareTicketIntake(jiraKey, notification.id);
      if (!intake.repositoryId) {
        navigate(`/tasks?ticket=${encodeURIComponent(jiraKey)}`);
        return;
      }
      const task = await startPlanning(
        intake.repositoryId,
        jiraKey,
        intake.issue.summary,
        intake.issue.description,
      );
      navigate(`/tasks/${task.id}`);
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy((state) => ({ ...state, [notification.id]: undefined }));
    }
  }

  async function handleSkip(notification: CloudNotification) {
    setBusy((state) => ({ ...state, [notification.id]: "skip" }));
    try {
      await skipTicket(notification);
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy((state) => ({ ...state, [notification.id]: undefined }));
    }
  }

  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-3">
      {tickets.map((notification) => {
        const state = busy[notification.id];
        return (
          <div
            key={notification.id}
            className="pointer-events-auto rounded-xl border bg-card p-4 text-card-foreground shadow-lg"
          >
            <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              New JIRA Ticket
            </p>
            <p className="mt-2 font-mono text-sm">
              {notificationJiraKey(notification)}
            </p>
            <p className="text-sm">{notificationSummary(notification)}</p>
            <div className="mt-4 flex gap-2">
              <Button
                variant="outline"
                size="sm"
                disabled={Boolean(state)}
                onClick={() => void handleSkip(notification)}
              >
                <X className="size-4" />
                {state === "skip" ? "Skipping..." : "Skip"}
              </Button>
              <Button
                size="sm"
                disabled={Boolean(state)}
                onClick={() => void handleCreatePlan(notification)}
              >
                <Check className="size-4" />
                {state === "plan" ? "Planning..." : "Create Plan"}
              </Button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
