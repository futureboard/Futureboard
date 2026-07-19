# xtask — build chaining & binary packaging

`xtask` is the Futureboard workspace task runner. It has two jobs:

- **`build-all` / `check-all`** — chain the per-edition cargo aliases
  (`build-ce`, `build-exclusive-win`, …) from `.cargo/config.toml`, because
  Cargo aliases cannot chain commands.
- **`package`** — build `FutureboardNative` and stage a clean, runnable
  application tree into `out/`, kept separate from the Cargo `target/` cache.

## `target/` vs `out/`

| Directory | Purpose | Committed? |
| --------- | ------- | ---------- |
| `target/` | Cargo compiler cache and intermediate output (`.pdb`, `.rlib`, `deps/`, `incremental/`, the linked binary). Owned by Cargo. | No |
| `out/`    | Distributable application layout — only the files a user needs to run the app. Produced by `xtask package`. | No |

`out/` and `target/` are both git-ignored. Packaging **never** copies the Cargo
target tree wholesale; it copies only the executable, known runtime sibling
libraries, the generated `Plugins/`/`Resources/` directories, and
`build-info.json`.

## Why packaging is not in `build.rs`

`build.rs` runs inside *every* compilation and must stay hermetic and fast.
Packaging is an explicit, post-build workflow that copies files, writes
metadata, validates the result, and atomically publishes it. Mixing that into
`build.rs` would run it on every incremental compile and couple the compiler
cache to the distributable layout. Packaging therefore lives in `xtask`.

## Output layout

```text
out/
├─ dev/
│  └─ windows-x64/                    # cargo package-dev
│     ├─ FutureboardNative.exe
│     ├─ FutureboardPluginHostX64.exe # out-of-process plugin/editor host (spawned by the app)
│     ├─ FutureboardPluginScanner.exe # isolated plugin scanner (spawned by the app)
│     ├─ onnxruntime.dll              # staged only if present beside the binary
│     ├─ Plugins/                     # empty for now; future binary plugins land here
│     ├─ Resources/
│     └─ build-info.json
└─ release/
   ├─ community/
   │  └─ windows-x64/         # cargo package-ce
   └─ exclusive/
      └─ windows-x64/         # cargo package-exclusive
```

- `dev` profile → `out/dev/<platform>` (edition omitted).
- any other profile → `out/<profile>/<edition>/<platform>`.

Target triples map to readable platform folders
(`x86_64-pc-windows-msvc → windows-x64`, `aarch64-apple-darwin → macos-arm64`,
etc.). Unknown triples are normalized to a safe slug instead of panicking.

## Creating packages (Windows PowerShell)

Development package (fast, unoptimized):

```powershell
cargo package-dev
# equivalent to:
cargo run -p xtask -- package --profile dev --edition community
```

Community release:

```powershell
cargo package-ce
# equivalent to:
cargo run -p xtask -- package `
  --profile release `
  --target x86_64-pc-windows-msvc `
  --edition community
```

Exclusive release (requires the private `crates/ExclusiveEdition/` source tree):

```powershell
cargo package-exclusive
# equivalent to:
cargo run -p xtask -- package `
  --profile release `
  --target x86_64-pc-windows-msvc `
  --edition exclusive
```

Optional flags:

- `--target <triple>` — cross to another platform folder (default: host triple).
- `--out <dir>` — root output directory (default: `out`).
- `--symbols` — also copy the `.pdb` into a separate `symbols/` directory.

## How the binary path is discovered

The packager never assumes `target/release/FutureboardNative.exe`. It runs

```text
cargo build --message-format=json-render-diagnostics ...
```

and parses the `compiler-artifact` JSON messages to read the exact executable
path Cargo produced. This works across custom target triples, profiles, and the
per-edition target directories.

## Adding runtime files to the staging manifest

Runtime files are staged explicitly, not by scraping `target/`:

- **Sidecar executables** — `FutureboardNative` spawns
  `FutureboardPluginHostX64.exe` and `FutureboardPluginScanner.exe`, resolving
  them next to its own binary at runtime. They are separate `[[bin]]` targets of
  the `sphere-plugin-host` package, built in the same cargo invocation and
  staged beside the app. Add more via `SIDECAR_BINARIES` in `src/cargo_build.rs`.
- **Sibling shared libraries** — add names to `RUNTIME_SIBLING_LIBS` in
  `src/staging.rs`. They are staged only when found next to the built binary
  (this is how `onnxruntime.dll` is picked up).
- **Resource files** — copy them into the `Resources/` directory during staging
  (extend `create_layout_dirs` / add a copy step in `src/package.rs`).
- **Binary plugins** — the empty `Plugins/` directory is reserved for future
  binary plugins. Nothing is staged there yet.

## build-info.json

Written into every package:

```json
{
  "schemaVersion": 1,
  "application": "Futureboard Studio",
  "binary": "FutureboardNative.exe",
  "edition": "community",
  "profile": "release",
  "target": "x86_64-pc-windows-msvc",
  "platform": "windows-x64",
  "version": "2026.7.2",
  "gitCommit": "…",
  "gitDirty": false,
  "builtAtUtc": "2026-07-19T12:34:56.789+00:00",
  "rustcVersion": "rustc …",
  "cargoVersion": "cargo …"
}
```

`version` comes from the `futureboard_native` package version. Git and toolchain
fields are best-effort — a missing `git`/toolchain yields `null` rather than
failing the package.

## Safe publishing

```text
build → collect artifacts → create staging → copy files → validate → publish
```

Staging happens in `out/.staging/<platform>-<edition>-<profile>/`. Only after
validation passes is the staging directory renamed into its final location
(moving any previous package aside first, restored on failure). A failed package
never leaves `out/` half-updated, and unrelated directories under `out/` are
never touched.
