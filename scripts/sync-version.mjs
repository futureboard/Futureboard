/**
 * sync-version.mjs
 *
 * Single source of truth: repoRoot/version.json
 * Sync targets:
 * - apps/electron/package.json
 * - apps/native/Cargo.toml
 *
 * Usage:
 *   node scripts/sync-version.mjs                    # sync from version.json
 *   node scripts/sync-version.mjs --check            # fail if out of sync
 *   node scripts/sync-version.mjs --version 1.2.3    # sync an explicit version
 *   node scripts/sync-version.mjs --version 1.2.3 --check
 */
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

// Resolve repo root from this script location, not from process.cwd(),
// so CI steps with different working directories still behave correctly.
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const checkOnly = process.argv.includes("--check");
const versionArgIdx = process.argv.indexOf("--version");
const versionOverride =
  versionArgIdx !== -1 ? process.argv[versionArgIdx + 1] : undefined;
if (versionArgIdx !== -1 && !versionOverride) {
  throw new Error("--version requires a value");
}

function readJson(p) {
  return JSON.parse(fs.readFileSync(p, "utf8"));
}

function writeJson(p, obj) {
  fs.writeFileSync(p, JSON.stringify(obj, null, 2) + "\n", "utf8");
}

function replaceTomlVersion(tomlText, newVersion) {
  const re = /^version\s*=\s*"(.*?)"\s*$/m;
  if (!re.test(tomlText)) {
    throw new Error(`No version = \"...\" line found`);
  }
  return tomlText.replace(re, `version = "${newVersion}"`);
}

const versionPath = path.join(repoRoot, "version.json");
if (!fs.existsSync(versionPath)) {
  throw new Error(`Missing ${versionPath}`);
}

const { version: jsonVersion } = readJson(versionPath);
const version = versionOverride ?? jsonVersion;
if (typeof version !== "string" || version.length < 1) {
  throw new Error(`Invalid version: expected a non-empty string`);
}

const targets = [
  {
    name: "apps/native/studio/Cargo.toml",
    path: path.join(repoRoot, "apps", "native", "studio", "Cargo.toml"),
    apply: (target) => {
      const currentText = fs.readFileSync(target.path, "utf8");
      const nextText = replaceTomlVersion(currentText, version);
      if (nextText !== currentText) {
        if (checkOnly) return { changed: true };
        fs.writeFileSync(target.path, nextText, "utf8");
        return { changed: true };
      }
      return { changed: false };
    },
  },
];

let dirty = false;
for (const target of targets) {
  const result = target.apply(target);
  if (result.changed) {
    dirty = true;
    const details =
      checkOnly && result.from !== undefined
        ? ` (${result.from} -> ${result.to})`
        : "";
    console.log(
      `[sync-version] ${checkOnly ? "out of sync" : "updated"}: ${target.name}${details}`,
    );
  } else {
    console.log(`[sync-version] ok: ${target.name}`);
  }
}

if (checkOnly && dirty) {
  console.error(
    `[sync-version] ERROR: version mismatch. Run: node scripts/sync-version.mjs`,
  );
  process.exit(1);
}
