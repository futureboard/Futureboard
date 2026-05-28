import { app } from "electron";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import { createHash } from "node:crypto";
import { fork, type ChildProcess } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import type {
  AudioPluginHostStatus,
  AudioPluginRegistryEntry,
  AudioPluginScanProgressEvent,
  AudioPluginScanResult,
} from "../ipc/channels.js";
import { sphereAudioNative } from "../native-audio/SphereAudioNative.js";

const _dirname = path.dirname(fileURLToPath(import.meta.url));
const req = createRequire(import.meta.url);

type NativePluginInfo = {
  id?: string;
  name?: string;
  vendor?: string;
  category?: string;
  subCategories?: string;
  sub_categories?: string;
  format?: string;
  path?: string;
  classId?: string | null;
  class_id?: string | null;
  version?: string | null;
  sdkMetadataLoaded?: boolean;
  sdk_metadata_loaded?: boolean;
};

type PluginEditorWindowOptions = {
  windowId: string;
  title: string;
  subtitle?: string;
  width?: number;
  height?: number;
  pluginPath?: string;
  classId?: string;
  format?: string;
};

type PluginHostAddon = {
  initPluginHost?: () => { available?: boolean; backend?: string; message?: string };
  scanVst3?: (paths: string[]) => NativePluginInfo[];
  scanClap?: (paths: string[]) => NativePluginInfo[];
  scanAudioPlugins?: (paths: string[]) => NativePluginInfo[];
  openPluginEditorWindow?: (options: PluginEditorWindowOptions) => number;
  openPluginEditorForPath?: (pluginPath: string) => number;
  closePluginEditorWindow?: (handle: number) => void;
  focusPluginEditorWindow?: (handle: number) => void;
  resizePluginEditorWindow?: (handle: number, width: number, height: number) => void;
  drainPluginEditorParamEvents?: () => Array<{ windowId?: string; window_id?: string; paramId?: number; param_id?: number; value?: number }>;
  getBackendVersion?: () => string;
};

type SqliteStatement = {
  run: (...args: unknown[]) => unknown;
  all: (...args: unknown[]) => unknown[];
  get: (...args: unknown[]) => unknown;
};

type SqliteDatabase = {
  exec: (sql: string) => unknown;
  prepare: (sql: string) => SqliteStatement;
};

let addon: PluginHostAddon | null | undefined;
let addonLoadError: string | null = null;
let db: SqliteDatabase | null | undefined;
let editorParamPoll: NodeJS.Timeout | null = null;
let forwardedEditorParamCount = 0;

function workspaceRoot(): string {
  return path.resolve(_dirname, "..", "..", "..", "..");
}

function candidateAddonPaths(): string[] {
  const candidates: string[] = [];
  if (process.resourcesPath) {
    candidates.push(path.join(process.resourcesPath, "PluginHost.node"));
  }
  const root = workspaceRoot();
  const hostRoot = path.join(root, "crates", "SpherePluginHost");
  candidates.push(path.join(hostRoot, "PluginHost.node"));
  candidates.push(path.join(hostRoot, "target", "release", "PluginHost.node"));
  candidates.push(path.join(hostRoot, "target", "release", "sphere_plugin_host.dll"));
  candidates.push(path.join(hostRoot, "target", "release", "PluginHost.dll"));
  candidates.push(path.join(hostRoot, "target", "debug", "PluginHost.node"));
  candidates.push(path.join(hostRoot, "target", "debug", "PluginHost.dll"));
  candidates.push(path.resolve(_dirname, "..", "..", "resources", "PluginHost.node"));
  return candidates;
}

function loadAddon(): PluginHostAddon | null {
  if (addon !== undefined) return addon;
  for (const candidate of candidateAddonPaths()) {
    if (!fs.existsSync(candidate)) continue;
    try {
      addon = req(candidate) as PluginHostAddon;
      console.log(`[PluginHost] Loaded native addon from: ${candidate}`);
      return addon;
    } catch (error) {
      addonLoadError = String(error);
      console.warn(`[PluginHost] Failed to load addon from ${candidate}:`, error);
    }
  }
  addon = null;
  if (!addonLoadError) {
    addonLoadError = `PluginHost addon not found. Searched:\n${candidateAddonPaths().map((p) => `  ${p}`).join("\n")}`;
  }
  console.warn(`[PluginHost] ${addonLoadError}`);
  return null;
}

