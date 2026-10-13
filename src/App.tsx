import { useEffect } from "react";
import { HashRouter, Route, Routes } from "react-router-dom";
import { AppLayout } from "@/components/layout/AppLayout";
import { Dashboard } from "@/pages/Dashboard";
import { Repositories } from "@/pages/Repositories";
import { Task } from "@/pages/Task";
import { PlanReview } from "@/pages/PlanReview";
import { TaskRun } from "@/pages/TaskRun";
import { TaskDiff } from "@/pages/TaskDiff";
import { Agent } from "@/pages/Agent";
import { Settings } from "@/pages/Settings";
import { applyTheme, resolveTheme, subscribeSystemTheme } from "@/lib/theme";
import { useAppStore } from "@/stores/appStore";

/**
 * Loads the persisted theme and keeps `<html>` in sync with the resolved
 * light/dark value, including live OS changes while in "system" mode.
 */
function useTheme() {
  const theme = useAppStore((state) => state.theme);
  const loadTheme = useAppStore((state) => state.loadTheme);
  const setResolvedTheme = useAppStore((state) => state.setResolvedTheme);

  useEffect(() => {
    void loadTheme();
  }, [loadTheme]);

  useEffect(() => {
    const resolved = resolveTheme(theme);
    applyTheme(resolved);
    setResolvedTheme(resolved);
    if (theme !== "system") {
      return;
    }
    return subscribeSystemTheme((systemResolved) => {
      applyTheme(systemResolved);
      setResolvedTheme(systemResolved);
    });
  }, [theme, setResolvedTheme]);
}

function App() {
  useTheme();
  const loadDefaultModel = useAppStore((state) => state.loadDefaultModel);

  useEffect(() => {
    void loadDefaultModel();
  }, [loadDefaultModel]);

  return (
    <HashRouter>
      <Routes>
        <Route element={<AppLayout />}>
          <Route index element={<Dashboard />} />
          <Route path="repositories" element={<Repositories />} />
          <Route path="tasks" element={<Task />} />
          <Route path="tasks/:taskId" element={<PlanReview />} />
          <Route path="tasks/:taskId/run" element={<TaskRun />} />
          <Route path="tasks/:taskId/diff" element={<TaskDiff />} />
          <Route path="agent" element={<Agent />} />
          <Route path="settings" element={<Settings />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}

export default App;
