import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Bot,
  ClipboardList,
  FolderGit2,
  GitPullRequest,
  LayoutDashboard,
  Loader2,
  Search,
  Settings,
  Sparkles,
} from "lucide-react";
import { api } from "@/api/tauri";
import { useAppStore } from "@/stores/appStore";
import { cn } from "@/lib/utils";
import type { SearchHit, SearchKind } from "@/types";

const SEARCH_DEBOUNCE_MS = 200;
const HITS_PER_SOURCE = 6;

interface PaletteItem {
  id: string;
  label: string;
  hint: string;
  group: string;
  icon: typeof Search;
  run: () => void;
}

const kindMeta: Record<SearchKind, { label: string; icon: typeof Search }> = {
  ticket: { label: "JIRA tickets", icon: ClipboardList },
  plan: { label: "Plans", icon: Sparkles },
  pull_request: { label: "Pull requests", icon: GitPullRequest },
  agent_run: { label: "Agent runs", icon: Bot },
};

function matches(item: { label: string; hint: string }, query: string): boolean {
  return `${item.label} ${item.hint}`.toLowerCase().includes(query);
}

/**
 * Ctrl+K command palette: navigation, quick actions, the local task list and
 * live search results in one overlay. Deliberately dependency-free; keyboard
 * handling follows the standard listbox pattern.
 */
