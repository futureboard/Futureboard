/**
 * Platform-agnostic peak-chunk persistence.
 *
 * Electron  → binary .bin files via IPC
 *   Path: {projectRoot}/Cache/Peaks/{fileId}/{spp}/chunk_{n}.bin
 *
 * Web       → IndexedDB ("futureboard-peak-chunks" / "chunks")
 */

import type { FileId } from "../types/daw";

let _projectRoot: string | null = null;

export function setPeakChunkProjectRoot(root: string | null): void {
  _projectRoot = root;
}

export function getPeakChunkProjectRoot(): string | null {
  return _projectRoot;
}

// ── Public API ────────────────────────────────────────────────────────────────

export async function writePeakChunk(
  fileId: FileId,
  spp: number,
  chunkIndex: number,
  data: Int16Array,
): Promise<void> {
  const buf = data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength) as ArrayBuffer;
  if (_projectRoot) {
    const bridge = (window as Window & typeof globalThis & { dawElectron?: { peakChunk?: { write: (a: string, b: number, c: number, d: ArrayBuffer, e: string) => Promise<void> } } }).dawElectron?.peakChunk;
    if (bridge) {
      await bridge.write(fileId, spp, chunkIndex, buf, _projectRoot);
      return;
    }
  }
  await _idbWrite(fileId, spp, chunkIndex, buf);
}

export async function readPeakChunk(
  fileId: FileId,
  spp: number,
  chunkIndex: number,
): Promise<Int16Array | null> {
  if (_projectRoot) {
    const bridge = (window as Window & typeof globalThis & { dawElectron?: { peakChunk?: { read: (a: string, b: number, c: number, d: string) => Promise<ArrayBuffer | null> } } }).dawElectron?.peakChunk;
    if (bridge) {
      const buf = await bridge.read(fileId, spp, chunkIndex, _projectRoot);
      return buf ? new Int16Array(buf as ArrayBuffer) : null;
    }
  }
  return _idbRead(fileId, spp, chunkIndex);
}

// ── IndexedDB fallback ────────────────────────────────────────────────────────

const IDB_NAME = "futureboard-peak-chunks";
const IDB_STORE = "chunks";
let _idb: IDBDatabase | null = null;

function _openIdb(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    if (typeof indexedDB === "undefined") { resolve(null); return; }
    const req = indexedDB.open(IDB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(IDB_STORE);
    req.onsuccess = () => { _idb = req.result; resolve(_idb); };
    req.onerror  = () => resolve(null);
  });
}

async function _getIdb(): Promise<IDBDatabase | null> {
  return _idb ?? _openIdb();
}

function _idbKey(fileId: FileId, spp: number, chunkIndex: number): string {
  return `${fileId}/${spp}/${chunkIndex}`;
}

async function _idbWrite(fileId: FileId, spp: number, chunkIndex: number, buf: ArrayBuffer): Promise<void> {
  const db = await _getIdb();
  if (!db) return;
  return new Promise((resolve) => {
    const tx = db.transaction(IDB_STORE, "readwrite");
    tx.objectStore(IDB_STORE).put(buf, _idbKey(fileId, spp, chunkIndex));
    tx.oncomplete = () => resolve();
    tx.onerror    = () => resolve();
  });
}

async function _idbRead(fileId: FileId, spp: number, chunkIndex: number): Promise<Int16Array | null> {
  const db = await _getIdb();
  if (!db) return null;
  return new Promise((resolve) => {
    const tx = db.transaction(IDB_STORE, "readonly");
    const req = tx.objectStore(IDB_STORE).get(_idbKey(fileId, spp, chunkIndex));
    req.onsuccess = () => {
      const r = req.result;
      resolve(r instanceof ArrayBuffer ? new Int16Array(r) : null);
    };
    req.onerror = () => resolve(null);
  });
}
