# SKILL.md — Futureboard Studio / Sphere Agent Skill

## Purpose

This skill teaches coding agents how to work safely and effectively inside the **Futureboard Studio / Sphere** codebase.

Futureboard Studio is an open-source DAW being built across several surfaces:

- **Futureboard Express** — WebUI
- **Futureboard Lite** — Electron
- **Futureboard Studio** — Native Rust / GPUI
- **SphereDirectAudioEngine / DAUx** — native realtime audio engine
- **SpherePluginHost** — plugin scanning, loading, processing, and editor hosting
- **SphereWebAudioCore** — WASM/WebAudio fallback engine
- **SphereUIComponents** — shared native UI components

This skill is for **controlled implementation**.

It is not permission to rewrite the entire repository.

---

# 0. Prime Directive

Always work from the **smallest safe scope**.

Before changing code:

1. Understand the exact user request.
2. Read the relevant task file or section only.
3. Inspect the current implementation.
4. Identify the smallest buildable patch.
5. List likely files to change.
6. Implement only the requested scope.
7. Run the relevant build/check command.
8. Report what changed, what was validated, and what remains.

Never implement an entire long roadmap unless the user explicitly asks for that.

Never rewrite the app unless the user explicitly asks for a rewrite.

If a task looks huge, implement the first safe slice and stop.

---

# 1. Project Identity

Use these names consistently:

- Futureboard Studio
- Futureboard
- Sphere Engine
- Sphere UI
- SphereDirectAudioEngine
- DAUx
- SpherePluginHost
- SphereWebAudioCore
- SphereUIComponents
- SphereGPUI / SphereGPUIGraphics when working on the standalone GPUI fork

Development style:

- spec-driven
- human-directed
- agent-assisted
- incremental
- performance-sensitive
- realtime-aware
- UI-first when the task is UI
- audio-safe when the task touches audio
- reversible whenever possible

The existing React/Electron implementation is valuable.

Treat working Electron/Web behavior as a **living product spec**, especially when porting features to Native GPUI.

Do not casually throw away working behavior.

---

# 2. Current Architecture Mindset

The repository may contain:

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
  gpui/
  SphereGPUI/

external/
  vst3sdk/
  clap/
  ARA_SDK/
  zed/

packages/
  assets/
  shared/

tasks/
  native/
  audio/
  plugin/
  webapp/
  docs/
```

Do not assume exact paths.

Inspect the repository first.

If the repository differs from this document, follow the repository.

---

# 3. How Agents Should Read the Repo

## Claude Code

- Read `CLAUDE.md` if present.
- Read only relevant task files or sections.
- Keep memory concise.
- Link to large specs instead of copying them into context.

## Codex

- Read `AGENTS.md` if present.
- Read only relevant task files or sections.
- Do not load every task file at once.

## Both

- `README.md`, `README.txt`, or `tasks/README.md` is an index if present.
- Long task files are reference specs, not one-shot prompts.
- The user may say:
  - `ทำ 006 section 3`
  - `continue plugin insert phase`
  - `fix TrackLane waveform renderer`
  - `ทำ MIDI editor slice B`
  - `แก้ GPUI panic`
- Respect the exact scope.

---

# 4. Standard Implementation Pattern

When given a coding task:

```txt
1. Restate exact scope.
2. Inspect relevant files.
3. Identify the current behavior.
4. Identify the smallest safe patch.
5. Implement.
6. Run validation.
7. Report summary, changed files, validation, and remaining TODOs.
```

Correct behavior:

```txt
User: Implement MIDI note mute + clipboard.

