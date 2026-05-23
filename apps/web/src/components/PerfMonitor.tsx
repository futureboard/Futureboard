import { useEffect, useRef, useState } from "react";
import { platform } from "../platform";
import { audioImportQueue } from "../engine/AudioImportQueue";
import { useProjectStore } from "../store/projectStore";
import { useDragWorkflowStore } from "../store/dragWorkflowStore";
import { backgroundTaskStats } from "../store/backgroundTaskStore";
import { isScrollingNow } from "../engine/scrollController";
import { shouldRunVisualFrame } from "../utils/visualFrameRate";

type GpuInfo = {
  hardwareAccelerationEnabled: boolean;
  features: Record<string, string>;
  gpuDescription: string | null;
  electronVersion: string;
  chromeVersion: string;
} | null;

type Stats = {
  fps: number;
  frameMs: number;
  isScrolling: boolean;
  gpuInfo: GpuInfo;
  webgl: string;
  webgpu: string;
  audioImport: ReturnType<typeof audioImportQueue.getDebugStats>;
  drag: ReturnType<typeof getDragStats>;
  backgroundTasks: ReturnType<typeof backgroundTaskStats>;
};

function getDragStats() {
  return useDragWorkflowStore.getState().stats;
}

function getWebGLStatus(): string {
  try {
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("webgl2") ?? canvas.getContext("webgl");
    if (!ctx) return "NO";
    const dbg = (ctx as WebGLRenderingContext).getExtension("WEBGL_debug_renderer_info");
    if (dbg) {
      const renderer = (ctx as WebGLRenderingContext).getParameter(dbg.UNMASKED_RENDERER_WEBGL) as string;
      return renderer ? `OK (${renderer.slice(0, 40)})` : "OK";
    }
    return "OK";
  } catch {
    return "NO";
  }
}

async function getWebGPUStatus(): Promise<string> {
  try {
    if (!("gpu" in navigator)) return "NO";
    const adapter = await (navigator as unknown as { gpu: { requestAdapter(): Promise<unknown> } }).gpu.requestAdapter();
    return adapter ? "OK" : "NO (no adapter)";
  } catch {
    return "NO";
  }
}

