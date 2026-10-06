import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

let granted: boolean | null = null;

/**
 * Sends a native OS notification. Best-effort: notification permission is
 * requested once and failures are swallowed so they never break the app.
 */
export async function notify(title: string, body: string): Promise<void> {
  try {
    if (granted === null) {
      granted = await isPermissionGranted();
      if (!granted) {
        granted = (await requestPermission()) === "granted";
      }
    }
    if (granted) {
      sendNotification({ title, body });
    }
  } catch {
    // OS notifications are best-effort.
  }
}