Agent:
- Inspect MIDI model/editor files.
- Add muted field/migration if needed.
- Add copy/paste/duplicate for selected notes.
- Keep playback/runtime untouched unless required.
- Build/check.
- Stop.
```

Incorrect behavior:

```txt
- Rewrite whole MIDI editor.
- Add CC lanes, runtime playback, automation, and rendering in one patch.
- Change unrelated theme/layout code.
- Claim validation without running it.
```

---

# 5. Hard Rules

## Do

- Make small patches.
- Preserve working behavior.
- Use existing stores/actions/models when possible.
- Centralize duplicated logic.
- Add safe placeholders for future-heavy features.
- Disable unfinished UI actions.
- Keep TypeScript clean.
- Keep Rust warnings low.
- Keep unsafe code isolated.
- Keep C++ boundaries explicit.
- Run build/check after implementation.
- Keep Web/Electron/Native boundaries clean.
- Use platform adapters instead of direct Electron/Node calls in React.
- Treat current app behavior as product intent.

## Do Not

- Rewrite the entire app unless explicitly requested.
- Rewrite audio engine while doing a UI task.
- Implement native DSP while doing a layout task.
- Implement VST/CLAP/AU hosting unless explicitly requested.
- Add random dependencies.
- Add fake-success actions.
- Duplicate project state in multiple stores.
- Put heavy audio buffers into React or persisted UI state.
- Parse JSON in realtime audio paths.
- Allocate heavily in realtime audio paths.
- Log from realtime audio callbacks.
- Block realtime threads.
- Break mouse/keyboard workflows while adding touch support.
- Replace the design system without permission.
- Move realtime audio through Node/Electron.
- Call parent GPUI entity updates from child entity updates.

---

# 6. Build and Check Commands

Use the smallest relevant validation command.

Frontend:

```bash
bun run build
bun run typecheck
bun run lint
```

Electron:

```bash
bun run build
bunx electron-builder --dir
```

Rust workspace:

```bash
cargo check
cargo test
cargo build
cargo build --release
```

Native UI:

```bash
cargo check -p sphere_ui_components
cargo check --manifest-path apps/native/Cargo.toml
cargo clippy -p sphere_ui_components -- -D warnings
```

Native audio:

```bash
cargo check -p sphere-direct-audio-engine
cargo build -p sphere-direct-audio-engine
cargo test -p sphere-direct-audio-engine
```

Plugin host:

```bash
cargo check -p sphere_plugin_host
cargo build -p sphere_plugin_host
cargo build -p sphere_plugin_host --release
```

WASM audio:

```bash
bun run --cwd apps/web build:audio:wasm
```

C++ / CMake if present:

```bash
cmake --build build
cmake --build build --config Release
```

If a command does not exist:

- Do not invent success.
- Say it was unavailable.
- Run the closest safe command.

Never claim success if validation was not run or failed.

---

# 7. Theme and UI Rules

Futureboard UI style:

- dark native DAW
- compact
- professional
- Zed-ish / pro audio workstation feel
- 11–13px UI text where appropriate
- subtle borders
- clear focus states
- dense but readable

The visual source of truth is the existing polished DAW dialog/panel style
(`AddTrackDialog` / `DialogWindow` and the shared Settings components). Every
new surface must look like it already shipped inside this app. When in doubt,
copy the nearest existing polished surface — do not invent a new look.

Use global theme tokens.

Do not invent hardcoded colors.

If a token is missing, add a semantic token first.

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

Avoid:

- random hex colors inside components
- generic SaaS card styling
- oversized controls
- neon UI unless requested
- copied plugin branding
- one-off local palettes
- detached mock UI

All reusable controls should come from shared components where possible:

- `SettingsPage`
- `SettingsSection`
- `SettingsRow`
- `SettingsComboBox`
- `SettingsToggle`
- `BoxListView`
- `ColorPickerPopover`
- `IconButton`
- `ComboBox`
- `ContextMenu`
- `MessageBox`

---

# 7a. No Slop GUI — Hard Visual Discipline

Futureboard is a **professional desktop DAW**, not an AI-generated landing
page, not a SaaS dashboard, not a crypto app. "Slop GUI" is any default,
generic, AI-looking treatment that screams *template*. It is banned on sight.

## Banned outright (no slop)

- **Gradient slop.** No purple→blue/teal hero gradients, no gradient buttons,
  no gradient backgrounds, no gradient borders, no gradient text. Surfaces are
  flat theme tokens. The only acceptable gradient is a *functional* one a DAW
  actually needs — a meter scale, a fade curve, a waveform shade, an EQ/spectrum
  fill — and only when the feature genuinely requires it, built from theme
  tokens, never decorative.
- **Glassmorphism / frosted-glass / heavy blur** as a style. (A subtle popover
  backdrop is fine; "glass cards" are not.)
- **Glow, neon, drop-shadow soup.** One subtle floating-window shadow is the
  budget. No glowing accents, no colored shadows, no shadow on every element.
- **Bubbly oversized radius.** Compact DAW radius only. No pill-shaped giant
  buttons, no 16px+ rounded "cards."
- **Emoji as UI.** Never use emoji as icons, status markers, or button glyphs.
- **Generic SaaS layout.** No big hero headers, no centered marketing cards, no
  huge whitespace, no "feature card grid," no onboarding-wizard fluff.
- **Decorative anything.** No gratuitous accent bars, no rainbow category dots
  beyond the real color system, no animated sparkles/gradients.

## Required instead

- Flat, dark, dense, token-driven surfaces.
- Tight spacing tuned for desktop mouse/keyboard precision.
- Borders and elevation done with `border.subtle` / `surface.raised`, not
  shadow and glow.
- Color used for **meaning** (status, accent, transport, meter), not decoration.
- Native compact controls that match the Settings/dialog language.

If a design looks like it could be a generic AI app screenshot, it is wrong.
Make it look like a tool a mixing engineer uses for eight hours a day.


---

# 7c. Layout Discipline — No Flexbox Guessing, No Crooked Panels

Futureboard layouts must be engineered, not guessed. A DAW screen is a dense
instrument panel: one wrong flex value can break hit-testing, scroll geometry,
plugin editor embedding, timeline alignment, or mixer sizing.

## Prime layout rule

Before changing layout, identify the owning rectangle and the child rectangles:

```txt
window
  -> app chrome
  -> main content rect
    -> left browser
    -> arrangement/timeline rect
    -> inspector rect
    -> bottom panel rect
    -> status bar rect
