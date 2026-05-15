import type { AudioProcessQuality, AudioPitchMode } from "../types/daw";

// ── Decoded audio ─────────────────────────────────────────────────────────────

/** Platform-neutral decoded audio — Float32Array channel data, no AudioBuffer dependency. */
// TS6 typed-array generic: Float32Array<ArrayBufferLike> is the widest form;
// Float32Array<ArrayBuffer> (the default) is narrower. Use the wide form so
// channel data from AudioBuffer.getChannelData() and from new Float32Array()
// are both assignable without casts.
export type F32 = Float32Array<ArrayBufferLike>;

export type DecodedAudioData = {
  fileId: string;
  sampleRate: number;
  channels: number;
  length: number;
  duration: number;
  channelData: F32[];
};

// ── Processing params ─────────────────────────────────────────────────────────

export type AudioProcessParams = {
  speedRatio: number;
  pitchSemitones: number;
  preservePitch: boolean;
  mode: AudioPitchMode;
  quality: AudioProcessQuality;
};

// ── Cache stats ───────────────────────────────────────────────────────────────

export type AudioCacheStats = {
  decodedEntries: number;
  decodedBytes: number;
  processedEntries: number;
  processedBytes: number;
};

// ── Conversion helpers ────────────────────────────────────────────────────────

export function audioBufferToDecodedAudio(fileId: string, buf: AudioBuffer): DecodedAudioData {
  const channelData: F32[] = [];
  for (let c = 0; c < buf.numberOfChannels; c++) {
    // new Float32Array(src) copies data and returns Float32Array<ArrayBuffer>
    channelData.push(new Float32Array(buf.getChannelData(c)));
  }
  return {
    fileId,
    sampleRate: buf.sampleRate,
    channels: buf.numberOfChannels,
    length: buf.length,
    duration: buf.duration,
    channelData,
  };
}

export function decodedAudioToAudioBuffer(ctx: AudioContext, decoded: DecodedAudioData): AudioBuffer {
  const buf = ctx.createBuffer(decoded.channels, decoded.length, decoded.sampleRate);
  for (let c = 0; c < decoded.channels; c++) {
    buf.copyToChannel(new Float32Array(decoded.channelData[c]), c);
  }
  return buf;
}

export function getDecodedAudioByteSize(decoded: DecodedAudioData): number {
  return decoded.channelData.reduce((sum, ch) => sum + ch.byteLength, 0);
}
