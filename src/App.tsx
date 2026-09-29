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

function App() {
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
