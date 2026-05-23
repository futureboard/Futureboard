/**
 * Module-level LRU cache for waveform peak chunks.
 *
 * Key: "${fileId}/${spp}/${chunkIndex}"
 * Value: Int16Array  (lo, hi interleaved, all channels, CHUNK_PEAKS peaks)
 * Limit: MAX_CACHE_BYTES (128 MB)
 *
 * Also tracks total canvas backing-store pixels so PerfMonitor can report it.
 */

export const CHUNK_PEAKS = 4096; // peaks per chunk (independent of spp)
const MAX_CACHE_BYTES = 128 * 1024 * 1024;

type CachedChunk = { data: Int16Array; bytes: number };

// ── Stats accessible by PerfMonitor ───────────────────────────────────────────

let _cacheBytes = 0;
let _evictions = 0;
let _canvasBackingPixels = 0;

export function getPeakCacheStats() {
  return {
    cacheBytes: _cacheBytes,
    loadedChunks: _lru.size,
    evictions: _evictions,
    canvasPixels: _canvasBackingPixels,
  };
}

/** Called by WaveformCanvas on each tile resize to track GPU backing-store usage. */
export function updateCanvasPixels(prev: number, next: number): void {
  _canvasBackingPixels = Math.max(0, _canvasBackingPixels - prev + next);
}

// ── LRU map (Map preserves insertion order; oldest entries are iterated first) ─

const _lru = new Map<string, CachedChunk>();
const _pending = new Map<string, Promise<Int16Array | null>>();
const _listeners = new Map<string, Set<() => void>>();

function chunkKey(fileId: string, spp: number, chunkIndex: number): string {
  return `${fileId}/${spp}/${chunkIndex}`;
}

function evictUntilFit(): void {
  const iter = _lru.entries();
  while (_cacheBytes > MAX_CACHE_BYTES) {
    const next = iter.next();
    if (next.done) break;
    const [key, entry] = next.value;
    _lru.delete(key);
    _cacheBytes -= entry.bytes;
    _evictions++;
  }
}

export function getCachedChunk(fileId: string, spp: number, chunkIndex: number): Int16Array | null {
  const key = chunkKey(fileId, spp, chunkIndex);
  const entry = _lru.get(key);
  if (!entry) return null;
  // Promote to MRU: delete + re-insert
  _lru.delete(key);
  _lru.set(key, entry);
  return entry.data;
}

export function putChunk(fileId: string, spp: number, chunkIndex: number, data: Int16Array): void {
  const key = chunkKey(fileId, spp, chunkIndex);
  const prev = _lru.get(key);
  if (prev) {
    _cacheBytes -= prev.bytes;
    _lru.delete(key);
  }
  const bytes = data.byteLength;
  _lru.set(key, { data, bytes });
  _cacheBytes += bytes;
  evictUntilFit();

  // Notify & remove listeners
  const cbs = _listeners.get(key);
  if (cbs) {
    cbs.forEach((cb) => cb());
    _listeners.delete(key);
  }
  _pending.delete(key);
}

/**
 * Request a chunk. Returns cached data immediately if present; otherwise
 * triggers `loader()` and calls `onReady` when the data arrives.
 * Duplicate concurrent loads for the same key are deduplicated.
 */
export function requestChunk(
  fileId: string,
  spp: number,
  chunkIndex: number,
  loader: () => Promise<Int16Array | null>,
  onReady: () => void,
): Int16Array | null {
  const cached = getCachedChunk(fileId, spp, chunkIndex);
  if (cached) return cached;

  const key = chunkKey(fileId, spp, chunkIndex);
  if (!_listeners.has(key)) _listeners.set(key, new Set());
  _listeners.get(key)!.add(onReady);

  if (!_pending.has(key)) {
    const p = loader()
      .then((data) => {
        if (data) {
          putChunk(fileId, spp, chunkIndex, data);
        } else {
          // Load failed — notify so renderers don't hang
          const cbs = _listeners.get(key);
          if (cbs) { cbs.forEach((cb) => cb()); _listeners.delete(key); }
          _pending.delete(key);
        }
        return data;
      })
      .catch(() => {
        _pending.delete(key);
        const cbs = _listeners.get(key);
        if (cbs) { cbs.forEach((cb) => cb()); _listeners.delete(key); }
        return null;
      });
    _pending.set(key, p);
  }

  return null;
}

/** Evict all chunks belonging to a file (e.g. on project close). */
export function evictFile(fileId: string): void {
  const prefix = `${fileId}/`;
  for (const [key, entry] of _lru.entries()) {
    if (key.startsWith(prefix)) {
      _lru.delete(key);
      _cacheBytes -= entry.bytes;
    }
  }
}

/** Evict everything (project close / memory pressure). */
export function clearPeakChunkCache(): void {
  _lru.clear();
  _cacheBytes = 0;
}
