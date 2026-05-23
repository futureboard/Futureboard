import type { DecodedAudioData, AudioCacheStats } from "./audioCacheTypes";
import { getDecodedAudioByteSize } from "./audioCacheTypes";

// ── Limits (bytes) ────────────────────────────────────────────────────────────

const MAX_DECODED_BYTES = 512 * 1024 * 1024;   // 512 MB
const MAX_PROCESSED_BYTES = 256 * 1024 * 1024; // 256 MB

// ── LRU Map helper ────────────────────────────────────────────────────────────
// JS Map preserves insertion order. Promote accessed entries to end = MRU.

class LRUAudioMap {
  private _map = new Map<string, DecodedAudioData>();
  private _bytes = 0;
  private readonly _limit: number;

  constructor(limitBytes: number) {
    this._limit = limitBytes;
  }

  get(key: string): DecodedAudioData | undefined {
    const entry = this._map.get(key);
    if (!entry) return undefined;
    // Promote to most-recently-used
    this._map.delete(key);
    this._map.set(key, entry);
    return entry;
  }

  set(key: string, data: DecodedAudioData): void {
    if (this._map.has(key)) {
      this._bytes -= getDecodedAudioByteSize(this._map.get(key)!);
      this._map.delete(key);
    }
    const size = getDecodedAudioByteSize(data);
    this._evictUntilFits(size);
    this._map.set(key, data);
    this._bytes += size;
  }

  delete(key: string): void {
    const entry = this._map.get(key);
    if (entry) {
      this._bytes -= getDecodedAudioByteSize(entry);
      this._map.delete(key);
    }
  }

  clear(): void {
    this._map.clear();
    this._bytes = 0;
  }

  get size(): number {
    return this._map.size;
  }

  get totalBytes(): number {
    return this._bytes;
  }

  keys(): IterableIterator<string> {
    return this._map.keys();
  }

  /** Remove entries whose key starts with the given prefix. */
  deleteByPrefix(prefix: string): void {
    for (const [k, v] of this._map) {
      if (k.startsWith(prefix)) {
        this._bytes -= getDecodedAudioByteSize(v);
        this._map.delete(k);
      }
    }
  }

  private _evictUntilFits(incomingBytes: number): void {
    const target = this._limit - incomingBytes;
    const iter = this._map.entries();
    while (this._bytes > target) {
      const next = iter.next();
      if (next.done) break;
      const [k, v] = next.value;
      this._bytes -= getDecodedAudioByteSize(v);
      this._map.delete(k);
    }
  }
}

// ── AudioCacheManager ─────────────────────────────────────────────────────────

class AudioCacheManager {
  private _decoded = new LRUAudioMap(MAX_DECODED_BYTES);
  private _processed = new LRUAudioMap(MAX_PROCESSED_BYTES);

  // ── Decoded audio ──────────────────────────────────────────────────────────

  getDecodedAudio(key: string): DecodedAudioData | undefined {
    return this._decoded.get(key);
  }

  setDecodedAudio(key: string, data: DecodedAudioData): void {
    this._decoded.set(key, data);
  }

  // ── Processed audio ────────────────────────────────────────────────────────

  getProcessedAudio(key: string): DecodedAudioData | undefined {
    return this._processed.get(key);
  }

  setProcessedAudio(key: string, data: DecodedAudioData): void {
    this._processed.set(key, data);
  }

  // ── Eviction ───────────────────────────────────────────────────────────────

  /** Remove all cached data for a specific file (decoded + any processed variants). */
  clearFileCache(fileId: string): void {
    // Match any cache version so old entries don't linger after version bumps.
    const segment = `:${fileId}:`;
    for (const key of [...this._decoded.keys()]) {
      if (key.includes(segment)) this._decoded.delete(key);
    }
    for (const key of [...this._processed.keys()]) {
      if (key.includes(segment)) this._processed.delete(key);
    }
  }

  /** Remove only processed variants (decoded stays warm — expensive to re-decode). */
  clearAllProcessed(): void {
    this._processed.clear();
  }

  clearAllAudioCache(): void {
    this._decoded.clear();
    this._processed.clear();
  }

  // ── Stats ──────────────────────────────────────────────────────────────────

  getStats(): AudioCacheStats {
    return {
      decodedEntries: this._decoded.size,
      decodedBytes: this._decoded.totalBytes,
      processedEntries: this._processed.size,
      processedBytes: this._processed.totalBytes,
    };
  }
}

export const audioCacheManager = new AudioCacheManager();
