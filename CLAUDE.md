# CLAUDE.md — Futureboard Studio Agent Rules

Before implementing any task spec, read:

1. `tasks/SKILL.md`
2. `DESIGN.md` if the task touches UI, layout, styling, windows, dialogs, panels, or component structure
3. The smallest relevant task file/section only

This file is a quick operating contract for Claude Code inside the Futureboard Studio repository.  
`tasks/SKILL.md` is the deeper source of truth.

---

## Prime Directive

Work in the smallest safe scope.

Do not rewrite the repository.  
Do not implement an entire roadmap unless explicitly requested.  
Do not "improve" unrelated code while fixing a specific bug.

Every patch must be:

- scoped
- buildable
- reversible
- validated
- honest about what was and was not tested

Before editing:

```txt
1. Restate the exact requested scope.
2. Inspect the relevant files.
3. Identify the current behavior.
4. Identify the smallest safe patch.
5. List likely files to change.
6. Implement only that patch.
7. Run the smallest relevant validation.
8. Report changed files, validation, and remaining TODOs.
```

Never claim validation passed if it was not run.

---

## Project Map

Futureboard surfaces:

- **Futureboard Express** — WebUI
- **Futureboard Lite** — Electron
- **Futureboard Studio** — Native Rust / GPUI
- **SphereDirectAudioEngine / DAUx** — native realtime audio engine
- **SpherePluginHost** — plugin scanning, loading, processing, and editor hosting
- **SphereWebAudioCore** — WASM/WebAudio fallback engine
- **SphereUIComponents** — shared native UI components

Common paths, but always inspect the repo because paths may differ:

```txt
apps/
  web/
  electron/
  native/
  experimental/native/

crates/
  SphereUIComponents/
  SphereDirectAudioEngine/
  SphereWebAudioCore/
  SpherePluginHost/

external/
  vst3sdk/
  clap/
  ARA_SDK/
  zed/

tasks/
  native/
  audio/
  plugin/
```

For WebUI WASM DSP work, inspect:

```txt
crates/SphereWebAudioCore
```

---

# PluginHost

If editing or creating PluginHostWrapper / plugin host integration, inspect:

```txt
crates/SpherePluginHost
external/vst3sdk
external/clap
```

Planned/target support includes:

- VST3
- CLAP
- AU
- LV2
- Linux/macOS platform paths

Native Studio should use the pure host core without requiring N-API.

Do not force JUCE into the project unless explicitly requested.

---

# Audio / Plugin Bridge Rules — Do Not Break Realtime

Futureboard audio work must be realtime-aware.

## Realtime hot paths must not contain

- heap allocation in steady-state processing
- `println!`, `eprintln!`, tracing/logging directly from audio callback
- filesystem I/O
- plugin scanning
- JSON parsing
- `serde_json::Value` lookup
- `HashMap<String, ...>` lookup per block
- blocking locks
- sleeps
- waits on UI thread
- Node/Electron calls
- unbounded queues
- panics across FFI

Use instead:

- preallocated buffers
- immutable runtime snapshots
- compact enums/indices resolved before playback
- atomics
- bounded lock-free/SPSC queues
- diagnostics rings drained by non-realtime threads

## Classify touched code before editing

When touching audio/plugin code, classify every changed function as one of:

```txt
Realtime callback / hot path
Audio control thread
Plugin host producer thread
UI/control path
Scanner/offline path
Test-only path
```

If it is realtime or producer-hot, apply realtime rules.

## Bridge producer rules

The plugin host producer must not rely on `sleep(250µs)` polling as the main wake mechanism.

Preferred architecture:

```txt
Engine/audio callback publishes request_seq
Engine signals named event / SetEvent
Host producer WaitForSingleObject
Host producer processes exact target instance
Host publishes response_seq
Engine freshness guard verifies response
```

Windows bridge threads should use appropriate scheduling hardening where relevant:

- `timeBeginPeriod(1)` while bridge is active
- MMCSS `"Pro Audio"` for producer thread
- suitable thread priority
- cleanup on shutdown

Do not remove freshness guards to hide dropouts.  
If the guard says stale, fix producer timing or bridge sequencing.

## Instance routing rules

Never broadcast MIDI or parameter events to all loaded plugin voices unless the feature explicitly says "broadcast".

Route by:

- `instance_id`
- region identity
- insert id
- track/insert mapping

Required behavior:

```txt
MIDI for insert A reaches only insert A.
Param event for insert A reaches only insert A.
Plugin state for insert A restores only insert A.
```

