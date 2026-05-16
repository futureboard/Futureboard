import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  EQUZ8_DB_RANGE,
  EQUZ8_FREQ_MAX,
  EQUZ8_FREQ_MIN,
  bandContributionDb,
  clamp,
  normalizeEquz8Params,
  serializeEquz8Params,
  totalEqGainDb,
  type Equz8Band,
  type Equz8BandType,
  type Equz8Params,
} from "../Core";

// ── Constants ────────────────────────────────────────────────────────────────

const BAND_COLORS = [
  "#38bdf8", // 1 – sky
  "#34d399", // 2 – emerald
  "#fb923c", // 3 – amber
  "#f472b6", // 4 – pink
  "#c084fc", // 5 – violet
  "#22d3ee", // 6 – cyan
  "#facc15", // 7 – gold
  "#f87171", // 8 – coral
] as const;

const TYPE_LABEL: Record<Equz8BandType, string> = {
  highpass: "HP",
  lowshelf: "LS",
  bell:     "BELL",
  highshelf:"HS",
  lowpass:  "LP",
};

const LOG_MIN  = Math.log10(EQUZ8_FREQ_MIN);
const LOG_MAX  = Math.log10(EQUZ8_FREQ_MAX);
const ML = 46, MR = 14, MT = 22, MB = 34;
const SAMPLES  = 640;
const SPEC_N   = 128;
const NODE_R   = 8;
const NODE_R_SEL = 11;
const NODE_HIT = 16;

// ── Types ────────────────────────────────────────────────────────────────────

type PlotRect = { left: number; right: number; top: number; bottom: number; width: number; height: number };
type NodePos  = { x: number; y: number };
type DragState = {
  bandIndex: number;
  startX: number; startY: number;
  startFreq: number; startGain: number; startQ: number;
};
type SpecState = {
  bins: Float32Array;
  peaks: Float32Array;
  timers: Int32Array;
  phase: number;
};

// ── Spectrum ─────────────────────────────────────────────────────────────────

function initSpec(): SpecState {
  const bins   = new Float32Array(SPEC_N);
  const peaks  = new Float32Array(SPEC_N);
  const timers = new Int32Array(SPEC_N);
  for (let i = 0; i < SPEC_N; i++) {
    const t  = i / SPEC_N;
    bins[i]  = Math.max(0, 0.58 - t * 0.43 + Math.random() * 0.12) * 0.68;
    peaks[i] = bins[i] + Math.random() * 0.04;
    timers[i]= Math.floor(Math.random() * 25);
  }
  return { bins, peaks, timers, phase: 0 };
}

function tickSpec(s: SpecState) {
  s.phase += 0.014;
  for (let i = 0; i < SPEC_N; i++) {
    const t      = i / SPEC_N;
    const base   = Math.max(0, 0.60 - t * 0.44);
    const noise  = (Math.random() - 0.5) * 0.07;
    const wave   = Math.sin(s.phase * 0.8 + t * 5.1) * 0.04
                 + Math.sin(s.phase * 0.3 + t * 2.3) * 0.025;
    const target = clamp(base + noise + wave, 0, 1) * 0.62;
    s.bins[i]    = s.bins[i] * 0.80 + target * 0.20;
    if (s.bins[i] > s.peaks[i]) {
      s.peaks[i]  = s.bins[i];
      s.timers[i] = 45;
    } else {
      if (s.timers[i]-- <= 0) s.peaks[i] = Math.max(s.bins[i], s.peaks[i] * 0.975);
    }
  }
}

// ── WebGPU ───────────────────────────────────────────────────────────────────

const WGSL = /* wgsl */`
struct Plot { left:f32, right:f32, top:f32, bottom:f32, w:f32, h:f32 }

@group(0) @binding(0) var<storage,read> bins:  array<f32>;
@group(0) @binding(1) var<storage,read> peaks: array<f32>;
@group(0) @binding(2) var<uniform>      plot:  Plot;

struct VOut {
  @builtin(position) pos: vec4f,
  @location(0) freq: f32,
  @location(1) amp:  f32,
  @location(2) kind: f32,
}

@vertex fn vs(@builtin(vertex_index) vi:u32, @builtin(instance_index) ii:u32) -> VOut {
  let n    = f32(arrayLength(&bins));
  let amp  = bins[ii];
  let peak = peaks[ii];
  let pct  = f32(ii) / n;
  let barW = (plot.right - plot.left) / n;
  let x0   = plot.left + f32(ii) * barW;
  let x1   = x0 + barW - 0.5;
  let barH = amp  * (plot.bottom - plot.top) * 0.93;
  let pkH  = peak * (plot.bottom - plot.top) * 0.93;

  var p: vec2f; var kind = 0.0;
  if (vi < 6u) {
    let y0 = plot.bottom - barH; let y1 = plot.bottom;
    let q  = array<vec2f,6>(vec2f(x0,y0),vec2f(x1,y0),vec2f(x0,y1),
                             vec2f(x1,y0),vec2f(x1,y1),vec2f(x0,y1));
    p = q[vi];
  } else {
    let py = plot.bottom - pkH;
    let q  = array<vec2f,6>(vec2f(x0,py-0.5),vec2f(x1,py-0.5),vec2f(x0,py+1.0),
                             vec2f(x1,py-0.5),vec2f(x1,py+1.0),vec2f(x0,py+1.0));
    p = q[vi-6u]; kind = 1.0;
  }
  let nx = (p.x / plot.w) * 2.0 - 1.0;
  let ny = 1.0 - (p.y / plot.h) * 2.0;
  return VOut(vec4f(nx,ny,0.0,1.0), pct, amp, kind);
}

@fragment fn fs(v:VOut) -> @location(0) vec4f {
  let t = v.freq;
  let r = mix(0.04f, 0.40f, t);
  let g = mix(0.72f, 0.12f, t);
  let b = mix(0.95f, 0.82f, t);
  if (v.kind > 0.5) { return vec4f(r+0.25,g+0.25,b+0.2, 0.55); }
  let a = (mix(0.14f, 0.05f, t)) * (0.35 + v.amp * 0.65);
  return vec4f(r, g, b, a);
}
`;