function pluginDbPath(): string {
  return path.join(app.getPath("userData"), "audio-plugin-registry.sqlite");
}

function presetRootPath(): string {
  return path.join(app.getPath("documents"), "Futureboard Studio", "Audio Plug-ins");
}

function presetSubfolders(): string[] {
  return [
    path.join(presetRootPath(), "VST3", "Effects"),
    path.join(presetRootPath(), "VST3", "Instruments"),
    path.join(presetRootPath(), "CLAP", "Effects"),
    path.join(presetRootPath(), "CLAP", "Instruments"),
  ];
}

function getDb(): SqliteDatabase | null {
  if (db !== undefined) return db;
  try {
    fs.mkdirSync(app.getPath("userData"), { recursive: true });
    const sqlite = req("node:sqlite") as { DatabaseSync?: new (location: string) => SqliteDatabase };
    if (!sqlite.DatabaseSync) throw new Error("node:sqlite DatabaseSync is unavailable");
    db = new sqlite.DatabaseSync(pluginDbPath());
    db.exec(`
      PRAGMA journal_mode = WAL;
      CREATE TABLE IF NOT EXISTS audio_plugins (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        vendor TEXT NOT NULL,
        format TEXT NOT NULL,
        category TEXT NOT NULL,
        sub_categories TEXT,
        kind TEXT NOT NULL,
        path TEXT NOT NULL,
        class_id TEXT,
        version TEXT,
        sdk_metadata_loaded INTEGER NOT NULL,
        preset_path TEXT NOT NULL,
        scanned_at INTEGER NOT NULL,
        metadata_json TEXT NOT NULL
      );
      CREATE INDEX IF NOT EXISTS idx_audio_plugins_kind ON audio_plugins(kind);
      CREATE INDEX IF NOT EXISTS idx_audio_plugins_path ON audio_plugins(path);
      CREATE INDEX IF NOT EXISTS idx_audio_plugins_name ON audio_plugins(name);
    `);
    try {
      db.exec("ALTER TABLE audio_plugins ADD COLUMN sub_categories TEXT");
    } catch {
      // Existing registries already have the column.
    }
  } catch (error) {
    console.warn("[PluginHost] SQLite registry unavailable:", error);
    db = null;
  }
  return db;
}

function defaultScanPaths(): string[] {
  const paths = new Set<string>();
  if (process.platform === "win32") {
    const programFiles = process.env.ProgramFiles ?? "C:\\Program Files";
    const programFilesX86 = process.env["ProgramFiles(x86)"];
    const localAppData = process.env.LOCALAPPDATA;
    paths.add(path.join(programFiles, "Common Files", "VST3"));
    paths.add(path.join(programFiles, "Common Files", "CLAP"));
    paths.add(path.join(programFiles, "VSTPlugins"));
    paths.add(path.join(programFiles, "Steinberg", "VSTPlugins"));
    if (programFilesX86) paths.add(path.join(programFilesX86, "Common Files", "VST3"));
    if (programFilesX86) paths.add(path.join(programFilesX86, "Common Files", "CLAP"));
    if (programFilesX86) paths.add(path.join(programFilesX86, "VSTPlugins"));
    if (programFilesX86) paths.add(path.join(programFilesX86, "Steinberg", "VSTPlugins"));
    if (localAppData) paths.add(path.join(localAppData, "Programs", "Common", "VST3"));
    if (localAppData) paths.add(path.join(localAppData, "Programs", "Common", "CLAP"));
  } else if (process.platform === "darwin") {
    paths.add("/Library/Audio/Plug-Ins/VST3");
    paths.add("/Library/Audio/Plug-Ins/CLAP");
    paths.add(path.join(app.getPath("home"), "Library", "Audio", "Plug-Ins", "VST3"));
    paths.add(path.join(app.getPath("home"), "Library", "Audio", "Plug-Ins", "CLAP"));
  } else {
    paths.add("/usr/lib/vst3");
    paths.add("/usr/local/lib/vst3");
    paths.add("/usr/lib/clap");
    paths.add("/usr/local/lib/clap");
    paths.add(path.join(app.getPath("home"), ".vst3"));
    paths.add(path.join(app.getPath("home"), ".clap"));
  }
  return [...paths];
}

