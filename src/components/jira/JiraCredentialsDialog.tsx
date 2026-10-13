import { useEffect, useState } from "react";
import { ExternalLink, Loader2 } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Button } from "@/components/ui/button";

/** Where users create an Atlassian OAuth 2.0 (3LO) app. */
const CONSOLE_URL = "https://developer.atlassian.com";
/** Must match `DEFAULT_CALLBACK_PORT` in src-tauri/src/oauth/mod.rs. */
const CALLBACK_URL = "http://localhost:8765/callback";
/** Must match `DEFAULT_SCOPES` in src-tauri/src/oauth/mod.rs. */
const SCOPES = [
  "read:jira-work",
  "write:jira-work",
  "read:jira-user",
  "manage:jira-webhooks",
  "offline_access",
];

interface JiraCredentialsDialogProps {
  open: boolean;
  busy?: boolean;
  onClose: () => void;
  onSubmit: (clientId: string, clientSecret: string) => void | Promise<void>;
}

/**
 * Collects the Atlassian app Client ID/secret in-app, so connecting JIRA does
 * not require editing environment variables or a JSON config file.
 */
export function JiraCredentialsDialog({
  open,
  busy = false,
  onClose,
  onSubmit,
}: JiraCredentialsDialogProps) {
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");

  useEffect(() => {
    if (!open) {
      setClientId("");
      setClientSecret("");
    }
  }, [open]);

  if (!open) {
    return null;
  }

  const canSubmit =
    clientId.trim().length > 0 && clientSecret.trim().length > 0 && !busy;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      onMouseDown={() => {
        if (!busy) {
          onClose();
        }
      }}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="JIRA OAuth credentials"
        onMouseDown={(event) => event.stopPropagation()}
        className="flex max-h-[85vh] w-full max-w-lg flex-col overflow-hidden rounded-lg border bg-popover text-popover-foreground shadow-xl"
      >
        <div className="border-b px-4 py-3">
          <h2 className="text-sm font-medium">Connect to JIRA</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            Paste the Client ID and secret from your own Atlassian OAuth 2.0 app.
            They are stored locally, encrypted at rest with a key held in the OS
            credential store.
          </p>
        </div>

        <div className="flex flex-col gap-3 overflow-y-auto p-4 text-sm">
          <label className="flex flex-col gap-1 text-xs text-muted-foreground">
            Client ID
            <input
              autoFocus
              value={clientId}
              onChange={(event) => setClientId(event.target.value)}
              placeholder="e.g. 1234567890abcdef"
              className="h-8 rounded-lg border border-input bg-background px-2 font-mono text-xs text-foreground"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs text-muted-foreground">
            Client secret
            <input
              type="password"
              value={clientSecret}
              onChange={(event) => setClientSecret(event.target.value)}
              placeholder="••••••••••••"
              className="h-8 rounded-lg border border-input bg-background px-2 font-mono text-xs text-foreground"
            />
          </label>

          <button
            type="button"
            onClick={() => void openUrl(CONSOLE_URL)}
            className="inline-flex items-center gap-1 self-start text-xs text-primary hover:underline"
          >
            <ExternalLink className="size-3.5" />
            Open the Atlassian Developer Console
          </button>

          <details className="rounded-lg border bg-muted/30 px-3 py-2">
            <summary className="cursor-pointer text-xs font-medium">
              How to get a Client ID and secret
            </summary>
            <ol className="mt-2 flex list-decimal flex-col gap-1.5 pl-4 text-xs text-muted-foreground">
              <li>
                Open{" "}
                <button
                  type="button"
                  onClick={() => void openUrl(CONSOLE_URL)}
                  className="text-primary hover:underline"
                >
                  developer.atlassian.com
                </button>{" "}
                and click <span className="font-medium">Create</span> →{" "}
                <span className="font-medium">OAuth 2.0 integration</span>.
              </li>
              <li>
                Under <span className="font-medium">Permissions</span>, add the
                Jira API scopes: <code className="rounded bg-muted px-1 font-mono">{SCOPES.join(", ")}</code>.
              </li>
              <li>
                Under <span className="font-medium">Authorization</span>, add the
                callback URL{" "}
                <code className="rounded bg-muted px-1 font-mono">{CALLBACK_URL}</code>.
              </li>
              <li>
                Open <span className="font-medium">Settings</span> and copy the{" "}
                <span className="font-medium">Client ID</span> and{" "}
                <span className="font-medium">Secret</span> here.
              </li>
            </ol>
            <p className="mt-2 text-xs text-muted-foreground">
              The callback URL must match exactly; the app listens on the fixed
              port 8765.
            </p>
          </details>
        </div>

        <div className="flex items-center justify-end gap-2 border-t px-4 py-3">
          <Button variant="outline" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button
            onClick={() => void onSubmit(clientId.trim(), clientSecret.trim())}
            disabled={!canSubmit}
          >
            {busy ? (
              <>
                <Loader2 className="animate-spin" />
                Connecting…
              </>
            ) : (
              "Save & connect"
            )}
          </Button>
        </div>
      </div>
    </div>
  );
}