## Plugin state is P0

DAW project save/load is not usable unless plugin state is persisted.

VST3 state work must support:

- component state
- controller state when available
- opaque binary blobs
- project snapshot persistence
- restore after plugin instantiation and before playback/editor open
- clear error reporting if restore fails

Do not silently discard plugin state.

## VST3 ProcessContext must be real

Do not hardcode:

```txt
tempo = 120
time signature = 4/4
playing = true
projectTimeSamples = 0
```

ProcessContext should come from actual engine transport and timeline state:

- sample rate
- block frames
- playing/stopped
- recording if available
- project time samples
- tempo map
- time signature map
- PPQ/bar position when available
- loop/cycle state when available

Hardcoded ProcessContext is only acceptable in an explicit test stub.

## Parameter automation must be end-to-end

Automation for bridged plugins is not complete until the event travels:

```txt
Automation lane / UI
-> engine automation event
-> runtime insert param event
-> shared bridge param ring
-> host producer
-> target plugin instance
-> VST3 parameter queue or documented fallback
```

A struct/ring definition with no push/pop is not an implementation.

No silent failure.

## PDC must include bridge latency

Bridge inserts must contribute to plugin delay compensation.

Minimum bridge latency should include at least one block if the architecture requires a block handoff, plus plugin-reported latency when available.

Parallel mix correctness matters.

---

# GUI / No Slop Rules

Futureboard Studio is a professional desktop DAW.

It must not look like:

- a generic SaaS dashboard
- an AI-generated landing page
- Bootstrap modal soup
- mobile-first cards
- crypto/web3 dashboard
- Tailwind demo UI
- browser default HTML UI

It should feel like:

- compact native DAW
- Zed-ish dark editor
- Ableton-style dense device/editor panels
- professional tool used for long sessions

---

## Dialog / Window UI Consistency Rules

All dialogs, floating windows, modal windows, project wizards, settings panels, confirmation prompts, utility popups, and plugin shells must share the same Futureboard Studio dialog language.

### Source of Truth

Use the existing polished `AddTrackDialog` / `DialogWindow` / current Settings surfaces as visual source of truth.

Do not invent a new dialog style for each feature.

Every dialog must feel like it belongs to the same desktop DAW application.

### Dialog Shell

Use the shared `DialogWindow` component wherever possible.

Default dialog shell:

- compact titlebar, usually 28–34px
- title in titlebar
- close button in titlebar
- dark panel / near-black blue-gray surface
- subtle border
- soft floating-window shadow
- rounded but not bubbly corners
- z-index above menus/panels
- no thick card-like border
- no bright web-modal background
- no opaque dark backdrop unless explicitly required

Preferred style:

```txt
background: dark panel / near-black blue-gray
border: subtle rgba white border
shadow: soft floating window shadow
radius: compact desktop radius
titlebar: compact 28-34px
```

Dialogs must not use:

- Bootstrap modal layout
- `p-8`, `p-10`, giant padding
- `text-2xl` headers
- huge centered cards
- big hero copy
- rounded-3xl bubbly panels
- gradient backgrounds
- glassmorphism
- neon glow
- emoji icons

---

## Theme Rules

Components must use project theme tokens.

Do not invent hardcoded colors.  
Do not add arbitrary Tailwind colors unless approved.  
If a color is missing, add a semantic token first.

Prefer semantic names:

```txt
surface.base
surface.panel
surface.raised
surface.sunken
border.subtle
border.strong
border.focus
text.primary
text.secondary
text.muted
accent.primary
accent.subtle
status.success
status.warning
status.error
transport.play
transport.record
meter.peak
automation.line
timeline.grid.major
timeline.grid.minor
```

Icons must inherit `currentColor`.

---

# Layout Discipline — No Flexbox Guessing

Do not let flexbox "figure it out" for DAW-critical surfaces.

Before changing layout, identify:

```txt
owner rect
content rect
scroll owner
clip owner
coordinate space
DPI/logical-vs-physical pixels
minimum size
resize behavior
```

DAW-critical surfaces must use explicit measured layout or a central layout helper:

- timeline ruler
- arrangement lanes
- track headers
- automation lanes
- MIDI piano roll
- velocity/CC lanes
- mixer strips
- plugin editor host viewport
- transport bar
- popovers/dropdowns anchored to controls
- split panels / resizable panels

## Flexbox rules

Flexbox is allowed for simple rows/columns only.

Flexbox is not allowed to guess geometry for:

- timeline grid
- ruler tick alignment
- playhead alignment
- waveform canvas
- MIDI note canvas
- automation overlay
- piano keyboard alignment
- plugin editor child window sizing
- scroll-synced panels

Required for nested flex containers:

```txt
min-width: 0
min-height: 0
overflow handled by the intended scroll owner only
```

Never rely on content expansion to size timeline or editor panels.

## Banned layout patterns

Do not use:

```txt
left: 220px
width: calc(100vw - 220px)
height: calc(100vh - ...)
magic sidebar constants
magic bottom-panel constants
negative margins to align panels
absolute positioning without named rect source
fixed plugin editor size unless plugin requires it
```

Use actual measured chrome metrics instead:

```txt
browser_width
inspector_width
bottom_panel_height
status_bar_height
track_header_width
ruler_height
scroll_x
scroll_y
zoom
dpi_scale
```

## Tailwind / CSS ban list for app chrome

Avoid these unless the user explicitly asks for a marketing/mockup surface:

```txt
container mx-auto
max-w-7xl
p-8 / p-10 / p-12
text-2xl / text-3xl / text-4xl
rounded-2xl / rounded-3xl / rounded-full for large controls
shadow-xl / shadow-2xl everywhere
bg-gradient-to-*
from-* via-* to-*
backdrop-blur-xl
glass / frosted
space-y-8 for dense tools
```

Preferred compact scale:

```txt
text: 11-13px for chrome
row height: 24-32px
titlebar: 28-34px
toolbar button: compact square/rect
dialog padding: tight, usually 10-16px depending on surface
border: 1px subtle
radius: compact
```

---

# Canvas / WGPU / DPI Layout Rules

Canvas/WGPU surfaces must use real viewport dimensions.

Rules:

- separate CSS logical size from physical pixel size
- multiply by device pixel ratio where required
- snap 1px lines carefully
- clip drawing to content rect
- never draw into track header/ruler/sidebar by accident
- use one coordinate conversion function for hit-test and render
- scroll/zoom math must be shared between grid, clips, notes, automation, and playhead

For every canvas-like surface, validate:

```txt
resize window
toggle sidebar
toggle inspector
resize bottom panel
zoom in/out
horizontal scroll
vertical scroll
DPI scale != 100%
```

---

# GPUI Layout Rules

GPUI surfaces must not update parent entities from child entity updates.

Use:

- command outcome
- queued events
- parent-owned mutation
- central command dispatcher

Avoid:

```txt
StudioLayout.update
  -> child.update
    -> StudioLayout.update
```

This can trigger GPUI double-lease/panic behavior.

For layout:

- prefer named rects/chrome metrics over duplicated constants
- apply `overflow_hidden` to content areas that must clip
- clamp labels/pills inside their lane rect
- do not let marker labels draw into headers
- do not create floating overlays without anchor/clamp logic

---

# Plugin Editor Window Layout Rules

Plugin editor hosting is not a normal web panel.

Windows:

- parent HWND/NSView must own the plugin child view
- child view must be clipped to plugin editor content rect
- no overlap with titlebar/header
- no size mismatch between host shell and plugin view
- plugin requested resize must update shell/client rect
- host resize must call plugin resize/onSize where supported
- DPI must be handled per monitor
- close/detach must happen before destroying parent window

Do not draw plugin UI with GPUI/WGPU.  
GPUI draws only the shell/header around the native child view.

---

# Component Construction Contract

Every new reusable component must define:

```txt
size behavior
minimum size
overflow behavior
focus behavior
keyboard behavior
disabled/loading/error states
theme token usage
```

Do not create:

- component-local persistent DAW state
- local mock data that looks real
- buttons with no callback
- fake success actions
- fake device/plugin lists in production UI
- dropdowns that do not clamp to screen
- dialogs that cannot close with Escape/Cancel
- controls that break when text is longer than expected

---

# Visual QA Checklist

Before claiming a GUI task is done, verify or explicitly state not verified:

```txt
1. Normal size
2. Narrow size
3. Tall/short window
4. Sidebar hidden/shown
5. Inspector hidden/shown
6. Bottom panel resized
7. Scroll position not zero
8. Zoom in/out if timeline/editor
9. DPI scale if relevant
10. Text does not wrap unexpectedly
11. No overlap into headers
12. No clipped important labels
13. Keyboard focus still works
14. Escape/cancel works for modal/gesture
15. Theme tokens only
```

If only code validation was run, do not claim visual validation.

---

# Shortcut and Input Routing

