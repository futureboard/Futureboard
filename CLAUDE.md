Before implementing task specs, read `tasks/SKILL.txt`.
Use it as the operating guide for all files in `tasks/`.

and read documentation for Native App Framework `frameworks/SphereEngine/website/spheredoc/docs`

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