function stableId(input: string, format = "plugin"): string {
  return `${format.toLowerCase()}:${createHash("sha1").update(input).digest("hex").slice(0, 24)}`;
}

function safeFileName(value: string): string {
  return value
    .replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 120) || "Unknown Plug-in";
}

function classifyPlugin(category: string, name: string, subCategories?: string): "effect" | "instrument" {
  const haystack = `${category} ${subCategories ?? ""} ${name}`.toLowerCase();
  if (/\b(instrument|synth|synthesizer|sampler|rompler|drum|piano|organ|bass|generator)\b/.test(haystack)) {
    return "instrument";
  }
  return "effect";
}

function normalizeCategoryLabel(format: string, category: string, subCategories?: string): string {
  const tags = (subCategories ?? "")
    .split("|")
    .map((tag) => tag.trim())
    .filter(Boolean);

  if (format === "VST3") {
    const has = (needle: string) => tags.some((tag) => tag.toLowerCase() === needle.toLowerCase());
    if (has("Instrument")) return "Instrument";
    if (has("EQ")) return "EQ";
    if (has("Dynamics")) return "Dynamics";
    if (has("Reverb")) return "Reverb";
    if (has("Delay")) return "Delay";
    if (/^audio module class$/i.test(category)) return tags.find((tag) => !/^fx$/i.test(tag)) ?? "Effect";
    return tags.length > 0 ? tags.join("|") : category;
  }

  if (format === "CLAP") {
    const specific = tags.filter((tag) => !/^(audio-effect|audio effect|plugin|utility)$/i.test(tag));
    const displayTags = specific.length > 0 ? specific : tags;
    if (displayTags.some((tag) => /^instrument$/i.test(tag))) return "Instrument";
    if (displayTags.some((tag) => /effect/i.test(tag))) return "Effect";
    if (/^audio effect$/i.test(category)) return "Effect";
    return displayTags[0] ?? category;
  }

  return category;
}

function parseEditorWindowIdentity(windowId: string): { trackId: string; insertId: string } | null {
  const parts = windowId.split(":");
  if (parts.length < 4 || parts[0] !== "plugin-editor") return null;
  return { trackId: parts[1], insertId: parts[2] };
}

function startEditorParamPolling(): void {
  if (editorParamPoll) return;
  editorParamPoll = setInterval(() => {
    const native = loadAddon();
    if (!native?.drainPluginEditorParamEvents) return;
    let events: Array<{ windowId?: string; window_id?: string; paramId?: number; param_id?: number; value?: number }> = [];
    try {
      events = native.drainPluginEditorParamEvents();
    } catch (error) {
      console.warn("[PluginHost] Failed draining editor parameter events:", error);
      return;
    }
    for (const event of events) {
      const windowId = event.windowId ?? event.window_id;
      const paramId = event.paramId ?? event.param_id;
      const value = event.value;
      if (typeof windowId !== "string" || typeof paramId !== "number" || typeof value !== "number") continue;
      const target = parseEditorWindowIdentity(windowId);
      if (!target) continue;
      try {
        forwardedEditorParamCount += 1;
        if (forwardedEditorParamCount <= 16 || forwardedEditorParamCount % 50 === 0) {
          console.log(
            `[PluginHost] editor param -> audio track=${target.trackId} insert=${target.insertId} param=${Math.trunc(paramId)} value=${value.toFixed(6)} count=${forwardedEditorParamCount}`,
          );
        }
        sphereAudioNative.updateInsertParam(target.trackId, target.insertId, String(Math.trunc(paramId)), value);
      } catch (error) {
        console.warn(
          `[PluginHost] Failed forwarding editor param track=${target.trackId} insert=${target.insertId} param=${paramId}:`,
          error,
        );
      }
    }
  }, 16);
  editorParamPoll.unref?.();
}

