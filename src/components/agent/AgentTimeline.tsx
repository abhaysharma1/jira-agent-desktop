import {
  Brain,
  CheckCircle2,
  FilePen,
  FileText,
  ListTodo,
  MessageSquare,
  Play,
  ShieldAlert,
  Terminal,
  Wrench,
  XCircle,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { AgentEvent } from "@/types";

interface EventView {
  icon: LucideIcon;
  label: string;
  muted?: boolean;
}

export function describeEvent(event: AgentEvent): EventView {
  switch (event.type) {
    case "message":
      return { icon: MessageSquare, label: event.content };
    case "reasoning":
      return { icon: Brain, label: event.content, muted: true };
    case "tool":
      return {
        icon: Wrench,
        label: `${event.title || event.name} (${event.status})`,
      };
    case "todo":
      return {
        icon: ListTodo,
        label: event.todos.map((todo) => todo.content).join("; "),
      };
    case "permission_request":
      return { icon: ShieldAlert, label: `permission: ${event.title}` };
    case "file_read":
      return { icon: FileText, label: `Read ${event.path}` };
    case "file_changed":
      return { icon: FilePen, label: `Changed ${event.path}` };
    case "command_started":
      return { icon: Terminal, label: `$ ${event.command}` };
    case "command_finished":
      return {
        icon: event.exitCode === 0 ? CheckCircle2 : XCircle,
        label: `exit ${event.exitCode}: ${event.command}`,
      };
    case "test_result":
      return {
        icon: event.passed ? CheckCircle2 : XCircle,
        label: event.passed ? "tests passed" : "tests failed",
      };
    case "finished":
      return { icon: CheckCircle2, label: "finished" };
    case "failed":
      return { icon: XCircle, label: `failed: ${event.error}` };
    case "started":
      return { icon: Play, label: "started" };
    default:
      return { icon: Wrench, label: "" };
  }
}

interface AgentTimelineProps {
  events: AgentEvent[];
  className?: string;
  emptyLabel?: string;
}

export function AgentTimeline({
  events,
  className,
  emptyLabel = "No events yet.",
}: AgentTimelineProps) {
  return (
    <div className={cn("overflow-auto rounded-lg border p-2 text-xs", className)}>
      {events.length === 0 ? (
        <p className="text-muted-foreground">{emptyLabel}</p>
      ) : (
        events.map((event, index) => {
          const view = describeEvent(event);
          const Icon = view.icon;
          return (
            <div
              key={index}
              className={cn(
                "flex items-start gap-2 py-0.5",
                view.muted && "text-muted-foreground italic",
              )}
            >
              <Icon className="mt-0.5 size-3.5 shrink-0" />
              <span className="whitespace-pre-wrap break-words">
                {view.label}
              </span>
            </div>
          );
        })
      )}
    </div>
  );
}
