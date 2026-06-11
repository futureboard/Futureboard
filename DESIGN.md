# Futureboard Studio DESIGN.md

This file is the **UI, layout, interaction, and visual-quality source of truth** for Futureboard Studio.

Futureboard Studio is a professional desktop DAW. It must feel like a serious tool that a musician, producer, mixing engineer, sound designer, or editor can use for hours without fighting the interface.

This document is intentionally strict. It exists to stop generic AI UI, layout drift, broken flexbox, oversized web controls, fake-looking mockups, and disconnected UI state.

---

# 0. Design Prime Directive

Futureboard is a **DAW**, not a website.

Every UI decision must optimize for:

```txt
precision
density
clarity
speed
musical timing
professional trust
long-session comfort
```

Before finishing any UI task, ask:

```txt
Would this feel native inside a desktop audio workstation?
Would this survive resize, DPI changes, scrolling, zooming, and 500 tracks?
Does this control affect real state, or is it fake decoration?
```

If the answer is no, the UI is not done.

---

# 1. Product Feel

Futureboard Studio should feel like:

- a modern DAW
- a desktop editor
- a technical creative workstation
- compact and fast
- calm, dark, and low-noise
- Ableton-like in workflow density
- Zed/Fleet-like in editor polish
- purpose-built for audio, MIDI, automation, plugins, and timeline editing

Futureboard Studio must **not** feel like:

- Bootstrap
- generic Tailwind demo UI
- SaaS dashboard
- admin panel
- crypto dashboard
- landing page
- mobile-first app
- oversized web form
- browser-default controls
- "AI-generated UI screenshot"

If a component looks like a web app template, it is wrong.

---

# 2. No Slop GUI Rules

"Slop GUI" means generic, decorative, oversized, inconsistent, or fake-looking UI that does not belong in a DAW.

## Banned outright

Do not use:

- decorative gradients
- purple/blue/teal hero gradients
- gradient buttons
- gradient borders
- gradient text
- glassmorphism
- frosted cards
- neon glows
- colored drop-shadow soup
- huge rounded cards
- pill-shaped giant buttons
- emoji as UI icons
- marketing-style hero sections
- big SaaS feature cards
- huge whitespace around dense tools
- random accent dots that do not encode real meaning
- default Tailwind dashboard layouts
- Bootstrap/modal/card/table vibes

Allowed only when functional:

- meter gradients
- waveform fade shading
- EQ/spectrum fills
- automation curves
- clip fade curves
- velocity/heat maps
- waveform peak shading

Even then, use theme tokens and keep it subtle.

## Required instead

Use:

- flat dark surfaces
- subtle borders
- compact controls
- token-driven color
- calm focus rings
- precise alignment
- dense but readable spacing
- native-feeling floating windows
- no decorative noise

Color must communicate meaning:

```txt
accent       = active/focused/selected intent
warning      = possible problem
danger       = destructive/failed
success      = complete/healthy
meter color  = audio level meaning
```

Color must not be random decoration.

---

# 3. Visual Direction

Core keywords:

```txt
compact
dark
technical
flat
native-feeling
audio-workstation
editor-like
low-noise
high-density
subtle borders
soft shadows
cyan accent
tabular numbers
musical grid
```

Contrast rules:

- Use strong contrast for active tools, playhead, selected clips, focused controls, record state, and destructive confirmations.
- Use low contrast for grid subdivisions, inactive panels, secondary labels, dividers, inactive meters, and disabled controls.
- Do not make every panel equally loud.
- Do not over-accent entire surfaces.

---

# 4. Theme Tokens

Use existing app theme tokens where possible.

Do not hardcode random colors inside components.

If a token is missing, add a semantic token first.

Preferred semantic token families:

```txt
surface.base
surface.panel
surface.raised
surface.sunken
surface.hover
surface.selected
surface.overlay

border.subtle
border.strong
border.focus
border.danger

text.primary
text.secondary
text.muted
text.faint
text.inverse

accent.primary
accent.hover
accent.active
accent.subtle
accent.border

status.success
status.warning
status.error
status.info

transport.play
transport.stop
transport.record
transport.loop
transport.metronome

meter.safe
meter.warn
meter.peak
meter.clip

timeline.grid.bar
timeline.grid.beat
timeline.grid.subdivision
timeline.playhead
timeline.selection
timeline.loop

automation.line
automation.point
automation.fill

plugin.header
plugin.surface
plugin.slot
```