function rowFromDb(row: Record<string, unknown>): AudioPluginRegistryEntry {
  let metadata: Partial<AudioPluginRegistryEntry> = {};
  if (typeof row.metadata_json === "string") {
    try { metadata = JSON.parse(row.metadata_json) as Partial<AudioPluginRegistryEntry>; } catch { metadata = {}; }
  }
  const format = String(row.format) as "VST3" | "CLAP";
  const rawCategory = String(row.category);
  const subCategories = typeof row.sub_categories === "string" ? row.sub_categories : metadata.subCategories;
  return {
    id: String(row.id),
    name: String(row.name),
    vendor: String(row.vendor),
    format,
    category: normalizeCategoryLabel(format, rawCategory, subCategories),
    rawCategory,
    subCategories,
    kind: row.kind === "instrument" ? "instrument" : "effect",
    path: String(row.path),
    classId: typeof row.class_id === "string" ? row.class_id : undefined,
    version: typeof row.version === "string" ? row.version : undefined,
    sdkMetadataLoaded: Number(row.sdk_metadata_loaded) === 1,
    presetPath: String(row.preset_path),
    scannedAt: Number(row.scanned_at),
  };
}

function normalizeNativePlugin(plugin: NativePluginInfo, scannedAt: number): AudioPluginRegistryEntry | null {
  if (!plugin.path) return null;
  const name = plugin.name?.trim() || path.basename(plugin.path, path.extname(plugin.path)) || "Unknown Plug-in";
  const vendor = plugin.vendor?.trim() || "Unknown Vendor";
  const rawCategory = plugin.category?.trim() || "Uncategorized";
  const subCategories = (plugin.subCategories ?? plugin.sub_categories)?.trim() || undefined;
  const classId = plugin.classId ?? plugin.class_id ?? undefined;
  const format = (plugin.format || "VST3").toUpperCase();
  const category = normalizeCategoryLabel(format, rawCategory, subCategories);
  const kind = classifyPlugin(rawCategory, name, subCategories);
  const id = plugin.id || stableId(`${plugin.path}:${classId ?? name}`, format);
  const presetDir = path.join(presetRootPath(), format === "CLAP" ? "CLAP" : "VST3", kind === "instrument" ? "Instruments" : "Effects");
  const presetName = `${safeFileName(name)}.pst`;
  return {
    id,
    name,
    vendor,
    format: format as "VST3" | "CLAP",
    category,
    rawCategory,
    subCategories,
    kind,
    path: plugin.path,
    classId,
    version: plugin.version ?? undefined,
    sdkMetadataLoaded: Boolean(plugin.sdkMetadataLoaded ?? plugin.sdk_metadata_loaded),
    presetPath: path.join(presetDir, presetName),
    scannedAt,
  };
}

function buildPresetBinary(plugin: AudioPluginRegistryEntry, pluginState: Buffer): Buffer {
  const metadata = {
    presetFormat: "Mochi preset: Futureboard",
    version: 1,
    createdAt: plugin.scannedAt,
    pluginMetadata: {
      id: plugin.id,
      name: plugin.name,
      vendor: plugin.vendor,
      format: plugin.format,
      category: plugin.category,
      rawCategory: plugin.rawCategory,
      subCategories: plugin.subCategories,
      kind: plugin.kind,
      path: plugin.path,
      classId: plugin.classId,
      version: plugin.version,
      sdkMetadataLoaded: plugin.sdkMetadataLoaded,
    },
    pluginState: {
      encoding: "binary",
      byteLength: pluginState.byteLength,
      source: pluginState.byteLength > 0 ? "native-plugin-state" : "pending-native-instantiation",
    },
  };
  const meta = Buffer.from(JSON.stringify(metadata), "utf8");
  const header = Buffer.alloc(24);
  header.write("FBPST", 0, "ascii");
  header.writeUInt8(0, 5);
  header.writeUInt16LE(1, 6);
  header.writeUInt32LE(meta.byteLength, 8);
  header.writeUInt32LE(pluginState.byteLength, 12);
  header.writeUInt32LE(0, 16);
  header.writeUInt32LE(0, 20);
  return Buffer.concat([header, meta, pluginState]);
}

