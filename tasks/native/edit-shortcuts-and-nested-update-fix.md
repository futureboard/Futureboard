# Edit Shortcuts (Ctrl+A/C/V/X/Delete) + Nested GPUI Update Panic Fix

## Problem

`cannot update StudioLayout while it is already being updated` (GPUI double
lease) on Delete/Cut/Paste, plus the A/C/V/X/Delete family mis-routing to
timeline clip commands while the MIDI editor was focused.

Root cause: `Timeline::mark_project_changed` runs an `on_project_changed`
callback that synchronously does `studio_layout.update(...)`. That callback is
invoked from `run_edit_command`, which the keyboard path reaches as
`StudioLayout::update → timeline.update → run_edit_command → mark_project_changed
→ studio_layout.update` — a nested lease on an entity already being updated.

## Fix

### PART B — no nested parent update (the panic)

`crates/SphereUIComponents/src/layout.rs` — the `set_project_changed_callback`
and `set_media_changed_callback` closures now wrap their `StudioLayout` update in
`cx.defer(...)`. The dirty mark runs after the current update stack unwinds, so
it is safe whether the callback fires from a Timeline gesture (App context) or
from the keyboard dispatch path (nested in `StudioLayout::update`). Dirty is a
flag the audio poll reads on its own cadence, so deferring one cycle is invisible.

This fixes the panic for **all** dispatch-driven edit commands (delete, cut,
paste, cycle-automation-target) at once — they all funnel through
`run_edit_command`/`mark_project_changed`.

### PART D/E — MIDI editor focus routing

The studio root uses `capture_key_down` (capture phase, fires before the focused
element's `on_key_down`), so global edit commands were dispatched even when the
piano roll was focused. Added a focus gate in
`crates/SphereUIComponents/src/layout/studio_render.rs`: when the docked piano
roll holds focus and the resolved command is in the A/C/V/X/Delete family
(`is_midi_routable_edit_command`), the handler returns **without**
`stop_propagation`, letting the event bubble to the piano roll's own
`on_key_down`. So Ctrl+A selects notes (not clips) and Delete removes notes (not
tracks/clips).

- `PianoRoll::is_focused(window)` added (`components/piano_roll.rs`).
- `PianoRoll::on_key` gained **Ctrl+X = cut** (copy then delete, one undo step);
  it already handled A/C/V/D/Delete/Backspace.

Text inputs keep native behavior via the pre-existing
`text_input_has_focus + is_text_input_key` gate (dialogs/search/inspector
name/clip-name handled earlier in the same handler).

### PART H — debug

- `FUTUREBOARD_EDIT_COMMAND_DEBUG=1` → `edit_command_debug()`:
  - `[edit-command] command=edit:delete target=Timeline`
  - `[edit-command] command=edit:select-all target=MidiEditor reason=focus-passthrough`
- `[shortcut] resolve ...` (keymap) and `[key] dispatched command=...` already
  existed.

## Validation

- `cargo check -p sphere_ui_components` — passed
- `cargo clippy -p sphere_ui_components -- -D warnings` — passed (no warnings)
- `cargo check --manifest-path apps/native/Cargo.toml` — passed

## Acceptance

- [x] Ctrl+A/C/V/X/Delete work in Timeline (existing dispatch, now panic-free).
- [x] Ctrl+A/C/V/X/Delete work in MIDI Editor (piano roll `on_key`, + new cut).
- [x] Text inputs keep native text shortcut behavior.
- [x] No nested StudioLayout update from Timeline/MIDI child entities (deferred).
- [x] No GPUI double-lease panic.
- [x] No panic on Delete with no selection / Ctrl+V with empty clipboard
      (existing guards: empty clipboard → no-op, empty selection → no-op).

## Notes / not done (scope)

- Did not introduce the aspirational `EditCommand`/`EditTargetContext`/
  `CommandOutcome` enums. The codebase already has a central string-command
  dispatch (`dispatch_command_id_from_bounds`); the panic and routing were the
  real defects and were fixed without a cross-cutting rewrite (smallest safe
  scope). A future refactor could formalize the outcome struct.
- Velocity/CC lanes are sub-areas of the same focused `PianoRoll` entity, so they
  inherit the same `on_key` routing.
- BPM numeric edit / hex color input live in popovers/dialogs with their own
  `capture_key_down` handled earlier in the chain; if any such field is found to
  leak A/C/V/X to the global handler, add it to `text_input_has_focus`.