Recommended base palette, if a local reference is needed:

```css
--fb-bg: #0b0f14;
--fb-panel: #11161d;
--fb-panel-2: #151a21;
--fb-surface: #1a2028;
--fb-surface-hover: #202733;
--fb-border: rgba(255, 255, 255, 0.08);
--fb-border-strong: rgba(255, 255, 255, 0.14);

--fb-text: #e7edf5;
--fb-text-muted: #9aa6b2;
--fb-text-faint: #66717f;

--fb-accent: #72d7d7;
--fb-accent-2: #4fb6c0;
--fb-accent-soft: rgba(114, 215, 215, 0.14);
--fb-accent-border: rgba(114, 215, 215, 0.36);

--fb-danger: #ef6b6b;
--fb-warning: #e2b866;
--fb-success: #80d18a;
```

Rules:

- Do not use bright Bootstrap blue as the default app accent.
- Do not paste arbitrary Tailwind color classes into feature components.
- Do not mix unrelated accent colors without semantic meaning.
- Component SVG icons must inherit `currentColor`.

---

# 5. Typography

Use Inter or the app font unless a platform-specific native font is already part of the design system.

## Sizes

Default guidance:

```txt
UI labels:              11-13px
section headers:        10-11px uppercase
buttons:                11-12px
inspector values:       11-13px
status text:            10-12px
timeline ruler labels:  10-12px
mixer labels:           10-12px
dialog titles:          12-14px
large headings:         welcome/onboarding only
```

Never use huge headings inside DAW working panels.

## Numeric values

Use tabular numbers for:

- time
- bars/beats
- BPM
- gain
- dB
- frequency
- Q
- pan
- velocity
- percentages
- CPU/FPS/memory
- samples/buffer size
- latency

CSS reference:

```css
font-variant-numeric: tabular-nums;
font-feature-settings: "tnum" 1;
```

## Wrapping

Rules:

- Timeline labels must not wrap vertically.
- BPM/time-signature pills must use nowrap.
- Mixer strip labels truncate, not wrap into tall strips.
- Plugin parameter labels should truncate with tooltip if needed.
- File names should ellipsize, not create horizontal overflow.

---

# 6. Density, Spacing, and Control Sizing

Futureboard is dense. Dense is good. Clutter is bad.

Recommended dimensions:

```txt
toolbar height:          30-38px
titlebar/transport:      34-44px
statusbar/footer:        22-28px
dialog titlebar:         28-34px
device/plugin header:    26-34px
inspector rows:          32-48px
settings rows:           42-58px
mixer strip width:       compact, stable, preferably 80-96px
small icon buttons:      22-28px
standard buttons:        26-32px height
compact inputs:          24-32px height
```

Avoid:

- `p-8`, `p-10`, `py-8`, `gap-8`
- huge vertical gaps
- mobile-sized switches
- large landing-page cards
- 40px+ buttons in dense work areas
- full-width primary web buttons unless it is a welcome/onboarding flow

Use spacing to group controls, not to decorate.

---

# 7. Borders, Radius, and Shadows

Recommended radius:

```txt
small controls:       5-7px
dropdowns/popovers:   8-12px
dialog windows:       10-14px
plugin/device panels: 8-12px
cards/pills:          6-10px
```

Rules:

- No bubbly 16px+ radius for standard controls.
- No `rounded-full` unless it is a real knob, meter dot, radio dot, or circular icon button.
- Use subtle borders for structure.
- Use shadows only for floating surfaces.

Allowed shadow surfaces:

- dialogs
- command palette
- dropdowns
- context menus
- floating plugin windows
- detached editors

Avoid shadows inside the main DAW panel grid.

---

# 8. Layout Discipline — No Flexbox Guessing

Layout bugs are product bugs.

Before writing or changing layout code, identify these four rectangles:

```txt
outer rect       = full component allocation
chrome rect      = headers/toolbars/fixed controls
content rect     = drawable/interactable content
scroll rect      = viewport that scrolls
```

For every complex surface, identify:

```txt
layout owner
scroll owner
clip owner
hit-test owner
draw owner
```

Do not mix these casually.

## Flexbox rules

Flexbox is allowed for simple rows/columns.

Flexbox is dangerous for:

- timeline ruler/grid alignment
- playhead positioning
- piano roll grid
- automation lanes
- waveform viewport
- plugin editor child window
- canvas/WGPU drawing surface
- mixer virtualization viewport

For complex editor surfaces, prefer explicit measured geometry.

Required flex hygiene:

```css
min-width: 0;
min-height: 0;
overflow: hidden; /* when this component owns clipping */
```

Use these on flex children that contain:

- text truncation
- scroll panes
- canvases
- timeline content
- inspectors
- plugin panels

If a flex child overflows horizontally, check `min-width: 0` before adding hacks.

## Banned layout patterns

Do not use:

```txt
left: 220px
width: calc(100vw - 220px)
height: calc(100vh - 140px)
margin-left: -...
magic negative margins
absolute positioning without a named owner rect
timeline math copied into multiple components
ruler width calculated differently than clip lane width
scroll offset applied in render but not hit-test
```

Hardcoded chrome sizes must not live inside leaf components.

Use measured layout metrics, shared constants, or parent-provided geometry.

## Tailwind / utility ban list

Inside DAW working surfaces, avoid:

```txt
container mx-auto
max-w-7xl
text-2xl text-3xl text-4xl
p-8 p-10 py-8
rounded-2xl rounded-3xl
shadow-xl shadow-2xl
bg-gradient-*
from-* via-* to-*
backdrop-blur*
glass*
```

These are usually website UI smells.

## Resize requirements

Every panel must survive:

- narrow width
- short height
- maximized window
- restored window
- bottom panel resize
- sidebar collapse
- inspector open/close
- high DPI
- Windows display scaling
- plugin window resize
- timeline horizontal scroll
- vertical track scroll
- zoom in/out

A layout is not correct until it survives these.

---

# 9. Canvas, WGPU, and DPI Rules

Canvas/WGPU surfaces must separate logical size from physical pixel size.

Track:

```txt
logical width/height      = CSS/layout points
physical width/height     = logical * devicePixelRatio
content origin            = where drawing starts
scroll offset             = horizontal/vertical timeline scroll
zoom scale                = px per beat/second/sample
```

Rules:

- Canvas backing size must match physical pixels.
- Drawing must scale for DPR.
- Hit testing must use the same coordinate conversion as rendering.
- Scroll and zoom math must be shared between ruler/grid/clips/playhead.
- Clip all drawing to the content rect.
- Do not draw timeline markers over the left header.
- Do not draw plugin/editor overlays outside their viewport.
- Do not create thousands of DOM grid lines.
- Render only the visible viewport plus small overscan.

Debug flags should exist for complex surfaces:

```txt
FUTUREBOARD_UI_DEBUG_CLIPS=1
FUTUREBOARD_TIMELINE_VIEWPORT_DEBUG=1
FUTUREBOARD_GPU_RENDERER_DEBUG=1
```

---

# 10. Dialog and Window Rules

All dialogs, floating windows, modal windows, confirmation prompts, settings panels, utility popups, and project flows must share one design language.

Use `DialogWindow` wherever possible.

Source of truth:

```txt
AddTrackDialog
DialogWindow
current polished Settings components
current compact DAW message box
```

Dialog shell:

- compact titlebar
- dark panel background
- subtle border
- soft floating shadow
- close button in titlebar
- consistent footer actions
- no thick card border
- no bright web modal background
- no huge opaque black backdrop by default

Backdrop rules:

```txt
floating utility dialog       = no backdrop
blocking confirmation         = subtle transparent blocker if needed
dangerous confirmation        = subtle backdrop allowed
project destructive action    = must offer Cancel
```

Dialog close rules:

- Escape behaves like Cancel where destructive action is possible.
- Cancel aborts the action.
- Do not destroy project state before confirmation.
- Do not create inconsistent dirty-state behavior.

If a dialog looks like Bootstrap, restyle it.

---

# 11. Forms and Controls

Native controls must be styled.

Inputs:

- dark surface
- subtle border
- compact height
- small radius
- cyan focus ring
- tabular numeric values where appropriate
- no white browser default fields

