## Project

**Futureboard Studio / Mochi DAW**

A browser-first DAW prototype built with React, Vite, Electron, and Web Audio.

Current goal:

- Build a usable DAW product prototype first.
- Validate UI/UX, workflow, data model, editor behavior, timeline behavior, mixer behavior, and project structure.
- Keep the prototype stable and TypeScript-clean.
- Later, move the audio/runtime stack to WASM for Web and native DSP service for Electron.
- Eventually rewrite the production runtime using the custom C++ SphereEngine.

This repo is currently a **prototype / living specification**, not the final native engine.

---

## High-Level Architecture

Current runtime:

```txt
React UI
Vite
Electron shell
Web Audio prototype
Canvas for heavy visual rendering
Zustand/project store
Command system
````

Planned runtime:

```txt
Web:
React UI
AudioWorklet
WASM DSP core

Electron:
React UI
Electron IPC
Native DSP service .exe

Future native:
SphereEngine C++
SphereUI / SphereReact
Native renderer
Native audio engine
Plugin host
```

Important rule:

> Do not lock the UI directly to WebAudio, Electron, Node, or future native APIs.
> Use adapters and boundaries.

---

## Repository Structure

Expected structure:

```txt
apps/
  web/
  electron/

core/
  Rust/C++ core experiments

packages/
  shared packages if needed

.claude/
  agent prompts / workflows
```

Do not create random top-level folders unless necessary.

---

## Development Philosophy

Build the product workflow first.

This prototype is used to discover:

* how the DAW should feel
* how editing should work
* how clips should behave
* how tracks should behave
* how MIDI editing should behave
* how the mixer should behave
* how commands should be structured
* what the future C++ runtime must support

Do not over-engineer final native architecture inside the React prototype.

Good:

* small safe patches
* clear component boundaries
* reusable editor logic
* strongly typed DAW model
* stable project store actions
* clean command system
* TypeScript-clean builds

Bad:

* huge unrelated rewrites
* redesigning the whole UI without being asked
* changing audio engine architecture casually
* mixing platform-specific APIs directly into UI components
* introducing random theme colors
* making generic SaaS UI
* adding large dependencies without clear reason

---

## Visual Direction

The UI direction is:

```txt
Zed Editor
DaVinci Resolve
modern DAW
native desktop app
dark creative workstation
compact professional tool
```

Avoid:

```txt
generic SaaS dashboard
Bootstrap-looking UI
oversized web app controls
mobile-looking UI
toy DAW visuals
random neon overload
large fonts everywhere
```

Typography:

* normal controls: 11px–13px
* labels: 10px–11px
* menu items: 12px
* dense DAW/editor UI is preferred

Spacing:

* compact but readable
* no missing padding
* no cramped broken layout
* no huge whitespace unless intentionally used for empty states

---

## Theme Rules

Use existing theme tokens whenever possible.

Do not introduce random colors.

Common color direction:

```txt
dark slate / charcoal surfaces
cyan accent
subtle violet/pastel track colors
dim grid lines
professional low-contrast UI
```

If a new color is needed, add it to the theme file intentionally.

---

## Current Important Features

The app currently has or is expected to have:

* top menu
* command palette
* browser panel
* arrangement timeline
* track headers
* audio clips
* MIDI clips
* waveform rendering
* inspector panel
* bottom workspace tabs:

  * Mixer
  * Editor
  * Effect Editor
* mixer channels
* master strip
* add track modal
* floating arrangement tools
* audio editor
* MIDI piano roll editor

---

## Clip Types

Clips must be type-aware.

Supported clip types:

```ts
type ClipType = "audio" | "midi";
```

Rules:

* Audio clips use waveform rendering.
* MIDI clips use MIDI clip rendering.
* MIDI clips must never attempt to render waveform.
* Audio-only controls must not appear for MIDI clips.
* MIDI-only controls must not appear for audio clips.

If a bug appears where MIDI clips show waveform errors, fix clip type routing first.

---

## MIDI Editor Rules

The MIDI Editor is a real piano roll editor.

Expected features:

* piano key lane
* piano roll grid
* velocity lane
* note create
* note select
* note move
* note resize
* note delete
* snap/grid
* quantize
* Ctrl+wheel zoom
* keyboard shortcuts
* HiDPI-safe canvas grid

Coordinate math is sensitive.

Do not duplicate coordinate calculations across random handlers.

Use centralized helpers for:

```txt
clientX → editor X
clientY → editor Y
x → time
time → x
y → pitch
pitch → y
duration → width
width → duration
```

Always account for:

* boundingClientRect
* piano key lane width
* scrollLeft
* scrollTop
* zoom / pxPerSecond
* row height
* velocity lane height
* devicePixelRatio only for canvas drawing

Important:

> DOM note positioning uses CSS pixels.
> Canvas drawing can use devicePixelRatio scaling.
> Do not mix them.

---

## Audio Editor Rules

Audio clips should show:

* waveform
* gain
* fades
* timing
* source file info
* process placeholders

Do not store huge raw audio data in React state if avoidable.

Prefer:

* decoded buffer in engine/cache
* waveform peaks for UI rendering
* canvas for large waveform rendering

---

## Timeline / Arrangement Rules

The Arrangement is the main editing surface.

Expected behavior:

* select track
* select clip
* clear selection on empty click
* drag clips
* resize clips
* split clips
* mute clips
* future automation
* future loop region

Use current tool state when implementing interactions.

Arrangement tools:

```ts
type ArrangementTool =
  | "pointer"
  | "pen"
  | "cut"
  | "glue"
  | "mute"
  | "time"
  | "automation";
