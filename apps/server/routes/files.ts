import { getDb } from "../db/schema";
import { error, json, withCors } from "../http";
import { saveFile } from "../storage/local";
import type { ApiFile, FileRow, RouteHandler } from "../types";

const MAX_AUDIO_FILE_BYTES = Number(process.env.MAX_AUDIO_FILE_BYTES ?? 250 * 1024 * 1024);
const SUPPORTED_AUDIO_TYPES = new Set([
  "audio/mpeg",
  "audio/mp3",
  "audio/wav",
  "audio/wave",
  "audio/x-wav",
]);

function toApiFile(row: FileRow): ApiFile {
  return {
    id: row.id,
    projectId: row.project_id,
    name: row.name,
    mimeType: row.mime_type,
    duration: row.duration,
    sampleRate: row.sample_rate,
    channels: row.channels,
    storageKey: row.storage_key,
    createdAt: row.created_at,
  };
}

export const fileRoutes: Record<string, RouteHandler> = {
  "POST /api/projects/:projectId/files": async (req: Request, params: Record<string, string>) => {
    const projectId = params.projectId;
    if (!projectId) return error("Project id is required", 400);

    const db = getDb();
    const project = db.query("SELECT id FROM projects WHERE id = ?").get(projectId);
    if (!project) return error("Project not found", 404);

    const formData = await req.formData();
    const file = formData.get("file") as File | null;
    if (!file) return error("No file provided", 400);
    if (!SUPPORTED_AUDIO_TYPES.has(file.type)) return error("Only WAV and MP3 files are supported", 415);
    if (file.size > MAX_AUDIO_FILE_BYTES) return error("Audio file exceeds the upload size limit", 413);

    const fileId = crypto.randomUUID();
    const arrayBuffer = await file.arrayBuffer();
    const storageKey = await saveFile(projectId, fileId, arrayBuffer);

    const now = Date.now();
    db.query(
      "INSERT INTO files (id, project_id, name, mime_type, duration, sample_rate, channels, storage_key, created_at) VALUES (?, ?, ?, ?, 0, 48000, 2, ?, ?)"
    ).run(fileId, projectId, file.name, file.type || "audio/wav", storageKey, now);

    const row = db.query("SELECT * FROM files WHERE id = ?").get(fileId) as FileRow | null;
    if (!row) return error("Failed to save file metadata", 500);
    return json(toApiFile(row), 201);
  },

  "GET /api/projects/:projectId/files": (req: Request, params: Record<string, string>) => {
    const projectId = params.projectId;
    if (!projectId) return error("Project id is required", 400);

    const db = getDb();
    const rows = db
      .query("SELECT * FROM files WHERE project_id = ? ORDER BY created_at DESC")
      .all(projectId) as FileRow[];
    return json(rows.map(toApiFile));
  },

  "GET /api/projects/:projectId/files/:fileId": async (req: Request, params: Record<string, string>) => {
    const projectId = params.projectId;
    const fileId = params.fileId;
    if (!projectId) return error("Project id is required", 400);
    if (!fileId) return error("File id is required", 400);

    const db = getDb();
    const row = db
      .query("SELECT * FROM files WHERE id = ? AND project_id = ?")
      .get(fileId, projectId) as FileRow | null;
    if (!row) return error("File not found", 404);
    if (new URL(req.url).searchParams.get("metadata") === "true") return json(toApiFile(row));

    const file = Bun.file(row.storage_key);
    if (!(await file.exists())) return error("Stored file is missing", 404);

    return withCors(new Response(file.stream(), {
      headers: {
        "Content-Type": row.mime_type,
        "Content-Length": String(file.size),
        "Accept-Ranges": "bytes",
      },
    }));
  },
};
