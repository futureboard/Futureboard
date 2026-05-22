import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const webRoot = path.resolve(__dirname, "..");
const workspaceRoot = path.resolve(webRoot, "..", "..");
const coreRoot = path.join(workspaceRoot, "crates", "SphereWebAudioCore");
const outDir = path.join(webRoot, "src", "engine", "wasm-pkg");
const generatedJs = path.join(outDir, "futureboard_core.js");
const generatedWasm = path.join(outDir, "futureboard_core_bg.wasm");
const release = !process.argv.includes("--debug");

function commandExists(command) {
  const probe = process.platform === "win32" ? "where" : "command";
  const args = process.platform === "win32" ? [command] : ["-v", command];
  const result = spawnSync(probe, args, { stdio: "ignore", shell: process.platform !== "win32" });
  return result.status === 0;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: workspaceRoot,
    stdio: "inherit",
    shell: false,
    ...options,
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function generatedPackageExists() {
  return fs.existsSync(generatedJs) && fs.existsSync(generatedWasm);
}

if (!fs.existsSync(coreRoot)) {
  console.warn(`[build-wasm-audio] SphereWebAudioCore not found: ${coreRoot}`);
  if (generatedPackageExists()) process.exit(0);
  process.exit(1);
}

if (!commandExists("wasm-pack")) {
  const message = "[build-wasm-audio] wasm-pack is not installed. Install with: cargo install wasm-pack --locked";
  if (generatedPackageExists()) {
    console.warn(`${message}\n[build-wasm-audio] Reusing existing generated package at ${outDir}`);
    process.exit(0);
  }
  console.error(message);
  process.exit(1);
}

run("rustup", ["target", "add", "wasm32-unknown-unknown"]);

fs.mkdirSync(outDir, { recursive: true });
run("wasm-pack", [
  "build",
  coreRoot,
  "--target",
  "web",
  ...(release ? ["--release"] : ["--dev"]),
  "--out-dir",
  outDir,
]);

if (!generatedPackageExists()) {
  console.error(`[build-wasm-audio] wasm-pack completed but expected files are missing in ${outDir}`);
  process.exit(1);
}

console.log(`[build-wasm-audio] ✓ Generated SphereWebAudioCore WASM → ${path.relative(workspaceRoot, outDir)}`);
