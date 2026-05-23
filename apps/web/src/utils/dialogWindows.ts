import { platform } from "../platform";
import { electronWindowAdapter, webWindowAdapter } from "../platform/windows/windowAdapter";
import { useWindowStore } from "../store/windowStore";
import type { AppWindowContentType } from "../store/windowStore";

type SettingsTab = "general" | "audio" | "midi" | "project" | "library" | "shortcuts" | "appearance" | "advanced";

type DialogConfig = {
  contentType: AppWindowContentType;
  title: string;
  width: number;
  height: number;
  minWidth?: number;
  minHeight?: number;
  resizable?: boolean;
  maximizable?: boolean;
  closable?: boolean;
  payload?: Record<string, unknown>;
};

export function openExternalCapableDialog(config: DialogConfig): Promise<string | null> {
  if (platform.kind === "electron") {
    return electronWindowAdapter.openExternalWindow({
      id: config.contentType,
      title: config.title,
      contentType: config.contentType,
      width: config.width,
      height: config.height,
      minWidth: config.minWidth,
      minHeight: config.minHeight,
      resizable: config.resizable,
      frame: true,
      transparent: false,
      maximizable: config.maximizable,
      payload: config.payload,
    });
  }

  return webWindowAdapter.openExternalWindow({
    title: config.title,
    contentType: config.contentType,
    width: config.width,
    height: config.height,
    minWidth: config.minWidth,
    minHeight: config.minHeight,
    resizable: config.resizable,
    payload: config.payload,
  });
}

export function openProjectWizardWindow(): Promise<string | null> {
  return openExternalCapableDialog({
    contentType: "projectWizard",
    title: "New Project",
    width: 780,
    height: platform.kind === "electron" ? 560 : 510,
    minWidth: 720,
    minHeight: 500,
    resizable: false,
    maximizable: false,
    closable: true,
  });
}

export function openSettingsWindow(initialTab: SettingsTab = "general"): Promise<string | null> {
  if (platform.kind !== "electron") {
    const ws = useWindowStore.getState();
    const existing = ws.windows.find((w) => w.contentType === "preferences");
    if (existing) {
      ws.updateWindowPayload(existing.id, { initialTab });
      ws.focusWindow(existing.id);
      return Promise.resolve(existing.id);
    }
  }

  return openExternalCapableDialog({
    contentType: "preferences",
    title: "Preferences",
    width: 860,
    height: 600,
    minWidth: 720,
    minHeight: 480,
    resizable: true,
    maximizable: false,
    closable: true,
    payload: { initialTab },
  });
}

export function openAddTrackWindow(): Promise<string | null> {
  return openExternalCapableDialog({
    contentType: "addTrack",
    title: "New Track",
    width: 560,
    height: 660,
    minWidth: 540,
    minHeight: 580,
    resizable: false,
    maximizable: false,
    closable: true,
  });
}

export function openPluginManagerWindow(): Promise<string | null> {
  return openExternalCapableDialog({
    contentType: "pluginManager",
    title: "Audio Plug-in Manager",
    width: 980,
    height: 640,
    minWidth: 860,
    minHeight: 520,
    resizable: true,
    maximizable: false,
    closable: true,
  });
}
