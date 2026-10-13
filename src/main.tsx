import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { applyTheme, cachedTheme, resolveTheme } from "@/lib/theme";
import "./index.css";

// Apply the theme before the first paint so returning users don't see a flash.
// The stored preference (SQLite) is reconciled once the app mounts.
applyTheme(cachedTheme() ?? resolveTheme("system"));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
