import { spawn, type ChildProcess } from "node:child_process";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import readline from "node:readline";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// ── Protocol types (mirror apps/floatingwindow/src/protocol.rs) ───────────────

export type WindowKind =
  | "Mixer"
  | "Midi"
  | "Analyzer"
  | "PluginEditorPlaceholder";

type SerializedWindowKind =
  | "mixer"
  | "midi"
  | "analyzer"
  | "plugin-editor-placeholder";

export interface WindowBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface FloatingWindowDescriptor {
  id: string;
  kind: WindowKind;
  title: string;
  initialBounds?: WindowBounds;
  alwaysOnTop?: boolean;
}

// Incoming messages from the native process
type NativeMessage =
  | { type: "windowOpened"; id: string }
  | { type: "windowClosed"; id: string }
  | { type: "windowBoundsChanged"; id: string; bounds: WindowBounds }
  | { type: "command"; commandId: string; payload: unknown };

// Outgoing messages to the native process
type OutgoingMessage =
  | { type: "openWindow"; window: SerializedDescriptor }
  | { type: "closeWindow"; id: string }
  | { type: "focusWindow"; id: string }
  | { type: "mixer:update"; tracks: unknown[]; master: unknown }
  | { type: "midi:updateDevices"; devices: unknown[] }
  | { type: "midi:event"; event: unknown };

interface SerializedDescriptor {
  id: string;
  kind: SerializedWindowKind;
  title: string;
  initialBounds?: WindowBounds;
  alwaysOnTop: boolean;
}

function serializeWindowKind(kind: WindowKind): SerializedWindowKind {
  switch (kind) {
    case "Mixer":
      return "mixer";
    case "Midi":
      return "midi";
    case "Analyzer":
      return "analyzer";
    case "PluginEditorPlaceholder":
      return "plugin-editor-placeholder";
  }
}

const executableName = process.platform === "win32" ? "floatingwindow.exe" : "floatingwindow";

function uniqueExistingPath(candidates: string[]): string | null {
  const seen = new Set<string>();
  for (const candidate of candidates) {
    const normalized = path.normalize(candidate);
    if (seen.has(normalized)) continue;
    seen.add(normalized);
    if (fs.existsSync(normalized)) return normalized;
  }
  return null;
}

function resolveFloatingWindowBinary(explicitPath?: string): string | null {
  const electronRoot = path.resolve(__dirname, "..");
  const workspaceRoot = path.resolve(__dirname, "..", "..", "..");

  const candidates = [
    explicitPath,
    path.join(process.resourcesPath, executableName),
    path.join(electronRoot, "resources", executableName),
    path.join(workspaceRoot, "apps", "electron", "resources", executableName),
    path.join(workspaceRoot, "apps", "floatingwindow", "target", "release", executableName),
    path.join(workspaceRoot, "apps", "floatingwindow", "target", "debug", executableName),
    path.join(__dirname, "..", "..", "floatingwindow", executableName),
  ].filter((candidate): candidate is string => Boolean(candidate));

  return uniqueExistingPath(candidates);
}

// ── Manager ───────────────────────────────────────────────────────────────────

export class FloatingWindowManager extends EventEmitter {
  private proc: ChildProcess | null = null;
  private openWindows = new Set<string>();

  /** Optional caller-provided path to the floatingwindow binary. */
  private readonly binaryPath?: string;

  constructor(binaryPath?: string) {
    super();
    this.binaryPath = binaryPath;
  }

  // ── lifecycle ──────────────────────────────────────────────────────────────

  start(): boolean {
    if (this.proc) return true;

    const binaryPath = resolveFloatingWindowBinary(this.binaryPath);
    if (!binaryPath) {
      console.warn("[FloatingWindowManager] floatingwindow binary not found");
      this.emit("runtimeError", new Error("floatingwindow binary not found"));
      return false;
    }

    const proc = spawn(binaryPath, [], {
      stdio: ["pipe", "pipe", "inherit"],
    });
    this.proc = proc;

    proc.on("error", (err) => {
      this.proc = null;
      this.openWindows.clear();
      console.warn(`[FloatingWindowManager] failed to spawn ${binaryPath}:`, err);
      this.emit("runtimeError", err);
    });

    proc.on("exit", (code) => {
      this.proc = null;
      this.openWindows.clear();
      this.emit("exit", code);
    });

    if (proc.stdout) {
      const rl = readline.createInterface({ input: proc.stdout });
      rl.on("line", (line) => {
        if (!line.trim()) return;
        try {
          const msg = JSON.parse(line) as NativeMessage;
          this.handleIncoming(msg);
        } catch {
          // non-JSON output — ignore
        }
      });
    }

    return true;
  }

  stop(): void {
    this.proc?.stdin?.end();
    this.proc?.kill();
    this.proc = null;
    this.openWindows.clear();
  }

  get running(): boolean {
    return this.proc !== null && !this.proc.killed;
  }

  // ── window control ─────────────────────────────────────────────────────────

  openWindow(desc: FloatingWindowDescriptor): void {
    this.send({
      type: "openWindow",
      window: {
        id: desc.id,
        kind: serializeWindowKind(desc.kind),
        title: desc.title,
        initialBounds: desc.initialBounds,
        alwaysOnTop: desc.alwaysOnTop ?? false,
      },
    });
  }

  closeWindow(id: string): void {
    this.send({ type: "closeWindow", id });
  }

  focusWindow(id: string): void {
    this.send({ type: "focusWindow", id });
  }

  // ── data push ──────────────────────────────────────────────────────────────

  pushMixerUpdate(tracks: unknown[], master: unknown): void {
    this.send({ type: "mixer:update", tracks, master });
  }

  pushMidiDevices(devices: unknown[]): void {
    this.send({ type: "midi:updateDevices", devices });
  }

  pushMidiEvent(event: unknown): void {
    this.send({ type: "midi:event", event });
  }

  // ── internal ───────────────────────────────────────────────────────────────

  private send(msg: OutgoingMessage): void {
    if (!this.proc?.stdin?.writable) return;
    try {
      this.proc.stdin!.write(JSON.stringify(msg) + "\n");
    } catch {
      // process died between check and write
    }
  }

  private handleIncoming(msg: NativeMessage): void {
    switch (msg.type) {
      case "windowOpened":
        this.openWindows.add(msg.id);
        this.emit("windowOpened", msg.id);
        break;
      case "windowClosed":
        this.openWindows.delete(msg.id);
        this.emit("windowClosed", msg.id);
        break;
      case "windowBoundsChanged":
        this.emit("windowBoundsChanged", msg.id, msg.bounds);
        break;
      case "command":
        this.emit("command", msg.commandId, msg.payload);
        break;
    }
  }
}

// ── Singleton ─────────────────────────────────────────────────────────────────

let _instance: FloatingWindowManager | null = null;

export function getFloatingWindowManager(binaryPath?: string): FloatingWindowManager {
  if (!_instance) {
    _instance = new FloatingWindowManager(binaryPath);
  }
  return _instance;
}
