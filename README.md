<img width="2111" height="684" alt="banner_ft" src="https://github.com/user-attachments/assets/aa5916cb-1e47-4fe8-a6c3-43099a38ee95" />

# Futureboard Studio

**Futureboard Studio** is a WebUI-based DAW built to start in the browser, scale into Electron, and eventually share its workflow with a native C++/Skia runtime in future.

The goal is simple:

> Build a serious DAW that feels like a desktop editor, runs with a WebUI workflow, and can use native audio when the browser is no longer enough.

Futureboard is not meant to be a toy browser DAW. It is designed around client-side processing, native desktop integration, folder-based projects, low-latency playback, extensible plugins, and a scalable render architecture.

---

## Current Status

Futureboard Studio is in active development.

Working / in progress:

- React / Vite WebUI
- Electron desktop client
- `miko://app/index.html` local ASAR loading
- Folder-based project workflow
- Native Rust audio addon loading
- Native metronome output
- Electron file browser with drive roots
- Project indexing
- Waveform rendering and peak cache system
- Track arrangement UI
- Mixer / inserts / effect editor UI
- Built-in plugin package structure
- Initial plugin manifest direction
- DAUx native audio backend direction

Known unstable areas:

- Native arrangement track playback
- Auto Import to Project
- Native snapshot media path resolution
- Pitch / speed processing
- Chunked waveform peak rendering for very long files
- Reverb / delay realtime parameter update path
- VST3 plugin host (in plan)
- Full audio graph / routing / bus / return handling

---

## Runtime Direction

```txt
Web Version
= React / Vite
= WASM audio processing
= browser-safe storage/cache
= lightweight projects and cloud entry point

Electron Client
= WebUI desktop shell
= local folder projects
= native file browser
= DAUx native audio engine
= native addons
= low-latency desktop workflow

Native Future (in Q2 2027)
= C++ / Skia / SphereEngine
= V8 + Yoga UI runtime
= native plugin/editor architecture
= same DAW concepts, deeper native performance
```

Current audio rule:

```txt
Electron = DAUx: OS Audio Backend
Web      = WASM only
```

This avoids confusing states where Electron looks native but silently routes playback through old Web/WASM paths.

---

## Core Architecture

```txt
apps/
├─ web/
│  └─ React/Vite WebUI
│
├─ electron/
│  └─ Electron desktop client
│
├─ native/
│  ├─ include/
│  ├─ src/
│  └─ CMakeLists.txt
│
frameworks/
├─ SphereDirectAudioEngine/
│  └─ Rust Audio Engine for Electron
│
└─ SphereWebAudioCore/
   └─ Rust/WASM web audio core

plugins/
└─ Equz8/
   ├─ Core/
   ├─ Editor/
   └─ package.json
```

---

## Electron App Loading

Packaged Electron loads the bundled WebUI through a custom protocol:

```txt
miko://app/index.html
```

The WebUI is packed into `app.asar`.

Expected flow:

```txt
Electron packaged app
→ register miko:// protocol
→ serve dist/index.html, JS, CSS, WASM, fonts, images
→ load miko://app/index.html
```

The hosted Web version remains separate.

```txt
Web version     = hosted web app
Electron client = bundled local ASAR app
```

---

## Native Audio Direction

Native addons:

```txt
DAUx.node
= audio device
= transport
= mixer
= routing
= realtime DSP graph
= metronome
= native project playback

DAUxPluginHost.node (in plan)
= VST3 / plugin scanning
= plugin loading
= parameter bridge
= plugin processing
= native editor bridge later
```

The current native audio issue is not the output callback. Metronome already proves native output works.

The critical path is:

```txt
Project clips
→ asset manifest
→ resolved mediaPath
→ native snapshot
→ Rust runtime clips
→ arrangement playback
```

---

## DAUx

DAUx is the planned native low-latency backend layer.

Target backends:

```txt
Windows
- WASAPI
- MME fallback only

macOS
- CoreAudio

Linux
- ALSA
```

Future optional backends:

```txt
ASIO
JACK
PipeWire
```

DAUx rules:

- no allocations in audio callback
- no locks in audio callback
- no filesystem access in audio callback
- no JSON parsing in audio callback
- no plugin scanning/loading in audio callback
- use lock-free command queues
- use immutable graph snapshots
- expose backend/device/buffer/latency/glitch status

---

## Project Format

Electron desktop mode uses folder-based projects.

```txt
<Project Name>/
├─ Cache/
│  ├─ Peaks/
│  ├─ Waveform/
│  ├─ Processed/
│  └─ Analysis/
│
├─ Media/
│  ├─ Audio/
│  ├─ MIDI/
│  ├─ Samples/
│  └─ Imports/
│
├─ Rendered/
│  ├─ Mixdowns/
│  ├─ Stems/
│  └─ Bounces/
│
└─ <Project Name>.mochiproj
```

