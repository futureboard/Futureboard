import type { WaveformCacheAdapter, WaveformCacheEntry } from "./types";

export class MemoryWaveformCache implements WaveformCacheAdapter {
  private store = new Map<string, WaveformCacheEntry>();

  async get(key: string): Promise<WaveformCacheEntry | null> {
    return this.store.get(key) ?? null;
  }

  async set(key: string, entry: WaveformCacheEntry): Promise<void> {
    this.store.set(key, entry);
  }

  async delete(key: string): Promise<void> {
    this.store.delete(key);
  }

  async clear(): Promise<void> {
    this.store.clear();
  }
}