// WebGPU flag constants (avoids dependency on dom.webgpu lib types)
const GPU_STORAGE_COPY_DST = 0x0080 | 0x0008; // STORAGE | COPY_DST
const GPU_UNIFORM_COPY_DST = 0x0040 | 0x0008;  // UNIFORM | COPY_DST
const GPU_VERTEX_FRAGMENT  = 0x1 | 0x2;         // VERTEX | FRAGMENT
const GPU_VERTEX_ONLY      = 0x1;               // VERTEX

// Use unknown for GPU objects to avoid needing dom.webgpu lib
type GpuCtx = {
  device:      unknown;
  ctx:         unknown;
  pipeline:    unknown;
  binsBuffer:  unknown;
  peaksBuffer: unknown;
  plotBuffer:  unknown;
  bindGroup:   unknown;
};

async function tryInitGpu(canvas: HTMLCanvasElement): Promise<GpuCtx | null> {
  try {
    const gpu = (navigator as unknown as { gpu?: unknown }).gpu as {
      requestAdapter: (o: unknown) => Promise<{
        requestDevice: () => Promise<{
          createShaderModule: (o: unknown) => unknown;
          createBuffer: (o: unknown) => unknown;
          createBindGroupLayout: (o: unknown) => unknown;
          createBindGroup: (o: unknown) => unknown;
          createRenderPipeline: (o: unknown) => unknown;
          createPipelineLayout: (o: unknown) => unknown;
          queue: { writeBuffer: (b: unknown, o: number, d: unknown) => void; submit: (c: unknown[]) => void };
          createCommandEncoder: () => {
            beginRenderPass: (d: unknown) => {
              setPipeline: (p: unknown) => void;
              setBindGroup: (i: number, g: unknown) => void;
              draw: (v: number, i: number) => void;
              end: () => void;
            };
            finish: () => unknown;
          };
        }>;
      } | null>;
      getPreferredCanvasFormat: () => string;
    } | undefined;

    if (!gpu) return null;
    const adapter = await gpu.requestAdapter({ powerPreference: "low-power" });
    if (!adapter) return null;
    const device  = await adapter.requestDevice();
    const ctx     = canvas.getContext("webgpu") as {
      configure: (o: unknown) => void;
      getCurrentTexture: () => { createView: () => unknown };
    } | null;
    if (!ctx) return null;

    const format = gpu.getPreferredCanvasFormat();
    ctx.configure({ device, format, alphaMode: "opaque" });

    const mod = device.createShaderModule({ code: WGSL });

    const binsBuffer  = device.createBuffer({ size: SPEC_N * 4, usage: GPU_STORAGE_COPY_DST });
    const peaksBuffer = device.createBuffer({ size: SPEC_N * 4, usage: GPU_STORAGE_COPY_DST });
    const plotBuffer  = device.createBuffer({ size: 6 * 4,      usage: GPU_UNIFORM_COPY_DST });

    const bgl = device.createBindGroupLayout({
      entries: [
        { binding: 0, visibility: GPU_VERTEX_FRAGMENT, buffer: { type: "read-only-storage" } },
        { binding: 1, visibility: GPU_VERTEX_FRAGMENT, buffer: { type: "read-only-storage" } },
        { binding: 2, visibility: GPU_VERTEX_ONLY,     buffer: { type: "uniform" } },
      ],
    });

    const bindGroup = device.createBindGroup({
      layout: bgl,
      entries: [
        { binding: 0, resource: { buffer: binsBuffer } },
        { binding: 1, resource: { buffer: peaksBuffer } },
        { binding: 2, resource: { buffer: plotBuffer } },
      ],
    });

    const pipeline = device.createRenderPipeline({
      layout: device.createPipelineLayout({ bindGroupLayouts: [bgl] }),
      vertex:   { module: mod, entryPoint: "vs" },
      fragment: {
        module: mod, entryPoint: "fs",
        targets: [{
          format,
          blend: {
            color: { srcFactor: "src-alpha", dstFactor: "one-minus-src-alpha", operation: "add" },
            alpha: { srcFactor: "one",       dstFactor: "one-minus-src-alpha", operation: "add" },
          },
        }],
      },
      primitive: { topology: "triangle-list" },
    });

    return { device, ctx, pipeline, binsBuffer, peaksBuffer, plotBuffer, bindGroup };
  } catch { return null; }
}

function renderGpu(g: GpuCtx, spec: SpecState, plot: PlotRect, w: number, h: number) {
  const dev = g.device as {
    queue: { writeBuffer: (b: unknown, o: number, d: unknown) => void; submit: (c: unknown[]) => void };
    createCommandEncoder: () => {
      beginRenderPass: (d: unknown) => {
        setPipeline: (p: unknown) => void; setBindGroup: (i: number, bg: unknown) => void;
        draw: (v: number, i: number) => void; end: () => void;
      };
      finish: () => unknown;
    };
  };
  const ctx = g.ctx as { getCurrentTexture: () => { createView: () => unknown } };

  dev.queue.writeBuffer(g.binsBuffer,  0, spec.bins);
  dev.queue.writeBuffer(g.peaksBuffer, 0, spec.peaks);
  dev.queue.writeBuffer(g.plotBuffer,  0, new Float32Array([plot.left, plot.right, plot.top, plot.bottom, w, h]));

  const enc  = dev.createCommandEncoder();
  const pass = enc.beginRenderPass({
    colorAttachments: [{
      view: ctx.getCurrentTexture().createView(),
      clearValue: { r: 0.047, g: 0.059, b: 0.082, a: 1 },
      loadOp: "clear", storeOp: "store",
    }],
  });
  pass.setPipeline(g.pipeline);
  pass.setBindGroup(0, g.bindGroup);
  pass.draw(12, SPEC_N);
  pass.end();
  dev.queue.submit([enc.finish()]);
}

