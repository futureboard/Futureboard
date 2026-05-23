import { useWindowStore } from "../../store/windowStore";
import type { AppWindowContentType } from "../../store/windowStore";

export type ExternalWindowConfig = {
  id?: string;
  title: string;
  contentType: AppWindowContentType;
  payload?: Record<string, unknown>;
  width: number;
  height: number;
  minWidth?: number;
  minHeight?: number;
  alwaysOnTop?: boolean;
  frame?: boolean;
  transparent?: boolean;
  resizable?: boolean;
  maximizable?: boolean;
};

export interface ExternalWindowAdapter {
  supportsExternalWindows: boolean;
  openExternalWindow(config: ExternalWindowConfig): Promise<string | null>;
  closeExternalWindow(id: string): Promise<void>;
  focusExternalWindow(id: string): Promise<void>;
}

// Web adapter: external windows not supported — fall back to internal floating window.
export const webWindowAdapter: ExternalWindowAdapter = {
  supportsExternalWindows: false,

  async openExternalWindow(config) {
    const ws = useWindowStore.getState();
    const id = ws.openFloating({
      contentType: config.contentType,
      title: config.title,
      width: config.width,
      height: config.height,
      minWidth: config.minWidth,
      minHeight: config.minHeight,
      resizable: config.resizable ?? true,
      payload: config.payload,
    });
    return id;
  },

  async closeExternalWindow(id) {
    useWindowStore.getState().closeWindow(id);
  },

  async focusExternalWindow(id) {
    useWindowStore.getState().focusWindow(id);
  },
};

// Electron adapter: uses preload IPC bridge.
// Falls back to internal floating window if IPC unavailable.
export const electronWindowAdapter: ExternalWindowAdapter = {
  supportsExternalWindows: true,

  async openExternalWindow(config) {
    const bridge = (window as unknown as { dawElectron?: { windows?: { openExternal?: (c: unknown) => Promise<string | null> } } }).dawElectron?.windows;
    if (!bridge?.openExternal) {
      return webWindowAdapter.openExternalWindow(config);
    }
    try {
      return await bridge.openExternal(config);
    } catch (e) {
      console.warn("[ElectronWindowAdapter] openExternal failed, falling back:", e);
      return webWindowAdapter.openExternalWindow(config);
    }
  },

  async closeExternalWindow(id) {
    const bridge = (window as unknown as { dawElectron?: { windows?: { closeExternal?: (id: string) => Promise<void> } } }).dawElectron?.windows;
    if (bridge?.closeExternal) {
      try { await bridge.closeExternal(id); return; } catch {}
    }
    useWindowStore.getState().closeWindow(id);
  },

  async focusExternalWindow(id) {
    const bridge = (window as unknown as { dawElectron?: { windows?: { focusExternal?: (id: string) => Promise<void> } } }).dawElectron?.windows;
    if (bridge?.focusExternal) {
      try { await bridge.focusExternal(id); return; } catch {}
    }
    useWindowStore.getState().focusWindow(id);
  },
};