```

Tool shortcuts:

```txt
V = Pointer
P = Pen
C = Cut
G = Glue
M = Mute
T = Time / Stretch
A = Automation
```

Do not trigger shortcuts while typing in inputs.

---

## Floating Toolbar Rules

The floating toolbar is for Arrangement editing tools.

It should remain:

* compact
* dark
* floating
* editor-like
* visually subtle

Do not make it huge.

Do not place it where it blocks clips heavily.

Preferred placement:

```txt
bottom-left inside Arrangement viewport
```

---

## Mixer Rules

The mixer should feel like a compact professional DAW console.

Avoid:

* too much visual noise
* huge controls
* meters as heavy decorative blocks
* raw HTML-looking sliders/buttons
* overly dark unreadable strips

Expected mixer components:

* inserts
* sends
* pan
* mute/solo
* fader
* meter
* track name
* master strip

Meter animation should be smooth:

* fast attack
* slower release
* avoid rerendering whole mixer at high FPS
* isolate meter updates

---

## Command System Rules

The command system is important.

Commands should support:

* command palette
* menus
* shortcuts
* undo/redo where applicable

Do not wire command behavior only to one button if it should be globally callable.

When adding an action, consider:

```txt
menu item
command palette entry
shortcut
store action
undo command
```

---

## Platform Rules

Do not directly call Electron, Node, or browser-only APIs from UI components.

Use platform adapters.

Target platforms:

```txt
web
electron
future native
```

Web:

* sandboxed
* file picker / drag-drop
* temporary local storage
* IndexedDB/OPFS later
* WASM audio later

Electron:

* real filesystem
* native dialogs
* native window controls
* later native DSP service via IPC

Future native:

* SphereEngine C++
* full native runtime

Platform-specific behavior should live behind services/adapters.

---

## Electron Rules

Electron must stay secure.

Required:

```txt
contextIsolation: true
nodeIntegration: false
sandbox: true where possible
preload bridge only
```

Renderer must not get raw unrestricted Node access.

Window controls and native APIs should go through preload IPC.

For titlebar UI:

* clickable controls must use `app-no-drag`
* logo should not be draggable
* use CSS background image for logo if needed

Example logo rule:

```tsx
<div
  className="app-logo"
  style={{ backgroundImage: `url(${logoApp})` }}
/>
```

Do not use draggable `<img>` for titlebar logo unless `draggable={false}` and drag behavior is fully disabled.

---

## Build Rules

The project must remain TypeScript-clean.

Before finishing a task, run or respect:

```bash
bun run build
```

or the closest project build command.

If using TypeScript with `erasableSyntaxOnly`, avoid syntax such as:

```ts
constructor(private foo: Foo) {}
```

Use explicit properties instead:

```ts
private foo: Foo;

