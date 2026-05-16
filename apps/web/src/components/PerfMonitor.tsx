import { useEffect, useRef, useState } from "react";
import { platform } from "../platform";

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
  gpuInfo: GpuInfo;
  webgl: string;
  webgpu: string;
};

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
    gpuInfo: null,
    webgl: "...",
    webgpu: "...",
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

    // Fetch GPU info once on mount
    if (platform.kind === "electron") {
      bridge().sys.getGpuInfo().then((info) => {
        setStats((s) => ({ ...s, gpuInfo: info }));
      }).catch(() => {});
    }

    getWebGLStatus();
    setStats((s) => ({ ...s, webgl: getWebGLStatus() }));
    getWebGPUStatus().then((v) => setStats((s) => ({ ...s, webgpu: v })));

    const tick = (now: number) => {
      const delta = now - lastFrameRef.current;
      lastFrameRef.current = now;

      const times = frameTimesRef.current;
      times.push(delta);
      if (times.length > 60) times.shift();

      const avgMs = times.reduce((a, b) => a + b, 0) / times.length;
      const fps = Math.round(1000 / avgMs);

      setStats((s) => ({ ...s, fps, frameMs: Math.round(avgMs * 10) / 10 }));
      rafRef.current = requestAnimationFrame(tick);
    };

    rafRef.current = requestAnimationFrame(tick);
    return () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
    };
  }, [visible]);

  if (!visible) return null;

  const { fps, frameMs, gpuInfo, webgl, webgpu } = stats;
  const hwAccel = gpuInfo?.hardwareAccelerationEnabled ?? (platform.kind !== "electron" ? true : null);
  const canvasOop = gpuInfo?.features?.["canvas_oop_rasterization"] ?? null;

  return (
    <div
      className="pointer-events-none fixed bottom-8 right-4 z-[9999] min-w-[240px] rounded border border-white/10 bg-black/80 p-2 font-mono text-[10px] text-white/80 shadow-xl backdrop-blur-sm"
    >
      <div className="mb-1 text-[9px] font-semibold uppercase tracking-widest text-white/40">Perf Monitor</div>

      <Row label="FPS" value={`${fps}`} ok={fps >= 55} warn={fps >= 30} />
      <Row label="Frame" value={`${frameMs} ms`} ok={frameMs <= 16.7} warn={frameMs <= 33} />

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
          <Row label="Chrome" value={gpuInfo?.chromeVersion ?? "..."} />
          <Divider />
        </>
      )}

      <Row
        label="WebGL2"
        value={webgl}
        ok={webgl.startsWith("OK")}
        warn={false}
      />
      <Row
        label="WebGPU"
        value={webgpu}
        ok={webgpu.startsWith("OK")}
        warn={false}
      />

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
