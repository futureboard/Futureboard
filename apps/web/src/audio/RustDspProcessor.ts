/**
 * Thin wrapper around the WASM DSP exports (process_speed_mono, process_pitch_mono,
 * process_time_stretch_mono). Lazily initialized — first call triggers the dynamic import.
 *
 * Operates on individual mono channels. Call per-channel and reassemble.
 */

import type { F32 } from "./audioCacheTypes";

type WasmDspModule = {
  process_speed_mono(input: Float32Array, speedRatio: number): Float32Array;
  process_pitch_mono(input: Float32Array, semitones: number): Float32Array;
  process_pitch_mono_quality?(input: Float32Array, semitones: number, grainSize: number): Float32Array;
  process_time_stretch_mono(input: Float32Array, stretchRatio: number): Float32Array;
  process_time_stretch_mono_quality?(input: Float32Array, stretchRatio: number, grainSize: number): Float32Array;
};

type DspQuality = "draft" | "balanced" | "high";

let _module: WasmDspModule | null = null;
let _initPromise: Promise<WasmDspModule | null> | null = null;

async function loadModule(): Promise<WasmDspModule | null> {
  try {
    const mod = await import("../engine/wasm-pkg/futureboard_core.js");
    await mod.default(); // init() — safe to call multiple times
    _module = mod as unknown as WasmDspModule;
    console.debug("[RustDsp] WASM DSP module ready");
    return _module;
  } catch (e) {
    console.warn("[RustDsp] Failed to load WASM DSP module, will use TypeScript fallback:", e);
    return null;
  }
}

export function ensureRustDsp(): Promise<WasmDspModule | null> {
  if (_module) return Promise.resolve(_module);
  if (!_initPromise) _initPromise = loadModule();
  return _initPromise;
}

export function isRustDspReady(): boolean {
  return _module !== null;
}

/** Apply speed resampling to all channels via WASM. Returns null if WASM unavailable. */
export function rustSpeedChannels(channels: F32[], speedRatio: number): Float32Array[] | null {
  if (!_module) return null;
  try {
    return channels.map((ch) => _module!.process_speed_mono(new Float32Array(ch), speedRatio));
  } catch (e) {
    console.warn("[RustDsp] process_speed_mono error:", e);
    return null;
  }
}

/** Apply pitch shift to all channels via WASM. Returns null if WASM unavailable. */
export function rustPitchChannels(
  channels: F32[],
  semitones: number,
  quality: DspQuality = "balanced",
): Float32Array[] | null {
  if (!_module) return null;
  try {
    const grainSize = grainSizeForQuality(quality);
    const result = channels.map((ch) => {
      const input = new Float32Array(ch);
      return _module!.process_pitch_mono_quality
        ? _module!.process_pitch_mono_quality(input, semitones, grainSize)
        : _module!.process_pitch_mono(input, semitones);
    });
    if (import.meta.env.DEV && semitones !== 0 && result.length > 0) {
      const inLen  = channels[0].length;
      const outLen = result[0].length;
      const inRms  = _rms(channels[0]);
      const outRms = _rms(result[0]);
      console.debug(`[RustDsp] pitch ${semitones > 0 ? "+" : ""}${semitones}st — in:${inLen} out:${outLen} rms ${inRms.toFixed(5)}→${outRms.toFixed(5)}`);
    }
    return result;
  } catch (e) {
    console.warn("[RustDsp] process_pitch_mono error:", e);
    return null;
  }
}

/** Root-mean-square of a Float32Array (dev-only diagnostic). */
function _rms(buf: Float32Array): number {
  if (buf.length === 0) return 0;
  let sum = 0;
  for (let i = 0; i < buf.length; i++) sum += buf[i] * buf[i];
  return Math.sqrt(sum / buf.length);
}

/** Apply time-stretch to all channels via WASM. Returns null if WASM unavailable. */
export function rustTimeStretchChannels(
  channels: F32[],
  stretchRatio: number,
  quality: DspQuality = "balanced",
): Float32Array[] | null {
  if (!_module) return null;
  try {
    const grainSize = grainSizeForQuality(quality);
    const result = channels.map((ch) =>
      _module!.process_time_stretch_mono_quality
        ? _module!.process_time_stretch_mono_quality(new Float32Array(ch), stretchRatio, grainSize)
        : _module!.process_time_stretch_mono(new Float32Array(ch), stretchRatio),
    );
    if (import.meta.env.DEV && result.length > 0) {
      console.debug(`[RustDsp] stretch ×${stretchRatio.toFixed(3)} — in:${channels[0].length} out:${result[0].length}`);
    }
    return result;
  } catch (e) {
    console.warn("[RustDsp] process_time_stretch_mono error:", e);
    return null;
  }
}

function grainSizeForQuality(quality: DspQuality): number {
  switch (quality) {
    case "draft":
      return 1024;
    case "high":
      return 4096;
    case "balanced":
    default:
      return 2048;
  }
}