```

Do not add layout wrappers until you know which rect they own.

Every major panel must have a clear contract:

```txt
owner
width / height source
min / max size
scroll owner
clip owner
overflow behavior
z-index/layer behavior
DPI coordinate space
```

If a patch changes layout without naming the owner rect and scroll/clip owner,
it is probably wrong.

## Banned layout behavior

- Random `flex: 1` on nested containers without a documented parent height.
- `height: 100%` chains where any parent has no explicit/min height.
- Mixing absolute positioning with flex children without a measured anchor rect.
- Hardcoded viewport constants when real chrome sizes exist.
- Magic offsets like `left: 220px`, `bottom: 48px`, `width: calc(100vw - 300px)`
  unless they are named constants derived from the layout model.
- Adding spacer divs to "make it look right".
- Letting child content decide the size of DAW panels that should be controlled
  by the shell.
- Using CSS grid/flex auto-placement for timeline, mixer, piano roll, ruler, or
  plugin editor host areas where exact alignment is required.
- Hiding layout bugs with `overflow: hidden` on the wrong parent.
- Fixing clipping by increasing padding until it "looks okay".
- Ignoring scrollbars, bottom panel height, inspector width, or collapsed browser
  state in geometry calculations.

## Required layout behavior

- One shell computes the main chrome metrics.
- Timeline/ruler/grid/clip geometry must use the same measured content rect.
- Left headers and scrollable content must be separate rects with explicit
  clipping.
- Bottom panel height must be part of timeline viewport math.
- Inspector/browser collapse state must be part of timeline viewport math.
- Every scrollable area has exactly one scroll owner.
- Every clipped drawing area has exactly one clip owner.
- Every floating overlay has a measured anchor and window-clamped bounds.
- Every resizable panel has min/max constraints.
- Every panel must behave correctly when:
  - window is maximized
  - window is very small
  - bottom panel is open/closed/resized
  - browser is hidden
  - inspector is visible
  - DPI scale changes
  - scrollbar appears/disappears

## Flexbox rules for Web/Tailwind surfaces

Flexbox is allowed for simple rows/columns, not for critical DAW geometry.

Good flexbox uses:

```txt
toolbar rows
button groups
settings rows
simple dialog content
menu items
small control clusters
```

Bad flexbox uses:

```txt
timeline ruler alignment
clip positioning
playhead alignment
piano roll note grid
mixer strip sizing
plugin editor viewport
waveform canvas sizing
scroll-synced lane/header layout
```

When using flexbox:

- Parent must have a known size.
- Children that shrink must set `min-width: 0` / `min-height: 0` where needed.
- Scroll children inside flex parents must explicitly use `min-height: 0`.
- Do not combine `gap` with pixel-perfect timeline coordinates.
- Do not rely on flex wrapping in DAW chrome unless explicitly requested.
- Do not use `justify-center` to hide incorrect width calculations.
- Do not use huge `p-*`, `m-*`, or `rounded-*` classes copied from SaaS UI.

Tailwind-specific bans in DAW UI:

```txt
p-6 p-8 p-10
text-xl text-2xl text-3xl
rounded-2xl rounded-3xl
shadow-xl shadow-2xl
backdrop-blur
bg-gradient-to-*
from-* via-* to-*
space-y-8 gap-8 gap-10
container mx-auto
max-w-7xl for app chrome
```

Use compact, intentional sizing instead.

## GPUI layout rules

- Prefer measured `Bounds`/rects from the shell over repeated local constants.
- Do not derive timeline body size from stale hardcoded sidebar widths.
- Do not let a child view mutate parent layout while the parent is rendering.
- Avoid deeply nested `relative()` containers unless each one owns a distinct
  coordinate space.
- If a child uses `absolute()`, its coordinate space must be documented by the
  parent.
- For scrollable DAW content, compute visible range from the scroll offset and
  content rect, not from full window bounds.
- For ruler/automation/tempo/time-signature lanes, labels must be nowrap,
  clamped inside the lane, and clipped by the lane content rect.

## Canvas/WGPU layout rules

Canvas/WGPU renderers must never guess dimensions from CSS alone.

Required:

```txt
logical width
logical height
device pixel ratio
scroll_x / scroll_y
zoom / px_per_beat or px_per_second
content origin
clip rect
```

Rules:

- Resize GPU/canvas surfaces only when measured size changes.
- Convert logical↔physical pixels explicitly.
- Keep ruler, grid, clips, playhead, automation, and MIDI notes on the same
  coordinate conversion path.
- Never draw outside the content clip rect unless the layer is intentionally
  global, like a floating overlay.
- Debug overlays must be env-gated.

## Plugin editor layout rules

Plugin editor hosting is not normal UI layout.

- The GPUI shell owns the outer plugin window/surface.
- The native child HWND/NSView owns the plugin editor client area.
- The plugin editor client rect must be measured, not guessed.
- Host chrome/header height must be subtracted before attaching/resizing the
  plugin view.
- VST3 `onSize()`/resize requests must update the host/client rect coherently.
- Do not put plugin child windows inside flexbox-like auto layout assumptions.
- Do not stretch plugin editors unless the plugin explicitly supports resize.
- Clamp plugin editor size to sane min/max bounds and monitor work area.

## Layout validation checklist

After any layout change, validate at least the relevant subset:

```txt
1. fresh launch default size
2. maximize window
3. resize narrower/wider
4. open/close browser
5. open/close inspector
6. resize bottom panel
7. scroll timeline horizontally and vertically
8. zoom timeline in/out
9. open popover/menu near every window edge
10. open plugin editor and resize if supported
11. test at 100%, 125%, 150% DPI where possible
12. verify no header bleed, no clipped labels, no phantom blank bands
```

If a patch affects timeline geometry, enable or add a debug flag like:

```txt
FUTUREBOARD_UI_DEBUG_CLIPS=1
FUTUREBOARD_TIMELINE_VIEWPORT_DEBUG=1
FUTUREBOARD_LAYOUT_DEBUG=1
```

Do not claim a layout fix is complete without checking the states it affects.

---

# 7d. No Slop Component Construction

A component is slop if it looks acceptable in isolation but breaks when placed
inside the real DAW shell.

## Required component contract

Every new reusable UI component must define:

```txt
size behavior: fixed / hug content / fill parent
min size
max size if any
keyboard focus behavior
mouse capture behavior
scroll behavior if any
empty state
loading state
error/disabled state
DPI behavior if custom drawing
```

Components must not secretly own app-level state or shell geometry.

## Banned component patterns

- Component-local mock data that looks real.
- Local persistent state that duplicates project state.
- Controls that visually work but do not call real callbacks.
- Buttons with no disabled/loading/error behavior.
- Menus/popovers that are not anchored to measured bounds.
- Dialogs that are not centered/clamped.
- Text labels that wrap in DAW chrome unless explicitly designed to wrap.
- Icons with hardcoded colors/sizes outside the shared icon system.
- One-off scroll containers inside another scroll container without a clear
  reason.
- Components that only look correct at one window size.

## Density contract

Default DAW chrome should be compact:

```txt
body/control text: 11–13px
secondary metadata: 10–12px
toolbar height: compact and consistent
buttons: desktop-sized, not web-landing-page-sized
rows: dense but readable
padding: intentional and small
```

Oversized font, padding, radius, and whitespace count as regressions unless the
user explicitly asks for a presentation/marketing surface.

## Visual QA before finishing UI tasks

Before reporting done, inspect the result against this checklist:

```txt
Does it match the nearest existing polished Futureboard surface?
Does it use theme tokens only?
Does it avoid gradient/glow/glass/card slop?
Does it keep compact DAW density?
Does it align with neighboring controls?
Does it survive resize/collapse/scroll/DPI?
Does it avoid wrapping/clipping text accidentally?
Does it have real callbacks or an explicitly disabled unfinished state?
```

If the answer is no, fix the component before claiming completion.

---

# 7b. Icon Rules

Icons are **SVG only**, from exactly three sources:

1. **Lucide** (`lucide` — primary set, preferred default)
2. **Tabler Icons** (when Lucide lacks a glyph)
3. **Custom in-house SVG** (hand-authored, committed to the repo asset/icon
   layer) for DAW-specific glyphs Lucide/Tabler do not cover (transport,
   meters, routing, MIDI, automation, plugin slots, etc.)

Hard rules:

- **No emoji** as icons, anywhere in product UI.
- **No icon fonts** (FontAwesome, Material Icons font, Ionicons, etc.).
- **No raster icons** (PNG/JPG/GIF) for UI chrome. SVG only.
- **No random one-off downloaded SVGs** pasted inline. Route every glyph through
  the shared icon component / asset layer.
- **Do not mix sets visually.** Prefer Lucide everywhere; reach for Tabler only
  when needed, matched to the same size and stroke weight so they read as one set.

Style contract:

- Icons inherit color from theme tokens via `currentColor` — never hardcode an
  icon fill/stroke color.
- Consistent size grid (e.g. 14/16/18/20px) and a single stroke width per scale.
- Monochrome by default; multi-color icons are not allowed for standard chrome.
- Render through the shared `IconButton` / icon component, not ad-hoc `<svg>` /
  `img` tags scattered in feature code.

Custom DAW icons:

- Author as clean single-path/stroke SVG on the same grid and stroke width as
  Lucide so they sit beside it seamlessly.
- Store in the shared asset/icon layer with a stable name; reference by name.
- No embedded gradients, filters, or raster data inside custom SVGs.

When a task needs a new glyph: check Lucide first, then Tabler, then author a
custom SVG that matches the set. Never fall back to emoji or an icon font.

---

# 8. GPUI / Native UI Rules

Native Studio uses GPUI for the app shell and controls.

Important GPUI rule:

> Do not update an entity while it is already being updated.

Avoid nested updates such as:

```txt
StudioLayout.update
  -> Timeline.update
    -> Timeline calls StudioLayout.update again
      -> GPUI double lease panic