async function writePreset(plugin: AudioPluginRegistryEntry): Promise<void> {
  await fsp.mkdir(path.dirname(plugin.presetPath), { recursive: true });
  const tmp = `${plugin.presetPath}.tmp`;
  await fsp.writeFile(tmp, buildPresetBinary(plugin, Buffer.alloc(0)));
  await fsp.rename(tmp, plugin.presetPath);
}

function resolveUniquePresetPath(plugin: AudioPluginRegistryEntry, occupied: Set<string>): AudioPluginRegistryEntry {
  const parsed = path.parse(plugin.presetPath);
  let candidate = plugin.presetPath;
  let index = 2;
  while (occupied.has(candidate.toLowerCase())) {
    candidate = path.join(parsed.dir, `${parsed.name} (${index})${parsed.ext}`);
    index += 1;
  }
  occupied.add(candidate.toLowerCase());
  return { ...plugin, presetPath: candidate };
}

function registryDisplayKey(plugin: AudioPluginRegistryEntry): string {
  return [
    plugin.vendor,
    plugin.name,
    plugin.format,
    plugin.category,
    plugin.kind,
  ].map((part) => part.trim().toLowerCase()).join("|");
}

function upsertPlugin(plugin: AudioPluginRegistryEntry): void {
  const registry = getDb();
  if (!registry) return;
  registry.prepare(`
    INSERT INTO audio_plugins (
      id, name, vendor, format, category, sub_categories, kind, path, class_id, version,
      sdk_metadata_loaded, preset_path, scanned_at, metadata_json
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      name = excluded.name,
      vendor = excluded.vendor,
      format = excluded.format,
      category = excluded.category,
      sub_categories = excluded.sub_categories,
      kind = excluded.kind,
      path = excluded.path,
      class_id = excluded.class_id,
      version = excluded.version,
      sdk_metadata_loaded = excluded.sdk_metadata_loaded,
      preset_path = excluded.preset_path,
      scanned_at = excluded.scanned_at,
      metadata_json = excluded.metadata_json
  `).run(
    plugin.id,
    plugin.name,
    plugin.vendor,
    plugin.format,
    plugin.rawCategory ?? plugin.category,
    plugin.subCategories ?? null,
    plugin.kind,
    plugin.path,
    plugin.classId ?? null,
    plugin.version ?? null,
    plugin.sdkMetadataLoaded ? 1 : 0,
    plugin.presetPath,
    plugin.scannedAt,
    JSON.stringify(plugin),
  );
}

function clearPluginRegistry(): void {
  const registry = getDb();
  if (!registry) return;
  registry.prepare("DELETE FROM audio_plugins").run();
}

function delayImmediate(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve));
}

function isSupportedPluginBundle(pluginPath: string): boolean {
  const extension = path.extname(pluginPath).toLowerCase();
  return extension === ".vst3" || extension === ".clap";
}

async function discoverPluginBundles(root: string, onFolder?: (folderPath: string, discovered: number) => void): Promise<string[]> {
  const found: string[] = [];
  const queue = [root];
  while (queue.length > 0) {
    const current = queue.shift();
    if (!current) continue;
    if (isSupportedPluginBundle(current)) {
      found.push(current);
      continue;
    }

    let entries: fs.Dirent[];
    try {
      entries = await fsp.readdir(current, { withFileTypes: true });
    } catch {
      continue;
    }

    for (const entry of entries) {
      const child = path.join(current, entry.name);
      if (isSupportedPluginBundle(child) && (entry.isDirectory() || entry.isFile())) {
        found.push(child);
        continue;
      }
      if (entry.isDirectory()) {
        queue.push(child);
      }
    }

    onFolder?.(current, found.length);
    await delayImmediate();
  }
  return found;
}

