# Futureboard Studio Design Contract

This document defines Futureboard Studio's finished visual language and the
rules for extending it without diluting its identity.

Futureboard does not borrow a look from another product. Existing polished
Studio surfaces, theme tokens, components, interaction behavior, and shipped
artwork are the authority. New work must continue that language rather than
reinterpret it.

## The Futureboard signature

Futureboard is a signal-first creative instrument. Its signature is:

- graphite surfaces arranged in clear working planes;
- a restrained cyan signal that marks focus, selection, live routing, and
  editable intent;
- compact controls with deliberate hit targets and precise alignment;
- quiet chrome around expressive musical content;
- typography and numbers calibrated for fast scanning;
- motion that confirms cause and effect, never decorates idle space;
- musical time expressed through consistent grids, rhythm, and alignment;
- depth conveyed by surface hierarchy and borders, not visual effects;
- plugin identities that can be distinctive inside a disciplined Studio frame.

The interface should feel unmistakably Futureboard even with all logos hidden.
That identity comes from proportion, density, state language, timing, and
craft—not ornamental branding.

## Preserve the completed language

Treat the current UI as a mature system.

- Extend existing patterns before creating new ones.
- Reuse semantic tokens and shared components.
- Preserve established density, radius, typography, icon scale, and state
  behavior.
- Make local corrections when a surface is inconsistent; do not use a feature
  task as permission for a broad redesign.
- Add a new pattern only when the interaction is genuinely new and existing
  patterns cannot express it.
- Review a new pattern in the full Studio shell, not as an isolated mockup.

## Product surfaces

The product UI is the native Rust/GPUI Studio.

Electron and the general-purpose Web UI are retired. They are not design
authority and must not receive new product UI work.

Web technology remains valid only inside embedded editors for built-in plugins.
Those editor bundles are hosted by the native app through CEF and must remain
bounded plugin surfaces. They do not define the Studio shell, menus, settings,
timeline, mixer, dialogs, or application navigation.

## Color and surface hierarchy

Use semantic theme tokens. Do not place arbitrary colors in feature components.

Surface order:

```txt
base workspace
  -> working panel
    -> raised control group
      -> floating menu/dialog
```

Use borders and small value shifts to separate adjacent planes. Reserve shadow
for genuinely floating windows and menus.

Use cyan for active meaning:

- focus and keyboard target;
- selected or armed edit state;
- live connection or routed signal;
- primary action when one action clearly leads.

Do not wash whole panels in accent color. Status colors communicate actual
health, warning, failure, recording, meter level, or clipping. They are never
decoration.

Decorative gradients, glass effects, glow, colored shadow stacks, giant pills,
and ornamental accent bars do not belong in Studio chrome. Functional meter,
fade, spectrum, heat, and waveform gradients are allowed when they encode data.

## Typography and numeric language

- Use the registered application font stack.
- Keep ordinary controls and labels compact, normally 11–13 px.
- Use smaller muted text only when it remains legible at supported DPI scales.
- Use tabular figures for time, bars/beats, dB, pan, percentages, samples,
  frequency, tempo, and parameter readouts.
- Keep units visually quieter than values without reducing clarity.
- Truncate chrome labels predictably; do not let them wrap into neighboring
  controls.
- Give icons and text a shared baseline and consistent optical weight.

## Geometry and density

Every region must have an explicit contract:

```txt
owner
state owner
coordinate space
size source and min/max
scroll owner
clip owner
overflow behavior
layer order
focus behavior
```

Do not fix geometry with spacer elements, unexplained offsets, repeated local
constants, or clipping on the wrong ancestor. Derive layout from measured bounds
and named shell metrics.

For flexible rows and columns:

- make the parent size known;
- set shrink/min constraints intentionally;
- assign exactly one scroll owner;
- avoid wrapping in DAW chrome unless the design explicitly calls for it;
- keep critical timeline, ruler, MIDI, waveform, mixer, and plugin-host geometry
  on explicit measured coordinates.

Validate affected layouts at normal, narrow, short, maximized, and high-DPI
sizes, including open/closed side panels and resized bottom panels where relevant.

## State language

The same state must look and behave the same everywhere.

- Hover suggests availability without looking selected.
- Focus is visible for keyboard and precision entry.
- Selection is stable and distinct from hover.
- Armed, recording, solo, mute, bypass, automation-read, and automation-write
  states each communicate their actual runtime meaning.
- Disabled controls explain unavailability when it is not obvious.
- Loading and processing states do not imitate success.
- Error state names the failed action and preserves recoverable user work.

UI must not lie. A control that appears active must connect to real project or
runtime state. If behavior is incomplete, disable it or label it honestly.

## Controls and interaction

- Use shared native controls before creating a one-off implementation.
- Keep hit targets usable while preserving workstation density.
- Make drag controls expose a visible affordance, fine adjustment, a precise
  value, and a predictable reset gesture where appropriate.
- Keep primary actions scarce; most controls should be visually quiet until
  active.
- Use destructive styling only for destructive actions and preserve Cancel.
- Anchor menus and popovers to measured bounds and clamp them to the window.
- Make Escape cancel transient gestures and safe-to-cancel dialogs.
- Respect focus priority: dialog, text/numeric input, focused editor, local
  surface, application command.
- Never trigger transport or destructive edit commands while typing into an
  input.

## Commands, menus, and dialogs

Use one command registry for menus, shortcuts, context menus, and command search.
Do not duplicate labels, enablement, or shortcut logic across surfaces.