export function CommandPalette() {
  const navigate = useNavigate();
  const tasks = useAppStore((state) => state.tasks);
  const pullRequests = useAppStore((state) => state.pullRequests);

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const listRef = useRef<HTMLDivElement | null>(null);

  const close = useCallback(() => {
    setOpen(false);
    setQuery("");
    setActive(0);
    setHits([]);
    setSearching(false);
  }, []);

  const openPalette = useCallback(() => {
    setQuery("");
    setActive(0);
    setHits([]);
    setOpen(true);
  }, []);

  const baseItems = useMemo<PaletteItem[]>(() => {
    const go = (route: string) => () => {
      close();
      navigate(route);
    };
    return [
      {
        id: "nav-dashboard",
        label: "Dashboard",
        hint: "Open the dashboard",
        group: "Navigation",
        icon: LayoutDashboard,
        run: go("/"),
      },
      {
        id: "nav-repositories",
        label: "Repositories",
        hint: "Manage repositories",
        group: "Navigation",
        icon: FolderGit2,
        run: go("/repositories"),
      },
      {
        id: "nav-tasks",
        label: "Tasks",
        hint: "Browse tasks and history",
        group: "Navigation",
        icon: ClipboardList,
        run: go("/tasks"),
      },
      {
        id: "nav-agent",
        label: "Agent",
        hint: "Open the agent console",
        group: "Navigation",
        icon: Bot,
        run: go("/agent"),
      },
      {
        id: "nav-settings",
        label: "Settings",
        hint: "Open settings",
        group: "Navigation",
        icon: Settings,
        run: go("/settings"),
      },
      {
        id: "action-add-repository",
        label: "Add repository",
        hint: "Register a local git repository",
        group: "Actions",
        icon: FolderGit2,
        run: go("/repositories"),
      },
      {
        id: "action-create-plan",
        label: "Create plan",
        hint: "Plan a new ticket",
        group: "Actions",
        icon: Sparkles,
        run: go("/tasks"),
      },
      {
        id: "action-running-agents",
        label: "View running agents",
        hint: "Open the agent console",
        group: "Actions",
        icon: Bot,
        run: go("/agent"),
      },
      ...tasks.slice(0, 25).map<PaletteItem>((task) => ({
        id: `task-${task.id}`,
        label: `Open ${task.jiraIssueKey}`,
        hint: task.title,
        group: "Tickets",
        icon: ClipboardList,
        run: go(`/tasks/${task.id}`),
      })),
      ...Object.values(pullRequests).map<PaletteItem>((pullRequest) => ({
        id: `pr-${pullRequest.id}`,
        label: `Open PR #${pullRequest.number}`,
        hint: pullRequest.url,
        group: "Pull requests",
        icon: GitPullRequest,
        run: () => {
          close();
          void openUrl(pullRequest.url);
        },
      })),
    ];
  }, [close, navigate, pullRequests, tasks]);

  const trimmed = query.trim().toLowerCase();
  // A bare issue key (e.g. `CC-142`) can be pulled straight from JIRA.
  const keyQuery = /^[a-z][a-z0-9]+-\d+$/i.test(query.trim())
    ? query.trim().toUpperCase()
    : null;

  const items = useMemo<PaletteItem[]>(() => {
    const staticItems = trimmed
      ? baseItems.filter((item) => matches(item, trimmed))
      : baseItems;
    const hitItems = hits.map<PaletteItem>((hit) => ({
      id: `hit-${hit.kind}-${hit.taskId}-${hit.title}`,
      label: hit.title,
      hint: hit.subtitle,
      group: kindMeta[hit.kind].label,
      icon: kindMeta[hit.kind].icon,
      run: () => {
        close();
        navigate(hit.route);
      },
    }));
    const jiraItems = keyQuery
      ? [
          {
            id: `jira-${keyQuery}`,
            label: `Look up ${keyQuery} in JIRA`,
            hint: "Fetch this ticket and start a plan",
            group: "JIRA",
            icon: Search,
            run: () => {
              close();
              navigate(`/tasks?ticket=${encodeURIComponent(keyQuery)}`);
            },
          } satisfies PaletteItem,
        ]
      : [];
    return [...hitItems, ...jiraItems, ...staticItems];
  }, [baseItems, close, hits, keyQuery, navigate, trimmed]);

  const activeIndex = items.length === 0 ? 0 : Math.min(active, items.length - 1);

  // Reset the cursor whenever the result set changes shape.
  useEffect(() => {
    setActive(0);
  }, [trimmed]);

  // Debounced remote search.
  useEffect(() => {
    if (!open) {
      return;
    }
    const value = query.trim();
    if (!value) {
      setHits([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    const handle = window.setTimeout(() => {
      api
        .search(value, HITS_PER_SOURCE)
        .then(setHits)
        .catch(() => setHits([]))
        .finally(() => setSearching(false));
    }, SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [open, query]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      const inMonaco =
        target instanceof Element && Boolean(target.closest(".monaco-editor"));
      const isToggle =
        (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k";

      if (isToggle) {
        // Monaco binds Ctrl+K as a chord prefix; don't steal it while editing.
        if (inMonaco && !open) {
          return;
        }
        event.preventDefault();
        if (open) {
          close();
        } else {
          openPalette();
        }
        return;
      }

      if (!open) {
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      } else if (event.key === "ArrowDown") {
        event.preventDefault();
        setActive((index) => Math.min(index + 1, Math.max(items.length - 1, 0)));
      } else if (event.key === "ArrowUp") {
        event.preventDefault();
        setActive((index) => Math.max(index - 1, 0));
      } else if (event.key === "Enter") {
        event.preventDefault();
        items[activeIndex]?.run();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeIndex, close, items, open, openPalette]);

  // Keep the highlighted row in view.
  useEffect(() => {
    const node = listRef.current?.querySelector<HTMLElement>(
      `[data-index="${activeIndex}"]`,
    );
    node?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, items]);

  const grouped = useMemo(() => {
    const groups: {
      group: string;
      entries: { item: PaletteItem; index: number }[];
    }[] = [];
    items.forEach((item, index) => {
      const last = groups[groups.length - 1];
      if (last && last.group === item.group) {
        last.entries.push({ item, index });
      } else {
        groups.push({ group: item.group, entries: [{ item, index }] });
      }
    });
    return groups;
  }, [items]);

  if (!open) {
    return null;
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 p-4 pt-[15vh]"
      onMouseDown={close}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(event) => event.stopPropagation()}
        className="w-full max-w-xl overflow-hidden rounded-lg border bg-popover text-popover-foreground shadow-xl"
      >
        <div className="flex items-center gap-2 border-b px-3">
          <Search className="size-4 shrink-0 text-muted-foreground" />
          <input
            autoFocus
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search tickets, plans, pull requests, agent runs…"
            className="h-11 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          />
          {searching ? (
            <Loader2 className="size-4 shrink-0 animate-spin text-muted-foreground" />
          ) : null}
        </div>

        <div ref={listRef} className="max-h-80 overflow-y-auto p-1">
          {items.length === 0 ? (
            <p className="px-3 py-6 text-center text-sm text-muted-foreground">
              {query.trim() ? "No matches" : "Nothing to show"}
            </p>
          ) : (
            grouped.map((group) => (
              <div key={group.group}>
                <p className="px-3 pb-1 pt-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  {group.group}
                </p>
                {group.entries.map(({ item, index }) => {
                  const Icon = item.icon;
                  return (
                    <button
                      key={item.id}
                      type="button"
                      data-index={index}
                      onMouseEnter={() => setActive(index)}
                      onClick={() => item.run()}
                      className={cn(
                        "flex w-full items-center gap-3 rounded-md px-3 py-2 text-left text-sm transition-colors",
                        index === activeIndex
                          ? "bg-accent text-accent-foreground"
                          : "hover:bg-accent/60",
                      )}
                    >
                      <Icon className="size-4 shrink-0 text-muted-foreground" />
                      <span className="flex-1 truncate">{item.label}</span>
                      <span className="max-w-[45%] truncate text-xs text-muted-foreground">
                        {item.hint}
                      </span>
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>

        <div className="border-t px-3 py-2 text-xs text-muted-foreground">
          ↑↓ navigate · Enter open · Esc close · Ctrl+K toggle
        </div>
      </div>
    </div>
  );
}
