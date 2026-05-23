/**
 * copy-floatingwindow.mjs
 *
 * Builds the floatingwindow Rust crate (both bin + napi-addon lib targets)
 * and copies the outputs into apps/electron/resources/:
 *
 *   floatingwindow.exe   — standalone native runtime (spawned by main process)
 *   floatingwindow.node  — NAPI addon (optional higher-perf IPC wrapper)
 *
 * Usage (called by npm scripts):
 *   node scripts/copy-floatingwindow.mjs          # release build (default)
 *   node scripts/copy-floatingwindow.mjs --debug  # debug build
 */

import fs from "fs";
import path from "path";
import { execSync } from "child_process";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDebug = process.argv.includes("--debug");
const profile = isDebug ? "debug" : "release";
const cargoFlag = isDebug ? "" : "--release";

// ── Paths ─────────────────────────────────────────────────────────────────────

const electronRoot   = path.resolve(__dirname, "..");
const workspaceRoot  = path.resolve(electronRoot, "..", "..");
const crateRoot      = path.join(workspaceRoot, "apps", "floatingwindow");
const targetDir      = path.join(crateRoot, "target", profile);
const resourcesDir   = path.join(electronRoot, "resources");

// ── Platform-specific lib filename ───────────────────────────────────────────

function libSourceName() {
  switch (process.platform) {
    case "win32":  return "floatingwindow.dll";
    case "darwin": return "libfloatingwindow.dylib";
    default:       return "libfloatingwindow.so";
  }
}

function binName() {
  return process.platform === "win32" ? "floatingwindow.exe" : "floatingwindow";
}

// ── Build ─────────────────────────────────────────────────────────────────────

console.log(`[copy-floatingwindow] Building ${profile} (bin + napi-addon lib)…`);

try {
  // Build standalone binary
  execSync(`cargo build ${cargoFlag} --bin floatingwindow`, {
    cwd: crateRoot,
    stdio: "inherit",
  });

  // Build NAPI cdylib (requires napi-addon feature so napi_build::setup() runs)
  execSync(`cargo build ${cargoFlag} --lib --features napi-addon`, {
    cwd: crateRoot,
    stdio: "inherit",
  });
} catch (e) {
  console.error("[copy-floatingwindow] cargo build failed");
  process.exit(1);
}

// ── Copy ──────────────────────────────────────────────────────────────────────

fs.mkdirSync(resourcesDir, { recursive: true });

// 1. floatingwindow binary
const binSrc  = path.join(targetDir, binName());
const binDest = path.join(resourcesDir, binName());

if (!fs.existsSync(binSrc)) {
  console.error(`[copy-floatingwindow] ERROR: binary not found:\n  ${binSrc}`);
  process.exit(1);
}
fs.copyFileSync(binSrc, binDest);
console.log(`[copy-floatingwindow] ✓ ${binName()}  →  resources/${binName()}  (${kb(binDest)} KB)`);

// 2. NAPI .node addon
const libSrc  = path.join(targetDir, libSourceName());
const libDest = path.join(resourcesDir, "floatingwindow.node");

if (!fs.existsSync(libSrc)) {
  console.error(`[copy-floatingwindow] ERROR: NAPI lib not found:\n  ${libSrc}`);
  process.exit(1);
}
fs.copyFileSync(libSrc, libDest);
console.log(`[copy-floatingwindow] ✓ ${libSourceName()}  →  resources/floatingwindow.node  (${kb(libDest)} KB)`);

// ── helpers ───────────────────────────────────────────────────────────────────

function kb(p) {
  return Math.ceil(fs.statSync(p).size / 1024);
}
