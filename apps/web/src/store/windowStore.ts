import { create } from "zustand";

export type AppWindowKind = "floating" | "dialog" | "external";

export type AppWindowContentType =
  | "projectWizard"
  | "unsavedChanges"
  | "preferences"
  | "plugin"
  | "mixer"
  | "editor"
  | "effectEditor"
  | "about"
  | "generic";

export type AppWindowState = {
  id: string;
  kind: AppWindowKind;
  contentType: AppWindowContentType;
  title: string;
  modal?: boolean;
  external?: boolean;
  x: number;
  y: number;
  width: number;
  height: number;
  minWidth?: number;
  minHeight?: number;
  focused: boolean;
  zIndex: number;
  resizable?: boolean;
  draggable?: boolean;
  closable?: boolean;
  payload?: Record<string, unknown>;
};

type OpenWindowConfig = Omit<AppWindowState, "id" | "focused" | "zIndex" | "x" | "y"> & {
  id?: string;
  x?: number;
  y?: number;
};

type WindowStore = {
  windows: AppWindowState[];
  pendingAction: (() => void) | null;
  topZIndex: number;

  openWindow: (config: OpenWindowConfig) => string;
  closeWindow: (id: string) => void;
  focusWindow: (id: string) => void;
  updateWindowBounds: (id: string, bounds: Partial<Pick<AppWindowState, "x" | "y" | "width" | "height">>) => void;
  bringToFront: (id: string) => void;
  closeAllDialogs: () => void;
  isWindowOpen: (contentType: AppWindowContentType) => boolean;
  openDialog: (config: Omit<OpenWindowConfig, "kind">) => string;
  openFloating: (config: Omit<OpenWindowConfig, "kind">) => string;
  setPendingAction: (action: (() => void) | null) => void;
  consumePendingAction: () => (() => void) | null;
};

const FLOATING_Z_BASE = 1000;
const DIALOG_Z_BASE = 2000;

function centerCoords(width: number, height: number) {
  if (typeof window === "undefined") return { x: 100, y: 100 };
  return {
    x: Math.max(0, Math.round((window.innerWidth - width) / 2)),
    y: Math.max(0, Math.round((window.innerHeight - height) / 2)),
  };
}

export const useWindowStore = create<WindowStore>((set, get) => ({
  windows: [],
  pendingAction: null,
  topZIndex: FLOATING_Z_BASE,

  openWindow(config) {
    const id = config.id ?? crypto.randomUUID();
    const existing = get().windows.find((w) => w.id === id);
    if (existing) {
      get().focusWindow(id);
      return id;
    }
    const zBase = config.kind === "dialog" ? DIALOG_Z_BASE : FLOATING_Z_BASE;
    const nextZ = Math.max(get().topZIndex + 1, zBase);
    const coords = centerCoords(config.width, config.height);
    const win: AppWindowState = {
      id,
      focused: true,
      zIndex: nextZ,
      x: config.x ?? coords.x,
      y: config.y ?? coords.y,
      draggable: config.draggable ?? true,
      resizable: config.resizable ?? false,
      closable: config.closable ?? true,
      ...config,
    };
    set((s) => ({
      windows: [...s.windows.map((w) => ({ ...w, focused: false })), win],
      topZIndex: nextZ,
    }));
    return id;
  },

  closeWindow(id) {
    set((s) => ({ windows: s.windows.filter((w) => w.id !== id) }));
  },

  focusWindow(id) {
    const nextZ = get().topZIndex + 1;
    set((s) => ({
      windows: s.windows.map((w) =>
        w.id === id
          ? { ...w, focused: true, zIndex: nextZ }
          : { ...w, focused: false }
      ),
      topZIndex: nextZ,
    }));
  },

  updateWindowBounds(id, bounds) {
    set((s) => ({
      windows: s.windows.map((w) => (w.id === id ? { ...w, ...bounds } : w)),
    }));
  },

  bringToFront(id) {
    get().focusWindow(id);
  },

  closeAllDialogs() {
    set((s) => ({ windows: s.windows.filter((w) => w.kind !== "dialog") }));
  },

  isWindowOpen(contentType) {
    return get().windows.some((w) => w.contentType === contentType);
  },

  openDialog(config) {
    return get().openWindow({ ...config, kind: "dialog", resizable: config.resizable ?? false });
  },

  openFloating(config) {
    return get().openWindow({ ...config, kind: "floating", resizable: config.resizable ?? true });
  },

  setPendingAction(action) {
    set({ pendingAction: action });
  },

  consumePendingAction() {
    const action = get().pendingAction;
    set({ pendingAction: null });
    return action;
  },
}));