// ── Canvas2D draw ─────────────────────────────────────────────────────────────

function syncSize(canvas: HTMLCanvasElement): { w: number; h: number } {
  const rect = canvas.getBoundingClientRect();
  const dpr  = devicePixelRatio || 1;
  const w    = Math.max(1, rect.width);
  const h    = Math.max(1, rect.height);
  const cw   = Math.round(w * dpr);
  const ch   = Math.round(h * dpr);
  if (canvas.width !== cw || canvas.height !== ch) {
    canvas.width  = cw;
    canvas.height = ch;
  }
  return { w, h };
}

function draw2d(
  canvas: HTMLCanvasElement,
  model: Equz8Params,
  enabled: boolean,
  spec: SpecState,
  gpuActive: boolean,
): NodePos[] {
  const { w, h } = syncSize(canvas);
  const dpr   = devicePixelRatio || 1;
  const ctx   = canvas.getContext("2d");
  if (!ctx) return [];

  const plot  = plotRect(w, h);

  ctx.save();
  ctx.scale(dpr, dpr);
  ctx.clearRect(0, 0, w, h);

  if (!gpuActive) {
    ctx.fillStyle = "#0c0f15";
    ctx.fillRect(0, 0, w, h);
    drawSpecCanvas(ctx, spec, plot);
  }

  drawGrid(ctx, plot);

  if (enabled && model.power) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(plot.left, plot.top, plot.width, plot.height);
    ctx.clip();

    // Per-band fills
    model.bands.forEach((band, i) => {
      if (!band.active) return;
      drawBandFill(ctx, band, i, plot);
    });

    // Combined EQ curve
    drawCurve(ctx, model.bands, plot);
    ctx.restore();
  } else {
    drawBypassLine(ctx, plot);
  }

  const positions = drawNodes(ctx, model, enabled, plot);
  ctx.restore();
  return positions;
}

function drawSpecCanvas(ctx: CanvasRenderingContext2D, spec: SpecState, plot: PlotRect) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(plot.left, plot.top, plot.width, plot.height);
  ctx.clip();

  const barW = plot.width / SPEC_N;
  for (let i = 0; i < SPEC_N; i++) {
    const t     = i / SPEC_N;
    const amp   = spec.bins[i]!;
    const peak  = spec.peaks[i]!;
    const x     = plot.left + i * barW;
    const barH  = amp  * plot.height * 0.9;
    const pkY   = plot.bottom - peak * plot.height * 0.9;

    // Bar gradient
    const r = Math.round(lerp(10, 100, t));
    const g = Math.round(lerp(185, 30, t));
    const b = Math.round(lerp(240, 210, t));
    const alpha = lerp(0.13, 0.05, t) * (0.4 + amp * 0.6);

    ctx.fillStyle = `rgba(${r},${g},${b},${alpha.toFixed(3)})`;
    ctx.fillRect(x, plot.bottom - barH, barW - 0.5, barH);

    // Peak dot
    ctx.fillStyle = `rgba(${r + 60},${g + 60},${b + 40},0.55)`;
    ctx.fillRect(x, pkY - 0.5, barW - 0.5, 1.5);
  }
  ctx.restore();
}

function drawGrid(ctx: CanvasRenderingContext2D, plot: PlotRect) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(plot.left, plot.top, plot.width, plot.height);
  ctx.clip();

  // Subtle octave band shading
  const octaves = [20, 40, 80, 160, 315, 630, 1250, 2500, 5000, 10000, 20000];
  for (let i = 0; i < octaves.length - 1; i += 2) {
    const x0 = freqToX(octaves[i]!,     plot);
    const x1 = freqToX(octaves[i + 1]!, plot);
    ctx.fillStyle = "rgba(255,255,255,0.012)";
    ctx.fillRect(x0, plot.top, x1 - x0, plot.height);
  }

  // dB lines
  for (const db of [-15, -12, -9, -6, -3, 3, 6, 9, 12, 15]) {
    const y = gainToY(db, plot);
    ctx.strokeStyle = Math.abs(db) === 6 || Math.abs(db) === 12
      ? "rgba(255,255,255,0.055)"
      : "rgba(255,255,255,0.028)";
    ctx.lineWidth = 0.75;
    ctx.setLineDash([]);
    ctx.beginPath();
    ctx.moveTo(plot.left, y);
    ctx.lineTo(plot.right, y);
    ctx.stroke();
  }

  // 0 dB
  ctx.strokeStyle = "rgba(255,255,255,0.13)";
  ctx.lineWidth   = 0.75;
  ctx.setLineDash([5, 4]);
  ctx.beginPath();
  ctx.moveTo(plot.left, gainToY(0, plot));
  ctx.lineTo(plot.right, gainToY(0, plot));
  ctx.stroke();
  ctx.setLineDash([]);

  // Freq lines
  const freqLines = [20,30,40,50,60,70,80,90,100,200,300,400,500,600,700,800,900,
    1000,2000,3000,4000,5000,6000,7000,8000,9000,10000,20000];
  for (const f of freqLines) {
    const x       = freqToX(f, plot);
    const decade  = f === 100 || f === 1000 || f === 10000;
    const half    = f === 20 || f === 50 || f === 200 || f === 500 || f === 2000 || f === 5000 || f === 20000;
    ctx.strokeStyle = decade ? "rgba(255,255,255,0.08)"
                     : half  ? "rgba(255,255,255,0.04)"
                              : "rgba(255,255,255,0.018)";
    ctx.lineWidth = decade ? 0.8 : 0.5;
    ctx.beginPath();
    ctx.moveTo(x, plot.top);
    ctx.lineTo(x, plot.bottom);
    ctx.stroke();
  }

  ctx.restore();

  // dB labels
  ctx.save();
  ctx.font = "400 9px Inter,system-ui,sans-serif";
  ctx.fillStyle = "rgba(150,170,205,0.38)";
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  for (const db of [-12, -6, 0, 6, 12]) {
    const y = gainToY(db, plot);
    ctx.fillText(db === 0 ? "0" : `${db > 0 ? "+" : ""}${db}`, plot.left - 7, y);
  }

  // Freq labels
  const freqLabels: [number, string][] = [
    [20,"20"], [50,"50"], [100,"100"], [200,"200"], [500,"500"],
    [1000,"1k"], [2000,"2k"], [5000,"5k"], [10000,"10k"], [20000,"20k"],
  ];
  ctx.font = "400 9px Inter,system-ui,sans-serif";
  ctx.fillStyle = "rgba(130,155,195,0.32)";
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (const [f, label] of freqLabels) {
    ctx.fillText(label, freqToX(f, plot), plot.bottom + 7);
  }
  ctx.restore();
}