Selects/ComboBoxes:

- native `<select>` is acceptable only if styled and visually consistent
- if native popup looks broken, use shared ComboBox/DawSelect
- options must be deduped
- render must be pure
- do not append options during render
- dropdown must anchor to trigger
- dropdown must clamp to window
- dropdown must re-anchor on scroll/resize if still open

Toggles:

- compact
- dark inactive track
- subtle active state
- no giant mobile switches

Buttons:

- compact
- muted by default
- accent only when primary/active
- danger only for destructive actions
- no Bootstrap blue block buttons

Sliders/Faders/Knobs:

- compact but precise
- values visible or accessible
- drag sensitivity appropriate to parameter
- Shift/Ctrl fine/coarse modifiers where useful
- no web range input left unstyled in DAW panels

---

# 12. Icon Rules

Use a consistent SVG icon system.

Allowed:

1. Lucide
2. Tabler if Lucide lacks a suitable glyph
3. Custom in-house SVG for DAW-specific glyphs

Rules:

- SVG only for UI chrome.
- No emoji icons.
- No icon fonts.
- No random downloaded one-off SVG pasted inline.
- Icons inherit `currentColor`.
- Consistent size grid: 14/16/18/20px.
- Consistent stroke width.
- Monochrome by default.
- Every icon button needs title/aria-label where applicable.

Custom DAW icons must live in the shared asset/icon layer.

---

# 13. Menus, Dropdowns, Context Menus, Command Palette

Menus should feel like desktop app menus.

Rules:

- dark surface
- subtle border
- soft shadow
- compact rows
- icons aligned
- shortcut labels aligned right
- disabled items dimmed
- checked items use subtle accent
- separators subtle
- no huge row height
- no web list-card style

Command Palette:

- search-first
- command label + category path + shortcut
- keyboard navigation
- no web table
- no page route
- floating dialog style

Menu actions and command palette must share the same command registry.

Do not duplicate command definitions.

---

# 14. State Honesty

UI must not lie.

Rules:

- No fake success actions.
- No clickable controls without a callback unless intentionally disabled.
- No local-only persistent state that looks real.
- No mock device/plugin/project data in production surfaces.
- No automation lane that cannot affect runtime unless clearly marked disabled/TODO.
- No plugin parameter UI that does not reach the plugin/engine.
- No inspector value that does not affect playback/state.
- No settings page that writes nowhere.

If behavior is not implemented:

- disable the control
- show honest status
- add TODO in code
- do not pretend it works

---

# 15. Timeline and Arrangement

The timeline must feel musical and readable.

Rules:

- grid is musical, not a static table
- bar lines strongest
- beat lines medium
- subdivisions faint
- labels adapt to zoom
- no overlapping labels
- no vertical text stacking
- playhead aligns with ruler/grid
- loop region aligns with grid
- clip positions use the same beat math as grid
- markers are clipped to content rect
- left track headers must not be overdrawn by ruler/markers

Layering contract:

```txt
background
grid/ruler background
track lane backgrounds
clips/regions
clip ghost preview
selection/marquee preview
automation/note overlays
playhead line
playhead head
floating tools/handles
popover/modal
```

Interaction rules:

- Clip draw preview is transient.
- Real clips are created on mouse up, not every mouse move.
- Escape cancels active gestures.
- Tool switch cancels active gestures.
- Lost focus cancels active gestures.
- Ctrl/Cmd-click toggles selection.
- Marquee select does not split/cut clips.
- Selection-only actions do not dirty project unless state actually changes.

Zoom rules:

- smooth
- anchored to mouse or viewport center
- preserves beat under cursor
- grid density adapts
- does not jump to timeline start

Performance:

- render only visible lanes/viewport
- virtualize track rows
- no DOM grid spam
- no full rerender on playhead tick

---

# 16. MIDI Editor

MIDI Editor must be a real DAW editor, not a debug canvas.

Required systems:

- piano roll
- note draw/select/move/resize/delete
- copy/paste/cut/duplicate
- mute/unmute notes
- velocity lane
- CC lanes
- pitch bend
- channel pressure
- quantize
- snap
- tool modes
- focus-safe shortcuts
- save/load roundtrip
- runtime playback where applicable

