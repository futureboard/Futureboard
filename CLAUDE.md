Before implementing task specs, read `tasks/SKILL.md`.

## PluginHost

if want edit/create PluginHostWrapper ??

- crates/SpherePluginHost
- external/vst3sdk
- external/clap
  **Support**: AU, LV2 and Linux/MacOS

## Dialog / Window UI Consistency Rules

All dialogs, floating windows, modal windows, project wizards, settings panels, confirmation prompts, and utility popups must share the same Futureboard Studio dialog design language.

### Source of Truth

Use the existing `AddTrackDialog` / current polished DAW dialog style as the visual source of truth.

Do not invent a new dialog style for each feature.

Every dialog must feel like it belongs to the same desktop DAW application.

### Visual Style

Dialogs must look like compact native DAW/editor windows, inspired by:

- Zed editor preferences/dialogs
- Ableton-style compact device/editor panels
- Futureboard Studio dark DAW theme

Dialogs must NOT look like:

- Bootstrap modals
- generic web admin forms
- SaaS dashboard cards
- mobile-first forms
- browser default HTML UI
- plain Tailwind demo components

### Dialog Shell

Use the shared `DialogWindow` component wherever possible.

Default dialog shell:

- rounded corners
- no dark opaque backdrop unless explicitly required
- subtle border
- soft shadow
- dark surface background
- compact titlebar
- close button in titlebar
- z-index above menus/panels
- no thick card-like border
- no bright web modal background

Preferred style:

```txt
background: dark panel / near-black blue-gray
border: subtle rgba white border
shadow: soft floating window shadow
radius: rounded but not bubbly
titlebar: compact 28-34px
```

and read DESIGN.md too
for WebUI WASM DSP vsersion: crates/SphereWebAudioCore

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