```

Correct pattern:

- Child editor returns `CommandOutcome`.
- Parent applies dirty state after child update returns.
- Or child emits a queued event that parent drains later.
- Or command dispatcher owns the mutation.

Preferred result object:

```rust
pub struct CommandOutcome {
    pub changed: bool,
    pub project_dirty: bool,
    pub status: Option<String>,
}
```

StudioLayout should be the owner of:

- project dirty state
- route switching
- close/quit flow
- global command dispatch
- status bar messages
- save/load project actions

Timeline, MIDI Editor, Mixer, and Inspector should return outcomes, not directly update parent state during their own update.

---

# 9. Shortcut and Input Routing Rules

Global shortcuts must respect focus.

Priority:

```txt
1. modal dialog
2. text input / combo search / numeric edit / hex color input
3. MIDI editor
4. timeline
5. app/global
```

Do not trigger DAW shortcuts while the user is typing.

Required edit shortcuts:

Windows/Linux:

```txt
Ctrl+A  Select All
Ctrl+C  Copy
Ctrl+V  Paste
Ctrl+X  Cut
Delete  Delete
Backspace Delete where appropriate
```

macOS:

```txt
Cmd+A
Cmd+C
Cmd+V
Cmd+X
Delete/Backspace
```

These must work in:

- Timeline
- MIDI Editor / Piano Roll
- Velocity lane
- CC lane

These must not break:

- text input editing
- project name input
- BPM input
- ComboBox search
- color hex input

Use a central command enum:

```rust
pub enum EditCommand {
    SelectAll,
    Copy,
    Paste,
    Cut,
    Delete,
    Duplicate,
}
```

Do not duplicate shortcut logic in random components.

---

# 10. Timeline Rules

Timeline must have one coordinate system.

Ruler, grid, clips, loop region, selections, playhead, automation lanes, and MIDI clips must align.

Layering contract:

```txt
background
grid / ruler background
track lane backgrounds
clips / regions
selection / marquee preview
automation / note overlays where applicable
playhead line
playhead head
floating tools / handles
popover / modal
```

Rules:

- Do not draw grid above clips unless explicitly intended.
- Playhead should be visible above content.
- Marquee selection must be transient.
- Clip draw preview must be transient.
- Do not create real project clips on every mouse move.
- Commit only on mouse up.
- Escape cancels active gestures.
- Tool switch cancels active gestures.
- Lost focus cancels active gestures.

For MIDI clip drawing:

- Show live ghost clip preview.
- Show length in bars/beats while dragging.
- Snap to grid if snap is enabled.
- Enforce non-zero minimum length.
- Commit once on mouse up.

---

# 11. MIDI Editor Rules

MIDI Editor must become a real DAW editor, not a debug surface.

Core systems:

- Piano roll
- Note draw/select/move/resize/delete
- Copy/paste/cut/duplicate
- Mute/unmute notes
- Velocity lane
- CC lanes
- Pitch bend
- Channel pressure
- Quantize
- Snap
- Tool modes
- Focus-safe shortcuts
- Save/load roundtrip
- Runtime playback later/where applicable

Data model should support stable note IDs.

Example:

```rust
pub struct MidiNote {
    pub id: String,
    pub pitch: u8,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub velocity: u8,
    pub muted: bool,
    pub selected: bool,
}
```

CC model:

```rust
pub enum MidiControllerKind {
    CC(u8),
    PitchBend,
    ChannelPressure,
    PolyPressure,
}
```

```rust
pub struct MidiControllerPoint {
    pub id: String,
    pub beat: f32,
    pub value: f32,
    pub selected: bool,
}
```

MIDI Editor shortcuts should not delete timeline clips when the MIDI editor has focus.

---

# 12. Automation Rules

Automation must not be fake visual state.

Track volume automation must sync with:

- mixer fader
- inspector
- runtime audio value
- automation lane
- project state

Use separate concepts:

```txt
base value      = manual value
effective value = value currently heard/seen after automation
```

For track volume:

```rust
pub struct TrackVolumeState {
    pub base_db: f32,
    pub effective_db: f32,
    pub automation_read: bool,
}
```

Rules:

- Manual fader edits base value.
- Automation read updates effective value.
- Runtime uses effective value.
- Automation-follow must not trigger user fader command loops.
- Do not call `LoadProject` every automation tick.
- Do not rebuild the whole graph for every point drag.
- Do not allocate or lock in audio callback.

Use canonical targets:

```rust
pub enum AutomationTarget {
    TrackVolume { track_id: String },
    TrackPan { track_id: String },
    SendGain { track_id: String, send_id: String },
    PluginParameter { track_id: String, insert_id: String, parameter_id: String },
    MasterVolume,
    MasterPan,
    Tempo,
}
```

Do not create multiple string spellings for the same target.

---

# 13. Audio Realtime Rules

Realtime audio paths must avoid:

- allocation
- blocking locks
- filesystem I/O
- plugin scanning
- UI calls
- Node/Electron calls
- JSON parsing
- logging
- panics/exceptions
- unbounded queues
- waiting on UI thread

Allowed in realtime paths:

- preallocated buffers
- immutable snapshots
- atomic values
- bounded/lock-free queues if designed safely
- fixed-size event queues
- native DSP calls
- plugin process calls after setup

Preferred architecture:

```txt
UI command
  -> audio control thread
    -> immutable runtime snapshot / small command
      -> realtime callback