constructor(foo: Foo) {
  this.foo = foo;
}
```

Do not “fix” build errors by disabling strict rules unless explicitly asked.

---

## Styling Rules

Use Tailwind and existing classes.

Keep UI compact.

Common requirements:

* no giant buttons
* no generic card UI
* no random gradients in app UI
* no excessive blur
* no inconsistent border radius
* no text wrapping in track names or clip labels
* truncate long labels
* use title tooltip for full text where useful

Clip labels, track names, and menu labels must not wrap.

---

## Image / Icon Rules

Primary icon set can be Lucide.

If DAW/audio-specific icons are missing, Tabler Icons are acceptable.

Recommended:

```txt
Lucide:
general shell/menu/actions

Tabler:
audio, MIDI, waveform, plugin, routing, DAW-specific icons
```

Do not mix too many icon styles in one small area.

---

## Performance Rules

HTML/DOM is acceptable for prototype controls, but heavy visuals should move toward Canvas.

Use DOM/React for:

* buttons
* menus
* inspector
* browser
* tabs
* modals
* simple controls

Use Canvas for:

* waveform
* timeline grid
* piano roll grid
* large meters later
* heavy visual editor surfaces

Avoid:

* thousands of DOM bars for waveform
* storing huge raw audio buffers in frequent React state
* rerendering all tracks/mixer strips on every playhead tick
* excessive layout thrashing

Consider:

* memoization
* virtualization
* throttled meter updates
* canvas rendering
* storing only lightweight UI state in React

---

## Audio Architecture Direction

Current prototype may use Web Audio.

Future architecture:

```txt
Web:
AudioWorklet + WASM DSP

Electron:
Native DSP .exe service + IPC

Native:
SphereEngine C++ direct runtime
```

Use an `AudioEngineAdapter` style boundary.

The UI should not care whether the engine is:

```txt
WebAudio
WASM
Native service
SphereEngine
```

---

## Native Rewrite Direction

Do not start rewriting the whole app in C++ yet.

The current React/Electron app is the living spec.

Only after product behavior stabilizes, port to:

```txt
SphereEngine C++
SphereUI / SphereReact
Native renderer
Native audio graph
DAUx / VST3 / CLAP plugin support
```

Current rule:

> Prototype first. Native rewrite later.

---

## Safe Implementation Strategy

When asked to implement a feature:

1. Understand current architecture.
2. Make the smallest safe patch.
3. Keep visual style consistent.
4. Keep TypeScript clean.
5. Avoid unrelated rewrites.
6. Avoid changing audio engine unless necessary.
7. Update shared types/store/actions only when needed.
8. Preserve existing behavior.
9. Prefer placeholders over fake broken implementations for future features.

---

## Common Pitfalls

Avoid these recurring bugs:

### MIDI clip rendered as audio clip

Symptom:

```txt
MIDI clip shows waveform error
```

Fix:

```txt
Check clip.type and renderer routing.
```

### Piano roll click position wrong

Check:

```txt
piano key lane width
scrollLeft
scrollTop
bounding rect
zoom
row height
DPR mixing
```

### Canvas blurry on HiDPI

Fix:

```txt
Scale canvas backing store by DPR.
Draw in CSS pixel coordinates after scaling.
Do not scale DOM note positions by DPR.
```

### Electron titlebar clicks broken

Check:

```txt
-webkit-app-region: drag
app-no-drag
pointer-events
```

### Dropdown/popover does not open

Check:

```txt
Radix Trigger asChild
Portal
z-index
overflow hidden
app-no-drag
pointer-events-none
```

### Vercel build fails with TS1294

Check for TypeScript parameter properties when `erasableSyntaxOnly` is enabled.

---

## Current Priorities

Near-term priorities:

1. Smoke test MIDI editor.
2. Fix any MIDI piano roll coordinate bugs.
3. Improve audio editor polish.
4. Add clip nudge with mouse wheel.
5. Make floating tools fully functional.
6. Improve README and GitHub presentation.
7. Keep Electron/Web split clean.
8. Continue platform adapter cleanup.

---

## Do Not Do Unless Asked

Do not:

* rewrite the whole UI
* replace the state management system
* rewrite audio engine architecture
* implement full DSP time-stretch
* implement full synth engine
* add native plugin hosting
* introduce a huge framework
* redesign the visual language
* remove working features
* change project structure drastically

---

## Tone of Work

This is an experimental DAW, but the code should be serious.

Prototype fast, but do not make throwaway chaos.

The goal is:

```txt
fast iteration
clean architecture boundaries
professional DAW workflow
future native rewrite readiness
```
