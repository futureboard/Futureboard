import { useEffect, useMemo, useRef, useState } from "react";
import { useUIStore } from "../store/uiStore";
import { useProjectStore } from "../store/projectStore";
import { buildSelectionState, getSelectionSummary } from "../store/selectionSelectors";
import { activeAudioEngine } from "../engine/activeAudioEngine";
import { useAudioBackendStore } from "../store/audioBackendStore";
import { C } from "../theme";
import { formatBarBeat } from "../utils/musicalTime";
import { pxPerBeat } from "../utils/musicalGrid";
import { audioCacheManager } from "../audio/AudioCacheManager";
import { getPeakCacheStats } from "../engine/peakChunkCache";
import { useBackgroundTaskStore, type BackgroundTask } from "../store/backgroundTaskStore";
import { shouldRunVisualFrame } from "../utils/visualFrameRate";
import { useSettingsStore } from "../store/settingsStore";

// ── Save status ───────────────────────────────────────────────────────────────
type SaveStatus = "saved" | "unsaved" | "saving" | "error";

const SAVE_COLOR: Record<SaveStatus, string> = {
  saved:   C.green,
  unsaved: C.yellow,
  saving:  C.accent,
  error:   C.red,
};
const SAVE_LABEL: Record<SaveStatus, string> = {
  saved:   "Saved",
  unsaved: "Unsaved",
  saving:  "Saving…",
  error:   "Save failed",
};

// ── Tool display names ────────────────────────────────────────────────────────
const TOOL_LABEL: Record<string, string> = {
  pointer:    "Pointer",
  pen:        "Pen",
  cut:        "Cut",
  glue:       "Glue",
  mute:       "Mute",
  time:       "Stretch",
  automation: "Auto",
};

// ── Small helpers ─────────────────────────────────────────────────────────────
function Dot({ color }: { color: string }) {
  return (
    <span
      aria-hidden
      style={{
        display: "inline-block",
        width: 5, height: 5,
        borderRadius: "50%",
        background: color,
        flexShrink: 0,
        marginBottom: 1,
      }}
    />
  );
}

function Sep() {
  return <span className="mx-[5px]" style={{ color: "rgba(255,255,255,0.12)" }} aria-hidden>·</span>;
}

