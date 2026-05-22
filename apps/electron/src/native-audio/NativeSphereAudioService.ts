/**
 * NativeSphereAudioService
 *
 * Manages the SphereDirectAudioEngine native Rust process from the Electron
 * main process.  Communicates via newline-delimited JSON on stdin/stdout
 * (stdio bridge).  Crashes are isolated — the UI renderer never sees a thrown
 * error from a process exit.
 *
 * Protocol (MVP):
 *   Renderer → Main (IPC) → Service (stdin JSON)
 *   Service stdout JSON → Main → Renderer (IPC reply)
 *
 *   Every message: { "id": "<uuid>", "method": "...", "params": {...} }
 *   Every reply:   { "id": "<uuid>", "result": <any> }
 *              or: { "id": "<uuid>", "error": "<string>" }
 */
import { spawn, type ChildProcess } from "node:child_process";
import path from "node:path";
import fs from "node:fs";
import { EventEmitter } from "node:events";
import type {
  SphereAudioStatus,
  SphereAudioDeviceInfo,
  SphereDeviceOpenConfig,
  SphereTransportState,
  SphereMeterSnapshot,
} from "../ipc/channels.js";

const ENGINE_BINARY_NAME =
  process.platform === "win32"
    ? "FutureBoardSphereAudio.exe"
    : "FutureBoardSphereAudio";

const ENGINE_VERSION_FALLBACK = "0.1.0-stub";

/** Paths where the native binary may be bundled (tried in order). */
function candidatePaths(appDir: string): string[] {
  return [
    // Packaged: next to the Electron resources directory
    path.join(appDir, "..", "native", ENGINE_BINARY_NAME),
    path.join(appDir, "..", "..", "native", ENGINE_BINARY_NAME),
    // Dev: built output of crates/SphereDirectAudioEngine
    path.join(appDir, "..", "..", "..", "frameworks", "SphereDirectAudioEngine", "target", "release", ENGINE_BINARY_NAME),
    path.join(appDir, "..", "..", "..", "frameworks", "SphereDirectAudioEngine", "target", "debug", ENGINE_BINARY_NAME),
  ];
}

// ── Pending RPC calls ─────────────────────────────────────────────────────────

type RpcResolve = (value: unknown) => void;
type RpcReject  = (reason: Error)  => void;

let _msgCounter = 0;
function nextId(): string {
  return `rpc_${++_msgCounter}`;
}

// ── Service singleton ─────────────────────────────────────────────────────────

export class NativeSphereAudioService extends EventEmitter {
  private _proc:     ChildProcess | null = null;
  private _binaryPath: string | null = null;
  private _pending  = new Map<string, { resolve: RpcResolve; reject: RpcReject }>();
  private _status: SphereAudioStatus = {
    available:    false,
    running:      false,
    streamOpen:   false,
    transportPlaying: false,
    positionSeconds:  0,
    version:      ENGINE_VERSION_FALLBACK,
    sampleRate:   44100,
    bufferSize:   256,
    inputDevice:  null,
    outputDevice: null,
    cpuLoad:      0,
    xrunCount:    0,
  };
  private _lineBuffer = "";

  // ── Discovery ──────────────────────────────────────────────────────────────

  findBinary(appDir: string): string | null {
    for (const p of candidatePaths(appDir)) {
      if (fs.existsSync(p)) return p;
    }
    return null;
  }

  // ── Process lifecycle ──────────────────────────────────────────────────────

  start(appDir: string): boolean {
    if (this._proc) return true; // already running

    this._binaryPath = this.findBinary(appDir);
    if (!this._binaryPath) {
      console.warn(
        "[SphereAudio] Binary not found. Searched:\n" +
        candidatePaths(appDir).map((p) => `  ${p}`).join("\n"),
      );
      return false;
    }

    console.log("[SphereAudio] Starting native engine:", this._binaryPath);

    this._proc = spawn(this._binaryPath, ["--ipc-stdio"], {
      stdio: ["pipe", "pipe", "pipe"],
    });

    this._proc.stdout?.setEncoding("utf8");
    this._proc.stdout?.on("data", (chunk: string) => this._onStdout(chunk));
    this._proc.stderr?.setEncoding("utf8");
    this._proc.stderr?.on("data", (line: string) =>
      console.warn("[SphereAudio:stderr]", line.trim()),
    );

    this._proc.on("error", (err) => {
      console.error("[SphereAudio] Process error:", err);
      this._handleExit();
    });

    this._proc.on("exit", (code, signal) => {
      console.warn(`[SphereAudio] Process exited (code=${code} signal=${signal})`);
      this._handleExit();
    });

    this._status = { ...this._status, running: true };
    return true;
  }

  stop(): void {
    if (!this._proc) return;
    try {
      this._proc.kill("SIGTERM");
    } catch {/* already gone */}
    this._proc = null;
    this._status = { ...this._status, running: false };
    this._rejectAllPending("Native engine stopped");
  }

  isRunning(): boolean {
    return this._proc !== null && !this._proc.killed;
  }

