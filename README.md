<img width="2111" height="684" alt="banner_ft" src="https://github.com/user-attachments/assets/aa5916cb-1e47-4fe8-a6c3-43099a38ee95" />

# Futureboard Studio

**Futureboard Studio** is a professional Digital Audio Workstation (DAW) built around a modern, hybrid web-native stack. It is designed to start directly in the browser, scale into Electron, and share its layout and core engine workflows with a native Rust GPUI application.

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
  - **Details**: Connects raw SDK interfaces in C++ (`external/vst3sdk`, `external/clap`) with the Rust ecosystem to allow scanning and hosting of native VST3 and CLAP plugins.
- [SphereAudioPlugins](crates/SphereAudioPlugins)
  - **Purpose**: Built-in audio plugin DSP.
  - **Details**: Contains the realtime-safe DSP code and parameters for stock insert effects (EQ, compression, delay).

---

## 🛠️ Additional Packages & Extensions

- [plugins/](plugins/) — Web/React UI and DSP editors for stock plugins (e.g., `Equz8`, `FB2AComp`, `UltraVerb`).
- [modules/](modules/) — High-level companion processors (e.g., `NoiseRemover`, `StemExtractor`).
- [extentions/](extentions/) — Extension templates (`template`, `template-react`, `template-vue`) for building customizable DAW extensions.
- [packages/shared/](packages/shared/) — Shared fonts, icons, menus, and layout manifests.

---

## 📦 Getting Started

### Prerequisites

You need the following installed:

- [Bun](https://bun.sh) (fast package manager and bundler for JS/TS)
- [Rust](https://rustup.rs) (1.75+ with WebAssembly target `wasm32-unknown-unknown`)
- [CMake](https://cmake.org) (required to compile native plugin wrappers and the VST3 SDK)

### Quick Start Commands

Install JS workspace dependencies:

```bash
bun install
```

#### Run Web Version

To run the React web application locally:

```bash
bun run dev:web
```

#### Run Native GPUI Client

To compile and run the native desktop shell:

```bash
cargo run -p futureboard_native
```

#### Build Built-in Audio Plugins Crate

```bash
bun run build:audio:plugins
```

---

## 🎨 Design Language

Futureboard Studio prioritizes a high-density, professional visual layout (similar to Ableton Live and Zed Editor).

- All dialogue windows, widgets, and preferences should reuse design elements specified in [DESIGN.md](DESIGN.md).
- Avoid standard Bootstrap or generic Tailwind configurations. Stick to the curated DAW dark color palette tokens.

---

## 📄 License

MIT License. See [LICENSE](LICENSE) for the full license text.