type ScanProgressCallback = (event: AudioPluginScanProgressEvent) => void;

function scannerWorkerPath(): string {
  return path.join(_dirname, "plugin-scanner-worker.js");
}

function scanBundleInWorker(bundlePath: string, timeoutMs = 45000): Promise<NativePluginInfo[]> {
  return new Promise((resolve, reject) => {
    const requestId = stableId(`${bundlePath}:${Date.now()}`, "scan");
    let settled = false;
    let child: ChildProcess | null = null;
    const finish = (callback: () => void) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        child?.kill();
      } catch {
        // ignore worker cleanup failures
      }
      callback();
    };
    const timer = setTimeout(() => {
      finish(() => reject(new Error(`Timed out scanning ${bundlePath}`)));
    }, timeoutMs);

    try {
      child = fork(scannerWorkerPath(), [], {
        env: { ...process.env, ELECTRON_RUN_AS_NODE: "1" },
        stdio: ["ignore", "ignore", "pipe", "ipc"],
      });
    } catch (error) {
      finish(() => reject(error));
      return;
    }

    child.stderr?.on("data", (chunk) => {
      const text = String(chunk).trim();
      if (text) console.warn(`[PluginHost scanner] ${text}`);
    });
    child.on("message", (message: unknown) => {
      const response = message as { id?: string; ok?: boolean; plugins?: NativePluginInfo[]; error?: string };
      if (response.id !== requestId) return;
      if (response.ok) {
        finish(() => resolve(Array.isArray(response.plugins) ? response.plugins : []));
      } else {
        finish(() => reject(new Error(response.error || `Failed scanning ${bundlePath}`)));
      }
    });
    child.on("error", (error) => {
      finish(() => reject(error));
    });
    child.on("exit", (code, signal) => {
      if (!settled) {
        finish(() => reject(new Error(`Scanner exited before response (code=${code ?? "null"} signal=${signal ?? "null"})`)));
      }
    });
    child.send({ id: requestId, path: bundlePath });
  });
}

export class PluginHostNative {
  async ensurePresetFolders(): Promise<void> {
    await Promise.all(presetSubfolders().map((folder) => fsp.mkdir(folder, { recursive: true })));
  }

  getStatus(): AudioPluginHostStatus {
    try {
      for (const folder of presetSubfolders()) {
        fs.mkdirSync(folder, { recursive: true });
      }
    } catch (error) {
      console.warn("[PluginHost] Failed to ensure preset folders:", error);
    }
    const native = loadAddon();
    let backend = "missing";
    let message = addonLoadError ?? "PluginHost native addon is unavailable.";
    if (native) {
      try {
        const status = native.initPluginHost?.();
        backend = status?.backend ?? native.getBackendVersion?.() ?? "PluginHost";
        message = status?.message ?? "PluginHost native scanner is ready.";
      } catch (error) {
        backend = "load-error";
        message = String(error);
      }
    }
    return {
      available: Boolean(native?.scanAudioPlugins ?? native?.scanVst3 ?? native?.scanClap),
      backend,
      message,
      dbPath: pluginDbPath(),
      presetRoot: presetRootPath(),
      defaultScanPaths: defaultScanPaths(),
    };
  }

  listPlugins(): AudioPluginRegistryEntry[] {
    const registry = getDb();
    if (!registry) return [];
    return registry
      .prepare("SELECT * FROM audio_plugins ORDER BY kind ASC, vendor ASC, name ASC")
      .all()
      .map((row) => rowFromDb(row as Record<string, unknown>));
  }

