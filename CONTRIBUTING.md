# Contributing to Futureboard Studio

[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](#-submission-checklist)
[![Rust](https://img.shields.io/badge/Rust-2024-ce422b?logo=rust&logoColor=white)](https://rustup.rs)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.x-3178c6?logo=typescript&logoColor=white)](https://www.typescriptlang.org)
[![Bun](https://img.shields.io/badge/Bun-runtime-fbf0df?logo=bun&logoColor=black)](https://bun.sh)
[![Code Style](https://img.shields.io/badge/style-rustfmt%20%2B%20clippy-blue)](#-submission-checklist)

Thanks for your interest in contributing! This document covers the **rules and conventions** specific to working on Futureboard Studio. By participating, you agree to our [Code of Conduct](CODE_OF_CONDUCT.md).

> For environment setup, build/run commands, and platform notes, see the [README](README.md#getting-started). This guide assumes you can already build the project.

---

## 🧭 General Guidelines & Mindset

> [!IMPORTANT]
> **The Prime Directive: work from the smallest safe scope.**
>
> 1. Avoid large-scale rewrites of components or subsystems unless explicitly requested.
> 2. Keep each pull request focused on a single feature, patch, or bug fix.
> 3. Preserve working behavior — respect existing store, event, and layout structures.
> 4. Ensure all builds and checks pass locally before opening a pull request.

---

## 🏢 Where Things Live

Futureboard is a monorepo of multiple runtime surfaces and crates.

**Applications (`apps/`)**

- [`apps/native`](apps/native) — the primary native app (Rust + **GPUI**); main target for new work.
- [`apps/web`](apps/web) — the React/TypeScript/Vite frontend; keep components pure web (route native calls through adapters).
- [`apps/electron`](apps/electron) — _discontinued_ wrapper; touch only for legacy debugging / preload maintenance.

**Core crates (`crates/`)**

- [`SphereWebAudioCore`](crates/SphereWebAudioCore) — web WASM audio core (DSP compiled to WASM, runs in an AudioWorklet).
- [`SphereDirectAudioEngine`](crates/SphereDirectAudioEngine) — native low-latency engine driving hardware streams (WASAPI / CoreAudio / ALSA); also builds an N-API addon (`DAUx`) for the Electron bridge.
- [`SphereUIComponents`](crates/SphereUIComponents) — native GPUI UI kit and layouts (follows [DESIGN.md](DESIGN.md)).
- [`SpherePluginHost`](crates/SpherePluginHost) — scanning/host bridge for external VST3 and CLAP plugins via C++ wrappers.
- [`SphereAudioPlugins`](crates/SphereAudioPlugins) — stock insert DSP (EQ, dynamics, time-based effects).

---

## 🔇 Real-Time Audio Constraints

Real-time audio callbacks run under strict deadlines — any delay causes audible dropouts, pops, and clicks.

> [!CAUTION]
> **Real-time DSP / audio-callback paths must NEVER:**
>
> - Allocate or free memory (no dynamic vectors, string allocations, or boxing).
> - Lock mutexes (use lock-free SPSC ring buffers / atomics instead).
> - Do filesystem I/O or network requests.
> - Invoke JS callbacks, parse JSON, or log (`println!`, `info!`, `console.log`).
> - Throw exceptions or panic.

**Allowed in DSP paths:** reading/writing pre-allocated buffers, lock-free SPSC queues for cross-thread coordination, and atomic parameter updates (e.g. `AtomicF32`).

---

## 🎨 UI & Styling Rules

All interface elements share the unified Futureboard Studio design language. Full specs: [DESIGN.md](DESIGN.md) and [AGENTS.md](AGENTS.md).

- **Theme tokens only** — never hardcode hex values or arbitrary Tailwind colors. Use the semantic tokens (`surface.*`, `border.*`, `text.*`, `accent.*`, `status.*`).
- **Dialogs** — always use the shared `DialogWindow`. Windows must look like compact desktop editor panels (Zed-/Ableton-style: dark panels, subtle borders, 11–13px text, tabular numbers for numeric data) — not SaaS web cards or Bootstrap modals.
- **No per-frame global re-renders** — high-frequency visuals (playhead, VU meters) draw directly to Canvas/GPUI, not through reactive state.

---

## 💻 Language-Specific Code Rules

**TypeScript / React** — explicit interfaces (no `any`, no parameter properties); keep components pure (no `fs`/shell from the UI — route through adapters).

**Rust** — isolate and document `unsafe`/FFI invariants; return clean `Result<T, E>` and never panic across FFI; avoid mutable global state.

**C++** — wrap third-party SDKs (e.g. `vst3sdk`) behind a minimal C-ABI bridge so SDK headers never leak into the Rust engine; manage plugin lifecycles defensively to avoid host crashes.

---

## 🌱 Commit & Pull Request Conventions

- One focused PR per feature, patch, or bug fix (see the Prime Directive).
- Present-tense, imperative commit subjects (e.g. `Fix VST3 editor child parenting`); name the relevant crate/app in the body when scope isn't obvious.
- Keep history tidy — squash noisy WIP commits before review.
- All checks in the [Submission Checklist](#-submission-checklist) must pass locally before opening the PR.

---

## 🔧 Submission Checklist

Run the validation relevant to what you changed:

```bash
# Frontend (TypeScript / React)
bun run typecheck && bun run lint && bun run build:web

# Audio & stock plugins
bun run build:audio:plugins

# Rust workspace
cargo check --workspace && cargo test --workspace
bun run cargo:fmt:check   # cargo fmt --all -- --check
bun run cargo:clippy      # cargo clippy --workspace -- -D warnings

# Native build smoke test
bun run build:native:debug
```
