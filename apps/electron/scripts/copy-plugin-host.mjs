import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const electronRoot = path.resolve(__dirname, "..");
const workspaceRoot = path.resolve(electronRoot, "..", "..");
const hostRoot = path.join(workspaceRoot, "crates", "SpherePluginHost");
const mode = process.argv.includes("--debug") ? "debug" : "release";

const sourceName =
  process.platform === "win32"
    ? "sphere_plugin_host.dll"
    : process.platform === "darwin"
      ? "libsphere_plugin_host.dylib"
      : "libsphere_plugin_host.so";

const source = path.join(hostRoot, "target", mode, sourceName);
const targetDir = path.join(electronRoot, "resources");
const target = path.join(targetDir, "PluginHost.node");

if (!fs.existsSync(source)) {
  console.error(`[copy-plugin-host] Missing native addon: ${source}`);
  process.exit(1);
}

fs.mkdirSync(targetDir, { recursive: true });
fs.copyFileSync(source, target);

const sizeKb = Math.round(fs.statSync(target).size / 1024);
console.log(`[copy-plugin-host] ✓ Copied ${mode} addon → resources/PluginHost.node  (${sizeKb} KB)`);