```

Never:

```txt
realtime callback -> UI
realtime callback -> filesystem
realtime callback -> Node
realtime callback -> plugin scanner
```

---

# 13a. Audio Hot Path Enforcement — No Exceptions

Any task touching `SphereDirectAudioEngine`, DAUx, plugin processing, recording,
MIDI playback, bridge processing, or stream callbacks must first classify every
changed function as one of:

```txt
Realtime hot path
Audio control path
Plugin host producer path
UI/control path
Build/test path
```

If a function can run from an audio callback, plugin process callback, bridge
producer cycle, or block renderer, treat it as realtime until proven otherwise.

## Mandatory realtime preflight

Before implementing an audio/plugin patch, inspect for existing behavior in the
exact files being changed:

```bash
rg "println!|eprintln!|dbg!|panic!|unwrap\(|expect\(" crates/SphereDirectAudioEngine crates/SpherePluginHost
rg "serde_json|HashMap|BTreeMap|Vec::new|collect::<Vec|\.clone\(\)" crates/SphereDirectAudioEngine crates/SpherePluginHost
rg "sleep\(|thread::sleep|Duration::from_micros|Duration::from_millis" crates/SphereDirectAudioEngine crates/SpherePluginHost
```

These greps are not proof by themselves, but they show danger zones. Do not
blindly delete everything. Understand whether the match is in tests/control code
or a hot path.

## Hard ban in realtime hot paths

Never add these to realtime code:

```txt
println!, eprintln!, dbg!
format!, serde_json, JSON parsing
filesystem/network/process calls
thread sleep / wait on UI thread
unbounded channel send/recv
Mutex/RwLock that can block
Vec/String allocation in steady state
HashMap lookup by String in steady state
panic/unwrap/expect
plugin scan/load/unload
project save/load
UI update / cx.notify / GPUI entity update
Node/Electron calls
```

## Required alternatives

Use:

```txt
preallocated buffers
fixed-size arrays or bounded rings
immutable runtime snapshots
compact numeric handles instead of String IDs
resolved enums instead of JSON/HashMap roles
atomic counters for diagnostics
bounded diagnostics ring drained by non-realtime thread
control-thread logging gated by debug flags
```

If a requested feature seems to require allocation/logging in the callback,
split it:

```txt
UI/control thread prepares immutable snapshot
audio callback reads snapshot without allocation
non-realtime drain thread prints diagnostics later
```

## Realtime patch report requirement

For audio/plugin patches, the final report must include:

```txt
Realtime safety:
- hot path functions touched
- allocations/logging/locks added: none, or explain why not hot path
- diagnostics mechanism used
```

Never claim "realtime safe" unless this check was actually done.


---

# 14. Audio Engine Rules

The UI controls audio but must not become the audio path.

Near-term:

- WebAudio / WASM remains fallback.
- Native engine is optional until stable.
- UI talks through adapter/controller boundaries.

Long-term:

- Rust DSP core
- SphereDirectAudioEngine
- DAUx
- native plugin graph
- WASM fallback for web
- separate native service where useful

Rules:

- React does not call `AudioContext` directly except through adapters.
- Native services communicate through safe IPC.
- Do not send huge audio buffers over JSON.
- Isolate transport/meter high-frequency updates.
- Do not full-rerender UI for every meter tick.

---

# 15. Audio Recording Rules

Audio Recording must be real and safe.

Minimum usable flow:

```txt
select input device/channel
arm audio track
press record
write WAV
stop
create audio clip
generate waveform
save/load with relative asset path
```

Recording must support:

- input/output channel enumeration
- track input route
- track output route
- record arm
- monitor Off/Auto/In
- transport record
- recording state machine
- WAV writer thread
- clip creation after stop
- waveform generation after record
- error handling
- project folder integration

Do not write to disk from the audio callback.

Use writer thread / ring buffer.

Recording state machine:

```txt
Idle
-> PrepareRecording
-> Recording
-> Finalizing
-> Complete
-> Idle
```

Common errors must be user-readable:

- no armed tracks
- no input device
- invalid channel
- unsaved project
- permission denied
- disk full
- writer failed
- input device disconnected

For unsaved projects, either:

- ask user to save before recording
- or record to temp and copy on Save

Do not silently lose recorded audio.

---

# 16. Plugin Host Rules

`SpherePluginHost` is the native plugin host layer.

Preferred architecture:

```txt
Electron / React
  -> preload IPC / native API
    -> PluginHost.node
      -> Rust N-API wrapper
        -> pure Rust host core / C ABI
          -> C++ VST3 / CLAP / AU host core
            -> plugin SDKs
```

Native Studio should use the pure host core without requiring N-API.

Do not duplicate the host bridge for Native and Electron.

Preferred split:

```txt
SpherePluginHost core  = pure Rust / C++ plugin host
napi wrapper           = thin Electron/Node wrapper only
```

Do not force JUCE into the project unless explicitly requested.

---

# 16a. Bridged Plugin Host Rules — No Fake Wiring

Bridged plugins are not allowed to have "looks connected" code paths that silently
do nothing. If a bridge structure exists, either wire it end-to-end or clearly
mark it unsupported in diagnostics/status.

The bridge must be treated as a cross-process realtime system:

```txt
Audio engine realtime callback
  -> shared memory request/audio/MIDI/param rings
  -> wake producer without polling
  -> plugin host producer
  -> plugin process call
  -> shared memory response
  -> engine freshness guard
