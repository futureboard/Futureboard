import { getDb } from "../db/schema";
import { error, json } from "../http";
import {
  createEmptyProject,
  normalizeBpm,
  normalizeProjectName,
  normalizeSampleRate,
  parseProjectData,
  readProjectPayload,
  toApiProject,
} from "../project";
import type { DawProject, ProjectRow, RouteHandler } from "../types";

export const projectRoutes: Record<string, RouteHandler> = {
  "GET /api/projects": () => {
    const db = getDb();
    const rows = db.query("SELECT * FROM projects ORDER BY updated_at DESC").all() as ProjectRow[];
    return json(rows.map(toApiProject));
  },

  "POST /api/projects": async (req: Request) => {
    const body = (await req.json().catch(() => ({}))) as Record<string, unknown>;
    const db = getDb();
    const id = crypto.randomUUID();
    const project = {
      ...createEmptyProject({
        id,
        name: typeof body.name === "string" ? body.name : undefined,
        bpm: typeof body.bpm === "number" ? body.bpm : undefined,
        sampleRate: typeof body.sampleRate === "number" ? body.sampleRate : undefined,
      }),
      ...readProjectPayload(body),
      id,
    } satisfies DawProject;
    const now = Date.now();

    db.query(
      "INSERT INTO projects (id, name, version, sample_rate, bpm, data, created_at, updated_at) VALUES (?, ?, 1, ?, ?, ?, ?, ?)"
    ).run(
      id,
      project.name,
      project.sampleRate,
      project.bpm,
      JSON.stringify(project),
      now,
      now
    );

    const row = db.query("SELECT * FROM projects WHERE id = ?").get(id) as ProjectRow | null;
    if (!row) return error("Failed to create project", 500);
    return json(toApiProject(row), 201);
  },

  "GET /api/projects/:id": (req: Request, params: Record<string, string>) => {
    const projectId = params.id;
    if (!projectId) return error("Project id is required", 400);

    const db = getDb();
    const row = db.query("SELECT * FROM projects WHERE id = ?").get(projectId) as ProjectRow | null;
    if (!row) return error("Project not found", 404);
    return json(toApiProject(row));
  },

  "PUT /api/projects/:id": async (req: Request, params: Record<string, string>) => {
    const projectId = params.id;
    if (!projectId) return error("Project id is required", 400);

    const body = (await req.json().catch(() => ({}))) as Record<string, unknown>;
    const db = getDb();
    const existingRow = db.query("SELECT * FROM projects WHERE id = ?").get(projectId) as ProjectRow | null;
    if (!existingRow) return error("Project not found", 404);

    const existingProject = parseProjectData(existingRow.data);
    const project = {
      ...existingProject,
      ...readProjectPayload(body),
      id: projectId,
      name:
        typeof body.name === "string"
          ? normalizeProjectName(body.name)
          : normalizeProjectName(readProjectPayload(body).name ?? existingRow.name),
      version: existingRow.version,
      sampleRate: normalizeSampleRate(readProjectPayload(body).sampleRate ?? existingRow.sample_rate),
      bpm: normalizeBpm(readProjectPayload(body).bpm ?? existingRow.bpm),
      tracks: "tracks" in existingProject && Array.isArray(existingProject.tracks) ? existingProject.tracks : [],
      files: "files" in existingProject && Array.isArray(existingProject.files) ? existingProject.files : [],
      ...readProjectPayload(body),
    } satisfies DawProject;
    const now = Date.now();

    const result = db.query(
      "UPDATE projects SET name = ?, version = ?, sample_rate = ?, bpm = ?, data = ?, updated_at = ? WHERE id = ?"
    ).run(
      project.name,
      project.version,
      project.sampleRate,
      project.bpm,
      JSON.stringify(project),
      now,
      projectId
    );

    if (result.changes === 0) return error("Project not found", 404);
    const row = db.query("SELECT * FROM projects WHERE id = ?").get(projectId) as ProjectRow | null;
    if (!row) return error("Project not found", 404);
    return json(toApiProject(row));
  },

  "POST /api/projects/:id/save": async (req: Request, params: Record<string, string>) => {
    const projectId = params.id;
    if (!projectId) return error("Project id is required", 400);

    const body = (await req.json().catch(() => null)) as unknown;
    const project = readProjectPayload(body);
    if (!project.name || !project.tracks || !project.files) {
      return error("Project save payload must include name, tracks, and files", 400);
    }

    const savedProject: DawProject = {
      id: projectId,
      name: project.name,
      version: project.version ?? 1,
      sampleRate: normalizeSampleRate(project.sampleRate),
      bpm: normalizeBpm(project.bpm),
      tracks: project.tracks,
      files: project.files,
    };

    const db = getDb();
    const now = Date.now();
    const result = db.query(
      "UPDATE projects SET name = ?, version = ?, sample_rate = ?, bpm = ?, data = ?, updated_at = ? WHERE id = ?"
    ).run(
      savedProject.name,
      savedProject.version,
      savedProject.sampleRate,
      savedProject.bpm,
      JSON.stringify(savedProject),
      now,
      projectId
    );

    if (result.changes === 0) return error("Project not found", 404);
    const row = db.query("SELECT * FROM projects WHERE id = ?").get(projectId) as ProjectRow | null;
    if (!row) return error("Project not found", 404);
    return json(toApiProject(row));
  },

  "DELETE /api/projects/:id": (req: Request, params: Record<string, string>) => {
    const projectId = params.id;
    if (!projectId) return error("Project id is required", 400);

    const db = getDb();
    const result = db.query("DELETE FROM projects WHERE id = ?").run(projectId);
    if (result.changes === 0) return error("Project not found", 404);
    return json({ ok: true });
  },

  "POST /api/projects/:id/export": () => {
    return error("Offline export is not implemented yet", 501);
  },
};