// ── StatusBar ─────────────────────────────────────────────────────────────────
export function StatusBar() {
  // Store subscriptions — individual selectors avoid broad re-renders
  const selectedClipIds       = useUIStore((s) => s.selectedClipIds);
  const selectedTrackId       = useUIStore((s) => s.selectedTrackId);
  const selectedBrowserFileId = useUIStore((s) => s.selectedBrowserFileId);
  const focusedPanel          = useUIStore((s) => s.focusedPanel);
  const currentTool           = useUIStore((s) => s.currentTool);
  const snapToGrid            = useUIStore((s) => s.snapToGrid);
  const gridDivision          = useUIStore((s) => s.arrangementGridDivision);
  const pixelsPerSecond       = useUIStore((s) => s.pixelsPerSecond);
  const saveStatus            = useUIStore((s) => s.saveStatus);
  const visualFrameRate       = useSettingsStore((s) => s.visualFrameRate);
  const backgroundTasks       = useBackgroundTaskStore((s) => s.tasks);
  const taskPanelOpen         = useBackgroundTaskStore((s) => s.panelOpen);
  const setTaskPanelOpen      = useBackgroundTaskStore((s) => s.setPanelOpen);

  const project       = useProjectStore((s) => s.project);
  const backendState  = useAudioBackendStore();
  const bpm           = project.bpm;
  const timeSignature = project.timeSignature;

  // High-frequency values live in local state, updated by a throttled RAF loop
  // so they never go through Zustand and don't trigger global re-renders.
  const [pos,   setPos]   = useState("1.1");
  const [fps,   setFps]   = useState(60);
  const [memMB, setMemMB] = useState<number | null>(null);

  // Refs let the RAF closure read current bpm/timeSig without restarting
  const bpmRef    = useRef(bpm);
  const timeSigRef = useRef(timeSignature);
  bpmRef.current   = bpm;
  timeSigRef.current = timeSignature;

  useEffect(() => {
    let frames    = 0;
    let lastFlush = performance.now();
    let lastFrameAt = 0;
    let raf: number;

    const tick = (now: number) => {
      if (shouldRunVisualFrame(lastFrameAt, now)) {
        frames++;
        lastFrameAt = now;
      }
      if (now - lastFlush >= 250) {        // ~4 Hz refresh
        const elapsed = now - lastFlush;
        setFps(Math.round((frames * 1000) / elapsed));
        setPos(
          formatBarBeat(
            activeAudioEngine.projectTime,
            bpmRef.current,
            timeSigRef.current ?? { numerator: 4, denominator: 4 },
          ),
        );
        // performance.memory is Chrome/Electron-only; cast safely
        const perf = performance as { memory?: { usedJSHeapSize: number } };
        if (perf.memory) setMemMB(perf.memory.usedJSHeapSize / 1_048_576);
        frames    = 0;
        lastFlush = now;
      }
      raf = requestAnimationFrame(tick);
    };

    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, []); // empty deps: bpm/timeSig are read via refs

  // Selection summary via centralized selector — recomputed when selection or project changes.
  const selText = useMemo(() => {
    const sel = buildSelectionState({
      focusedPanel,
      selectedTrackId,
      selectedClipIds,
      selectedBrowserFileId,
    });
    return getSelectionSummary(project, sel) || "No selection";
  }, [focusedPanel, selectedTrackId, selectedClipIds, selectedBrowserFileId, project]);

  const timeSig   = timeSignature ?? { numerator: 4, denominator: 4 };
  const ppb       = Math.round(pxPerBeat(pixelsPerSecond, bpm));
  const saveSt    = saveStatus as SaveStatus;
  const isUnlimitedFps = visualFrameRate === "unlimited";
  const fpsLabel  = isUnlimitedFps ? `${fps} fps ∞` : `${fps} fps`;
  const fpsColor  = fps >= 55 ? C.green : fps >= 30 ? C.yellow : C.red;
  const audioStats = audioCacheManager.getStats();
  const sourceBytes = project.files.reduce((sum, file) => sum + (file.size ?? 0), 0);
  const peakBytes = getPeakCacheStats().cacheBytes;
  const missingAssets = project.files.filter((file) => file.storageProvider === "missing").length;
  const audioDebug = [
    backendState.active ? engineLabel(backendState.active) : "Audio initializing",
    backendState.runtime,
    backendState.contextState && backendState.contextState !== "uninitialized" ? backendState.contextState : null,
    backendState.fallbackReason ? `fallback: ${backendState.fallbackReason}` : null,
    backendState.error ? `error: ${backendState.error}` : null,
    sourceBytes > 0 ? `${formatBytes(sourceBytes)} source` : null,
    audioStats.decodedBytes > 0 ? `${formatBytes(audioStats.decodedBytes)} decoded` : null,
    peakBytes > 0 ? `peaks ${formatBytes(peakBytes)}` : null,
    audioStats.processedBytes > 0 ? `processed ${formatBytes(audioStats.processedBytes)}` : null,
    missingAssets > 0 ? `${missingAssets} missing` : null,
  ].filter(Boolean).join(" · ");
  const taskStatus = useMemo(() => getBackgroundStatus(Object.values(backgroundTasks)), [backgroundTasks]);

  return (
    <div
      className="relative flex h-[22px] shrink-0 select-none items-center justify-between overflow-visible border-t border-daw-border px-3 text-[10px]"
      style={{ background: C.sunken }}
    >
      {/* ── Left: Save · Selection · Tool ── */}
      <div className="flex min-w-0 shrink items-center">
        <span className="flex items-center gap-1 text-daw-dim">
          <Dot color={SAVE_COLOR[saveSt]} />
          {SAVE_LABEL[saveSt]}
        </span>
        <Sep />
        <span className="max-w-[18ch] truncate text-daw-dim" title={selText}>
          {selText}
        </span>
        <Sep />
        <span className="text-daw-faint">
          {TOOL_LABEL[currentTool] ?? currentTool}
        </span>
      </div>

      {/* ── Center: Position · BPM · TimeSig · Snap · Zoom ── */}
      <div className="hidden shrink-0 items-center lg:flex">
        <span className="tabular-nums text-daw-text">{pos}</span>
        <Sep />
        <span className="tabular-nums text-daw-dim">{bpm} BPM</span>
        <Sep />
        <span className="tabular-nums text-daw-dim">
          {timeSig.numerator}/{timeSig.denominator}
        </span>
        <Sep />
        <span
          title={snapToGrid ? `Snap to grid: ${gridDivision}` : "Snap to grid: OFF"}
          style={{ color: snapToGrid ? C.accent : C.faint }}
        >
          {snapToGrid ? `Snap ${gridDivision}` : "Free"}
        </span>
        <Sep />
        <span className="tabular-nums text-daw-faint">{ppb} px/bt</span>
      </div>

      {/* ── Right: FPS · Memory · Audio ── */}
      <div className="flex shrink-0 items-center">
        <button
          type="button"
          className="mr-1 max-w-[30ch] truncate rounded border border-white/10 bg-white/[0.03] px-2 py-[1px] text-left text-daw-dim hover:border-daw-accent/40 hover:text-daw-text"
          title="Background tasks"
          onClick={() => setTaskPanelOpen(!taskPanelOpen)}
        >
          <Dot color={taskStatus.color} /> <span className="ml-1">{taskStatus.label}</span>
        </button>
        <Sep />
        <span
          className="tabular-nums"
          style={{ color: fpsColor }}
          title="UI frames per second"
        >
          {fpsLabel}
        </span>
        {memMB !== null && (
          <>
            <Sep />
            <span className="tabular-nums text-daw-faint" title="JS heap usage">
              {memMB >= 1024
                ? `${(memMB / 1024).toFixed(1)} GB`
                : `${Math.round(memMB)} MB`}
            </span>
          </>
        )}
        <Sep />
        <span className="max-w-[36ch] truncate text-daw-faint" title={audioDebug || "Audio OK"}>
          {audioDebug || "Audio OK"}
        </span>
      </div>
      {taskPanelOpen && <BackgroundTaskPanel tasks={Object.values(backgroundTasks)} onClose={() => setTaskPanelOpen(false)} />}
    </div>
  );
}

function getBackgroundStatus(tasks: BackgroundTask[]): { label: string; color: string } {
  const failed = tasks.filter((task) => task.status === "failed");
  if (failed.length > 0) return { label: `${failed.length} background job${failed.length === 1 ? "" : "s"} failed`, color: C.red };
  const runningImport = tasks.find((task) => task.kind === "import" && task.status === "running");
  if (runningImport) return { label: progressLabel("Importing audio", runningImport), color: C.accent };
  const runningPeak = tasks.find((task) => (task.kind === "peak-generation" || task.kind === "waveform") && task.status === "running");
  if (runningPeak) return { label: progressLabel("Generating waveforms", runningPeak), color: C.violet };
  const runningCache = tasks.find((task) => task.kind === "peak-loading" && task.status === "running");
  if (runningCache) return { label: "Loading peak chunks...", color: C.blue };
  const nativeOrSave = tasks.find((task) => (task.kind === "native-sync" || task.kind === "project-save") && (task.status === "running" || task.status === "queued"));
  if (nativeOrSave) return { label: nativeOrSave.kind === "project-save" ? "Saving project..." : "Syncing native engine...", color: C.yellow };
  return { label: "Background idle", color: C.green };
}

function progressLabel(prefix: string, task: BackgroundTask): string {
  const progress = task.progress;
  if (!progress || progress.total <= 0) return prefix;
  return `${prefix} ${progress.current}/${progress.total}`;
}

function BackgroundTaskPanel({ tasks, onClose }: { tasks: BackgroundTask[]; onClose: () => void }) {
  const sorted = [...tasks].sort((a, b) => (b.updatedAt ?? 0) - (a.updatedAt ?? 0));
  const active = sorted.filter((task) => task.status === "running" || task.status === "paused");
  const queued = sorted.filter((task) => task.status === "queued");
  const failed = sorted.filter((task) => task.status === "failed");
  const complete = sorted.filter((task) => task.status === "complete").slice(0, 8);

  return (
    <div className="absolute bottom-[26px] right-3 z-[300] w-[360px] overflow-hidden rounded-lg border border-daw-border bg-daw-surface shadow-2xl">
      <div className="flex items-center justify-between border-b border-daw-border px-3 py-2">
        <div className="text-[11px] font-semibold text-daw-text">Background Tasks</div>
        <button type="button" className="rounded px-1.5 py-0.5 text-[10px] text-daw-faint hover:bg-white/10 hover:text-daw-text" onClick={onClose}>
          Close
        </button>
      </div>
      <div className="max-h-[360px] overflow-auto p-2">
        <TaskSection title="Active" tasks={active} empty="No active jobs" />
        <TaskSection title="Queued" tasks={queued} empty="Queue is clear" />
        <TaskSection title="Failed" tasks={failed} empty="No failures" />
        <TaskSection title="Recently Completed" tasks={complete} empty="Nothing completed yet" />
      </div>
    </div>
  );
}

function TaskSection({ title, tasks, empty }: { title: string; tasks: BackgroundTask[]; empty: string }) {
  return (
    <section className="mb-2 last:mb-0">
      <div className="mb-1 px-1 text-[9px] font-semibold uppercase tracking-wider text-daw-faint">{title}</div>
      {tasks.length === 0 ? (
        <div className="px-1 py-1 text-[10px] text-daw-faint/70">{empty}</div>
      ) : (
        <div className="space-y-1">
          {tasks.map((task) => <TaskRow key={task.id} task={task} />)}
        </div>
      )}
    </section>
  );
}

function TaskRow({ task }: { task: BackgroundTask }) {
  const pct = task.progress && task.progress.total > 0
    ? Math.max(0, Math.min(100, (task.progress.current / task.progress.total) * 100))
    : null;
  return (
    <div className="rounded-md border border-white/10 bg-black/15 px-2 py-1.5">
      <div className="flex min-w-0 items-center gap-2">
        <span className="shrink-0 text-[11px] text-daw-faint">{taskIcon(task.kind)}</span>
        <span className="min-w-0 flex-1 truncate text-[11px] text-daw-text" title={task.title}>{task.title}</span>
        <span className="rounded border border-white/10 px-1.5 py-[1px] text-[9px] text-daw-dim">{task.status}</span>
        {task.cancellable && (
          <button
            type="button"
            className="rounded px-1.5 py-[1px] text-[9px] text-daw-faint hover:bg-white/10 hover:text-daw-text"
            onClick={() => useBackgroundTaskStore.getState().cancelTask(task.id)}
          >
            Cancel
          </button>
        )}
      </div>
      {task.detail && <div className="mt-0.5 truncate pl-5 text-[10px] text-daw-faint" title={task.detail}>{task.detail}</div>}
      {pct !== null && (
        <div className="mt-1 h-1 overflow-hidden rounded-full bg-white/10">
          <div className="h-full rounded-full bg-daw-accent" style={{ width: `${pct}%` }} />
        </div>
      )}
      {task.error && <div className="mt-0.5 truncate pl-5 text-[10px] text-red-300" title={task.error}>{task.error}</div>}
    </div>
  );
}

function taskIcon(kind: BackgroundTask["kind"]): string {
  switch (kind) {
    case "import": return "IN";
    case "media-copy": return "CP";
    case "metadata-scan": return "MD";
    case "waveform":
    case "peak-generation":
    case "peak-loading": return "WF";
    case "native-sync": return "NS";
    case "project-save": return "SV";
    case "recording": return "RC";
    default: return "BG";
  }
}

function engineLabel(active: string): string {
  switch (active) {
    case "sphere-native": return "Sphere Native";
    case "rust-wasm": return "Rust WASM";
    case "web-audio": return "WebAudio";
    default: return active;
  }
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)}GB`;
  if (bytes >= 1024 * 1024) return `${Math.round(bytes / (1024 * 1024))}MB`;
  return `${Math.max(1, Math.round(bytes / 1024))}KB`;
}