```

## Producer wake rule

Do not use sleep polling as the primary bridge producer mechanism.

Bad:

```rust
loop {
    service_audio_bridge();
    std::thread::sleep(Duration::from_micros(250));
}
```

Required on Windows:

```txt
engine publishes request_seq
engine signals named event / SetEvent
host producer WaitForSingleObject
bounded timeout only for shutdown/watchdog
freshness guard remains in engine
```

Temporary hardening is acceptable but not a replacement for event-driven wake:

```txt
timeBeginPeriod(1)
MMCSS "Pro Audio" for producer thread
suitable thread priority
clean shutdown with AvRevertMmThreadCharacteristics/timeEndPeriod
```

If the event is not signaled, shutdown must not deadlock.

## Per-instance routing rule

Never broadcast MIDI or parameter events from one bridged insert to every loaded
plugin voice.

Every bridge region/ring/event must be routed by stable instance identity:

```txt
bridge region -> instance_id -> exact loaded voice/processor
```

Required tests or diagnostics:

```txt
MIDI event for instance A does not reach instance B
Param event for instance A does not reach instance B
Shutdown does not deadlock with no events
```

## Freshness guard rule

The freshness guard is correct. Do not remove it to hide timing bugs.

If stale audio appears:

```txt
fix producer scheduling/wake
fix bridge deadline
fix host processing time
do not bypass the guard permanently
```

Stale plugin output is worse than silence/dry fallback because it can feed old
audio into a new timeline position.

## Bridge diagnostics rule

Bridge diagnostics must not print from the audio callback.

Use:

```txt
bridge_missed_count
bridge_recovered_count
midi_ring_full_count
param_ring_full_count
diagnostics_ring_dropped_count
last_instance_id / last_seq / last_error on control side
```

Print only from a non-realtime thread and only behind debug flags.


---

# 17. PluginHost.node Rules

`PluginHost.node` is a control bridge, not the realtime audio path.

Good uses:

- initialize plugin host
- scan plugin folders
- list discovered plugins
- load/unload plugin instances
- query parameters
- set parameter values
- save/load plugin state
- open/close native plugin editor windows
- report plugin metadata

Bad uses:

- realtime audio processing through Node
- sending audio buffers through JSON
- blocking scan during playback
- loading untrusted plugins in UI process without isolation plan

Do not route:

```txt
AudioEngine -> Node -> PluginHost.node -> VST3
```

Preferred first API:

```ts
initPluginHost(): void
shutdownPluginHost(): void
scanVst3(paths: string[]): Promise<PluginInfo[]>
loadPlugin(options: LoadPluginOptions): string
unloadPlugin(instanceId: string): void
getParameters(instanceId: string): ParameterInfo[]
setParameter(instanceId: string, parameterId: number, normalizedValue: number): void
saveState(instanceId: string): Uint8Array
loadState(instanceId: string, state: Uint8Array): void
openEditor(instanceId: string, parentWindowHandle?: bigint): void
closeEditor(instanceId: string): void
```

JSON is acceptable for scanner/control metadata, not realtime.

---

# 18. VST3 / CLAP / AU Rules

VST3 may use `external/vst3sdk`.

Keep SDK use contained.

Recommended VST3 phases:

```txt
Phase 1 — scanner metadata
Phase 2 — load/unload component/controller
Phase 3 — parameters
Phase 4 — state save/load
Phase 5 — native processing path
Phase 6 — editor hosting
```

VST3 editor hosting:

- GPUI draws shell/header.
- Native child HWND/NSView hosts plugin editor.
- C++ host attaches `IPlugView`.
- Do not draw plugin UI with GPUI/WGPU.
- Do not use old NanoVG plugin editor path for Native.
- Do not open editor from audio thread.
- Close must call `removed()`/detach safely.
- Resize must call `onSize()`.

Windows:

- create child `HWND`
- style includes `WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN`
- must not be `WS_POPUP`
- attach VST3 platform type `"HWND"`

macOS:

- create child `NSView`
- attach VST3 platform type `"NSView"`

Linux:

- support depends on X11/Wayland/plugin format details
- scaffold safely if not implemented

---

# 18a. VST3 Correctness Rules — State, Context, Params, PDC

A VST3 plugin that opens its editor is not automatically "working." A DAW host is
not usable until state, timing, parameters, MIDI routing, and latency are wired
correctly.

## Plugin state is P0

Every live insert must be able to save and restore plugin state.

Required model:

```txt
Project insert
  -> plugin identity
  -> plugin format/class id
  -> component state blob
  -> controller state blob
  -> payload version
```

Required VST3 behavior:

```txt
IComponent::getState / setState
IEditController::getState / setState when available
opaque binary blobs
restore after instantiate, before playback/editor open
structured error if restore fails
```

Rules:

- Do not interpret plugin state bytes in Rust.
- Do not silently discard failed state restore.
- Do not fetch or restore state on the realtime callback.
- If project format is text-only, base64 is acceptable as a temporary layer.
- Missing state is okay; failed state must be surfaced.

## ProcessContext must be real

Never hardcode VST3 `ProcessContext` as production behavior.

Bad permanent values:

```txt
tempo = 120.0 forever
time signature = 4/4 forever
state = playing forever
projectTimeSamples never advances
```

Required per-block context where available:

```txt
sample_rate
block_frames
playing/stopped
recording if known
project_time_samples
continuous sample position
tempo at current timeline position
time signature at current timeline position
PPQ position if available
bar start PPQ if available
loop/cycle state if supported
```

Set VST3 context flags only for fields that are actually valid.

The engine already owns musical timing. The bridge/in-process processor must
receive that timing; do not duplicate fake host state inside the C++ bridge.

## Parameter automation must be end-to-end

A parameter event is not implemented until it reaches the target plugin instance.

Required chain:

```txt
automation lane / parameter command
  -> engine runtime event
  -> target insert identity
  -> in-process VST3 parameter queue
  or shared bridge param ring
  -> host producer pop
  -> exact plugin instance
  -> VST3 inputParameterChanges or documented fallback
```

Rules:

- Do not write only to a local HashMap for bridged plugins.
- Do not silently ignore unsupported bridged automation.
- Do not broadcast parameters to all loaded plugins.
- Include sample/block offset when available.
- Ring-full behavior must be non-blocking and reported via counters/diagnostics.

## PDC must include bridged plugins

Latency compensation must count:

```txt
in-process plugin reported latency
bridge handoff latency
bridged plugin reported latency
bus/return/send topology rules
```

Minimum bridged latency is at least one processing block if the architecture
requires a one-block handoff.

If plugin-reported latency is unavailable:

```txt
bridge_latency_samples = block_frames
```

If available:

```txt
bridge_latency_samples = block_frames + plugin_reported_latency_samples
```

Add tests for no-plugin, in-process plugin, bridge-minimum, and bridge+reported
latency cases where possible.


---

# 19. Plugin Crash Isolation

A `.node` module runs in the Electron process.

If a plugin crashes, the app may crash.

Short-term:

- acceptable for controlled experiments
- keep operations minimal
- fail safely when possible

Long-term:

```txt
Electron Renderer
  -> Electron Main / Preload
    -> plugin host client
      -> IPC
        -> FutureBoardPluginHost process
          -> VST3/CLAP/AU plugins
```

Long-term process should provide:

- crash isolation
- safer scanning
- recovery after plugin failure
- editor isolation where possible

Do not claim plugin hosting is production-safe until isolation exists.

---

# 20. Project, Assets, and Save Rules

Project data must have a single source of truth.

Project save should support portable projects.

Project folder structure may be:

```txt
My Project/
  My Project.fbs
  Assets/
    Audio/
    Audio/Recordings/
    MIDI/
    Samples/
    Presets/
    Templates/
    Other/
  Cache/
    Peaks/
    Waveforms/
    Analysis/
  Bounces/
  Exports/
  Backups/
