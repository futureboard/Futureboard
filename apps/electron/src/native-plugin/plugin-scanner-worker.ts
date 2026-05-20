import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const _dirname = path.dirname(fileURLToPath(import.meta.url));
const req = createRequire(import.meta.url);

type NativePluginInfo = {
  id?: string;
  name?: string;
  vendor?: string;
  category?: string;
  format?: string;
  path?: string;
  classId?: string | null;
  class_id?: string | null;
  version?: string | null;
  sdkMetadataLoaded?: boolean;
  sdk_metadata_loaded?: boolean;
};

type PluginHostAddon = {
  scanVst3?: (paths: string[]) => NativePluginInfo[];
};

type RequestMessage = {
  id: string;
  path: string;
};

function workspaceRoot(): string {
  return path.resolve(_dirname, "..", "..", "..", "..");
}

function candidateAddonPaths(): string[] {
  const candidates: string[] = [];
  if (process.resourcesPath) {
    candidates.push(path.join(process.resourcesPath, "PluginHost.node"));
  }
  const root = workspaceRoot();
  const hostRoot = path.join(root, "frameworks", "SpherePluginHost");
  candidates.push(path.join(hostRoot, "PluginHost.node"));
  candidates.push(path.join(hostRoot, "target", "release", "PluginHost.node"));
  candidates.push(path.join(hostRoot, "target", "release", "PluginHost.dll"));
  candidates.push(path.join(hostRoot, "target", "debug", "PluginHost.node"));
  candidates.push(path.join(hostRoot, "target", "debug", "PluginHost.dll"));
  candidates.push(path.resolve(_dirname, "..", "..", "resources", "PluginHost.node"));
  return candidates;
}

function loadAddon(): PluginHostAddon {
  const errors: string[] = [];
  for (const candidate of candidateAddonPaths()) {
    if (!fs.existsSync(candidate)) continue;
    try {
      return req(candidate) as PluginHostAddon;
    } catch (error) {
      errors.push(`${candidate}: ${String(error)}`);
    }
  }
  throw new Error(`PluginHost addon not available. ${errors.join(" | ")}`);
}

process.on("message", (message: RequestMessage) => {
  try {
    if (!message?.id || !message.path) throw new Error("Invalid scanner request");
    const addon = loadAddon();
    if (!addon.scanVst3) throw new Error("PluginHost scanVst3 export is unavailable");
    const plugins = addon.scanVst3([message.path]);
    process.send?.({ id: message.id, ok: true, plugins });
  } catch (error) {
    process.send?.({ id: message?.id, ok: false, error: String(error) });
  }
});