  // ── RPC ───────────────────────────────────────────────────────────────────

  private _send(method: string, params: unknown = {}): Promise<unknown> {
    if (!this._proc?.stdin) {
      return Promise.reject(new Error("Native engine not running"));
    }
    return new Promise<unknown>((resolve, reject) => {
      const id = nextId();
      this._pending.set(id, { resolve, reject });
      const msg = JSON.stringify({ id, method, params }) + "\n";
      this._proc!.stdin!.write(msg, (err) => {
        if (err) {
          this._pending.delete(id);
          reject(err);
        }
      });
      // Timeout after 5 seconds
      setTimeout(() => {
        if (this._pending.has(id)) {
          this._pending.delete(id);
          reject(new Error(`RPC timeout: ${method}`));
        }
      }, 5_000);
    });
  }

  private _onStdout(chunk: string): void {
    this._lineBuffer += chunk;
    const lines = this._lineBuffer.split("\n");
    this._lineBuffer = lines.pop() ?? "";
    for (const line of lines) {
      if (!line.trim()) continue;
      try {
        const msg = JSON.parse(line) as {
          id?: string;
          method?: string;
          result?: unknown;
          error?: string;
          event?: string;
          data?: unknown;
        };

        // RPC reply
        if (msg.id) {
          const pending = this._pending.get(msg.id);
          if (pending) {
            this._pending.delete(msg.id);
            if (msg.error) {
              pending.reject(new Error(msg.error));
            } else {
              pending.resolve(msg.result);
            }
          }
          continue;
        }

        // Push event (meters, transport, etc.)
        if (msg.event) {
          this.emit(msg.event, msg.data);
        }
      } catch (e) {
        console.warn("[SphereAudio] Failed to parse stdout line:", line, e);
      }
    }
  }

  private _handleExit(): void {
    this._proc = null;
    this._status = { ...this._status, running: false };
    this._rejectAllPending("Native engine process exited unexpectedly");
    this.emit("crashed");
  }

  private _rejectAllPending(reason: string): void {
    for (const { reject } of this._pending.values()) {
      reject(new Error(reason));
    }
    this._pending.clear();
  }

  // ── Public API (called by IPC handlers) ────────────────────────────────────

  async getStatus(): Promise<SphereAudioStatus> {
    if (!this.isRunning()) return { ...this._status, running: false };
    try {
      const result = await this._send("getStatus") as Partial<SphereAudioStatus>;
      this._status = { ...this._status, ...result, running: true };
    } catch {/* process may be starting — return cached */}
    return this._status;
  }

  async getVersion(): Promise<string> {
    if (!this.isRunning()) return ENGINE_VERSION_FALLBACK;
    try {
      return (await this._send("getVersion")) as string;
    } catch {
      return ENGINE_VERSION_FALLBACK;
    }
  }

  async listInputDevices(): Promise<SphereAudioDeviceInfo[]> {
    if (!this.isRunning()) return [];
    return (await this._send("listInputDevices")) as SphereAudioDeviceInfo[];
  }

  async listOutputDevices(): Promise<SphereAudioDeviceInfo[]> {
    if (!this.isRunning()) return [];
    return (await this._send("listOutputDevices")) as SphereAudioDeviceInfo[];
  }

  async openDevice(config: SphereDeviceOpenConfig): Promise<void> {
    await this._send("openDevice", config);
  }

  async closeDevice(): Promise<void> {
    await this._send("closeDevice");
  }

  async startPlayback(): Promise<void> {
    await this._send("start");
  }

  async stopPlayback(): Promise<void> {
    await this._send("stop");
  }

  async setTransportState(state: SphereTransportState): Promise<void> {
    await this._send("setTransportState", state);
  }

  async getTransportState(): Promise<{ playing: boolean; positionSeconds: number }> {
    if (!this.isRunning()) return { playing: false, positionSeconds: 0 };
    return (await this._send("getTransportState")) as {
      playing: boolean;
      positionSeconds: number;
    };
  }

  async updateTrackParam(
    trackId: string,
    paramId: string,
    value: unknown,
  ): Promise<void> {
    await this._send("updateTrackParam", { trackId, paramId, value });
  }

  async updateInsertParam(
    trackId: string,
    insertId: string,
    paramId: string,
    value: unknown,
  ): Promise<void> {
    await this._send("updateInsertParam", { trackId, insertId, paramId, value });
  }

  async loadProject(snapshot: unknown): Promise<void> {
    await this._send("loadProject", snapshot);
  }

  async updateClip(clipId: string, patch: unknown): Promise<void> {
    await this._send("updateClip", { clipId, patch });
  }

  async getMeters(): Promise<SphereMeterSnapshot> {
    if (!this.isRunning()) {
      return { tracks: {}, master: { left: 0, right: 0 }, timestamp: Date.now() };
    }
    return (await this._send("getMeters")) as SphereMeterSnapshot;
  }
}

// ── Module singleton ──────────────────────────────────────────────────────────

export const nativeSphereAudioService = new NativeSphereAudioService();
