import { Database } from "bun:sqlite";

let _db: Database | null = null;

export function getDb(): Database {
  if (_db) return _db;
  _db = new Database("mochi-daw.sqlite", { create: true });
  _db.exec("PRAGMA journal_mode = WAL;");
  _db.exec(`
    CREATE TABLE IF NOT EXISTS projects (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      version INTEGER NOT NULL DEFAULT 1,
      sample_rate INTEGER NOT NULL DEFAULT 48000,
      bpm INTEGER NOT NULL DEFAULT 120,
      data TEXT NOT NULL DEFAULT '{}',
      created_at INTEGER NOT NULL,
      updated_at INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS files (
      id TEXT PRIMARY KEY,
      project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
      name TEXT NOT NULL,
      mime_type TEXT NOT NULL,
      duration REAL NOT NULL DEFAULT 0,
      sample_rate INTEGER NOT NULL DEFAULT 48000,
      channels INTEGER NOT NULL DEFAULT 2,
      storage_key TEXT NOT NULL,
      created_at INTEGER NOT NULL
    );
  `);
  return _db;
}