Rules:

- `Media/` is persistent.
- `Cache/` is regeneratable.
- `Rendered/` is user output.
- `.mochiproj` stores project metadata and relative paths.
- Audio files must not be embedded into `.mochiproj`.
- Runtime `File`, `Blob`, `AudioBuffer`, or decoded buffers must not be the source of truth.

---

## Auto Import to Project

In Electron folder-project mode, any audio file added to the arrangement must be imported into the project package first.

Correct flow:

```txt
Drop / Import Audio
→ copy into <Project>/Media/Audio/
→ create asset manifest entry
→ create clip with assetId
→ generate/request waveform peak cache
→ save/update .mochiproj
→ send native snapshot with mediaPath
→ SphereAudio schedules clip
```

No Electron audio clip should depend only on a temporary runtime file reference.

---

## Native Snapshot Rule

Native Rust playback requires each audio clip to resolve to a real filesystem path.

Expected native clip snapshot:

```ts
{
  id: string;
  trackId: string;
  assetId: string;
  mediaPath: string;
  startBeat: number;
  durationBeats: number;
  offsetSeconds?: number;
  gain: number;
  mute?: boolean;
}
```

Bad:

```txt
mediaPath: null
mediaPath: ""
mediaPath: miko://...
mediaPath: blob:...
```

Good:

```txt
H:/Projects/My Song/Media/Audio/vocal.wav
```

If SphereAudio logs this:

```txt
1 clips (0 with paths)
all clips have null/empty mediaPath
RuntimeProject built 0 runtime clips
StartTransport: 0 clips scheduled
```

then Rust Audio is not the problem. The snapshot/media resolver is broken.

---

## Schema Compatibility Note

Current project schema may use:

```txt
files[]
```

while the native snapshot expects:

```txt
assets[]
asset.relativePath
clip.assetId
```

If clips currently have:

```txt
clip.fileId
```

then native snapshot generation must support this mapping:

```txt
clip.fileId
→ project.files[]
→ file.relativePath / path
→ projectRoot + relative path
→ clip.mediaPath
```

Recommended fix:

```txt
normalize files[] into assets[]
OR
make native snapshot resolver support both assets[] and files[]
```

Resolver priority:

```txt
clip.assetId ?? clip.fileId ?? clip.sourceId ?? clip.importId
```

Search order:

```txt
project.assets first
project.files second
```

---

## File Browser

Electron File Browser supports native filesystem browsing.

Target browser tree:

```txt
Browser Tree
├─ Quick Access
├─ Futureboard Studio
│  ├─ Loops
│  ├─ Presets
│  ├─ Samples
│  └─ Templates
├─ Drives
│  ├─ C:
│  ├─ D:
│  └─ E:
├─ Current Project
│  ├─ Media
│  ├─ Cache
│  └─ Rendered
└─ Imports
```

File Browser requirements:

- compact DAW/editor panel
- drive roots in Electron
- browser-safe behavior in Web
- audio metadata badges
- cached/imported/missing states
- drag/import uses Auto Import to Project
- indexing status uses spinner/loading UX, not noisy numeric counters
- detailed indexing counts belong in Developer/Debug only

---

## Waveform and Peak Cache

Waveform rendering must not be whole-file based.

Long files such as 2–3 hour recordings must use chunked progressive rendering.

Concept:

```txt
Waveform = tiled map rendering

visible chunks first
nearby chunks next
background chunks later
cache everything
draw only visible range
```

Electron cache layout:

```txt
Cache/Peaks/<assetId>/
├─ meta.json
├─ ch0_spp8192_chunk000000.bin
├─ ch0_spp8192_chunk000001.bin
└─ ...
```

Rules:

- no DOM waveform spam
- no full-file redraw
- no blocking UI during peak generation
- coarse chunks first
- finer chunks on zoom
- placeholder while missing
- cache is regeneratable

---

## Pitch / Speed

Pitch and speed processing is under active work.

Required shape:

```ts
clip.audioProcess = {
  speedRatio,
  pitchSemitones,
  preservePitch,
  mode,
  quality,
};
```

Rules:

- speed changes effective clip duration
- pitch with preserve pitch should not change visual duration
- cache key must include processing params
- processing must not reload the whole audio engine
- Web uses WASM
- Electron uses DAUx/native if implemented, or explicit WASM fallback
- no silent no-op

Expected visual behavior:

```txt
speedRatio 2.0 = half duration / half width
speedRatio 0.5 = double duration / double width
pitch +6 preservePitch = same width
```

---

## Plugins

Futureboard uses plugin package discovery through manifests.

Planned plugin root:

```txt
{appdir}/plugins/**/manifest.json
```

Example:

