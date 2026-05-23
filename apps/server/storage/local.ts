import { join } from "path";
import { mkdirSync } from "fs";

const UPLOADS_DIR = process.env.UPLOADS_DIR ?? "./uploads";

export function ensureProjectDir(projectId: string): string {
  const dir = join(UPLOADS_DIR, projectId);
  mkdirSync(dir, { recursive: true });
  return dir;
}

export function getFilePath(projectId: string, fileId: string): string {
  return join(UPLOADS_DIR, projectId, fileId);
}

export async function saveFile(projectId: string, fileId: string, data: ArrayBuffer): Promise<string> {
  const dir = ensureProjectDir(projectId);
  const path = join(dir, fileId);
  await Bun.write(path, data);
  return path;
}
