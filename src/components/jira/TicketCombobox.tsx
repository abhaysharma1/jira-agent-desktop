import { useEffect, useRef, useState } from "react";
import { ChevronDown, Loader2, Search } from "lucide-react";
import { cn } from "cn";

import type { JiraIssue } from "@/types";

/** The identity shown for a ticket: always includes its title (`summary`). */
export function ticketIdentity(
  issue: Pick<JiraIssue, "key" | "summary">,
): string {
  return `${issue.key} — ${issue.summary}`;
}

interface TicketComboboxProps {
  /** Searches for tickets; called with the query (empty loads the defaults). */
  search: (query: string) => Promise<JiraIssue[]>;
  onSelect: (issue: JiraIssue) => void;
  disabled?: boolean;
  placeholder?: string;
}

/**
 * A filterable combobox for picking a JIRA ticket by name. Renders each option
 * as `KEY — Title` and reports the full issue on selection.
 */
export function TicketCombobox({
  search,
  onSelect,
  disabled,
  placeholder = "Search tickets by name or key...",
}: TicketComboboxProps) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [options, setOptions] = useState<JiraIssue[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [highlight, setHighlight] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const requestId = useRef(0);

  // Keep the latest `search` without making it an effect dependency, so an
  // inline arrow function from the parent does not trigger repeated searches.
  const searchRef = useRef(search);
  useEffect(() => {
    searchRef.current = search;
  }, [search]);

  useEffect(() => {
    function onDocumentMouseDown(event: MouseEvent) {
      if (!containerRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", onDocumentMouseDown);
    return () =>
      document.removeEventListener("mousedown", onDocumentMouseDown);
  }, []);

  useEffect(() => {
    if (!open || disabled) {
      return;
    }
    const id = ++requestId.current;
    setLoading(true);
    const handle = window.setTimeout(() => {
      searchRef
        .current(query)
        .then((issues) => {
          if (requestId.current !== id) return;
          setOptions(issues);
          setHighlight(0);
          setError(null);
        })
        .catch((err) => {
          if (requestId.current !== id) return;
          setOptions([]);
          setError(String(err));
        })
        .finally(() => {
          if (requestId.current === id) setLoading(false);
        });
    }, 250);
    return () => window.clearTimeout(handle);
  }, [query, open, disabled]);

  function choose(issue: JiraIssue) {
    onSelect(issue);
    setQuery(ticketIdentity(issue));
    setOpen(false);
  }

  function onKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    if (!open && (event.key === "ArrowDown" || event.key === "Enter")) {
      setOpen(true);
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setHighlight((value) => Math.min(value + 1, options.length - 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setHighlight((value) => Math.max(value - 1, 0));
    } else if (event.key === "Enter") {
      event.preventDefault();
      const issue = options[highlight];
      if (issue) {
        choose(issue);
      }
    } else if (event.key === "Escape") {
      setOpen(false);
    }
  }

  return (
    <div ref={containerRef} className="relative">
      <div className="relative">
        <Search className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <input
          type="text"
          role="combobox"
          aria-expanded={open}
          aria-controls="ticket-combobox-list"
          className="h-8 w-full rounded-lg border border-input bg-background pr-7 pl-7 text-sm text-foreground"
          value={query}
          placeholder={placeholder}
          disabled={disabled}
          onChange={(event) => {
            setQuery(event.target.value);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onKeyDown={onKeyDown}
        />
        {loading ? (
          <Loader2 className="absolute top-1/2 right-2 size-3.5 -translate-y-1/2 animate-spin text-muted-foreground" />
        ) : (
          <ChevronDown className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        )}
      </div>

      {open ? (
        <div
          id="ticket-combobox-list"
          role="listbox"
          className="absolute z-20 mt-1 max-h-64 w-full overflow-auto rounded-lg border border-input bg-popover p-1 text-popover-foreground shadow-md"
        >
          {error ? (
            <p className="px-2 py-1.5 text-xs text-destructive">{error}</p>
          ) : options.length === 0 ? (
            <p className="px-2 py-1.5 text-xs text-muted-foreground">
              {loading ? "Searching..." : "No tickets found."}
            </p>
          ) : (
            options.map((issue, index) => (
              <button
                key={issue.key}
                type="button"
                role="option"
                aria-selected={index === highlight}
                onMouseEnter={() => setHighlight(index)}
                onMouseDown={(event) => {
                  event.preventDefault();
                  choose(issue);
                }}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm",
                  index === highlight ? "bg-muted" : "hover:bg-muted",
                )}
              >
                <span className="font-mono text-xs text-muted-foreground">
                  {issue.key}
                </span>
                <span className="truncate">{issue.summary || "(no title)"}</span>
              </button>
            ))
          )}
        </div>
      ) : null}
    </div>
  );
}
