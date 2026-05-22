/**
 * copy-native-audio.mjs
 *
 * Copies the built SphereDirectAudioEngine native addon (.dll/.dylib/.so)
 * into the Electron resources directory so the main process can load it.
 *
 * Usage (called automatically by npm scripts):
 *   node scripts/copy-native-audio.mjs          # release build
 *   node scripts/copy-native-audio.mjs --debug  # debug build
 */

import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDebug = process.argv.includes("--debug");

// ── Platform-specific addon filename ─────────────────────────────────────────

function addonSourceName() {
  switch (process.platform) {
    case "win32":  return "DAUx.dll";
    case "darwin": return "libDAUx.dylib";
    default:       return "libDAUx.so";
  }
}

// napi-rs / Node.js expects .node extension for native addons.
const NODE_ADDON_NAME = "DAUx.node";

// ── Paths ─────────────────────────────────────────────────────────────────────

const electronRoot  = path.resolve(__dirname, "..");
const workspaceRoot = path.resolve(electronRoot, "..", "..");
const engineRoot    = path.join(workspaceRoot, "crates", "SphereDirectAudioEngine");
const profile       = isDebug ? "debug" : "release";
const sourcePath    = path.join(engineRoot, "target", profile, addonSourceName());

// Destination: apps/electron/resources/ (packaged via electron-builder extraResources)
const resourcesDir  = path.join(electronRoot, "resources");
const destPath      = path.join(resourcesDir, NODE_ADDON_NAME);

// ── Copy ──────────────────────────────────────────────────────────────────────

if (!fs.existsSync(sourcePath)) {
  console.error(
    `[copy-native-audio] ERROR: Built addon not found at:\n  ${sourcePath}\n` +
    `  Run: cd crates/SphereDirectAudioEngine && cargo build${isDebug ? "" : " --release"}`
  );
  process.exit(1);
}

fs.mkdirSync(resourcesDir, { recursive: true });
fs.copyFileSync(sourcePath, destPath);

const sizeKb = Math.ceil(fs.statSync(destPath).size / 1024);
console.log(
  `[copy-native-audio] ✓ Copied ${profile} addon → resources/${NODE_ADDON_NAME}  (${sizeKb} KB)`
);
