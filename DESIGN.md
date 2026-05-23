# Futureboard Studio DESIGN.md

This file is the UI and interaction source of truth for Futureboard Studio.

Futureboard is a DAW, not a web dashboard. Every UI element must feel like it belongs inside a serious desktop audio workstation.

---

## 1. Product Feel

Futureboard Studio should feel like:

- a modern DAW
- a desktop editor
- a professional creative tool
- Ableton-like in compact device/workflow thinking
- dark, calm, technical, and fast

Futureboard Studio must NOT feel like:

- Bootstrap
- generic Tailwind demo UI
- SaaS dashboard
- mobile-first form app
- admin panel
- browser default HTML form
- random oversized website UI

If a component looks like a web form, it is wrong.

---

## 2. Visual Direction

Keywords:

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
soft borders
subtle shadows
cyan accent
```

Use contrast carefully:

- strong contrast for active tools, playhead, selected clips, focused controls
- low contrast for inactive panels, secondary labels, grid lines, dividers

---

## 3. Theme Tokens

Use existing app theme tokens where possible.

Recommended base colors:

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

Do not use bright Bootstrap blue as the default app accent.

---

## 4. Typography

Use Inter or the app font.

General rules:

- UI labels: 11-13px
- section headers: 10-11px uppercase
- buttons: 11-12px
- inspector values: 11-13px
- status text: 10-12px
- large headings only in onboarding/project hub, not DAW panels

Use tabular numbers for time, bar.beat, BPM, gain, dB, frequency, Q, pan, percentages, px/beat, CPU/FPS/memory.

```css
font-variant-numeric: tabular-nums;
font-feature-settings: "tnum" 1;
```

---

## 5. Spacing and Density

This is a DAW. Dense is good. Clutter is bad.

Use compact spacing:

- toolbar height: 30-38px
- titlebar/transport: 34-44px
- inspector rows: 32-48px
- settings rows: 46-58px
- device header: 26-34px
- footer/statusbar: 22-28px

Avoid large web card padding, huge vertical gaps, oversized inputs, and mobile-size controls unless in touch mode.

---

## 6. Borders, Radius, Shadows

Suggested radius:

```txt
small controls: 5-7px
dialog windows: 10-14px
plugin/device panels: 8-12px
cards/pills: 6-10px
```

Borders should be subtle:

```css
border: 1px solid rgba(255, 255, 255, 0.08);
```

Use shadows for dialogs, command palette, dropdowns, context menus, and floating plugin windows.
Avoid heavy web card shadows inside main DAW panels.

---

## 7. Dialog and Window Rules

All dialogs must share the same design language.

Use `DialogWindow` whenever possible.

Dialogs must look like compact native DAW/editor windows.

They must NOT look like:

- Bootstrap modals
- web admin forms
- generic Tailwind modals
- mobile sheets

Dialog shell rules:

- rounded corners
- subtle border
- soft floating shadow
- compact titlebar
- no heavy black backdrop by default
- dark panel background
- close button in titlebar
- consistent footer actions

Backdrop rules:

- floating dialogs: no backdrop
- blocking dialogs: transparent interaction blocker if needed
- dangerous confirmations: subtle backdrop allowed

If a dialog looks like Bootstrap, restyle it.

---

## 8. Forms and Controls

Native controls must be styled.

Inputs, selects, toggles, sliders, and buttons must match Futureboard DAW style.

Inputs:

- dark surface
- subtle border
- compact height
- small radius
- cyan focus ring
- tabular numeric values

Selects:

- native `<select>` is acceptable only if styled
- if native popup looks broken, use custom `DawSelect`
- never leave white OS/browser dropdowns in main UI

Toggles:

- compact, not giant mobile switches
- dark inactive track
- cyan/blue active state

Buttons:

- compact
- muted by default
- accent only when primary/active
- no Bootstrap blue block style

---

## 9. Menus, Dropdowns, Command Palette

Menus should feel like desktop app menus.

Rules:

- dark surface
- subtle border
- soft shadow
- compact rows
- icons aligned
- shortcut labels aligned right
- disabled items dimmed
- checked items use subtle cyan accent
- separators subtle

Command Palette:

- CmdK style
- search-first
- command label + category path + shortcut
- keyboard navigation
- no web table
- no page route
- floating dialog style

Menu actions and command palette must share the same command registry.
Do not duplicate command definitions.

---

## 10. Timeline and Arrangement

The timeline must feel musical and readable.

Rules:

- grid is musical, not a static table
- bar lines strongest
- beat lines medium
- subdivisions faint
- labels adapt to zoom
- no overlapping labels
- playhead aligns with ruler/grid
- loop region aligns with grid
- clip positions use the same beat math as grid

Rendering:

- use canvas for grid, waveform, playhead overlays where possible
- do not create thousands of DOM grid lines
- render only visible viewport
- support 128/256/500 tracks with virtualization

Zoom:

- smooth
- anchored to mouse or viewport center
- does not jump to start
- preserves beat under cursor
- grid density adapts

---

## 11. Track Header

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

Performance:

- virtualize vertical track rows
- meter updates must not rerender every TrackHeader
- use canvas/gradient/imperative VU meter
- do not render 500 headers if only 20 are visible

---

## 12. Mixer Strip

MixerStrip must scale to 128/256/500 channels.

Rules:

- horizontal virtualization
- master strip pinned
- visible strips + overscan only
- no full render of all channels
- meter updates isolated from React rerenders
- inserts/sends only render for visible strips
- compact fader UI
- pan/gain/mute/solo must remain usable

VU meters:

- never use div-per-segment spam
- use canvas or single-div gradient
- batch meter rendering when possible

---

## 13. Inspector

Inspector should feel like a DAW property panel.

Rules:

- fixed usable width
- no horizontal overflow
- long file names truncate
- sections compact
- labels uppercase/muted
- values tabular
- controls aligned
- vertical scroll only when content exceeds height

Audio Inspector must include:

- gain
- mute
- fades
- timing
- speed/pitch
- processing mode/status
- source info

Inspector controls must affect actual playback/state. No fake UI.

---

## 14. Audio Editor

Audio Editor must be usable, not cramped.

Rules:

- waveform/editor area `min-w-0 flex-1`
- inspector fixed width around 300-340px
- no horizontal overflow
- process buttons must fit or stack
- source file name ellipsized
- footer/status must not overlap content

Audio edits should update realtime when possible.

If editing gain, pitch, or speed requires pressing Spacebar to refresh, that is a bug.

---

## 15. Plugin UI

Built-in plugins must look like stock DAW devices.

Plugin UI must NOT look like:

- raw HTML mockup
- huge web graph panel
- default form controls
- random plugin skin unrelated to Futureboard

Plugin UI rules:

- constrained max width
- balanced graph/control layout
- compact header
- styled controls
- device shell
- subtle border
- cyan accents
- graph/grid readable
- values tabular
- no pointless empty space

Effect Editor can be wide, but plugin UI should not stretch stupidly wide.

---

## 16. Built-in Plugin Architecture

Plugins should use Core + Editor separation.

Core:

- no React imports
- metadata
- default params
- param schema
- normalization
- DSP hooks/helpers

Editor:

- React UI
- parameter controls
- graph/editor interaction

Registry:

- built-in plugins are registered by plugin id
- Effect Editor loads editor from registry
- InsertDevice stores pluginId + params
- do not hardcode each plugin directly into MixerPanel/EffectEditor

---

## 17. Keyboard Shortcuts UI

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

---

## 18. Settings / Preferences

Settings must look like a native editor preferences window.

Rules:

- compact sidebar
- subtle selected state
- row-based settings layout
- small uppercase section headers
- compact controls
- subtle footer
- no web dashboard cards

Settings must separate:

- App Preferences
- Project Settings
- Runtime Audio State
- Account/Cloud Settings
- Experimental/Developer Settings

Do not mix all settings into one random object.

---

## 19. Project Wizard / New Project

New Project must follow Add Track dialog style.

Rules:

- no Bootstrap form layout
- template selector as compact DAW cards/pills
- compact project parameters grid
- styled controls
- subtle summary strip
- consistent footer buttons

If it looks like a web registration form, it is wrong.

---

## 20. Selection and Marquee

Terminology:

- Marquee / Rectangle Select = select only
- Snip / Split = cut/split clips
- Snap = grid alignment

Do not confuse these.

Ctrl/Cmd click:

- toggle selection

Ctrl/Cmd drag for selection:

- draw rectangle
- select objects covered/intersecting
- do not split
- do not cut
- do not mark project dirty
- do not create undo entry

Split commands are separate.

---

## 21. Audio Processing UI

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
- status must show Rust/WASM, TypeScript fallback, cached, processing, failed
- pitch/speed changes must update Rust backend when Rust is active
- cache keys must include audio processing params
- waveform visual duration must match effective audio duration

Gain:

- inspector gain must affect playback
- use GainNode when possible
- gain should not invalidate processed pitch cache

---

## 22. Waveform Rendering

Waveforms should be cache-driven.

Rules:

- decode once
- generate peak cache
- use multi-resolution peaks if possible
- draw from cache
- do not regenerate peaks every render
- do not render waveform via DOM
- use canvas/WebGL/WebGPU later if needed

Waveform visual width must reflect effective clip duration.

Speed:

- 2.0x = half width
- 0.5x = double width

Pitch preserve:

- pitch change alone should not change visual width

---

## 23. Performance Rules

Futureboard must scale.

Targets:

- 128 tracks: smooth
- 256 tracks: usable
- 500 tracks: should not freeze

Rules:

- virtualize track headers/lanes
- virtualize mixer strips
- canvas for grid/waveform/overlays/meters where appropriate
- no DOM spam for realtime visuals
- no React parent rerender cascade on meters/playhead
- no processing inside React render
- no giant arrays created every animation frame

Realtime visuals:

- playhead via canvas/ref
- VU meters via canvas/ref
- waveform via canvas
- grid via canvas

React:

- controls, panels, menus, inspectors, dialogs
- not per-frame render surfaces

---

## 24. Client-Side Processing Architecture

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

Native:

- future SphereEngine/Skia final boss

Server:

- serve/compile/static host
- optional OAuth/cloud sync
- optional 2GB user storage
- not required for realtime DAW processing

---

## 25. Storage Philosophy

Storage should be provider-based.

Providers:

- local browser storage
- local filesystem via Electron
- Futureboard Cloud
- Google Drive
- network storage via gateway/Electron
- SMB/NFS through mounted server or native client

Project manifest references assets.
Local cache accelerates work.
Source storage remains provider-backed.

Do not hardcode local paths as the only project truth.

---

## 26. Icons

Use a consistent icon system.

Preferred:

- lucide-react
- Tabler icons if already used

Rules:

- do not mix random icon styles
- icon stroke widths should feel consistent
- icons must not disappear due to wrong library/import
- all icon buttons need title/aria-label

---

## 27. UI Review Checklist

Before finishing any UI task, check:

- [ ] Does it match Futureboard DAW style?
- [ ] Does it avoid Bootstrap/WebUI/admin vibes?
- [ ] Does it reuse shared components?
- [ ] Does it use dark styled controls?
- [ ] Is spacing compact?
- [ ] Does it work at laptop size?
- [ ] Does it avoid horizontal overflow?
- [ ] Does it avoid DOM spam?
- [ ] Does it scale to many tracks/channels?
- [ ] Does UI state actually connect to behavior?
- [ ] Does build pass?

If the UI looks like a generic website, it is not done.

---

## 28. Agent Instructions

When implementing UI:

1. Inspect existing polished components first.
2. Reuse shared dialog/control/panel patterns.
3. Do not invent a new style.
4. Keep DAW density.
5. Avoid generic Tailwind examples.
6. Make the UI work, not just appear.
7. If behavior is not implemented, show honest disabled/TODO state.
8. Run build.
9. Do not claim success if only UI changed.

When unsure, prefer compact, subtle, dark, editor-like, DAW-like, and performant.

## Theme Rules

- Components must use the project theme tokens from `theme.ts`.
- Do not invent hardcoded colors.
- Do not add arbitrary Tailwind colors unless approved.
- If a color is missing, add a semantic token first.
- Prefer semantic names:
  - `surface.base`
  - `surface.panel`
  - `surface.raised`
  - `border.subtle`
  - `border.strong`
  - `text.primary`
  - `text.secondary`
  - `accent.primary`
  - `status.success`
  - `status.warning`
  - `status.error`
- UI must remain dark, compact, and DAW-native.