export function PerfMonitor({ visible }: { visible: boolean }) {
  const [stats, setStats] = useState<Stats>({
    fps: 0,
    frameMs: 0,
    isScrolling: false,
    gpuInfo: null,
    webgl: "...",
    webgpu: "...",
    audioImport: audioImportQueue.getDebugStats(),
    drag: getDragStats(),
    backgroundTasks: backgroundTaskStats(),
  });

  const frameTimesRef = useRef<number[]>([]);
  const lastFrameRef = useRef(performance.now());
  const rafRef = useRef<number | null>(null);

  useEffect(() => {
    if (!visible) {
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
      return;
    }

    if (platform.kind === "electron") {
      bridge().sys.getGpuInfo().then((info) => {
        setStats((s) => ({ ...s, gpuInfo: info }));
      }).catch(() => {});
    }

    setStats((s) => ({ ...s, webgl: getWebGLStatus() }));
    getWebGPUStatus().then((v) => setStats((s) => ({ ...s, webgpu: v })));

    const tick = (now: number) => {
      if (!shouldRunVisualFrame(lastFrameRef.current, now)) {
        rafRef.current = requestAnimationFrame(tick);
        return;
      }
      const delta = now - lastFrameRef.current;
      lastFrameRef.current = now;

      const times = frameTimesRef.current;
      times.push(delta);
      if (times.length > 60) times.shift();

      const avgMs = times.reduce((a, b) => a + b, 0) / times.length;
      const fps = Math.round(1000 / avgMs);

      setStats((s) => ({
        ...s,
        fps,
        frameMs: Math.round(avgMs * 10) / 10,
        isScrolling: isScrollingNow(),
        audioImport: audioImportQueue.getDebugStats(),
        drag: getDragStats(),
        backgroundTasks: backgroundTaskStats(),
      }));
      rafRef.current = requestAnimationFrame(tick);
    };

    rafRef.current = requestAnimationFrame(tick);
    return () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
    };
  }, [visible]);

  if (!visible) return null;

  const { fps, frameMs, isScrolling, gpuInfo, webgl, webgpu, audioImport, drag, backgroundTasks } = stats;
  const { peakMeta, project } = useProjectStore.getState();
  const visibleTrackCount = project.tracks.length;
  const hwAccel  = gpuInfo?.hardwareAccelerationEnabled ?? (platform.kind !== "electron" ? true : null);
  const canvasOop = gpuInfo?.features?.["canvas_oop_rasterization"] ?? null;

  // Count loaded peak levels across all files
  let totalLevelCount = 0;
  for (const fileLevels of peakMeta.values()) totalLevelCount += fileLevels.size;

  const peakCacheMB  = audioImport.peakCacheBytes / 1024 / 1024;
  const decodedMB    = audioImport.decodedBufferBytes / 1024 / 1024;
  const canvasMpx    = audioImport.canvasPixels / 1_000_000;

  return (
    <div
      className="pointer-events-none fixed bottom-8 right-4 z-[9999] min-w-[260px] rounded border border-white/10 bg-black/80 p-2 font-mono text-[10px] text-white/80 shadow-xl backdrop-blur-sm"
    >
      <div className="mb-1 text-[9px] font-semibold uppercase tracking-widest text-white/40">Perf Monitor</div>

      <Row label="FPS"      value={`${fps}`}        ok={fps >= 55}      warn={fps >= 30} />
      <Row label="Frame"    value={`${frameMs} ms`} ok={frameMs <= 16.7} warn={frameMs <= 33} />
      <Row label="Scrolling" value={isScrolling ? "active (fast draw)" : "idle"} ok={!isScrolling} warn={false} />

      <Divider />

      {platform.kind === "electron" && (
        <>
          <Row
            label="GPU HW"
            value={hwAccel === true ? "enabled" : hwAccel === false ? "SW render (disabled)" : "unknown"}
            ok={hwAccel === true}
            warn={hwAccel === null}
          />
          <Row
            label="Canvas OOP"
            value={canvasOop ?? "unknown"}
            ok={canvasOop === "enabled" || canvasOop === "enabled_on"}
            warn={canvasOop === null}
          />
          <Row label="Electron" value={gpuInfo?.electronVersion ?? "..."} />
          <Row label="Chrome"   value={gpuInfo?.chromeVersion   ?? "..."} />
          <Divider />
        </>
      )}

      <Row label="WebGL2" value={webgl}  ok={webgl.startsWith("OK")}  warn={false} />
      <Row label="WebGPU" value={webgpu} ok={webgpu.startsWith("OK")} warn={false} />

      <Divider />

      <Row label="Drag events" value={`${drag.dragOverEventsPerSecond}/s`} ok={drag.dragOverEventsPerSecond < 120} warn={drag.dragOverEventsPerSecond < 240} />
      <Row label="Drag frames" value={`${drag.dragPreviewFramesPerSecond}/s · ${drag.dragPreviewUpdateMs.toFixed(2)} ms`} ok={drag.dragPreviewUpdateMs < 4} warn={drag.dragPreviewUpdateMs < 10} />
      <Row label="Drag mut" value={`${drag.projectMutationsDuringDrag} project / ${drag.nativeSyncDuringDrag} native`} ok={drag.projectMutationsDuringDrag === 0 && drag.nativeSyncDuringDrag === 0} warn={false} />
      <Divider />

      <Row label="Import Q" value={`${audioImport.importQueuePending} queued / ${audioImport.importQueueActive} active`} ok={audioImport.importQueueActive === 0} warn={audioImport.importQueueActive > 0} />
      <Row label="Peak Q" value={`${audioImport.peakQueuePending} queued / ${audioImport.peakQueueActive} active`} ok={audioImport.peakQueueActive === 0} warn={audioImport.peakQueueActive > 0} />
      <Row label="BG tasks" value={`${backgroundTasks.active} active / ${backgroundTasks.failed} failed`} ok={backgroundTasks.active === 0 && backgroundTasks.failed === 0} warn={backgroundTasks.failed === 0} />
      <Row label="Sources"  value={`${audioImport.sourceTotalMB.toFixed(1)} MB`} />

      <Divider />

      <Row
        label="Peak cache"
        value={`${peakCacheMB.toFixed(1)} MB · ${audioImport.loadedChunks} chunks`}
        ok={peakCacheMB < 64}
        warn={peakCacheMB < 100}
      />
      <Row
        label="Evictions"
        value={`${audioImport.evictions}`}
        ok={audioImport.evictions === 0}
        warn={audioImport.evictions > 0}
      />
      <Row
        label="Decoded buf"
        value={`${decodedMB.toFixed(1)} MB`}
        ok={audioImport.decodedBuffersCount === 0}
        warn={audioImport.decodedBuffersCount > 0}
      />
      <Row
        label="Canvas px"
        value={`${canvasMpx.toFixed(1)} Mpx`}
        ok={canvasMpx < 50}
        warn={canvasMpx < 200}
      />
      <Row label="Peak lvls" value={`${totalLevelCount} levels / ${project.files.length} files`} />
      <Row label="Tracks"    value={`${visibleTrackCount}`} />

      <div className="mt-1.5 text-[9px] text-white/30">Ctrl+Shift+P to toggle</div>
    </div>
  );
}

function Row({ label, value, ok, warn }: { label: string; value: string; ok?: boolean; warn?: boolean }) {
  const color =
    ok === true ? "text-green-400" :
    ok === false && warn === true ? "text-yellow-400" :
    ok === false ? "text-red-400" :
    "text-white/60";

  return (
    <div className="flex justify-between gap-4">
      <span className="text-white/40">{label}</span>
      <span className={color}>{value}</span>
    </div>
  );
}

function Divider() {
  return <div className="my-1 h-px bg-white/10" />;
}

function bridge() {
  const b = typeof window !== "undefined" ? window.dawElectron : undefined;
  if (!b) throw new Error("dawElectron bridge not found");
  return b;
}
