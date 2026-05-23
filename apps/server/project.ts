import type { ApiProject, DawProject, ProjectRow } from "./types";

export function createEmptyProject(input: {
  id: string;
  name?: string;
  bpm?: number;
  sampleRate?: number;
}): DawProject {
  return {
    id: input.id,
    name: normalizeProjectName(input.name),
    version: 1,
    sampleRate: normalizeSampleRate(input.sampleRate),
    bpm: normalizeBpm(input.bpm),
    tracks: [],
    files: [],
  };
}

export function parseProjectData(value: string): DawProject | Record<string, never> {
  try {
    const parsed = JSON.parse(value) as unknown;
    return isDawProject(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

export function toApiProject(row: ProjectRow): ApiProject {
  return {
    id: row.id,
    name: row.name,
    version: row.version,
    sampleRate: row.sample_rate,
    bpm: row.bpm,
    data: parseProjectData(row.data),
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export function readProjectPayload(input: unknown): Partial<DawProject> {
  if (!input || typeof input !== "object") return {};
  const body = input as Record<string, unknown>;
  const data = body.data && typeof body.data === "object" ? body.data : body;
  if (!data || typeof data !== "object") return {};

  const project = data as Partial<DawProject>;
  const payload: Partial<DawProject> = {};

  if (typeof project.id === "string") payload.id = project.id;
  if (typeof project.name === "string") payload.name = normalizeProjectName(project.name);
  if (typeof project.version === "number") payload.version = project.version;
  if (typeof project.sampleRate === "number") payload.sampleRate = normalizeSampleRate(project.sampleRate);
  if (typeof project.bpm === "number") payload.bpm = normalizeBpm(project.bpm);
  if (Array.isArray(project.tracks)) payload.tracks = project.tracks;
  if (Array.isArray(project.files)) payload.files = project.files;

  return payload;
}

export function normalizeProjectName(value: unknown): string {
  if (typeof value !== "string") return "Untitled Project";
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed.slice(0, 120) : "Untitled Project";
}

export function normalizeBpm(value: unknown): number {
  return normalizeNumber(value, 120, 20, 300);
}

export function normalizeSampleRate(value: unknown): number {
  return normalizeNumber(value, 48000, 8000, 192000);
}

function normalizeNumber(value: unknown, fallback: number, min: number, max: number): number {
  if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.round(value)));
}

function isDawProject(value: unknown): value is DawProject {
  if (!value || typeof value !== "object") return false;
  const project = value as Partial<DawProject>;
  return (
    typeof project.id === "string" &&
    typeof project.name === "string" &&
    typeof project.version === "number" &&
    typeof project.sampleRate === "number" &&
    typeof project.bpm === "number" &&
    Array.isArray(project.tracks) &&
    Array.isArray(project.files)
  );
}
