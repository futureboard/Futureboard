<img width="2111" height="684" alt="banner_ft" src="https://github.com/user-attachments/assets/aa5916cb-1e47-4fe8-a6c3-43099a38ee95" />

# Futureboard Studio

**Futureboard Studio** is a professional Digital Audio Workstation (DAW) built around a modern, hybrid web-native stack. It is designed to start directly in the browser, scale into Electron, and share its layout and core engine workflows with a native Rust GPUI application.

<!-- Badges -->
[![License: MIT](https://img.shields.io/badge/License-MIT-22c55e.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2024-ce422b?logo=rust&logoColor=white)](https://rustup.rs)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.x-3178c6?logo=typescript&logoColor=white)](https://www.typescriptlang.org)
[![Bun](https://img.shields.io/badge/Bun-runtime-fbf0df?logo=bun&logoColor=black)](https://bun.sh)
[![WebAssembly](https://img.shields.io/badge/WebAssembly-DSP-654ff0?logo=webassembly&logoColor=white)](https://webassembly.org)
[![GPUI](https://img.shields.io/badge/UI-GPUI-1f6feb)](https://www.gpui.rs)
[![VST3](https://img.shields.io/badge/Plugins-VST3-ff7a00)](https://steinbergmedia.github.io/vst3_dev_portal/)
[![CLAP](https://img.shields.io/badge/Plugins-CLAP-8957e5)](https://cleveraudio.org)
[![Platforms](https://img.shields.io/badge/Platforms-Windows%20%7C%20macOS%20%7C%20Linux%20%7C%20Web-0ea5e9)](#-getting-started)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

---

## 📑 Table of Contents

- [Architectural Overview](#-architectural-overview)
- [Core Engines & Frameworks](#️-core-engines--frameworks-crates)
- [Additional Packages & Extensions](#️-additional-packages--extensions)
- [Getting Started](#-getting-started)
- [Building the Native App](#-building-the-native-app)
- [npm / Bun Scripts Reference](#-bun-scripts-reference)
- [Debugging & Diagnostics](#-debugging--diagnostics)
- [Contributing](#-contributing)
- [License](#-license)

---

## 🚀 Architectural Overview

Futureboard Studio is split into active modular directories targeting different runtime environments:

### 📱 Applications (`apps/`)

- **Web Version (`apps/web`)**
  - An interactive React + TypeScript + Vite single-page application.
  - Leverages the high-performance WASM AudioWorklet fallback for audio processing in sandboxed web environments.
- **Native Version (`apps/native`)**
  - A high-performance desktop shell built on Rust using the **GPUI** framework (the layout/rendering engine behind the Zed editor).
  - Integrates natively with our Rust direct audio engine without Electron or browser-engine overhead.
- **Electron Version (`apps/electron`)** — _Discontinued_
  - A desktop wrapper linking the React-based frontend with native audio components via an N-API control bridge.
  - Maintained as legacy reference code.
- **Server Module (`apps/server`)**
  - Collaboration stream sync and file hosting server.

---

## ⚙️ Core Engines & Frameworks (`crates/`)

The core DAW logic, DSP, and user interface kit are written in modular Rust and C++ crates:

- [SphereWebAudioCore](crates/SphereWebAudioCore)
  - **Purpose**: Web WASM Audio core.
  - **Details**: Provides the web-compatible implementation of the DAW’s transport, flat audio graph (tracks → master), mixer, and meters, compiled to WebAssembly for browser runs.
- [SphereDirectAudioEngine](crates/SphereDirectAudioEngine)
  - **Purpose**: Native DAUx engine.
  - **Details**: Low-latency direct audio engine for desktop builds. Interfaces directly with cpal and system-level API targets (WASAPI with exclusive mode + MMCSS on Windows, CoreAudio on macOS, ALSA on Linux). Exposes a C/Rust native API as well as an N-API/Node wrapper for JavaScript IPC.
- [SphereUIComponents](crates/SphereUIComponents)
  - **Purpose**: Native UI Kit & CoreUI.
  - **Details**: Futureboard Studio's shared desktop components and styling system built in Rust utilizing **GPUI** and Skia.
- [SpherePluginHost](crates/SpherePluginHost)
  - **Purpose**: Plugin hosting wrapper.
  - **Details**: Connects raw SDK interfaces in C++ (`external/vst3sdk`, `external/clap`) with the Rust ecosystem to allow scanning and hosting of native VST3 and CLAP plugins. The native plugin editor is embedded directly into the GPUI window (no separate floating editor).
- [SphereAudioPlugins](crates/SphereAudioPlugins)
  - **Purpose**: Built-in audio plugin DSP.
  - **Details**: Contains the realtime-safe DSP code and parameters for stock insert effects (EQ, compression, delay).

---

## 🛠️ Additional Packages & Extensions

- [plugins/](plugins/) — Web/React UI and DSP editors for stock plugins (e.g., `Equz8`, `FB2AComp`, `UltraVerb`).
- [modules/](modules/) — High-level companion processors (e.g., `NoiseRemover`, `StemExtractor`).
- [extentions/](extentions/) — Extension templates (`template`, `template-react`, `template-vue`) for building customizable DAW extensions.
- [packages/shared/](packages/shared/) — Shared fonts, icons, menus, and layout manifests.
- [external/](external/) — Vendored SDKs and native dependencies (`vst3sdk`, `clap`, `yoga`, `ARA_SDK`) pulled in as git submodules.

---

## 📦 Getting Started

### Prerequisites

You need the following installed:

| Tool | Version | Used for |
|---|---|---|
| [Bun](https://bun.sh) | latest | JS/TS package manager, bundler and task runner |
| [Rust](https://rustup.rs) | 1.78+ (edition 2024) | Native app, audio engine, WASM DSP |
| `wasm32-unknown-unknown` target | — | Web audio core (`rustup target add wasm32-unknown-unknown`) |
| [CMake](https://cmake.org) | 3.20+ | Compiling the C++ plugin host + VST3 SDK |
| A C++ toolchain | — | MSVC (Windows), Xcode CLT (macOS), GCC/Clang (Linux) |

> [!IMPORTANT]
> This repository uses **git submodules** for vendored SDKs (`external/vst3sdk`, `external/clap`, …). Clone with `--recursive`, or run `git submodule update --init --recursive` after cloning.

### Clone

```bash
git clone --recursive https://github.com/futureboard/Futureboard
cd Futureboard
```

Already cloned without submodules?

```bash
git submodule update --init --recursive
```

### Install JS workspace dependencies

```bash
bun install
```

### Run the Web Version

```bash
bun run dev:web
```

### Run the Native GPUI Client

```bash
bun run dev:native
# equivalent to: cargo run -p futureboard_native
```

### Run the Collaboration Server

```bash
bun run dev:server
```

---

## 🖥️ Building the Native App

The native desktop client is a Rust binary (`futureboard_native`) that links the GPUI UI kit, the direct audio engine, and the C++ VST3/CLAP plugin host. CMake and a C++ toolchain are required because the plugin host SDKs are compiled from source.

### Debug build & run

```bash
bun run build:native:debug   # cargo build -p futureboard_native
bun run dev:native           # cargo run -p futureboard_native
```

### Release build

```bash
bun run build:native         # cargo build --release -p futureboard_native
```

The optimized binary is emitted to `target/release/futureboard_native` (`.exe` on Windows).

### Package distributable bundles

```bash
# macOS .app bundle
bun run bundle:native:mac

# macOS .app + .dmg installer
bun run bundle:native:mac:dmg

# Windows portable/installer layout
bun run bundle:native:win
```

Packaging scripts live in [`packaging/native/`](packaging/native).

### Platform notes

- **Windows** — Uses WASAPI (exclusive mode + MMCSS) for low-latency audio. Build with the MSVC toolchain (`rustup default stable-msvc`). The VST3 native editor is embedded as a `WS_CHILD` inside the GPUI window.
- **macOS** — Uses CoreAudio. Requires Xcode Command Line Tools (`xcode-select --install`).
- **Linux** — Uses ALSA. Install ALSA dev headers (e.g. `sudo apt install libasound2-dev`) plus the usual GPUI system deps.

### Build everything (WASM + native + electron)

```bash
bun run build:all
```

---

## 🧰 Bun Scripts Reference

| Script | Description |
|---|---|
| `dev:web` | Run the React web app (Vite dev server) |
| `dev:native` | Build & run the native GPUI client |
| `dev:server` | Run the collaboration sync server |
| `dev:electron` | Run web + Electron (legacy) concurrently |
| `build:web` | Production build of the web app |
| `build:wasm` | Compile the WASM audio core for the web |
| `build:native` | Release build of the native client |
| `build:native:debug` | Debug build of the native client |
| `build:audio:plugins` | Check stock plugin crate + extension template |
| `bundle:native:mac` / `:mac:dmg` / `win` | Package native distributables |
| `cargo:check` / `cargo:build` / `cargo:release` | Workspace cargo build variants |
| `cargo:test` | Run the Rust workspace test suite |
| `cargo:clippy` / `cargo:fmt` / `cargo:fmt:check` | Lint & format Rust code |
| `check` | `cargo:check` + web lint |
| `lint` | `cargo:clippy` + web lint |
| `fmt` | `cargo fmt --all` |

---

## 🔍 Debugging & Diagnostics

Several subsystems expose verbose logging behind environment variables (set to `1` to enable):

| Variable | Logs |
|---|---|
| `FUTUREBOARD_PLUGIN_DEBUG` | Insert add/set/remove/bypass mutations and engine-sync per-insert details |
| `FUTUREBOARD_PLUGIN_VIEW_DEBUG` | Native plugin editor lifecycle: open → host region → child HWND → IPlugView attach/resize/detach (`[plugin-view]` / `[vst3-editor]`) |
| `FUTUREBOARD_ROUTING_DEBUG` | Send/return/bus routing graph at build time (nodes, sends, cycle ACCEPT/REJECT) |

Example (PowerShell):

```powershell
$env:FUTUREBOARD_PLUGIN_VIEW_DEBUG=1; cargo run -p futureboard_native
```

Example (bash):

```bash
FUTUREBOARD_PLUGIN_VIEW_DEBUG=1 cargo run -p futureboard_native
```

---

## 🤝 Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for the development rules, real-time audio safety constraints, UI/theming guidelines, and the pre-PR submission checklist. UI work should also follow [DESIGN.md](DESIGN.md) and [AGENTS.md](AGENTS.md).

---

## 📄 License

MIT License. See [LICENSE](LICENSE) for the full license text.
