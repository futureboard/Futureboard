import { create } from "zustand";

export type BackgroundTaskKind =
  | "import"
  | "media-copy"
  | "metadata-scan"
  | "waveform"
  | "peak-generation"
  | "peak-loading"
  | "native-sync"
  | "project-save"
  | "recording";

export type BackgroundTaskStatus =
  | "queued"
  | "running"
  | "paused"
  | "complete"
  | "failed"
  | "cancelled";

export type BackgroundTask = {
  id: string;
  kind: BackgroundTaskKind;
  title: string;
  detail?: string;
  status: BackgroundTaskStatus;
  progress?: {
    current: number;
    total: number;
  };
  startedAt?: number;
  updatedAt?: number;
  error?: string;
  cancellable?: boolean;
  parentId?: string;
};

type AddTaskInput = Omit<BackgroundTask, "id" | "status" | "updatedAt"> & {
  id?: string;
  status?: BackgroundTaskStatus;
};

type BackgroundTaskStore = {
  tasks: Record<string, BackgroundTask>;
  panelOpen: boolean;
  addTask: (task: AddTaskInput) => string;
  updateTask: (id: string, patch: Partial<Omit<BackgroundTask, "id">>) => void;
  completeTask: (id: string, patch?: Partial<Omit<BackgroundTask, "id" | "status">>) => void;
  failTask: (id: string, error: string, patch?: Partial<Omit<BackgroundTask, "id" | "status" | "error">>) => void;
  cancelTask: (id: string) => void;
  getActiveTasks: () => BackgroundTask[];
  getSummaryByKind: () => Record<BackgroundTaskKind, { active: number; failed: number; queued: number }>;
  setPanelOpen: (open: boolean) => void;
};

const ALL_KINDS: BackgroundTaskKind[] = [
  "import",
  "media-copy",
  "metadata-scan",
  "waveform",
  "peak-generation",
  "peak-loading",
  "native-sync",
  "project-save",
  "recording",
];

const RECENT_COMPLETE_MS = 30_000;
const KEEP_COMPLETED = 20;

function pruneCompleted(tasks: Record<string, BackgroundTask>): Record<string, BackgroundTask> {
  const now = Date.now();
  const entries = Object.entries(tasks);
  const completed = entries
    .filter(([, task]) => task.status === "complete" || task.status === "cancelled")
    .sort(([, a], [, b]) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
  const keep = new Set(
    completed
      .filter(([, task], index) => index < KEEP_COMPLETED || now - (task.updatedAt ?? now) < RECENT_COMPLETE_MS)
      .map(([id]) => id),
  );
  return Object.fromEntries(entries.filter(([id, task]) => task.status !== "complete" && task.status !== "cancelled" || keep.has(id)));
}

export const useBackgroundTaskStore = create<BackgroundTaskStore>((set, get) => ({
  tasks: {},
  panelOpen: false,

  addTask: (input) => {
    const id = input.id ?? crypto.randomUUID();
    const now = Date.now();
    set((state) => ({
      tasks: pruneCompleted({
        ...state.tasks,
        [id]: {
          ...input,
          id,
          status: input.status ?? "queued",
          startedAt: input.startedAt,
          updatedAt: now,
        },
      }),
    }));
    return id;
  },

  updateTask: (id, patch) => set((state) => {
    const current = state.tasks[id];
    if (!current) return state;
    const nextStatus = patch.status ?? current.status;
    return {
      tasks: pruneCompleted({
        ...state.tasks,
        [id]: {
          ...current,
          ...patch,
          status: nextStatus,
          startedAt: nextStatus === "running" && !current.startedAt ? Date.now() : patch.startedAt ?? current.startedAt,
          updatedAt: Date.now(),
        },
      }),
    };
  }),

  completeTask: (id, patch) => get().updateTask(id, { ...patch, status: "complete" }),
  failTask: (id, error, patch) => get().updateTask(id, { ...patch, status: "failed", error }),
  cancelTask: (id) => get().updateTask(id, { status: "cancelled" }),

  getActiveTasks: () => Object.values(get().tasks).filter((task) => task.status === "queued" || task.status === "running" || task.status === "paused"),

  getSummaryByKind: () => {
    const summary = Object.fromEntries(ALL_KINDS.map((kind) => [kind, { active: 0, failed: 0, queued: 0 }])) as Record<
      BackgroundTaskKind,
      { active: number; failed: number; queued: number }
    >;
    for (const task of Object.values(get().tasks)) {
      if (task.status === "failed") summary[task.kind].failed++;
      if (task.status === "queued") summary[task.kind].queued++;
      if (task.status === "running" || task.status === "paused") summary[task.kind].active++;
    }
    return summary;
  },

  setPanelOpen: (panelOpen) => set({ panelOpen }),
}));

export function backgroundTaskStats() {
  const tasks = Object.values(useBackgroundTaskStore.getState().tasks);
  return {
    active: tasks.filter((task) => task.status === "queued" || task.status === "running" || task.status === "paused").length,
    failed: tasks.filter((task) => task.status === "failed").length,
  };
}
