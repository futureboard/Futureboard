# Contributing to Futureboard Studio

[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](#-submission-checklist)
[![Rust](https://img.shields.io/badge/Rust-2024-ce422b?logo=rust&logoColor=white)](https://rustup.rs)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.x-3178c6?logo=typescript&logoColor=white)](https://www.typescriptlang.org)
[![Bun](https://img.shields.io/badge/Bun-runtime-fbf0df?logo=bun&logoColor=black)](https://bun.sh)
[![Code Style](https://img.shields.io/badge/style-rustfmt%20%2B%20clippy-blue)](#-submission-checklist)

Welcome to Futureboard Studio! We are thrilled to have you here. This document serves as the guide for contributing code, styling interfaces, and writing DSP algorithms for both the Web and Native builds of Futureboard.

Before starting development, please review this guide to ensure alignment with our development rules, styling paradigms, and real-time audio safety standards.

---

## 🧭 General Guidelines & Mindset

> [!IMPORTANT]
> **The Prime Directive: Work from the smallest safe scope.**
>
> 1. Avoid large-scale rewrites of components or subsystems unless explicitly requested.
> 2. Keep pull requests focused on a single feature, patch, or bug fix.
> 3. Preserve working behavior: always respect existing store, event, and layout structures.
> 4. Ensure all builds and checks pass locally before opening a pull request.

---

## 🏢 Tech Stack Directory Guide

Futureboard is organized as a monorepo containing multiple runtime environments and crates.

### 📱 Applications (`apps/`)

- [apps/web](apps/web) — The React/TypeScript Vite frontend. All components here should remain pure web components and rely on adapters rather than native bindings.
- [apps/native](apps/native) — The native application shell using **GPUI** and Rust, linking to native audio features.
- [apps/electron](apps/electron) — _Discontinued_ Electron environment. Only touch when debugging legacy code or maintaining the main process preload script.

### ⚙️ Core Crates (`crates/`)

- [crates/SphereWebAudioCore](crates/SphereWebAudioCore) — Web WASM Audio core. Rust-based DSP code that compiles into WASM for high-performance timeline playback in browser environments (using AudioWorklet).
- [crates/SphereDirectAudioEngine](crates/SphereDirectAudioEngine) — Native DAUx engine. Controls hardware and low-latency streams (WASAPI, CoreAudio, ALSA) and compiles as a `.node` addon via N-API.
- [crates/SphereUIComponents](crates/SphereUIComponents) — Native UI Kit and CoreUI layouts using GPUI, aligning with the designs outlined in `DESIGN.md`.
- [crates/SpherePluginHost](crates/SpherePluginHost) — Scanning/bridge layer hosting external VST3 and CLAP plugins via C++ wrappers.
- [crates/SphereAudioPlugins](crates/SphereAudioPlugins) — Stock DSP logic for standard inserts (EQ, Dynamics, Time-based effects).

---

## 🔇 Real-Time Audio Constraints (The Real-Time Thread)

Real-time audio callbacks operate under strict execution deadlines. Any delay will cause audible dropouts, pops, and clicks.

> [!CAUTION]
> **Real-time audio process loops must NEVER:**
>
> - Allocate or deallocate memory (no dynamic vectors, string allocations, or box instantiations).
> - Lock mutexes (use lock-free ring buffers / atomics instead).
> - Perform file system reads/writes or make network requests.
> - Invoke JavaScript callbacks, deserialize JSON data, or print debug logs (no `println!`, `info!`, or `console.log`).
> - Throw exceptions or panic.

**Allowed Operations in DSP Paths:**

- Reading and writing to pre-allocated buffers.
- Inter-thread coordination using lock-free single-producer single-consumer (SPSC) queues.
- Mutating parameter configurations via atomic variables (e.g. `AtomicF32`).

---

## 🎨 User Interface & Styling Rules

To ensure a seamless desktop DAW experience, all interface elements must share the unified Futureboard Studio design language.

### Theme Compliance

- Never use hardcoded hex values or arbitrary Tailwind colors.
- Utilize the semantic theme tokens defined in `theme.ts`:
  - `surface.base` / `surface.panel` / `surface.raised`
  - `border.subtle` / `border.strong`
  - `text.primary` / `text.secondary`
  - `accent.primary` / `status.success` / `status.error`

### Dialog and Modal Windows

- Always use the shared `DialogWindow` component.
- Windows must look like compact desktop editor panels (resembling Zed editor preferences or Ableton properties) with rounded corners, dark panel backgrounds, and high-density, compact spacing.
- Do not style windows like generic SaaS web cards or Bootstrap modals. Maintain a low text size (11–13px) and tabular numbers (`font-variant-numeric: tabular-nums`) for numeric data.
- For complete specifications, see [DESIGN.md](DESIGN.md) and [AGENTS.md](AGENTS.md).

---

## 💻 Language-Specific Code Rules

### TypeScript / React

- Write clean, explicit TypeScript interfaces. Avoid using parameter properties or typing variables as `any`.
- Keep components pure. React should never access files directly via Node’s `fs` or execute shell processes. Keep the UI environment decoupled and route native commands through adapter interfaces.
- Avoid triggering global re-renders on high-frequency visual updates (e.g. playhead progression or mixer VU metering). Visual meters should draw directly on Canvas elements.

### Rust

- Isolate `unsafe` logic and document all FFI invariants.
- Return clean `Result<T, E>` types; avoid panicking across the FFI boundaries.
- Avoid mutable global state.

### C++

- Wrap third-party plugin SDKs (like `vst3sdk`) behind an explicit, minimal C-ABI bridge to prevent leaking SDK headers into the Rust engine.
- Manage plugin instance lifecycles defensively to prevent host crashes.

---
### Get Source Code and Setup

This repo vendors native SDKs (`external/vst3sdk`, `external/clap`, …) as **git submodules**. Always clone recursively:

```bash
git clone --recursive https://github.com/futureboard/Futureboard
cd Futureboard
```

Already cloned without submodules? Initialize them:

```bash
git submodule update --init --recursive
```

#### Prerequisites

- [Bun](https://bun.sh) — JS/TS dependencies and task runner.
- [Rust](https://rustup.rs) 1.78+ (edition 2024). Add the web target: `rustup target add wasm32-unknown-unknown`.
- [CMake](https://cmake.org) 3.20+ and a C++ toolchain (MSVC on Windows, Xcode CLT on macOS, GCC/Clang + `libasound2-dev` on Linux) — required to compile the C++ plugin host.

#### Setup — WebUI

```bash
bun install
bun run dev:web
```

#### Setup — Desktop (Native GPUI client)

Development builds run through Cargo (wrapped by Bun scripts):

```bash
# debug run
bun run dev:native            # cargo run -p futureboard_native

# release build
bun run build:native          # cargo build --release -p futureboard_native
```

See the [Building the Native App](README.md#-building-the-native-app) section of the README for packaging and platform notes.

---

## 🔍 Debugging & Diagnostics

Set these environment variables to `1` to enable verbose subsystem logging while developing:

| Variable | Logs |
|---|---|
| `FUTUREBOARD_PLUGIN_DEBUG` | Insert add/set/remove/bypass mutations + engine-sync per-insert details |
| `FUTUREBOARD_PLUGIN_VIEW_DEBUG` | Native plugin editor lifecycle (`[plugin-view]` / `[vst3-editor]`): open → host region → child HWND → IPlugView attach/resize/detach |
| `FUTUREBOARD_ROUTING_DEBUG` | Send/return/bus routing graph at build time (nodes, sends, cycle ACCEPT/REJECT) |

```bash
FUTUREBOARD_PLUGIN_VIEW_DEBUG=1 cargo run -p futureboard_native
```

---

## 🌱 Commit & Pull Request Conventions

- Keep each PR focused on one feature, patch, or bug fix (see the Prime Directive above).
- Write present-tense, imperative commit subjects (e.g. `Fix VST3 editor child parenting`).
- Reference the relevant crate/app in the message body when scope isn't obvious.
- Rebase / keep history tidy; squash noisy WIP commits before review.
- Ensure all checks in the [Submission Checklist](#-submission-checklist) pass locally before opening the PR.

---

## 🔧 Submission Checklist

Before submitting a pull request, run validation commands to ensure code quality:

### Frontend Check (TypeScript/React)

```bash
bun run typecheck
bun run lint
bun run build:web
```

### Audio & Plugins Check

```bash
bun run build:audio:plugins
```

### Rust Engine Crate Tests

```bash
cargo check --workspace
cargo test --workspace
```

### Rust Lint & Format

```bash
bun run cargo:fmt:check   # cargo fmt --all -- --check
bun run cargo:clippy      # cargo clippy --workspace -- -D warnings
```

### Native Build Smoke Test

```bash
bun run build:native:debug   # cargo build -p futureboard_native
```