```json
{
  "id": "futureboard.equz8",
  "name": "Equz8",
  "version": "0.1.0",
  "type": "audio-effect",
  "category": "eq",
  "vendor": "Futureboard",
  "entry": {
    "core": "./Core/index.js",
    "editor": "./Editor/index.html"
  },
  "params": "./Core/params.json",
  "editor": {
    "kind": "web",
    "framework": "react",
    "width": 720,
    "height": 360
  },
  "capabilities": {
    "hasEditor": true,
    "supportsAutomation": true,
    "supportsPresets": true
  }
}
```

Plugin architecture:

```txt
Core
= params
= schema
= DSP hooks
= preset model
= no React

Editor
= UI
= React / Vue / Svelte / Vanilla / Sphere UI
= communicates with host through protocol

Host
= owns params
= updates audio engine
= handles automation
```

Do not hardcode built-in plugins directly into mixer/effect editor.

---

## VST3 Host Direction

VST3 host will likely be native/Rust-based.

Temporary debug GUI can use `egui`.

```txt
Rust VST3 Host
├─ Plugin scanner
├─ Plugin loader
├─ Parameter inspector
├─ MIDI monitor
├─ Audio process test
└─ egui debug UI
```

Electron integration:

```txt
Electron Main
→ sphere-pluginhost.node
→ VST3 host core
```

---

## UI Design

Futureboard UI must feel like a DAW/editor, not a web dashboard.

Style direction:

```txt
compact
dark
technical
editor-like
DAW-like
low-noise
high-density
subtle borders
cyan accent
```

---

## Settings Direction

Audio settings should stay simple.

Current rule:

```txt
Electron = DAUx: OS Audio Backend
Web      = WASM only
```

Settings > Audio should expose:

Electron:

```txt
Engine: DAUx
Backend:
- Windows: WASAPI, MME fallback
- macOS: CoreAudio
- Linux: ALSA

Buffer Size:
- 64
- 128
- 256
- 512
- 1024

Sample Rate:
- Device Default
- 44100
- 48000
- 96000
```

Web:

```txt
Engine: WASM
Backend: browser-managed / unavailable
```

Do not show confusing WebAudio/RustWASM/native hybrid options.

---

## Development Notes

Useful search commands:

```bash
rg "WasmAudio|RustDsp|futureboard_core|WebAudioEngineAdapter|ClipScheduler|new AudioContext|AudioWorklet" apps packages frameworks
```

```bash
rg "assetId|raw.assetId|function normalize|normalize.*Clip|fileId" apps/web/src/store/normalize.ts apps
```

```bash
rg "mediaPath|buildNativeProjectSnapshot|loadProject|SphereNativeAudioEngineAdapter" apps packages frameworks
```

Important debugging logs:

```txt
[AudioEngine] active backend
[SphereAudio IPC] loadProject
[SphereAudio] RuntimeProject built
[SphereAudio callback] StartTransport
[NativeSnapshot] clip mediaPath exists
```

---

## Known Critical Issues

### 1. Native clip playback silent

Symptom:

```txt
Metronome works
Track playback silent
SphereAudio says clips have null mediaPath
```

Cause:

```txt
Project schema has files[]
Native snapshot expects assets[]
Clip has fileId
Resolver expects assetId / relativePath
```

Fix direction:

```txt
normalize files[] into assets[]
or native resolver supports files[]
resolve clip.fileId → project.files[] → relativePath → mediaPath
```

### 2. Auto Import to Project needed

Electron must copy imported audio into `Media/Audio` and create a project asset before creating timeline clips.

### 3. Pitch / Speed broken

Processing path, cache key, visual duration, and native/Web backend routing need to be repaired.

### 4. Waveform for long clips

Whole-file waveform generation breaks for multi-hour clips. Needs chunk/tile cache.

---

## Scripts

Example scripts may vary by workspace.

```bash
bun install
bun run build
bun run dev
```

Electron:

```bash
bun run --cwd apps/electron dev
bun run --cwd apps/electron build
```

Web:

```bash
bun run --cwd apps/web dev
bun run --cwd apps/web build
```

---

## Repository Principles

1. WebUI is the primary UI workflow.
2. Electron is the serious desktop runtime.
3. Web version remains WASM-only.
4. Electron uses DAUx/native audio.
5. Folder projects are the source of truth.
6. Media belongs in `Media/`.
7. Cache belongs in `Cache/`.
8. Runtime files/buffers are never project truth.
9. Plugins are manifest-driven.
10. Audio callback must be realtime-safe.
11. No silent backend fallback.
12. No fake native mode.
13. If a backend is active, the UI must say so.
14. If a feature is not wired, show honest status.

---

## License

MIT License.

Futureboard Studio is intended to be open-source with optional donation/subscription/supporter layers for cloud, samples, stock plugins, AI, or additional services.
