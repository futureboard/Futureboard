# Timeline MIDI Clip Live Draw Preview

## Goal

While drawing a MIDI clip with the Pen tool on an instrument/MIDI lane, show a
live animated ghost clip and a musical-length label so the user knows the exact
bounds and length **before** releasing the mouse.

## Implementation

All in `crates/SphereUIComponents/src/components/timeline/timeline.rs`.

- **State model** — `pen_clip_draw` upgraded from `Option<(String, f32)>` to
  `Option<ClipDrawPreview>` (`track_id`, `start_beat`, `current_beat`, `dragging`).
  Pure transient UI state on the `Timeline` component; no project mutation during
  the gesture.
- **Shared bounds** — `compute_pen_clip_span(state, start, end) -> (clip_start, length)`
  snaps the length to the MIDI grid when snap is on and clamps to the minimum
  clip length. Used by **both** the live preview and the commit, so they can
  never disagree (WYSIWYG).
- **Mouse down** (`on_add_clip`) — starts the preview at the snapped start beat.
- **Mouse move** (`on_edit_mouse_move`) — left-drag with the Pen tool updates
  `current_beat` (snapped) and flips `dragging`; repaints only on a real change.
- **Mouse up** (`on_pen_mouse_up` / `_out`) — commits the real clip from the
  preview bounds via the existing `CreateClip` edit command, exactly once.
- **Escape / cancel** — already routed through `cancel_active_gesture` →
  `reset_input_state`, which clears `pen_clip_draw`.
- **Overlay** (`pen_clip_draw_overlay`) — translucent track-colored ghost in the
  lane coordinate space, a pulsing outline (`with_animation` +
  `pulsating_between`, self-animating even when the cursor is still), a
  "New MIDI Clip" title placeholder, a `{len} bt` bottom readout, a subtle
  full-height end guide, and a floating label showing length + `start → end`
  in bar.beat (`format_clip_length`, `state.format_bar_beat`).

## Theme

Track color + theme tokens only (`Colors::with_alpha`, `surface_panel`,
`surface_panel_alt`, `divider`, `text_*`). No hardcoded colors.

## Validation

- `cargo check -p sphere_ui_components` — passed
- `cargo clippy -p sphere_ui_components -- -D warnings` — passed (no warnings)
- `cargo check --manifest-path apps/native/Cargo.toml` — passed

## Acceptance criteria

- [x] User can clearly see the MIDI clip size while drawing (ghost clip).
- [x] User knows bars/beats before release (floating length + range label).
- [x] Preview feels live and responsive (repaints on snapped-beat change).
- [x] No fake commit during drag (real clip created only on mouse up).
- [x] No flicker (single overlay, change-gated notify, shared bounds helper).
- [x] Theme tokens only.

## Notes / deferred

- Minimum clip length follows the project's existing `MIN_MIDI_CLIP_BEATS`
  (4 beats) rather than one grid step — kept to avoid changing clip-creation
  behavior elsewhere. The preview reflects the real committed size.
- Left-drag normalizes bounds via `normalize_range`, so dragging left of the
  start is handled correctly.
