import type { WaveformCacheAdapter, WaveformCacheEntry } from "./types";
import { MemoryWaveformCache } from "./MemoryWaveformCache";

// When a folder project is active, peak cache is written to
// <projectRoot>/Cache/Peaks/ instead of the global userData cache.
let _projectRoot: string | null = null;

export function setElectronWaveformCacheProjectRoot(root: string | null): void {
  _projectRoot = root;
}

type WaveformCacheBridge = {
  get(key: string, projectRoot?: string): Promise<WaveformCacheEntry | null>;
  set(key: string, entry: WaveformCacheEntry & { peaks: number[] }, projectRoot?: string): Promise<void>;
  delete(key: string, projectRoot?: string): Promise<void>;
  clear(): Promise<void>;
};

function getBridge(): WaveformCacheBridge | null {
  return (window as unknown as { dawElectron?: { waveformCache?: WaveformCacheBridge } })
    .dawElectron?.waveformCache ?? null;
}

export class ElectronWaveformCache implements WaveformCacheAdapter {
  private fallback = new MemoryWaveformCache();

  async get(key: string): Promise<WaveformCacheEntry | null> {
    const bridge = getBridge();
    if (!bridge) return this.fallback.get(key);
    try {
      const entry = await bridge.get(key, _projectRoot ?? undefined);
      if (!entry) return null;
      if (Array.isArray(entry.peaks)) {
        return { ...entry, peaks: new Int16Array(entry.peaks) };
      }
      return entry;
    } catch (e) {
      console.warn("[ElectronWaveformCache] get failed:", e);
      return this.fallback.get(key);
    }
  }

  async set(key: string, entry: WaveformCacheEntry): Promise<void> {
    const bridge = getBridge();
    if (!bridge) return this.fallback.set(key, entry);
    try {
      const serialized = {
        ...entry,
        peaks: Array.from(entry.peaks instanceof Int16Array || entry.peaks instanceof Float32Array ? entry.peaks : entry.peaks),
      };
      await bridge.set(key, serialized, _projectRoot ?? undefined);
    } catch (e) {
      console.warn("[ElectronWaveformCache] set failed:", e);
      await this.fallback.set(key, entry);
    }
  }

  async delete(key: string): Promise<void> {
    const bridge = getBridge();
    if (!bridge) return this.fallback.delete(key);
    try {
      await bridge.delete(key, _projectRoot ?? undefined);
    } catch {
      await this.fallback.delete(key);
    }
  }

  async clear(): Promise<void> {
    const bridge = getBridge();
    if (!bridge) return this.fallback.clear();
    try {
      await bridge.clear();
    } catch {
      await this.fallback.clear();
    }
  }
}