function drawBandFill(
  ctx: CanvasRenderingContext2D,
  band: Equz8Band,
  bandIndex: number,
  plot: PlotRect,
) {
  const color  = BAND_COLORS[bandIndex]! ;
  const y0line = gainToY(0, plot);

  ctx.beginPath();
  ctx.moveTo(plot.left, y0line);

  for (let i = 0; i <= SAMPLES; i++) {
    const x    = plot.left + (i / SAMPLES) * plot.width;
    const freq = xToFreq(x, plot);
    const db   = band.active ? bandContributionDb(band, freq) : 0;
    ctx.lineTo(x, gainToY(db, plot));
  }

  ctx.lineTo(plot.right, y0line);
  ctx.closePath();

  // Gradient fill based on band color
  const hex = color.replace("#", "");
  const r   = parseInt(hex.slice(0, 2), 16);
  const g   = parseInt(hex.slice(2, 4), 16);
  const b   = parseInt(hex.slice(4, 6), 16);

  const grad = ctx.createLinearGradient(0, plot.top, 0, plot.bottom);
  grad.addColorStop(0,   `rgba(${r},${g},${b},0.22)`);
  grad.addColorStop(0.5, `rgba(${r},${g},${b},0.09)`);
  grad.addColorStop(1,   `rgba(${r},${g},${b},0.03)`);
  ctx.fillStyle = grad;
  ctx.fill();
}

function drawCurve(ctx: CanvasRenderingContext2D, bands: Equz8Band[], plot: PlotRect) {
  const pts: Array<{ x: number; y: number }> = Array.from({ length: SAMPLES + 1 }, (_, i) => {
    const x    = plot.left + (i / SAMPLES) * plot.width;
    const freq = xToFreq(x, plot);
    return { x, y: gainToY(totalEqGainDb(bands, freq), plot) };
  });

  const y0 = gainToY(0, plot);

  // Gradient fill under curve
  const fillGrad = ctx.createLinearGradient(0, plot.top, 0, plot.bottom);
  fillGrad.addColorStop(0,    "rgba(120,210,255,0.18)");
  fillGrad.addColorStop(0.45, "rgba(80,170,220,0.07)");
  fillGrad.addColorStop(1,    "rgba(30,100,180,0.0)");

  ctx.beginPath();
  ctx.moveTo(pts[0]!.x, pts[0]!.y);
  for (let i = 1; i < pts.length; i++) {
    const p = pts[i]!;
    ctx.lineTo(p.x, p.y);
  }
  ctx.lineTo(pts[pts.length - 1]!.x, y0);
  ctx.lineTo(pts[0]!.x, y0);
  ctx.closePath();
  ctx.fillStyle = fillGrad;
  ctx.fill();

  // Main curve line
  ctx.beginPath();
  ctx.moveTo(pts[0]!.x, pts[0]!.y);
  for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i]!.x, pts[i]!.y);

  ctx.strokeStyle = "rgba(180,230,255,0.92)";
  ctx.lineWidth   = 1.6;
  ctx.shadowColor = "rgba(100,200,255,0.7)";
  ctx.shadowBlur  = 8;
  ctx.stroke();
  ctx.shadowBlur  = 0;

  // Second subtle glow pass
  ctx.beginPath();
  ctx.moveTo(pts[0]!.x, pts[0]!.y);
  for (let i = 1; i < pts.length; i++) ctx.lineTo(pts[i]!.x, pts[i]!.y);
  ctx.strokeStyle = "rgba(100,200,255,0.25)";
  ctx.lineWidth   = 5;
  ctx.stroke();
}

function drawBypassLine(ctx: CanvasRenderingContext2D, plot: PlotRect) {
  const y = gainToY(0, plot);
  ctx.beginPath();
  ctx.moveTo(plot.left, y);
  ctx.lineTo(plot.right, y);
  ctx.strokeStyle = "rgba(255,255,255,0.10)";
  ctx.lineWidth   = 1;
  ctx.setLineDash([5, 6]);
  ctx.stroke();
  ctx.setLineDash([]);
}

