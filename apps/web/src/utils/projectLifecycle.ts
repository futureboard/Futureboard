import type { DawProject } from "../types/daw";
import { audioAssetManager } from "../engine/AudioAssetManager";
import { nativeServices } from "../engine/nativeServices";
import { platform } from "../platform";
import type { SaveProjectResult } from "../platform";
import { useHistoryStore } from "../store/historyStore";
import { useProjectStore } from "../store/projectStore";
import { useRecentProjectsStore } from "../store/recentProjectsStore";
import { useUIStore } from "../store/uiStore";
import { showToast } from "../components/ui/Toast";

type GuardIntent = "open" | "new" | "switch" | "close";

export const PROJECT_ACTION_CHANNEL = "futureboard-project-actions";

export type ProjectActionMessage =
  | { type: "openProjectPath"; filePath: string };

export function requestMainWindowOpenProject(filePath: string): void {
  if (typeof BroadcastChannel === "undefined") return;
  const channel = new BroadcastChannel(PROJECT_ACTION_CHANNEL);
  channel.postMessage({ type: "openProjectPath", filePath } satisfies ProjectActionMessage);
  channel.close();
}

function recentFromSavedProject(project: DawProject, result?: SaveProjectResult | null) {
  const projectRoot = result?.projectRoot ?? platform.folderProject.getProjectRoot() ?? undefined;
  const filePath = result?.path ?? platform.folderProject.getProjectFilePath() ?? undefined;

  if (platform.kind === "electron" && !projectRoot && !filePath) return null;

  return {
    id: project.id,
    name: project.name,
    projectFilePath: filePath,
    projectRoot,
    storageMode: projectRoot ? "folder" as const : platform.kind === "electron" ? "folder" as const : "browser" as const,
    source: platform.kind === "electron" ? "local" as const : "browser" as const,
  };
}

export function rememberSavedProject(project: DawProject, result?: SaveProjectResult | null): void {
  const recent = recentFromSavedProject(project, result);
  if (recent) useRecentProjectsStore.getState().addRecentProject(recent);
}

export function clearProjectSelectionState(): void {
  const ui = useUIStore.getState();
  useHistoryStore.getState().clear();
  ui.setSelectedClipIds([]);
  ui.setSelectedTrackId(null);
  ui.setSelectedBrowserFileId(null);
}

export async function saveCurrentProjectAndRemember(): Promise<boolean> {
  const ui = useUIStore.getState();
  ui.setSaveStatus("saving");
  try {
    const project = useProjectStore.getState().project;
    const result = await platform.projectStorage.saveProject(project);
    if (!result) {
      ui.setSaveStatus("unsaved");
      return false;
    }
    ui.setSaveStatus("saved");
    rememberSavedProject(useProjectStore.getState().project, result);
    return true;
  } catch (e) {
    console.warn("[ProjectLifecycle] save failed:", e);
    ui.setSaveStatus("error");
    showToast("Failed to save project.", true);
    return false;
  }
}

export async function guardUnsavedProject(intent: GuardIntent): Promise<boolean> {
  if (useUIStore.getState().saveStatus !== "unsaved") return true;

  const projectName = useProjectStore.getState().project.name;
  const detailByIntent: Record<GuardIntent, string> = {
    open: "Opening another project will discard unsaved changes in the current one.",
    new: "Creating a new project will discard unsaved changes in the current one.",
    switch: "Switching projects will discard unsaved changes in the current one.",
    close: "Closing Futureboard Studio will discard unsaved changes in the current project.",
  };

  if (platform.kind === "electron" && platform.capabilities.nativeDialogs) {
    const { response } = await platform.dialog.showMessageBox({
      type: "warning",
      title: "Unsaved Changes",
      message: `Save changes to "${projectName}"?`,
      detail: detailByIntent[intent],
      buttons: ["Save", "Don't Save", "Cancel"],
      defaultId: 0,
      cancelId: 2,
    });
    if (response === 0) return saveCurrentProjectAndRemember();
    return response === 1;
  }

  const discard = window.confirm(
    `Discard unsaved changes in "${projectName}"?\n\n${detailByIntent[intent]}`,
  );
  return discard;
}

export async function loadOpenedProject(project: DawProject): Promise<void> {
  // Pre-warm peak metadata concurrently so clips can render their waveforms on
  // the very first mount, rather than flashing "Waveform pending" → "loading...".
  const preWarmed = await Promise.all(
    project.files.map(async (file) => ({
      fileId: file.id,
      meta:   await audioAssetManager.tryLoadWaveformMeta(file),
    })),
  );

  useProjectStore.getState().loadProject(project);
  clearProjectSelectionState();
  useUIStore.getState().setSaveStatus("saved");
  rememberSavedProject(project);
  // Sync to localStorage so external windows (Add Track, Mixer, Piano Roll) opened after
  // this point load THIS project, not a stale one from a previous session.
  useProjectStore.getState().saveLocal();
  console.log("[projectLifecycle] loadOpenedProject: project ready, baseline set, dirty=false");

  // Re-apply pre-warmed metadata immediately (loadProject reset the store).
  const store = useProjectStore.getState();
  for (const { fileId, meta } of preWarmed) {
    if (meta) store.setPeakMeta(fileId, meta);
  }

  // Notify waveform components that all pre-warmed peak data is committed.
  // WaveformCanvas listens for this to retry drawing after project open.
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent("projectWaveformReady"));
  }

  void audioAssetManager.restoreProjectAssets(project);
}

/**
 * Single entry point for opening a project from a file path.
 * Waits for native services to settle, reads the file, pre-warms peaks,
 * commits to the store, and fires `projectWaveformReady`.
 * All open-by-path code paths (Recent Projects, startup argv, app-command)
 * must go through this function.
 */
export async function openProjectFromPath(filePath: string): Promise<boolean> {
  if (!platform.folderProject.isSupported) return false;

  // Ensure IPC / native services have settled before issuing peak reads.
  await nativeServices.whenReadyOrSettled();

  const project = await platform.folderProject.openByPath(filePath);
  if (!project) return false;

  await loadOpenedProject(project);
  return true;
}
