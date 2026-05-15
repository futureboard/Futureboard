import type { DecodedAudioData, AudioProcessParams, F32 } from "./audioCacheTypes";
import { audioCacheManager } from "./AudioCacheManager";
import { buildProcessedCacheKey, isIdentityTransform } from "./audioCacheKeys";
import { resampleChannels } from "./dsp/resample";
import { timeStretchGranular } from "./dsp/timeStretch";
import { timeStretchWSOLA } from "./dsp/wsola";
import { pitchShiftDraft } from "./dsp/pitchShift";
import {
  ensureRustDsp,
  isRustDspReady,
  rustPitchChannels,
  rustTimeStretchChannels,
} from "./RustDspProcessor";

export type ProcessorKind = "rust-wasm" | "ts-wsola" | "ts-granular" | "ts-resample";

// ── AudioProcessingService ────────────────────────────────────────────────────

class AudioProcessingService {
  constructor() {
    ensureRustDsp().then((mod) => {
      if (mod) {
        // WASM just loaded — any TS-processed results in cache were produced without Rust.
        // Clear them so next playback reprocesses with the WASM path.
        audioCacheManager.clearAllProcessed();
        console.debug("[AudioProcessing] WASM ready — processed cache cleared for Rust reprocessing");
      }
    }).catch(() => {});
  }

  chooseBestProcessor(): ProcessorKind {
    return isRustDspReady() ? "rust-wasm" : "ts-wsola";
  }

  getProcessingCapabilities() {
    return { typescript: true, rustWasm: isRustDspReady() };
  }

  /**
   * Process decoded audio with speed/pitch/mode params.
   * Checks the cache first; processes and caches on miss.
   * Returns the source unchanged for identity transforms (no processing needed).
   */
  async processClipAudio(
    decoded: DecodedAudioData,
    params: AudioProcessParams,
  ): Promise<{ result: DecodedAudioData; processorUsed: ProcessorKind }> {
    if (isIdentityTransform(params)) {
      return { result: decoded, processorUsed: "ts-resample" };
    }

    const key = buildProcessedCacheKey(decoded.fileId, decoded.sampleRate, params);
    const cached = audioCacheManager.getProcessedAudio(key);
    if (cached) {
      console.debug("[AudioProcessing] cache hit:", key);
      return { result: cached, processorUsed: this._processorForMode(params) };
    }

    await ensureRustDsp();

    console.debug(`[AudioProcessing] processing mode=${params.mode}:`, params);

    let result: DecodedAudioData;
    let processorUsed: ProcessorKind;

    if (params.mode === "resample" || !params.preservePitch) {
      ({ result, processorUsed } = await this._processResample(decoded, params));
    } else if (isRustDspReady()) {
      ({ result, processorUsed } = await this._processRust(decoded, params));
    } else {
      ({ result, processorUsed } = await this._processTypeScript(decoded, params));
    }

    audioCacheManager.setProcessedAudio(key, result);
    console.debug(`[AudioProcessing] cached (${processorUsed}) key:`, key);
    return { result, processorUsed };
  }

  /** Return cached processed audio or null without triggering processing. */
  getCachedProcessed(
    decoded: DecodedAudioData,
    params: AudioProcessParams,
  ): DecodedAudioData | null {
    if (isIdentityTransform(params)) return decoded;
    const key = buildProcessedCacheKey(decoded.fileId, decoded.sampleRate, params);
    return audioCacheManager.getProcessedAudio(key) ?? null;
  }

  /** Remove all processed variants for a file so next request reprocesses. */
  invalidateProcessedAudio(fileId: string): void {
    audioCacheManager.clearFileCache(fileId);
  }

  // ── Resample path (mode=resample or preservePitch=false) ─────────────────

  private async _processResample(
    decoded: DecodedAudioData,
    params: AudioProcessParams,
  ): Promise<{ result: DecodedAudioData; processorUsed: ProcessorKind }> {
    let channels: F32[] = decoded.channelData.map((ch) => new Float32Array(ch));

    if (params.speedRatio !== 1) {
      channels = resampleChannels(channels, params.speedRatio);
    }
    if (params.pitchSemitones !== 0) {
      const pitchRatio = Math.pow(2, params.pitchSemitones / 12);
      channels = resampleChannels(channels, pitchRatio);
    }

    await tick();
    return {
      result:        makeResult(decoded, channels),
      processorUsed: "ts-resample",
    };
  }

  // ── Rust WASM path ────────────────────────────────────────────────────────

  private async _processRust(
    decoded: DecodedAudioData,
    params: AudioProcessParams,
  ): Promise<{ result: DecodedAudioData; processorUsed: ProcessorKind }> {
    const { speedRatio, pitchSemitones, quality, mode } = params;
    let channels: F32[] = decoded.channelData.map((ch) => new Float32Array(ch));

    if (speedRatio !== 1) {
      const stretchRatio = 1 / speedRatio;
      const result = rustTimeStretchChannels(channels, stretchRatio);
      if (result) {
        channels = result;
      } else {
        // Rust failed for stretch — fall back to TS per mode
        channels = this._tsStretch(channels, stretchRatio, quality, mode);
      }
    }

    if (pitchSemitones !== 0) {
      const result = rustPitchChannels(channels, pitchSemitones);
      if (result) {
        channels = result;
      } else {
        channels = pitchShiftDraft(channels, pitchSemitones, quality);
      }
    }

    await tick();
    return { result: makeResult(decoded, channels), processorUsed: "rust-wasm" };
  }