function drawNodes(
  ctx: CanvasRenderingContext2D,
  model: Equz8Params,
  enabled: boolean,
  plot: PlotRect,
): NodePos[] {
  const positions: NodePos[] = [];

  ctx.save();
  ctx.beginPath();
  ctx.rect(plot.left - 16, plot.top - 16, plot.width + 32, plot.height + 32);
  ctx.clip();

  for (let i = 0; i < model.bands.length; i++) {
    const band  = model.bands[i]!;
    const isSel = i === model.selectedBand;
    if (!band.active && !isSel) {
      positions.push({ x: freqToX(band.freq, plot), y: gainToY(0, plot) });
      continue;
    }

    const x  = freqToX(band.freq, plot);
    const y  = gainToY(enabled && model.power ? totalEqGainDb(model.bands, band.freq) : 0, plot);
    positions.push({ x, y });

    const color  = BAND_COLORS[i]!;
    const hex    = color.replace("#", "");
    const cr     = parseInt(hex.slice(0, 2), 16);
    const cg     = parseInt(hex.slice(2, 4), 16);
    const cb     = parseInt(hex.slice(4, 6), 16);
    const r      = isSel ? NODE_R_SEL : NODE_R;

    // Outer glow ring
    if (isSel || band.active) {
      ctx.beginPath();
      ctx.arc(x, y, r + 5, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(${cr},${cg},${cb},${isSel ? 0.18 : 0.06})`;
      ctx.fill();
    }

    // Node body
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    if (isSel) {
      const ng = ctx.createRadialGradient(x - 2, y - 2, 0, x, y, r);
      ng.addColorStop(0, `rgba(${cr},${cg},${cb},1)`);
      ng.addColorStop(1, `rgba(${Math.round(cr * 0.6)},${Math.round(cg * 0.6)},${Math.round(cb * 0.6)},0.95)`);
      ctx.fillStyle = ng;
    } else if (band.active) {
      ctx.fillStyle = `rgba(${cr},${cg},${cb},0.18)`;
    } else {
      ctx.fillStyle = "rgba(20,25,36,0.9)";
    }
    ctx.fill();

    // Border
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.strokeStyle = isSel
      ? `rgba(${cr},${cg},${cb},1)`
      : band.active
      ? `rgba(${cr},${cg},${cb},0.75)`
      : "rgba(80,95,120,0.45)";
    ctx.lineWidth = isSel ? 1.75 : 1.25;
    if (isSel) {
      ctx.shadowColor = color;
      ctx.shadowBlur  = 10;
    }
    ctx.stroke();
    ctx.shadowBlur = 0;

    // Band number
    ctx.fillStyle = isSel ? "#000" : band.active ? color : "rgba(80,95,120,0.7)";
    ctx.font      = `700 ${isSel ? 9 : 8}px Inter,system-ui,sans-serif`;
    ctx.textAlign    = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(String(band.id), x, y + 0.5);
  }

  ctx.restore();
  return positions;
}

// ── Geometry helpers ─────────────────────────────────────────────────────────

function plotRect(w: number, h: number): PlotRect {
  return { left: ML, right: w - MR, top: MT, bottom: h - MB, width: w - ML - MR, height: h - MT - MB };
}
function freqToX(f: number, p: PlotRect) {
  return p.left + ((Math.log10(f) - LOG_MIN) / (LOG_MAX - LOG_MIN)) * p.width;
}
function xToFreq(x: number, p: PlotRect) {
  return clamp(Math.pow(10, LOG_MIN + ((x - p.left) / p.width) * (LOG_MAX - LOG_MIN)), EQUZ8_FREQ_MIN, EQUZ8_FREQ_MAX);
}
function gainToY(db: number, p: PlotRect) {
  return p.top + p.height * 0.5 - (db / EQUZ8_DB_RANGE) * p.height * 0.5;
}
function yToGain(y: number, p: PlotRect) {
  return clamp(((p.top + p.height * 0.5 - y) / (p.height * 0.5)) * EQUZ8_DB_RANGE, -EQUZ8_DB_RANGE, EQUZ8_DB_RANGE);
}
function normalizeBand(b: Equz8Band): Equz8Band {
  return {
    ...b,
    freq: clamp(b.freq, EQUZ8_FREQ_MIN, EQUZ8_FREQ_MAX),
    gain: b.type.includes("pass") ? 0 : clamp(b.gain, -EQUZ8_DB_RANGE, EQUZ8_DB_RANGE),
    q:    clamp(b.q, 0.1, 12),
  };
}
function formatFreq(f: number): string {
  if (f >= 10000) return `${(f / 1000).toFixed(1)}k`;
  if (f >= 1000)  return `${(f / 1000).toFixed(f % 100 === 0 ? 1 : 2).replace(/\.?0+$/, "")}k`;
  return String(Math.round(f));
}
function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

// ── Main component ────────────────────────────────────────────────────────────

type Props = {
  params: Record<string, number | string | boolean>;
  enabled: boolean;
  onParamsChange: (p: Record<string, number | string | boolean>) => void;
  onToggleEnabled: () => void;
  onReset: () => void;
};

export function Equz8Editor({ params, enabled, onParamsChange, onToggleEnabled, onReset }: Props) {
  const gpuCanvasRef = useRef<HTMLCanvasElement>(null);
  const c2dCanvasRef = useRef<HTMLCanvasElement>(null);
  const gpuRef       = useRef<GpuCtx | null>(null);
  const gpuReadyRef  = useRef(false);
  const specRef      = useRef<SpecState>(initSpec());
  const modelRef     = useRef<Equz8Params>(normalizeEquz8Params(params));
  const enabledRef   = useRef(enabled);
  const dragRef      = useRef<DragState | null>(null);
  const nodePos      = useRef<NodePos[]>([]);
  const [, bump]     = useState(0); // force re-render for inspector position

  const model = useMemo(() => normalizeEquz8Params(params), [params]);

  // Keep refs in sync
  useEffect(() => { modelRef.current  = model;   }, [model]);
  useEffect(() => { enabledRef.current = enabled; }, [enabled]);

  // Init WebGPU async
  useEffect(() => {
    const canvas = gpuCanvasRef.current;
    if (!canvas) return;
    tryInitGpu(canvas).then((ctx) => {
      gpuRef.current     = ctx;
      gpuReadyRef.current = !!ctx;
    });
    return () => {
      (gpuRef.current?.device as { destroy?: () => void } | undefined)?.destroy?.();
      gpuRef.current = null;
      gpuReadyRef.current = false;
    };
  }, []);

  // RAF loop
  useEffect(() => {
    const gpuCanvas = gpuCanvasRef.current;
    const c2dCanvas = c2dCanvasRef.current;
    if (!c2dCanvas) return;

    let id = 0;
    const tick = () => {
      tickSpec(specRef.current);
      const m       = modelRef.current;
      const en      = enabledRef.current;
      const gpu     = gpuRef.current;
      const gpuOn   = gpuReadyRef.current && !!gpu;

      // Sync GPU canvas size
      if (gpuCanvas && gpuOn) {
        const { w, h } = syncSize(gpuCanvas);
        const plot      = plotRect(w, h);
        renderGpu(gpu!, specRef.current, plot, w, h);
      }

      const positions = draw2d(c2dCanvas, m, en, specRef.current, gpuOn);
      nodePos.current = positions;
      id = requestAnimationFrame(tick);
    };
    id = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(id);
  }, []);

  const updateBand = useCallback((i: number, patch: Partial<Equz8Band>) => {
    const m     = modelRef.current;
    const bands = m.bands.map((b, j) => j === i ? normalizeBand({ ...b, ...patch }) : b);
    onParamsChange(serializeEquz8Params({ ...m, bands }));
  }, [onParamsChange]);

  const selectBand = useCallback((i: number) => {
    onParamsChange({ selectedBand: i });
    bump(n => n + 1);
  }, [onParamsChange]);

  const hitBand = useCallback((x: number, y: number, w: number, h: number): number => {
    const plot = plotRect(w, h);
    const m    = modelRef.current;
    for (let pass = 0; pass < 2; pass++) {
      for (let i = 0; i < m.bands.length; i++) {
        if (pass === 0 && i !== m.selectedBand) continue;
        if (pass === 1 && i === m.selectedBand) continue;
        const band = m.bands[i]!;
        if (!band.active && i !== m.selectedBand) continue;
        const en = enabledRef.current;
        const nx = freqToX(band.freq, plot);
        const ny = gainToY(en && m.power ? totalEqGainDb(m.bands, band.freq) : 0, plot);
        if (Math.hypot(x - nx, y - ny) <= NODE_HIT) return i;
      }
    }
    return -1;
  }, []);

  const onPointerDown = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
    const canvas = c2dCanvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const m = modelRef.current;
    const plot = plotRect(rect.width, rect.height);
    const hit  = hitBand(x, y, rect.width, rect.height);
    const bi   = hit >= 0 ? hit : m.selectedBand;
    const band = m.bands[bi]!;
    selectBand(bi);

    if (hit < 0 && x >= plot.left && x <= plot.right && y >= plot.top && y <= plot.bottom) {
      updateBand(bi, {
        freq: xToFreq(x, plot),
        gain: band.type.includes("pass") ? band.gain : yToGain(y, plot),
      });
    }

    dragRef.current = { bandIndex: bi, startX: e.clientX, startY: e.clientY, startFreq: band.freq, startGain: band.gain, startQ: band.q };
    e.currentTarget.setPointerCapture(e.pointerId);
  }, [hitBand, selectBand, updateBand]);

  const onPointerMove = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
    const drag   = dragRef.current;
    const canvas = c2dCanvasRef.current;
    if (!drag || !canvas) return;
    const rect = canvas.getBoundingClientRect();
    const plot = plotRect(rect.width, rect.height);
    const dx   = e.clientX - drag.startX;
    const dy   = e.clientY - drag.startY;
    const band = modelRef.current.bands[drag.bandIndex]!;
    const logShift = (dx / Math.max(1, plot.width)) * (LOG_MAX - LOG_MIN);
    const freq = clamp(Math.pow(10, Math.log10(drag.startFreq) + logShift), EQUZ8_FREQ_MIN, EQUZ8_FREQ_MAX);
    updateBand(drag.bandIndex, band.type.includes("pass")
      ? { freq, q: drag.startQ - dy * 0.035 }
      : { freq, gain: drag.startGain - dy * ((EQUZ8_DB_RANGE * 2) / Math.max(1, plot.height)) });
  }, [updateBand]);

  const onPointerUp = useCallback((e: React.PointerEvent<HTMLCanvasElement>) => {
    dragRef.current = null;
    e.currentTarget.releasePointerCapture(e.pointerId);
  }, []);

  const sel    = model.bands[model.selectedBand]!;
  const selPos = nodePos.current[model.selectedBand];

  return (
    <div
      className="flex h-full max-h-[520px] min-h-[380px] w-[1100px] max-w-[1400px] flex-col overflow-hidden rounded-[7px] text-[11px]"
      style={{
        background: "#0c0f15",
        border: "1px solid rgba(255,255,255,0.09)",
        boxShadow: "0 8px 40px rgba(0,0,0,0.75), 0 1px 0 rgba(255,255,255,0.04) inset",
      }}
    >
      {/* ── Header ── */}
      <div
        className="flex h-[34px] shrink-0 items-center justify-between px-3"
        style={{
          background: "linear-gradient(180deg, #191d28 0%, #141820 100%)",
          borderBottom: "1px solid rgba(255,255,255,0.07)",
        }}
      >
        <div className="flex items-center gap-2.5">
          <button
            type="button"
            onClick={onToggleEnabled}
            className="h-[13px] w-[13px] shrink-0 rounded-full transition-all"
            style={enabled
              ? { background: "#38bdf8", boxShadow: "0 0 10px rgba(56,189,248,0.85)", border: "1px solid rgba(56,189,248,0.5)" }
              : { background: "#1a1f2c", border: "1px solid rgba(255,255,255,0.12)" }}
          />
          <span className="font-semibold tracking-[0.09em]" style={{ fontSize: "12px", color: "#c8d8ec" }}>EQUZ8</span>
          <span className="tracking-[0.2em] uppercase" style={{ fontSize: "8px", color: "rgba(140,165,200,0.38)" }}>
            8-Band Parametric EQ
          </span>
        </div>

        <div className="flex items-center gap-2">
          {/* Output gain readout */}
          <span
            className="tabular-nums rounded px-2 py-[3px]"
            style={{ fontSize: "10px", color: "#4a6080", background: "#0a0d13", border: "1px solid rgba(255,255,255,0.05)" }}
          >
            {enabled && model.power ? "+0.0 dB" : "BYPASSED"}
          </span>
          <button
            type="button"
            onClick={onReset}
            className="rounded px-2 py-[3px] transition-colors"
            style={{ fontSize: "10px", color: "#5a7090", background: "#131720", border: "1px solid rgba(255,255,255,0.07)" }}
            onMouseEnter={(e) => { (e.currentTarget as HTMLButtonElement).style.color = "#90aac8"; }}
            onMouseLeave={(e) => { (e.currentTarget as HTMLButtonElement).style.color = "#5a7090"; }}
          >
            Reset
          </button>
        </div>
      </div>

      {/* ── Body ── */}
      <div className="flex min-h-0 flex-1 flex-col">
        {/* Canvas area */}
        <div className="relative min-h-0 flex-1">
          {/* WebGPU spectrum canvas (bottom layer) */}
          <canvas
            ref={gpuCanvasRef}
            className="absolute inset-0 h-full w-full"
            style={{ zIndex: 0 }}
          />
          {/* Canvas2D overlay (EQ curve, nodes) */}
          <canvas
            ref={c2dCanvasRef}
            className="absolute inset-0 h-full w-full touch-none cursor-crosshair"
            style={{ zIndex: 1 }}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            onDoubleClick={() => updateBand(model.selectedBand, { active: !sel.active })}
          />

          {/* Bypass overlay */}
          {(!enabled || !model.power) && (
            <div className="pointer-events-none absolute inset-0" style={{ background: "rgba(8,10,18,0.55)", zIndex: 2 }} />
          )}

          {/* Floating inspector */}
          {selPos && (
            <FloatingInspector
              band={sel}
              bandIndex={model.selectedBand}
              nodePos={selPos}
              containerWidth={1100}
              containerHeight={520}
              onUpdate={(patch) => updateBand(model.selectedBand, patch)}
              onToggleActive={() => updateBand(model.selectedBand, { active: !sel.active })}
            />
          )}
        </div>

        {/* ── Band strip ── */}
        <BandStrip
          model={model}
          onSelect={selectBand}
          onToggleActive={(i) => updateBand(i, { active: !model.bands[i]!.active })}
        />
      </div>
    </div>
  );
}

// ── FloatingInspector ─────────────────────────────────────────────────────────

function FloatingInspector({
  band,
  bandIndex,
  nodePos,
  containerWidth,
  containerHeight,
  onUpdate,
  onToggleActive,
}: {
  band: Equz8Band;
  bandIndex: number;
  nodePos: NodePos;
  containerWidth: number;
  containerHeight: number;
  onUpdate: (patch: Partial<Equz8Band>) => void;
  onToggleActive: () => void;
}) {
  const PANEL_W = 190;
  const PANEL_H = 210;
  const PAD     = 14;

  // Position near node, nudge to stay in bounds
  let left = nodePos.x + 14;
  let top  = nodePos.y - PANEL_H / 2;
  if (left + PANEL_W > containerWidth  - PAD) left = nodePos.x - PANEL_W - 14;
  if (top  < PAD)                              top  = PAD;
  if (top  + PANEL_H > containerHeight - PAD)  top  = containerHeight - PANEL_H - PAD;

  const color   = BAND_COLORS[bandIndex]!;
  const isPass  = band.type.includes("pass");

  return (
    <div
      className="pointer-events-auto absolute flex flex-col gap-2 rounded-[6px] p-3"
      style={{
        left, top,
        width: PANEL_W,
        zIndex: 10,
        background: "rgba(10,13,20,0.92)",
        border: `1px solid rgba(${hexToRgb(color)},0.35)`,
        boxShadow: `0 4px 24px rgba(0,0,0,0.7), 0 0 0 1px rgba(${hexToRgb(color)},0.1)`,
        backdropFilter: "blur(12px)",
      }}
    >
      {/* Header */}
      <div className="flex items-center justify-between">
        <span className="font-bold tabular-nums" style={{ fontSize: "14px", color }}>Band {band.id}</span>
        <button
          type="button"
          onClick={onToggleActive}
          className="rounded px-2 py-[2px] text-[9px] uppercase tracking-wider transition-all"
          style={band.active
            ? { background: `rgba(${hexToRgb(color)},0.18)`, color, border: `1px solid rgba(${hexToRgb(color)},0.4)` }
            : { background: "#0f1219", color: "#3a4555", border: "1px solid rgba(255,255,255,0.07)" }}
        >
          {band.active ? "ON" : "OFF"}
        </button>
      </div>

      {/* Type selector */}
      <div className="flex gap-1">
        {(["highpass", "lowshelf", "bell", "highshelf", "lowpass"] as Equz8BandType[]).map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => onUpdate({ type: t, gain: t.includes("pass") ? 0 : band.gain })}
            className="flex-1 rounded py-[3px] text-[8px] uppercase transition-all"
            style={band.type === t
              ? { background: `rgba(${hexToRgb(color)},0.22)`, color, border: `1px solid rgba(${hexToRgb(color)},0.5)`, fontWeight: 600 }
              : { background: "#0c1018", color: "#3a4555", border: "1px solid rgba(255,255,255,0.05)" }}
          >
            {TYPE_LABEL[t]}
          </button>
        ))}
      </div>

      {/* Freq */}
      <InspectorRow
        label="FREQ"
        value={formatFreq(band.freq)}
        unit="Hz"
        color={color}
        onDrag={(d) => onUpdate({ freq: clamp(band.freq * Math.pow(1.006, d), EQUZ8_FREQ_MIN, EQUZ8_FREQ_MAX) })}
      />

      {/* Gain */}
      <InspectorRow
        label="GAIN"
        value={`${band.gain >= 0 ? "+" : ""}${band.gain.toFixed(1)}`}
        unit="dB"
        color={color}
        disabled={isPass}
        onDrag={(d) => onUpdate({ gain: clamp(band.gain + d * 0.1, -EQUZ8_DB_RANGE, EQUZ8_DB_RANGE) })}
      />

      {/* Q */}
      <InspectorRow
        label="Q"
        value={band.q.toFixed(2)}
        color={color}
        onDrag={(d) => onUpdate({ q: clamp(band.q + d * 0.03, 0.1, 12) })}
      />

      {/* Q slider */}
      <input
        type="range" min={0.1} max={12} step={0.01} value={band.q}
        onChange={(e) => onUpdate({ q: parseFloat(e.target.value) })}
        className="w-full"
        style={{ accentColor: color, height: "3px", cursor: "ew-resize" }}
      />
    </div>
  );
}

function InspectorRow({
  label, value, unit, color, disabled, onDrag,
}: {
  label: string; value: string; unit?: string; color: string; disabled?: boolean; onDrag: (d: number) => void;
}) {
  const ref = useRef<{ y: number } | null>(null);
  return (
    <div
      className={`flex flex-col gap-[3px] ${disabled ? "pointer-events-none opacity-25" : "cursor-ns-resize"}`}
      onPointerDown={(e) => { ref.current = { y: e.clientY }; e.currentTarget.setPointerCapture(e.pointerId); }}
      onPointerMove={(e) => { const s = ref.current; if (!s) return; onDrag(s.y - e.clientY); ref.current = { y: e.clientY }; }}
      onPointerUp={(e)   => { ref.current = null; e.currentTarget.releasePointerCapture(e.pointerId); }}
    >
      <span className="uppercase tracking-[0.12em]" style={{ fontSize: "8px", color: "rgba(100,130,165,0.6)" }}>{label}</span>
      <div
        className="flex items-center justify-between rounded px-2"
        style={{ height: "26px", background: "#080b11", border: `1px solid rgba(${hexToRgb(color)},0.22)` }}
      >
        <span className="tabular-nums" style={{ fontSize: "12px", color: "#c0d0e0", fontWeight: 500 }}>{value}</span>
        {unit && <span style={{ fontSize: "9px", color: "rgba(90,115,145,0.65)" }}>{unit}</span>}
      </div>
    </div>
  );
}

function hexToRgb(hex: string): string {
  const h = hex.replace("#", "");
  return `${parseInt(h.slice(0,2),16)},${parseInt(h.slice(2,4),16)},${parseInt(h.slice(4,6),16)}`;
}

// ── BandStrip ─────────────────────────────────────────────────────────────────

function BandStrip({
  model,
  onSelect,
  onToggleActive,
}: {
  model: Equz8Params;
  onSelect: (i: number) => void;
  onToggleActive: (i: number) => void;
}) {
  return (
    <div
      className="flex h-[52px] shrink-0 items-center gap-[4px] px-2"
      style={{ background: "#0a0d14", borderTop: "1px solid rgba(255,255,255,0.07)" }}
    >
      {model.bands.map((band, i) => {
        const isSel  = i === model.selectedBand;
        const color  = BAND_COLORS[i]!;
        const rgb    = hexToRgb(color);
        return (
          <button
            key={band.id}
            type="button"
            onClick={() => onSelect(i)}
            onDoubleClick={(e) => { e.stopPropagation(); onToggleActive(i); }}
            className="relative flex h-[38px] flex-1 flex-col items-center justify-center gap-[3px] rounded-[5px] transition-all"
            style={isSel
              ? { background: `rgba(${rgb},0.16)`, border: `1px solid rgba(${rgb},0.55)`, boxShadow: `0 0 10px rgba(${rgb},0.12) inset` }
              : band.active
              ? { background: "#0f1219", border: "1px solid rgba(255,255,255,0.07)" }
              : { background: "#0a0c12", border: "1px solid rgba(255,255,255,0.04)", opacity: 0.45 }}
          >
            {/* Active dot */}
            <span
              className="absolute right-[5px] top-[5px] h-[4px] w-[4px] rounded-full"
              style={{ background: band.active ? color : "rgba(255,255,255,0.07)" }}
            />
            <span
              className="tabular-nums font-bold leading-none"
              style={{ fontSize: "11px", color: isSel ? color : band.active ? "rgba(180,200,220,0.7)" : "#2a3545" }}
            >
              {band.id}
            </span>
            <span
              className="leading-none tracking-wide"
              style={{ fontSize: "7.5px", color: isSel ? color : "rgba(80,105,135,0.7)" }}
            >
              {TYPE_LABEL[band.type]}
            </span>
            <span
              className="tabular-nums leading-none"
              style={{ fontSize: "7px", color: isSel ? `rgba(${rgb},0.75)` : "rgba(60,80,110,0.8)" }}
            >
              {formatFreq(band.freq)}
            </span>
          </button>
        );
      })}
    </div>
  );
}