Rules:

- MIDI editor shortcuts must not delete timeline clips when MIDI editor has focus.
- Velocity/CC lanes must align with note grid.
- Notes must use stable IDs.
- Muted notes must not play.
- Clipboard payload must be versioned.
- Paste position should be musically predictable.
- Quantize preview should be visual before commit where possible.
- Selection and edit commands must be undoable where applicable.

Rendering:

- high note counts must remain usable
- draw visible range only
- note labels appear only when readable
- no text wrapping inside notes
- no DOM-per-note if scale demands canvas/WGPU later

---

# 17. Automation

Automation must not be fake visual state.

Automation must sync with:

- relevant UI control
- runtime effective value
- project state
- automation lane
- save/load

Use separate concepts:

```txt
base value       = manual value
effective value  = value currently heard/seen after automation
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

- Manual edits change base value.
- Automation read updates effective value.
- Runtime uses effective value.
- Automation-follow must not trigger user command loops.
- Do not call LoadProject every automation tick.
- Do not rebuild the whole graph for every point drag.
- Do not allocate or lock in audio callback.
- Plugin parameter automation must reach in-process and bridged plugins or be clearly disabled.

Canonical target model:

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

# 18. Track Header

TrackHeader must remain compact and performant.

Required:

- track icon/type
- name
- channel number
- clip count/status
- mute/solo/arm/delete controls
- volume
- pan
- compact VU meter
- dB readout if space allows

Rules:

- long names truncate
- no horizontal overflow
- meter updates must not rerender every TrackHeader
- virtualize vertical track rows
- no full render of 500 headers when 20 are visible
- controls must remain usable at compact widths

VU meters:

- use canvas/ref/imperative drawing where possible
- no div-per-segment spam
- batch updates when possible

---

# 19. Mixer Strip

MixerStrip must scale to 128/256/500 channels.

Rules:

- horizontal virtualization
- master strip pinned
- visible strips + overscan only
- inserts/sends only render for visible strips
- meter updates isolated from parent rerenders
- compact fader UI
- pan/gain/mute/solo must remain usable
- strip width must be stable and intentional

Avoid:

- huge strip padding
- web card layout
- full render of all channels
- div-per-segment meters
- wrapping labels that increase strip height

---

# 20. Inspector

Inspector should feel like a DAW property panel.

Rules:

- fixed usable width
- no horizontal overflow
- vertical scroll only when content exceeds height
- long file names truncate
- sections compact
- labels uppercase/muted
- values tabular
- controls aligned
- no fake controls

Audio Inspector should include:

- gain
- mute
- fades
- timing
- speed/pitch
- processing mode/status
- source info

Inspector controls must affect actual playback/state.

---

# 21. Audio Editor

Audio Editor must be usable, not cramped or fake.

Rules:

- waveform/editor area uses `min-w-0 flex-1`
- inspector fixed width around 300-340px unless design says otherwise
- no horizontal overflow
- process buttons fit or stack
- source file name ellipsized
- footer/status must not overlap content
- waveform visual duration must match effective audio duration

Audio edits should update realtime when possible.

If editing gain, pitch, or speed requires pressing Spacebar to refresh, that is a bug.

---

# 22. Plugin UI and Effect Editor

Built-in plugins must look like stock DAW devices.

Plugin UI must NOT look like:

- raw HTML mockup
- huge web graph panel
- default form controls
- random plugin skin unrelated to Futureboard
- SaaS settings card

Plugin UI rules:

- constrained max width unless feature requires large graph
- compact header
- styled controls
- device shell
- subtle border
- cyan accents
- readable graph/grid
- tabular values
- no pointless empty space
- no stretching stupidly wide

Effect Editor can be wide, but devices should have intentional layout.

Built-in plugins should use Core + Editor separation.

Core:

- no React imports
- metadata
- default params
- parameter schema
- normalization
- DSP hooks/helpers

Editor:

- UI controls
- parameter editing
- graph/editor interaction

Registry:

- built-in plugins registered by plugin id
- Effect Editor loads editor from registry
- InsertDevice stores pluginId + params
- do not hardcode each plugin directly into MixerPanel/EffectEditor

---

# 23. External Plugin Editor Window Rules

External plugin editors are native child views, not web components.

Windows:

- parent shell is Futureboard/GPUI
- child view is plugin-owned HWND/NSView
- child rect equals content rect
- no overlap with titlebar
- no drawing outside shell
- resize host and plugin consistently
- handle DPI per monitor
- detach plugin view before destroying parent
- do not open editor from audio thread

VST3 Windows style reference:

```txt
WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN
```

Must not be `WS_POPUP` when embedded.

Plugin editor layout bugs are not cosmetic; they can break focus, mouse, keyboard, and lifecycle.

---

# 24. Settings / Preferences

Settings must look like native editor preferences.

Rules:

- compact sidebar
- subtle selected state
- row-based layout
- small uppercase section headers
- compact controls
- subtle footer
- no web dashboard cards
- no mock device lists
- refresh scans off-render
- device lists use shared list components
- controls persist to actual settings store

Settings must separate:

- App Preferences
- Project Settings
- Runtime Audio State
- Account/Cloud Settings
- Experimental/Developer Settings

Do not mix all settings into one random object.

---

# 25. Project Wizard / Welcome / New Project

New Project must follow compact DAW dialog style.

Rules:

- no Bootstrap form layout
- no web registration form
- compact project parameter grid
- styled controls
- template selector as compact cards/pills
- subtle summary strip
- consistent footer buttons

Welcome is a start hub, not a marketing landing page.

Welcome may be more spacious than DAW panels, but still must feel like Futureboard Studio.

---

# 26. Keyboard Shortcuts UI

Keyboard Shortcuts belongs inside Settings/Preferences.

Rules:

- command registry is source of truth
- no hardcoded duplicate shortcut table
- search/filter
- keycap styling
- category groups
- disabled editing if editor not implemented
- menu item opens Settings at Shortcuts tab

Must not be a Bootstrap table.

Global shortcuts must respect focus.

Priority:

```txt
modal dialog
text input / combo search / numeric edit / color hex input
MIDI editor
timeline
app/global
```

Typing into inputs must never trigger transport or edit commands.

---

# 27. Audio Processing UI

Speed/Pitch must be honest.

Controls:

- Speed
- Pitch
- Preserve Pitch
- Mode: Resample, Monophonic, Polyphonic, Percussive, Granular/Texture
- Quality
- Status

Rules:

- do not claim Polyphonic if it uses Granular secretly
- status must show Rust/WASM/TypeScript fallback/cached/processing/failed
- pitch/speed changes must update active backend
- cache keys must include audio processing params
- waveform visual duration must match effective audio duration

Gain:

- inspector gain must affect playback
- gain should not invalidate pitch/speed cache unless required

---

# 28. Waveform Rendering

Waveforms should be cache-driven.

Rules:

- decode once
- generate peak cache
- use multi-resolution peaks where possible
- draw from cache
- do not regenerate peaks every render
- do not render waveform via DOM
- use canvas/WebGL/WebGPU when needed
- stream large files from disk where possible
- avoid decode-to-RAM explosion for huge WAV files

Waveform visual width must reflect effective clip duration:

```txt
2.0x speed = half width
0.5x speed = double width
pitch preserve only = width unchanged
```

---

# 29. Performance Rules

Futureboard must scale.

Targets:

```txt
128 tracks: smooth
256 tracks: usable
500 tracks: should not freeze
1000 tracks: stress/debug target
```

Rules:

- virtualize track headers/lanes
- virtualize mixer strips
- canvas/WGPU for grid/waveform/overlays/meters where appropriate
- no DOM spam for realtime visuals
- no React parent rerender cascade on meters/playhead
- no processing inside React render
- no giant arrays created every animation frame
- no filesystem/device scans during render
- no sync path stats on UI thread for recent projects

Realtime visuals:

```txt
playhead  -> canvas/ref/imperative
VU meters -> canvas/ref/imperative
waveform  -> cache + canvas
grid      -> canvas/WGPU
```

React/GPUI declarative UI is for controls, panels, menus, inspectors, dialogs — not per-frame render surfaces.

---

# 30. Client-Side Processing Architecture

Futureboard is client-first.

Browser:

- CSR
- WebAudio
- AudioWorklet
- Rust WASM DSP
- Canvas rendering
- IndexedDB/OPFS cache

Electron:

- local filesystem
- native dialogs
- temp/cache folders
- optional native service bridge
- renderer uses preload IPC

Native:

- Rust/GPUI
- SphereDirectAudioEngine / DAUx
- SpherePluginHost
- WGPU/Sphere graphics where applicable

Server:

- serve/compile/static host
- optional OAuth/cloud sync
- optional storage
- not required for realtime DAW processing

Do not route realtime audio through server or Node.

---

# 31. Storage Philosophy

Storage should be provider-based.

Providers:

- local browser storage
- local filesystem via Electron
- native filesystem
- Futureboard Cloud
- Google Drive
- network storage through mounted filesystem or gateway
- SMB/NFS where supported

Project manifest references assets.
Local cache accelerates work.
Source storage remains provider-backed.

Do not hardcode local paths as the only project truth.

---

# 32. Component Construction Contract

Every new component must define:

```txt
purpose
owner state
input props
output callbacks/events
loading state
empty state
error/disabled state
focus behavior
keyboard behavior
minimum size
maximum/overflow behavior
scroll owner
visual source of truth
```

Before coding, identify the nearest existing polished component and match it.

Reusable UI should use shared components where possible:

```txt
DialogWindow
SettingsPage
SettingsSection
SettingsRow
SettingsComboBox
SettingsToggle
SettingsSlider
BoxListView
IconButton
ComboBox
ContextMenu
MessageBox
ColorPickerPopover
```

Do not create a new one-off control if a shared one exists.

---

# 33. UI Review Checklist

Before finishing any UI task, check:

- [ ] Does it match Futureboard DAW style?
- [ ] Does it avoid Bootstrap/WebUI/admin/SaaS vibes?
- [ ] Does it reuse shared components?
- [ ] Does it use theme tokens?
- [ ] Does it avoid hardcoded colors?
- [ ] Is spacing compact?
- [ ] Are controls 11-13px where appropriate?
- [ ] Does it work at laptop size?
- [ ] Does it work maximized?
- [ ] Does it avoid horizontal overflow?
- [ ] Does text truncate instead of wrapping badly?
- [ ] Does it survive sidebar/inspector/bottom-panel changes?
- [ ] Does it survive high DPI?
- [ ] Does it avoid DOM spam?
- [ ] Does it scale to many tracks/channels?
- [ ] Does UI state connect to real behavior?
- [ ] Are incomplete actions disabled or honestly labeled?
- [ ] Does build/check pass?

If the UI looks like a generic website, it is not done.

---

# 34. Layout QA Checklist

Before finishing any layout-sensitive task, test or reason through:

```txt
normal window
narrow window
short window
maximized window
sidebar hidden/shown
inspector hidden/shown
bottom panel collapsed/expanded/resized
timeline horizontally scrolled
timeline vertically scrolled
zoomed far in
zoomed far out
DPI 100%
DPI 125/150/200%
long labels/file names
empty state
many tracks/channels/items
```

For timeline/editor/plugin surfaces, verify:

- ruler/grid/content align
- hit-test matches drawing
- playhead aligns with grid
- labels do not stack vertically
- markers do not draw over headers
- popovers clamp to window
- scrollbars do not cover content incorrectly
- child plugin window matches content rect

---

# 35. Agent Instructions

When implementing UI:

1. Inspect existing polished components first.
2. Reuse shared dialog/control/panel patterns.
3. Do not invent a new style.
4. Keep DAW density.
5. Avoid generic Tailwind examples.
6. Define layout owner/scroll owner/clip owner.
7. Make the UI work, not just appear.
8. If behavior is not implemented, show honest disabled/TODO state.
9. Run the smallest relevant build/check.
10. Do not claim success if only UI changed.

When unsure, prefer:

```txt
compact
subtle
dark
editor-like
DAW-like
performant
token-driven
```

---

# 36. Final Rule

Futureboard Studio can be ambitious, but its UI must be disciplined.

Build the interface like a real tool:

```txt
no slop
no fake state
no layout guessing
no uncontrolled overflow
no decorative noise
no generic web UI
```

If it would embarrass a DAW user after eight hours of editing, fix it.
