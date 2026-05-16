import type { WaveformCacheAdapter, WaveformCacheEntry } from "./types";
import { MemoryWaveformCache } from "./MemoryWaveformCache";

const DB_NAME = "futureboard-waveform-cache";
const STORE_NAME = "waveforms";
const DB_VERSION = 1;

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = (e) => {
      const db = (e.target as IDBOpenDBRequest).result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME);
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function idbGet(db: IDBDatabase, key: string): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const req = tx.objectStore(STORE_NAME).get(key);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function idbSet(db: IDBDatabase, key: string, value: unknown): Promise<void> {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const req = tx.objectStore(STORE_NAME).put(value, key);
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error);
  });
}

function idbDelete(db: IDBDatabase, key: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const req = tx.objectStore(STORE_NAME).delete(key);
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error);
  });
}

function idbClear(db: IDBDatabase): Promise<void> {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const req = tx.objectStore(STORE_NAME).clear();
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error);
  });
}

export class WebWaveformCache implements WaveformCacheAdapter {
  private dbPromise: Promise<IDBDatabase> | null = null;
  private fallback = new MemoryWaveformCache();
  private broken = false;

  private db(): Promise<IDBDatabase> {
    if (this.broken) return Promise.reject(new Error("IDB unavailable"));
    if (!this.dbPromise) {
      this.dbPromise = openDb().catch((e) => {
        this.broken = true;
        throw e;
      });
    }
    return this.dbPromise;
  }

  async get(key: string): Promise<WaveformCacheEntry | null> {
    try {
      const db = await this.db();
      const raw = await idbGet(db, key) as (WaveformCacheEntry & { peaks: number[] | Float32Array | Int16Array }) | undefined;
      if (!raw) return null;
      if (!(raw.peaks instanceof Int16Array) && !(raw.peaks instanceof Float32Array) && Array.isArray(raw.peaks)) {
        raw.peaks = new Int16Array(raw.peaks);
      }
      return raw;
    } catch {
      return this.fallback.get(key);
    }
  }

  async set(key: string, entry: WaveformCacheEntry): Promise<void> {
    try {
      const db = await this.db();
      const stored: WaveformCacheEntry = {
        ...entry,
        peaks: entry.peaks instanceof Int16Array || entry.peaks instanceof Float32Array
          ? entry.peaks
          : new Int16Array(entry.peaks),
      };
      await idbSet(db, key, stored);
    } catch {
      await this.fallback.set(key, entry);
    }
  }

  async delete(key: string): Promise<void> {
    try {
      const db = await this.db();
      await idbDelete(db, key);
    } catch {
      await this.fallback.delete(key);
    }
  }

  async clear(): Promise<void> {
    try {
      const db = await this.db();
      await idbClear(db);
    } catch {
      await this.fallback.clear();
    }
  }
}
