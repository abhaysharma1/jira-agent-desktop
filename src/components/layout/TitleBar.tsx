import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Waypoints, X } from "lucide-react";
import { cn } from "@/lib/utils";

const appWindow = getCurrentWindow();

/** Windows-style maximize glyph: a single rounded square. */
function MaximizeGlyph() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <rect
        x="0.5"
        y="0.5"
        width="9"
        height="9"
        rx="1.5"
        fill="none"
        stroke="currentColor"
      />
    </svg>
  );
}

/** Windows-style restore glyph: two offset rounded squares. */
function RestoreGlyph() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <rect
        x="2.5"
        y="0.5"
        width="7"
        height="7"
        rx="1.5"
        fill="none"
        stroke="currentColor"
      />
      <path
        d="M0.5 2.5V8.5A0.5 0.5 0 0 0 1 9H7"
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
      />
    </svg>
  );
}

interface TitleBarButtonProps {
  label: string;
  onClick: () => void;
  variant?: "default" | "close";
  children: ReactNode;
}

function TitleBarButton({
  label,
  onClick,
  variant = "default",
  children,
}: TitleBarButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "inline-flex w-11 items-center justify-center text-muted-foreground transition-colors",
        variant === "close"
          ? "hover:bg-destructive hover:text-destructive-foreground"
          : "hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
      )}
    >
      {children}
    </button>
  );
}

export function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;

    void appWindow.isMaximized().then(setMaximized).catch(() => {});
    void appWindow
      .onResized(() => {
        void appWindow.isMaximized().then(setMaximized).catch(() => {});
      })
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
  }, []);

  return (
    <div
      data-tauri-drag-region
      className="flex h-9 shrink-0 select-none items-center border-b bg-sidebar text-sidebar-foreground"
    >
      <div
        data-tauri-drag-region
        className="flex flex-1 items-center gap-2 px-3"
      >
        <span className="flex size-5 items-center justify-center rounded-md bg-primary/10 text-primary">
          <Waypoints className="size-3.5" />
        </span>
        <span className="text-sm font-semibold tracking-tight">
          TicketBridge
        </span>
      </div>
      <div className="flex h-full items-stretch">
        <TitleBarButton label="Minimize" onClick={() => void appWindow.minimize()}>
          <Minus className="size-3.5" />
        </TitleBarButton>
        <TitleBarButton
          label={maximized ? "Restore" : "Maximize"}
          onClick={() => void appWindow.toggleMaximize()}
        >
          {maximized ? <RestoreGlyph /> : <MaximizeGlyph />}
        </TitleBarButton>
        <TitleBarButton
          label="Close"
          variant="close"
          onClick={() => void appWindow.close()}
        >
          <X className="size-3.5" />
        </TitleBarButton>
      </div>
    </div>
  );
}
