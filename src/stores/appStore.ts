import { create } from "zustand";
import { api } from "@/api/tauri";
import type { AgentEvent, AgentTask, AppInfo, Repository } from "@/types";

interface AppState {
  appInfo: AppInfo | null;
  repositories: Repository[];
  tasks: AgentTask[];
  activeTask: AgentTask | null;
  events: AgentEvent[];
  loading: boolean;
  error: string | null;

  loadAppInfo: () => Promise<void>;
  loadRepositories: () => Promise<void>;
  setActiveTask: (task: AgentTask | null) => void;
  pushEvent: (event: AgentEvent) => void;
  setError: (error: string | null) => void;
}

export const useAppStore = create<AppState>((set) => ({
  appInfo: null,
  repositories: [],
  tasks: [],
  activeTask: null,
  events: [],
  loading: false,
  error: null,

  loadAppInfo: async () => {
    try {
      const appInfo = await api.getAppInfo();
      set({ appInfo });
    } catch (error) {
      set({ error: String(error) });
    }
  },

  loadRepositories: async () => {
    set({ loading: true, error: null });
    try {
      const repositories = await api.listRepositories();
      set({ repositories, loading: false });
    } catch (error) {
      set({ error: String(error), loading: false });
    }
  },

  setActiveTask: (activeTask) => set({ activeTask }),
  pushEvent: (event) => set((state) => ({ events: [...state.events, event] })),
  setError: (error) => set({ error }),
}));
