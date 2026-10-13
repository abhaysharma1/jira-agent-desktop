import type { ResolvedTheme, ThemePreference } from "@/types";

/** Where the applied theme is cached so the next boot can paint immediately. */
const STORAGE_KEY = "ticketbridge.theme";

const DARK_QUERY = "(prefers-color-scheme: dark)";

function prefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia(DARK_QUERY).matches
  );
}

/** Resolve a preference to the theme that should actually be shown. */
export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference === "system") {
    return prefersDark() ? "dark" : "light";
  }
  return preference;
}

/**
 * Apply a resolved theme to `<html>` and cache it for the next boot. Toggling
 * the `dark` class flips the CSS variables declared in `index.css`; setting
 * `color-scheme` makes native controls, scrollbars, and form fields match.
 */
export function applyTheme(theme: ResolvedTheme): void {
  if (typeof document === "undefined") {
    return;
  }
  const root = document.documentElement;
  root.classList.toggle("dark", theme === "dark");
  root.style.colorScheme = theme;
  try {
    window.localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Storage can be unavailable; the backend remains the source of truth.
  }
}

/** The theme cached from the last run, used to avoid a flash before load. */
export function cachedTheme(): ResolvedTheme | null {
  try {
    const value = window.localStorage.getItem(STORAGE_KEY);
    return value === "dark" || value === "light" ? value : null;
  } catch {
    return null;
  }
}

/** Runs `onChange` whenever the OS light/dark preference changes. */
export function subscribeSystemTheme(
  onChange: (theme: ResolvedTheme) => void,
): () => void {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return () => {};
  }
  const media = window.matchMedia(DARK_QUERY);
  const handler = () => onChange(media.matches ? "dark" : "light");
  media.addEventListener("change", handler);
  return () => media.removeEventListener("change", handler);
}
