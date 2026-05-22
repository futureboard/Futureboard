import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const electronRoot = path.resolve(__dirname, "..");
const workspaceRoot = path.resolve(electronRoot, "..", "..");
const hostRoot = path.join(workspaceRoot, "frameworks", "SpherePluginHost");
const debug = process.argv.includes("--debug");
const cargoArgs = ["build", ...(debug ? [] : ["--release"] )];

function run(command, args, options = {}) {
  return spawnSync(command, args, {
    cwd: hostRoot,
    stdio: "inherit",
    shell: false,
    ...options,
  });
}

function hasMsvcCompiler() {
  if (process.platform !== "win32") return true;
  const result = spawnSync("where", ["cl"], { stdio: "ignore", shell: false });
  return result.status === 0;
}

/**
 * Locate vswhere.exe.
 * Search order:
 *   1. .bin/vswhere.exe  — downloaded by CI or a local developer setup script
 *   2. The copy bundled with the VS Installer (standard machine installation)
 */
function findVsWhere() {
  const candidates = [
    path.join(workspaceRoot, ".bin", "vswhere.exe"),
    "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\vswhere.exe",
  ];
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  throw new Error(
    "[build-plugin-host] vswhere.exe not found.\n" +
    "  • In CI it is downloaded to .bin/vswhere.exe — check the workflow step.\n" +
    "  • Locally, install Visual Studio (any edition) or run:\n" +
    "    Invoke-WebRequest https://github.com/microsoft/vswhere/releases/latest/download/vswhere.exe -OutFile .bin\\vswhere.exe"
  );
}

/**
 * Use vswhere.exe to locate the Visual Studio installation path and instance ID
 * dynamically, so the script works on any machine regardless of VS year or edition.
 */
function findVsDevShell() {
  const vswhere = findVsWhere();
  console.log(`[build-plugin-host] Using vswhere: ${vswhere}`);

  const installPath = spawnSync(
    vswhere,
    ["-latest", "-property", "installationPath"],
    { encoding: "utf8", shell: false }
  ).stdout.trim();

  const instanceId = spawnSync(
    vswhere,
    ["-latest", "-property", "instanceId"],
    { encoding: "utf8", shell: false }
  ).stdout.trim();

  if (!installPath || !instanceId) {
    throw new Error(
      "[build-plugin-host] vswhere.exe could not locate a Visual Studio installation. " +
        "Make sure Visual Studio (with the C++ workload) is installed."
    );
  }

  console.log(`[build-plugin-host] VS installationPath: ${installPath}`);
  console.log(`[build-plugin-host] VS instanceId:       ${instanceId}`);

  const devShellModule = `${installPath}\\Common7\\Tools\\Microsoft.VisualStudio.DevShell.dll`;
  return { devShellModule, instanceId };
}

let result;
if (process.platform === "win32" && !hasMsvcCompiler()) {
  const powershell = "C:\\Windows\\SysWOW64\\WindowsPowerShell\\v1.0\\powershell.exe";
  const { devShellModule, instanceId } = findVsDevShell();
  const cargoCommand = `cargo ${cargoArgs.join(" ")}`;
  const script = `&{Import-Module "${devShellModule}"; Enter-VsDevShell ${instanceId}; ${cargoCommand}; exit $LASTEXITCODE}`;
  console.log("[build-plugin-host] MSVC cl.exe not found; entering Visual Studio DevShell before cargo build.");
  result = spawnSync(powershell, ["-noe", "-c", script], {
    cwd: hostRoot,
    stdio: "inherit",
    shell: false,
  });
} else {
  result = run("cargo", cargoArgs);
}

process.exit(result.status ?? 1);
