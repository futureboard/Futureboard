import { getDb } from "./db/schema";
import { error, json, noContent, withCors } from "./http";
import { projectRoutes } from "./routes/projects";
import { fileRoutes } from "./routes/files";
import type { RouteHandler } from "./types";

getDb();

const PORT = parseInt(process.env.PORT ?? "3001");

function matchRoute(
  method: string,
  pathname: string,
  pattern: string
): Record<string, string> | null {
  const patternParts = pattern.split(" ");
  if (patternParts.length !== 2) return null;
  const [patMethod, patPath] = patternParts;
  if (!patMethod || !patPath) return null;
  if (patMethod !== method) return null;

  const patSegments = patPath.split("/").filter(Boolean);
  const urlSegments = pathname.split("/").filter(Boolean);
  if (patSegments.length !== urlSegments.length) return null;

  const params: Record<string, string> = {};
  for (let i = 0; i < patSegments.length; i++) {
    const patSegment = patSegments[i];
    const urlSegment = urlSegments[i];
    if (!patSegment || !urlSegment) return null;

    if (patSegment.startsWith(":")) {
      params[patSegment.slice(1)] = urlSegment;
    } else if (patSegment !== urlSegment) {
      return null;
    }
  }
  return params;
}

const allRoutes: Record<string, RouteHandler> = {
  "GET /health": () => json({ ok: true, service: "mochi-daw-server" }),
  ...projectRoutes,
  ...fileRoutes,
};

Bun.serve({
  port: PORT,
  async fetch(req) {
    if (req.method === "OPTIONS") return noContent();

    const url = new URL(req.url);
    const method = req.method;
    const pathname = url.pathname;

    for (const [pattern, handler] of Object.entries(allRoutes)) {
      const params = matchRoute(method, pathname, pattern);
      if (params !== null) {
        try {
          return withCors(await handler(req, params));
        } catch (cause) {
          console.error(cause);
          return error("Internal server error", 500);
        }
      }
    }

    return error("Not found", 404);
  },
  development: {
    hmr: false,
    console: true,
  },
});

console.log(`Mochi DAW server running on http://localhost:${PORT}`);