Global shortcuts must respect focus priority:

```txt
1. modal dialog
2. text input / combo search / numeric edit / hex color input
3. MIDI editor
4. timeline
5. app/global
```

Required edit shortcuts:

```txt
Ctrl/Cmd+A
Ctrl/Cmd+C
Ctrl/Cmd+V
Ctrl/Cmd+X
Delete
Backspace where appropriate
```

These must work in timeline/MIDI/velocity/CC contexts, but must not fire while typing in text inputs.

Use a central command enum.  
Do not duplicate shortcut logic randomly.

---

# State Ownership

Single source of truth:

- tracks live in project state
- clips live in track/project state
- MIDI notes live in MIDI clip state
- mixer values live in track mixer state
- inserts live in `track.inserts`
- routing lives in track routing/sends
- plugin registry is separate from live instances
- automation lanes live in project/track automation state
- transient meters do not dirty project

Never create disconnected fake DAW state just to make UI look finished.

---

# Debug Flags

Use debug flags instead of noisy always-on logs.

Common flags:

```txt
FUTUREBOARD_BOOT_DEBUG=1
FUTUREBOARD_WINDOW_POSITION_DEBUG=1
FUTUREBOARD_COMBOBOX_DEBUG=1
FUTUREBOARD_SHORTCUT_DEBUG=1
FUTUREBOARD_EDIT_COMMAND_DEBUG=1
FUTUREBOARD_SELECTION_DEBUG=1
FUTUREBOARD_TRANSPORT_DEBUG=1
FUTUREBOARD_TRANSPORT_FREEZE_DEBUG=1
FUTUREBOARD_AUDIO_DEBUG=1
FUTUREBOARD_AUDIO_DEVICE_DEBUG=1
FUTUREBOARD_RECORDING_DEBUG=1
FUTUREBOARD_RECORD_WRITER_DEBUG=1
FUTUREBOARD_ROUTING_DEBUG=1
FUTUREBOARD_AUTOMATION_DEBUG=1
FUTUREBOARD_AUTOMATION_SYNC_DEBUG=1
FUTUREBOARD_PLUGIN_DEBUG=1
FUTUREBOARD_PLUGIN_SCAN_DEBUG=1
FUTUREBOARD_PLUGIN_VIEW_DEBUG=1
FUTUREBOARD_PLUGIN_BRIDGE_DEBUG=1
FUTUREBOARD_PLUGIN_STATE_DEBUG=1
FUTUREBOARD_PLUGIN_CONTEXT_DEBUG=1
FUTUREBOARD_PARAM_BRIDGE_DEBUG=1
FUTUREBOARD_PDC_DEBUG=1
FUTUREBOARD_WAVEFORM_DEBUG=1
FUTUREBOARD_GPU_RENDERER_DEBUG=1
FUTUREBOARD_WELCOME_DEBUG=1
FUTUREBOARD_SHUTDOWN_DEBUG=1
FUTUREBOARD_LAYOUT_DEBUG=1
FUTUREBOARD_UI_DEBUG_CLIPS=1
```

Never log every audio block unless throttled or drained outside realtime.

---

# Validation Commands

Use the smallest relevant command.

Rust:

```bash
cargo check
cargo test
cargo build
cargo clippy -- -D warnings
```

Native UI:

```bash
cargo check -p sphere_ui_components
cargo check --manifest-path apps/native/Cargo.toml
cargo clippy -p sphere_ui_components -- -D warnings
```

Audio:

```bash
cargo check -p sphere-direct-audio-engine
cargo test -p sphere-direct-audio-engine
cargo build -p sphere-direct-audio-engine
```

Plugin host:

```bash
cargo check -p sphere_plugin_host
cargo build -p sphere_plugin_host
cargo build -p sphere_plugin_host --release
```

Frontend:

```bash
bun run build
bun run typecheck
bun run lint
```

WASM audio:

```bash
bun run --cwd apps/web build:audio:wasm
```

If a command does not exist, say so and run the closest safe command.  
Do not invent success.

---

# Reporting Format

After implementation, report:

```txt
Summary:
- what changed

Files changed:
- key files

Validation:
- command run
- result

Notes:
- intentional TODOs
- unsupported features kept disabled
- next recommended slice
```

If validation failed, summarize the error concisely and either fix it within scope or stop honestly.

Do not paste huge diffs unless asked.

---

# Final Rule

Futureboard Studio can be ambitious.

Claude patches must be boring, scoped, buildable, and reversible.

Build the monster one safe organ at a time.
