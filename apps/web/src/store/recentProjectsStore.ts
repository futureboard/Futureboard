import { create } from "zustand";

const STORAGE_KEY = "futureboard.recentProjects.v1";
const MAX_RECENT = 20;

export type RecentProject = {
  id: string;
  name: string;
  path?: string;
  /** Absolute path to the .mochiproj file (folder projects). */
  projectFilePath?: string;
  /** Absolute path to the project folder root (folder projects). */
  projectRoot?: string;
  /** How this project is stored. */
  storageMode?: "folder" | "browser" | "cloud";
  source: "local" | "browser" | "remote";
  lastOpenedAt: number;
  lastModifiedAt?: number;
};

type RecentProjectsStore = {
  recentProjects: RecentProject[];
  addRecentProject: (project: Omit<RecentProject, "lastOpenedAt"> & { lastOpenedAt?: number }) => void;
  removeRecentProject: (projectId: string) => void;
  clearRecentProjects: () => void;
};

function loadFromStorage(): RecentProject[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (p): p is RecentProject =>
        typeof p === "object" &&
        p !== null &&
        typeof p.id === "string" &&
        typeof p.name === "string" &&
        isSavedRecentProject(p as Partial<RecentProject>)
    );
  } catch {
    return [];
  }
}

export function isSavedRecentProject(project: Partial<RecentProject>): boolean {
  if (!project.id || !project.name) return false;
  if (project.projectFilePath || project.projectRoot || project.path) return true;
  return project.storageMode === "browser" && project.source === "browser";
}

function saveToStorage(projects: RecentProject[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(projects));
  } catch {
    // ignore storage quota errors
  }
}

export const useRecentProjectsStore = create<RecentProjectsStore>((set) => ({
  recentProjects: loadFromStorage(),

  addRecentProject: (project) =>
    set((s) => {
      const entry: RecentProject = { lastOpenedAt: Date.now(), ...project };
      if (!isSavedRecentProject(entry)) return s;
      // Deduplicate by id or path
      const filtered = s.recentProjects.filter(
        (p) =>
          p.id !== entry.id &&
          !(entry.path && p.path === entry.path) &&
          !(entry.projectFilePath && p.projectFilePath === entry.projectFilePath) &&
          !(entry.projectRoot && p.projectRoot === entry.projectRoot)
      );
      const next = [entry, ...filtered].slice(0, MAX_RECENT);
      saveToStorage(next);
      return { recentProjects: next };
    }),

  removeRecentProject: (projectId) =>
    set((s) => {
      const next = s.recentProjects.filter((p) => p.id !== projectId);
      saveToStorage(next);
      return { recentProjects: next };
    }),

  clearRecentProjects: () => {
    saveToStorage([]);
    set({ recentProjects: [] });
  },
}));
