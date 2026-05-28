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

// In a Cargo workspace, artifacts land in workspaceRoot/target by default.
const sourceCandidates = [
  path.join(workspaceRoot, "target", mode, sourceName),
  path.join(hostRoot, "target", mode, sourceName),
];
const source = sourceCandidates.find((p) => fs.existsSync(p));
const targetDir = path.join(electronRoot, "resources");
const target = path.join(targetDir, "PluginHost.node");

if (!source) {
  console.error(
    `[copy-plugin-host] Missing native addon. Checked:\n` +
    sourceCandidates.map((p) => `  ${p}`).join("\n")
  );
  process.exit(1);
}

fs.mkdirSync(targetDir, { recursive: true });
fs.copyFileSync(source, target);

const sizeKb = Math.round(fs.statSync(target).size / 1024);
console.log(`[copy-plugin-host] ✓ Copied ${mode} addon → resources/PluginHost.node  (${sizeKb} KB)`);