Dialogs use the established compact Studio shell:

- clear title and close behavior;
- restrained raised surface and border;
- consistent action footer;
- no unnecessary full-window blocker;
- explicit Cancel for destructive or project-mutating actions;
- focus trapped only while the dialog is truly modal.

Utility windows and plugin editors remain independent desktop surfaces with
correct focus, DPI, resize, and teardown behavior.

## Timeline and arrangement

Use one musical coordinate model for ruler, grid, clips, automation, loop range,
markers, playhead, drawing, and hit-testing.

- Bar lines lead, beats support, subdivisions recede.
- Grid density and labels adapt to zoom without collisions.
- Headers and scrollable content occupy separate measured rectangles.
- Gestures use transient previews and commit once at the correct boundary.
- Escape, focus loss, and tool changes cancel active transient gestures safely.
- Zoom preserves the beat or time beneath its anchor.
- Cull work outside the visible viewport.

Layer consistently:

```txt
workspace and lanes
grid and ruler
clips/regions
gesture previews and selection
notes/automation overlays
playhead
handles and floating tools
menus/dialogs
```

## MIDI, automation, and audio editing

Editors must share the timeline's coordinate and state discipline.

- Stable IDs survive edits, undo, clipboard operations, and persistence.
- Notes, points, fades, handles, and waveform features draw and hit-test through
  the same transform.
- Manual and automated values distinguish base value from effective value.
- Automation-follow updates never create user-command feedback loops.
- Muted or disabled musical data must not reach playback.
- Waveform width reflects effective duration and processing state.
- Processing controls expose the real backend, progress, failure, and cache
  state.
- High item counts require viewport culling, caching, batching, or custom GPU
  drawing rather than broad entity rerenders.

## Track headers, mixer, and inspector

Track headers and mixer strips are compact channel instruments, not cards.

- Keep widths stable and labels truncated.
- Isolate meter updates from parent layout/render work.
- Virtualize large vertical and horizontal collections.
- Keep mute, solo, arm, pan, gain, routing, sends, and inserts connected to the
  same state heard by the engine.
- Pin master/global controls intentionally.

Inspectors present the selected object's real properties. Use aligned labels,
tabular values, compact sections, vertical scrolling when needed, and no fake
controls or horizontal overflow.

## Built-in plugin editor signature

A built-in plugin may express its own instrument/effect character, but it still
belongs to Futureboard.

The editor may use React/Vite/Tailwind because it is compiled and embedded as a
plugin-specific static surface. Within that boundary:

- make the signal path and parameter hierarchy visually obvious;
- keep the most performance-relevant values scannable;
- use purposeful custom graphics for meters, curves, oscillators, envelopes, or
  physical controls;
- preserve consistent focus, hover, drag, reset, fine-adjust, disabled, and
  error behavior;
- use stable parameter IDs and reflect authoritative native values;
- avoid application navigation, browser conventions, remote content, and
  generic dashboard composition;
- fit the declared editor bounds and handle native resize/DPI correctly;
- render usefully before optional animation or analysis data arrives.

The native Studio frame owns window chrome, lifecycle, focus integration, asset
loading, and the CEF host. The embedded editor owns only its plugin content.

## External plugin editors

External editors are plugin-owned native child views.

- Match the child view exactly to the measured client rectangle.
- Keep it out of titlebar and host chrome.
- Respect the plugin's resize capability and requests.
- Convert logical and physical coordinates explicitly at each platform boundary.
- Forward focus, keyboard, mouse, and IME behavior correctly.
- Detach the plugin view before destroying its parent.
- Never open, resize, or destroy editor windows from the audio thread.

## Performance and rendering

- Do not make the whole timeline, mixer, or track list rerender on playhead or
  meter ticks.
- Cache waveforms and expensive analysis; do not regenerate them during normal
  render.
- Draw only visible items plus controlled overscan.
- Resize WGPU/custom surfaces only when measured dimensions change.
- Convert logical and physical pixels explicitly.
- Keep debug overlays and high-rate logs environment-gated.
- Keep render functions pure of scanning, filesystem, decoding, and project
  mutation.

## Accessibility and long-session comfort

- Preserve keyboard access and visible focus.
- Provide text/tooltips for icon-only controls.
- Do not rely on color alone for destructive, armed, muted, selected, or error
  states.
- Keep contrast readable without making inactive chrome visually loud.
- Avoid unnecessary animation; respect reduced-motion behavior where available.
- Keep repeated operations consistent so muscle memory remains reliable.

## Review checklist

Before finishing a UI change, verify the relevant items:

- [ ] It continues the Futureboard signature without redesigning adjacent UI.
- [ ] It uses existing components and semantic tokens.
- [ ] Visible state matches real project/runtime state.
- [ ] Drawing and hit-testing share coordinates.
- [ ] Focus, keyboard, Escape, and cancellation behavior are correct.
- [ ] Long labels, empty state, errors, and disabled state are handled.
- [ ] Resizing, side panels, bottom panels, scrolling, zoom, and DPI do not break
      geometry.
- [ ] High-frequency updates do not invalidate broad UI trees.
- [ ] Large track/channel/item counts have a bounded rendering strategy.
- [ ] Built-in Web UI remains inside its plugin boundary.
- [ ] Compile/test results and visual/runtime checks are reported separately.

## Final rule

Futureboard's signature is precision made visible. Every extension should feel
inevitable inside the existing Studio: compact, calm, musical, honest, and fast.
