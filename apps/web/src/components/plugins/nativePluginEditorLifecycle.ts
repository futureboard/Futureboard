import type { InsertDevice } from "../../types/daw";
import { useProjectStore } from "../../store/projectStore";

const editorHandles = new Map<string, number>();
const pendingEditorOpens = new Map<string, Promise<void>>();

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function editorKey(trackId: string, insertId: string): string {
  return `${trackId}:${insertId}`;
}

export function pluginEditorWindowId(trackId: string, insertId: string): string {
  return `plugin-editor:${trackId}:${insertId}`;
}

export function isNativeVst3Insert(insert: InsertDevice): boolean {
  return insert.type === "native-plugin" &&
    typeof insert.params.format === "string" &&
    insert.params.format.toUpperCase() === "VST3";
}

export async function openNativeInsertEditor(
  trackId: string,
  insert: InsertDevice,
  insertIndex?: number,
): Promise<void> {
  if (!isNativeVst3Insert(insert)) return;
  const key = editorKey(trackId, insert.id);
  const pending = pendingEditorOpens.get(key);
  if (pending) return pending;

  const existing = editorHandles.get(key);
  if (existing) {
    try {
      const focused = await window.dawElectron?.sphereAudio?.focusInsertEditor(trackId, insert.id);
      if (focused) return;
      editorHandles.delete(key);
    } catch (error) {
      console.warn(`[PluginEditorLifecycle] focus failed for ${key}:`, error);
      editorHandles.delete(key);
    }
  }

  const pluginPath = typeof insert.params.modulePath === "string"
    ? insert.params.modulePath
    : typeof insert.params.path === "string"
      ? insert.params.path
      : undefined;
  const classId = typeof insert.params.classId === "string" ? insert.params.classId : undefined;
  const format = typeof insert.params.format === "string" ? insert.params.format : undefined;
  if (!pluginPath || !classId || !format) return;

  const pluginName = insert.name || "Plugin";
  const insertLabel = insertIndex != null ? `Insert ${insertIndex + 1}` : "Insert";
  // Latency and CPU are shown as 0 until a live query API is available.
  const projectName = useProjectStore.getState().project?.name || "Untitled Project";
  const windowTitle = `${pluginName} - ${insertLabel} | Latency: 0.0 ms @CPU: 0% - ${projectName}`;
  console.log(`[PluginEditorLifecycle] open insertId=${insert.id} title="${windowTitle}"`);

  const openTask = (async () => {
    let handle: number | null = null;
    for (let attempt = 0; attempt < 6; attempt += 1) {
      if (attempt > 0) await wait(160);
      handle = await window.dawElectron?.sphereAudio?.openInsertEditor({
        trackId,
        insertId: insert.id,
        windowId: pluginEditorWindowId(trackId, insert.id),
        title: windowTitle,
        width: 820,
        height: 560,
      }) ?? null;
      if (handle) break;
    }
    if (handle) {
      editorHandles.set(key, handle);
    } else {
      console.warn(`[PluginEditorLifecycle] open returned no handle for ${key}`);
    }
  })();
  pendingEditorOpens.set(key, openTask);
  try {
    await openTask;
  } catch (error) {
    console.warn(`[PluginEditorLifecycle] open failed for ${key}:`, error);
  } finally {
    pendingEditorOpens.delete(key);
  }
}

export async function closeNativeInsertEditor(trackId: string, insertId: string): Promise<void> {
  const key = editorKey(trackId, insertId);
  await pendingEditorOpens.get(key)?.catch(() => undefined);
  editorHandles.delete(key);
  try {
    await window.dawElectron?.sphereAudio?.closeInsertEditor(trackId, insertId);
  } catch (error) {
    console.warn(`[PluginEditorLifecycle] close failed for ${key}:`, error);
  }
}