```

When saving a new project from an unsaved workspace:

- create project folder
- copy referenced/imported assets
- rewrite project references as relative paths
- write project file atomically
- update recent projects
- clear dirty state only after success

Do not copy plugin binaries by default.

Do not duplicate assets repeatedly.

Use asset IDs or hashes where possible.

Atomic save pattern:

```txt
write temp file
flush if possible
rename temp to final
```

---

# 21. Welcome / Startup Rules

Welcome is a start hub, not a disconnected app.

Preferred boot:

```txt
load settings
create StudioLayout state
initialize renderer mode
initialize WGPU if enabled
warm audio if appropriate
show Welcome route
```

Welcome should support:

- New Project
- Open Project
- Recent Projects
- Continue Without Project
- Audio Setup
- Feeds
- Default Project Path

User can name a project directly in Welcome.

Continue Without Project:

- creates unsaved in-memory workspace
- does not create folder
- title shows Unsaved
- Save/Save As creates folder and copies assets

Open Project flow should live in Welcome.

Do not scatter open project dialogs across unrelated surfaces.

---

# 22. Window and Dialog Rules

Windows/dialogs must be centered and safe.

Unsaved changes dialog must support:

- Save
- Don’t Save
- Cancel

Rules:

- Cancel aborts destructive action.
- Escape behaves like Cancel.
- Do not lose project changes without explicit Don’t Save.

Windows positioning:

- do not default to `(0,0)` on Windows
- restore saved bounds only if valid
- otherwise center on monitor/work area
- clamp inside monitor
- handle DPI/logical vs physical pixels carefully
- use one shared helper for child windows

Debug flag:

```txt
FUTUREBOARD_WINDOW_POSITION_DEBUG=1
```

Dialog close flow:

- request close
- if dirty, show dialog
- perform pending close only after decision
- do not destroy immediately

Shutdown flow:

```txt
set shutting_down = true
stop transport
stop audio
stop workers
detach plugin editors
close queues
then close GPUI windows
```

Do not call `cx.notify()` after shutdown begins.

Use central helper:

```rust
crate::shutdown::is_shutting_down()
```

Avoid verbose duplicated `ShutdownState::global()` checks.

---

# 23. Settings UI Rules

Settings pages must use shared components.

Required shared components:

- `SettingsPage`
- `SettingsSection`
- `SettingsRow`
- `SettingsComboBox`
- `SettingsToggle`
- `SettingsSlider`
- `BoxListView`
- `RestartRequired` helpers

ComboBox rules:

- options must be deduped
- render must be pure
- do not append options during render
- anchor dropdown to trigger bounds
- refresh anchor on overlay render
- close or re-anchor on scroll
- clamp to window
- handle Windows DPI/coordinate conversions

MIDI settings:

- device lists should use `BoxListView`, not loose checkbox rows

Audio settings:

- backend/input/output/sample rate/buffer should use shared rows and ComboBoxes

Performance settings:

- renderer/GPU device should use ComboBox
- restart-required note should be centralized

---

# 24. Color Picker Rules

Use reusable `ColorPickerPopover`.

It should support:

- preset swatches
- custom hex color
- recent colors
- Auto Color
- save/load custom colors
- global theme tokens
- anchored popover

Do not limit project colors to only fixed swatches.

Do not create multiple color picker implementations.

Use in:

- Add Track
- Track Inspector
- Track Header
- Mixer
- Clips
- Automation lanes later

Persist stable color representation, preferably hex or a serializable color object.

---

# 25. State Ownership Rules

Single source of truth:

- tracks live in project state
- clips live in track/project state
- MIDI notes live in MIDI clip state
- mixer values live in track mixer state
- inserts live in `track.inserts`
- routing lives in track routing/sends
- plugin scan registry is separate from live instances
- automation lanes live in project/track automation state
- transient meters do not dirty project
- high-frequency transport movement is isolated/throttled

Never:

- create local-only fake persistent DAW state
- create a second insert chain outside `track.inserts`
- create a second mixer model outside track state
- create a disconnected plugin gallery as the real plugin system
- persist huge audio buffers in UI state

---

# 26. Platform Boundary Rules

Web:

- no Node APIs
- browser-safe fallbacks
- IndexedDB/OPFS where appropriate
- WebAudio/WASM fallback only

Electron:

- renderer uses preload IPC
- main handles filesystem/dialog/process
- no direct `fs`, `child_process`, or Electron imports inside React UI
- native modules loaded only in approved boundaries

Native service:

- spawned by main/native controller
- renderer/UI talks through safe boundary
- crash should not kill UI where possible
- fallback to WebAudio/WASM if unavailable

---

# 27. TypeScript Rules

Avoid syntax that may break strict/erasable TS settings.

Avoid:

- TypeScript parameter properties if repo rejects them
- huge `any`
- unchecked optional fields
- mutating state directly
- duplicate types in random files
- direct Electron imports inside React UI
- `fs` or `child_process` inside React UI

Prefer:

- explicit interfaces
- type guards
- normalized defaults
- selectors
- immutable updates
- small helpers
- adapter interfaces
- stable event payloads
- throttled high-frequency UI updates

---

# 28. Rust Rules

Do:

- isolate unsafe
- document every unsafe FFI assumption
- wrap raw pointers in handles
- use `Result<T, E>`
- avoid panics across FFI
- avoid locks/allocations in realtime paths
- keep N-API thin
- move heavy logic outside JS wrappers

Do not:

- expose raw C++ pointers to JS
- let panics cross FFI
- use global mutable state casually
- hold locks in realtime audio paths
- scan filesystem on audio thread
- serialize large buffers through JSON
- use Node for realtime processing

---

# 29. C++ Rules

C++ is allowed for native SDK integration.

Likely use cases:

- VST3SDK
- CLAP/AU host integration
- plugin editor views
- native HWND/NSView handling
- ABI bridge for Rust
- low-level OS audio/plugin APIs

Do:

- keep C++ behind small C ABI or explicit bridge
- isolate SDK headers
- avoid leaking SDK types into N-API
- own lifecycle clearly
- make destroy/free explicit
- keep platform-specific window code separated

Do not:

- spread SDK types through the app
- expose C++ exceptions across C ABI
- expose plugin pointers to JS
- allocate/log/block in realtime process functions
- make React depend on C++ headers
- merge UI window code into audio processing

---

# 30. Touch / Pen Rules

Use Pointer Events where applicable.

Support:

- mouse
- touch
- pen/stylus

Use:

- pointer capture
- touch-action control on DAW surfaces
- invisible touch hit targets where needed
- long press context menu where appropriate

Do not:

- make desktop UI huge
- break mouse/keyboard workflow
- turn the app into mobile layout
- break precise timeline editing

---

# 31. Long Task File Rules

Long task files are reference specs.

Correct:

```txt
Read tasks/native/midi-editor-plan.md.
Implement only Phase B.
```

Incorrect:

```txt
Implement all phases A-Z.
```

When a file is huge:

- search within it
- read requested section
- ignore unrelated sections
- ask only if scope is ambiguous
- otherwise implement first safe subset and report what remains

---

# 32. Dependency Rules

Do not add dependencies casually.

Before adding a dependency, check:

- is there already a project utility?
- does it work in Web/Electron/Native?
- does it break packaging?
- does it add native build complexity?
- is it compatible with Bun/Vite/Electron?
- is it safe for realtime/native boundaries?
- does it pull in JUCE or unwanted frameworks?

For plugin work:

- VST3SDK under `external/vst3sdk` is acceptable when requested.
- JUCE is not acceptable unless explicitly requested.
- CLAP SDK is acceptable only when explicitly requested.
- Hosting must stay modular by format.

---

# 33. Scanner / Registry Rules

Plugin scanner and plugin registry are not live plugin instances.

Scanner:

- discovers plugins
- reads metadata
- caches results if supported
- survives bad plugins where possible
- avoids blocking UI
- supports rescanning

Registry:

- stores known plugin metadata
- persists outside project state if appropriate
- used by plugin picker

Project inserts:

- store selected plugin identity
- store plugin state
- store parameter values/automation
- map to live instances only while project is loaded

---

# 34. Debug Flag Conventions

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
FUTUREBOARD_WAVEFORM_DEBUG=1
FUTUREBOARD_GPU_RENDERER_DEBUG=1
FUTUREBOARD_WELCOME_DEBUG=1
FUTUREBOARD_SHUTDOWN_DEBUG=1
```