  // ── TypeScript path ───────────────────────────────────────────────────────

  private async _processTypeScript(
    decoded: DecodedAudioData,
    params: AudioProcessParams,
  ): Promise<{ result: DecodedAudioData; processorUsed: ProcessorKind }> {
    const { speedRatio, pitchSemitones, quality, mode } = params;
    let channels: F32[] = decoded.channelData.map((ch) => new Float32Array(ch));

    if (speedRatio !== 1) {
      const stretchRatio = 1 / speedRatio;
      channels = this._tsStretch(channels, stretchRatio, quality, mode);
    }

    if (pitchSemitones !== 0) {
      channels = this._tsPitch(channels, pitchSemitones, quality, mode);
    }

    await tick();
    const processorUsed = mode === "granular" || mode === "percussive"
      ? "ts-granular"
      : "ts-wsola";
    return { result: makeResult(decoded, channels), processorUsed };
  }

  // ── DSP helpers ───────────────────────────────────────────────────────────

  /**
   * Time-stretch channels via the algorithm appropriate for the chosen mode.
   * - polyphonic / monophonic → WSOLA (cross-correlation grain search)
   * - granular                → OLA with current balanced grain size
   * - percussive              → OLA with short grains (fewer smoothing artifacts on transients)
   */
  private _tsStretch(
    channels: F32[],
    stretchRatio: number,
    quality: AudioProcessParams["quality"],
    mode: AudioProcessParams["mode"],
  ): F32[] {
    switch (mode) {
      case "polyphonic":
      case "monophonic":
        return timeStretchWSOLA(channels, stretchRatio, quality) as F32[];

      case "percussive":
        // OLA with draft-quality grain (shorter = better transient definition)
        return timeStretchGranular(channels, stretchRatio, "draft") as F32[];

      case "granular":
      default:
        return timeStretchGranular(channels, stretchRatio, quality) as F32[];
    }
  }

  /**
   * Pitch-shift channels.
   * polyphonic/monophonic: resample + WSOLA stretch (less robotic than OLA path).
   * granular/percussive:   resample + OLA (matches their stretch algorithm).
   */
  private _tsPitch(
    channels: F32[],
    semitones: number,
    quality: AudioProcessParams["quality"],
    mode: AudioProcessParams["mode"],
  ): F32[] {
    if (mode === "polyphonic" || mode === "monophonic") {
      return pitchShiftWSola(channels, semitones, quality) as F32[];
    }
    return pitchShiftDraft(channels, semitones, quality) as F32[];
  }

  private _processorForMode(params: AudioProcessParams): ProcessorKind {
    if (!params.preservePitch || params.mode === "resample") return "ts-resample";
    if (isRustDspReady()) return "rust-wasm";
    return params.mode === "granular" || params.mode === "percussive"
      ? "ts-granular"
      : "ts-wsola";
  }
}

export const audioProcessingService = new AudioProcessingService();

// ── module-level helpers ──────────────────────────────────────────────────────

type F32A = Float32Array<ArrayBufferLike>;

/** Pitch-shift using WSOLA for the stretch step (polyphonic/monophonic). */
function pitchShiftWSola(channels: F32A[], semitones: number, quality: "draft" | "balanced" | "high"): Float32Array[] {
  const clamped = Math.max(-24, Math.min(24, semitones));
  if (clamped === 0 || channels.length === 0) return channels.map((ch) => new Float32Array(ch));

  const pitchRatio    = Math.pow(2, clamped / 12);
  const originalLength = channels[0].length;

  // 1. Resample to change pitch (also changes duration)
  const resampled = channels.map((ch) => {
    const ratio  = pitchRatio;
    const inLen  = ch.length;
    const outLen = Math.max(1, Math.ceil(inLen / ratio));
    const out    = new Float32Array(outLen);
    const last   = inLen - 1;
    for (let i = 0; i < outLen; i++) {
      const src = i * ratio;
      const lo  = Math.floor(src) | 0;
      const hi  = lo < last ? lo + 1 : last;
      out[i]    = ch[lo] + (ch[hi] - ch[lo]) * (src - lo);
    }
    return out as F32A;
  });

  // 2. WSOLA stretch back to original duration
  const stretched = timeStretchWSOLA(resampled, pitchRatio, quality);

  // 3. Trim/pad to exactly original length
  return stretched.map((ch) => {
    if (ch.length === originalLength) return ch;
    const out = new Float32Array(originalLength);
    out.set(ch.subarray(0, Math.min(ch.length, originalLength)));
    return out;
  });
}

function makeResult(decoded: DecodedAudioData, channels: F32A[]): DecodedAudioData {
  const outLen = channels[0]?.length ?? 0;
  return {
    fileId:      decoded.fileId,
    sampleRate:  decoded.sampleRate,
    channels:    decoded.channels,
    length:      outLen,
    duration:    outLen / decoded.sampleRate,
    channelData: channels,
  };
}

function tick(): Promise<void> {
  return new Promise<void>((r) => setTimeout(r, 0));
}