  async scanVst3(paths?: string[], onProgress?: ScanProgressCallback): Promise<AudioPluginScanResult> {
    const status = this.getStatus();
    const native = loadAddon();
    const requestedPaths = (paths?.length ? paths : status.defaultScanPaths)
      .map((p) => path.normalize(p))
      .filter((p, index, arr) => arr.indexOf(p) === index);
    const scanPaths = requestedPaths.filter((p) => fs.existsSync(p));
    const failed: AudioPluginScanResult["failed"] = [];
    const plugins: AudioPluginRegistryEntry[] = [];
    onProgress?.({ type: "started", status, scannedPaths: requestedPaths });
    if (!(native?.scanAudioPlugins ?? native?.scanVst3 ?? native?.scanClap)) {
      const result = { status, plugins: this.listPlugins(), scannedPaths: scanPaths, generatedPresets: 0, failed };
      onProgress?.({ type: "complete", result });
      return result;
    }

    const scannedAt = Date.now();
    let generatedPresets = 0;
    const occupiedPresetPaths = new Set<string>();
    const seenDisplayKeys = new Set<string>();
    clearPluginRegistry();
    for (const root of scanPaths) {
      let bundles: string[];
      try {
        bundles = await discoverPluginBundles(root, (folderPath, discovered) => {
          onProgress?.({ type: "folder", path: folderPath, discovered });
        });
      } catch (error) {
        failed.push({ path: root, error: String(error) });
        onProgress?.({ type: "failed", path: root, error: String(error) });
        continue;
      }

      for (const bundlePath of bundles) {
        try {
          const nativePlugins = await scanBundleInWorker(bundlePath);
          for (const nativePlugin of nativePlugins) {
            const normalizedPlugin = normalizeNativePlugin(nativePlugin, scannedAt);
            if (!normalizedPlugin) continue;
            const displayKey = registryDisplayKey(normalizedPlugin);
            if (seenDisplayKeys.has(displayKey)) continue;
            seenDisplayKeys.add(displayKey);
            const plugin = resolveUniquePresetPath(normalizedPlugin, occupiedPresetPaths);
            await writePreset(plugin);
            generatedPresets += 1;
            upsertPlugin(plugin);
            plugins.push(plugin);
            onProgress?.({ type: "plugin", plugin, generatedPresets });
          }
        } catch (error) {
          failed.push({ path: bundlePath, error: String(error) });
          onProgress?.({ type: "failed", path: bundlePath, error: String(error) });
        }
        await delayImmediate();
      }
    }

    const result = {
      status: this.getStatus(),
      plugins: this.listPlugins(),
      scannedPaths: scanPaths,
      generatedPresets,
      failed,
    };
    onProgress?.({ type: "complete", result });
    return result;
  }

  openPluginEditorWindow(options: PluginEditorWindowOptions): number | null {
    const native = loadAddon();
    if (!native?.openPluginEditorWindow) return null;
    startEditorParamPolling();
    return native.openPluginEditorWindow(options);
  }

  openPluginEditorForPath(pluginPath: string): number | null {
    const native = loadAddon();
    if (!native?.openPluginEditorForPath) return null;
    startEditorParamPolling();
    return native.openPluginEditorForPath(pluginPath);
  }

  closePluginEditorWindow(handle: number): void {
    const native = loadAddon();
    native?.closePluginEditorWindow?.(handle);
  }

  focusPluginEditorWindow(handle: number): void {
    const native = loadAddon();
    native?.focusPluginEditorWindow?.(handle);
  }

  resizePluginEditorWindow(handle: number, width: number, height: number): void {
    const native = loadAddon();
    native?.resizePluginEditorWindow?.(handle, width, height);
  }

  presetPathForPlugin(pluginId: string): string | null {
    const registry = getDb();
    if (!registry) return null;
    const row = registry.prepare("SELECT preset_path FROM audio_plugins WHERE id = ?").get(pluginId) as Record<string, unknown> | undefined;
    return typeof row?.preset_path === "string" ? row.preset_path : null;
  }
}

export const pluginHostNative = new PluginHostNative();