Do not log every audio block unless throttled or ring-buffered.

---

# 34a. Audio/Plugin Debug Flags and Counters

Audio/plugin debug output must be opt-in, throttled, and non-realtime.

Recommended additional flags:

```txt
FUTUREBOARD_AUDIO_WATCHDOG_DEBUG=1
FUTUREBOARD_REALTIME_DIAGNOSTICS_DEBUG=1
FUTUREBOARD_BRIDGE_WAKE_DEBUG=1
FUTUREBOARD_BRIDGE_INSTANCE_DEBUG=1
FUTUREBOARD_VST3_CONTEXT_DEBUG=1
FUTUREBOARD_VST3_STATE_DEBUG=1
FUTUREBOARD_PARAM_BRIDGE_DEBUG=1
FUTUREBOARD_PDC_DEBUG=1
```

Required pattern:

```txt
audio callback:
  atomic counter / bounded diagnostics ring push
control thread:
  drains diagnostics
  checks debug env
  prints throttled summary
```

Never print one line per audio block by default.

Useful summary format:

```txt
[bridge] missed=12 recovered=11 midi_full=0 param_full=0 dropped_diag=0 last_seq=42
[vst3-context] instance=... playing=true tempo=128.0 sig=4/4 sample=123456
[pdc] track=... bridge=256 plugin=128 total=384
```


---

# 35. Reporting Format

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

Do not paste huge diffs unless asked.

If build fails:

- show concise error summary
- fix it if within scope
- otherwise stop and explain clearly

If task is too large:

- implement first safe subset
- validate
- say what remains

Never claim success if validation failed or was not run.

---

# 35a. Audio/Plugin Validation Checklist

For any patch touching audio engine, bridge, VST3, MIDI playback, recording, or
plugin host, run the smallest relevant commands and report failures honestly.

Minimum command candidates:

```bash
cargo check -p sphere-direct-audio-engine
cargo test -p sphere-direct-audio-engine
cargo check -p sphere_plugin_host
cargo test -p sphere_plugin_host
cargo check --manifest-path apps/native/Cargo.toml
```

If C++ bridge changed, also run the relevant CMake/Cargo build path used by the
repo. If the command does not exist, say so and run the closest available check.

## Required targeted validation by feature

Producer wake / bridge timing:

```txt
producer wakes without sleep polling
bounded timeout exits on shutdown
freshness guard still trips on stale data
no deadlock if event is never signaled
```

Plugin state:

```txt
state payload serializes/deserializes
component/controller blobs roundtrip
project reload restores stored state
restore failure surfaces insert warning/error
```

ProcessContext:

```txt
stopped transport reports stopped
playing transport advances projectTimeSamples
tempo map changes reach VST3 context
time signature map changes reach VST3 context
```

Parameter bridge:

```txt
instance A param does not reach instance B
ring full does not block realtime thread
unsupported automation is surfaced
```

PDC:

```txt
no plugin = 0
bridge plugin = at least block_frames
bridge plugin + reported latency = block_frames + reported latency
```

Realtime cleanup:

```txt
no new realtime println/eprintln/dbg
no new steady-state Vec/String allocation in touched hot path
diagnostics drain works off the audio thread
watchdog before/after noted if available
```

Do not claim "fixed dropout" solely because code compiles. Use diagnostics,
counters, or a reproducible manual test.


---

# 36. Good First Slices

## DAW UI

1. fix exact bug
2. isolate state
3. align coordinate math
4. reduce rerenders
5. build/check

## Timeline

1. fix hit test
2. fix gesture lifecycle
3. add preview state
4. commit on mouse up
5. validate shortcuts

## MIDI Editor

1. model/migration
2. note editing
3. clipboard
4. velocity
5. CC lanes
6. playback/runtime

## Automation

1. canonical target model
2. base/effective value split
3. UI sync
4. runtime sync
5. write modes later

## Audio Recording

1. device/channel enumeration
2. track input route model
3. arm/monitor UI
4. record validation
5. WAV writer thread
6. input capture
7. clip creation

## Plugin Host

1. scanner metadata
2. load/unload
3. parameters
4. state
5. process path
6. editor view

## Plugin Editor

1. GPUI shell
2. native child HWND/NSView
3. IPlugView attach
4. resize/detach
5. fallback UI

---

# 37. Anti-Patterns

These are warning signs:

```txt
"Implemented full DAW engine"
"Rewrote the whole timeline"
"Moved audio processing into React"
"Added plugin hosting while fixing UI colors"
"Added fake local mixer state"
"Scanner works but build was not run"
"VST3 host implemented with no crash/isolation note"
"Realtime path sends JSON to Node"
"Plugin editor embedded with raw HWND in React state"
"Fixed shortcut by updating parent from child entity"
"Called LoadProject on every automation tick"
"Recorded audio by writing file inside callback"
```

Stop and reduce scope.

---

# 38. Final Rule

Futureboard Studio is allowed to be ambitious.

Agent patches must be boring, scoped, buildable, and reversible.

Build the monster one safe organ at a time.
