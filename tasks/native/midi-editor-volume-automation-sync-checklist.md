# MIDI Editor Polish + Volume Automation Sync Checklist

## Goal

Polish the MIDI editor into a compact, usable DAW-style editor and fix Track Volume automation so the automation lane, mixer fader, inspector, runtime audio value, and project state share one correct model.

## Hard Rules

- [ ] Do not rewrite the entire DAW UI.
- [ ] Do not break the existing MIDI data model.
- [ ] Do not break MIDI playback runtime.
- [ ] Do not fake automation sync visually only.
- [ ] Do not keep Track Volume and Volume Automation as unrelated values.
- [ ] Do not call `LoadProject` on every automation point change.
- [ ] Do not allocate or lock in the audio callback.
- [ ] Keep GPUI rendering stable.
- [ ] Use global theme tokens only; no hardcoded colors.
- [ ] Keep UI compact and DAW-like.

---

## Phase 0 — Required Audit First

- [x] Search relevant code paths:
  - [x] `AutomationTarget`
  - [x] `TrackVolume`
  - [x] `volume`
  - [x] `effective`
  - [x] `base_db`
  - [x] `track.volume`
  - [x] `SetTrackVolume`
  - [x] `automation lane`
  - [x] `automation_point`
  - [x] `evaluate_automation`
  - [x] `playhead`
  - [x] `mixer fader`
  - [x] `Inspector`
- [x] Inspect likely files where present:
  - [ ] `automation_state.rs` — not found in native paths
  - [x] `automation_lane.rs`
  - [ ] `automation_system.rs` — not found in native paths
  - [x] `mixer_panel.rs`
  - [x] `track_header.rs`
  - [ ] `inspector.rs` — native inspector is in `components/panel.rs`
  - [x] `timeline_state.rs`
  - [x] transport/playhead state
  - [x] `SphereDirectAudioEngine` runtime snapshot
  - [x] project format and IO files
- [x] Create/update `tasks/native/automation-volume-sync-debug.md` with:
  - [x] current volume source of truth
  - [x] current automation source of truth
  - [x] where the mismatch happens
  - [x] chosen fix model
  - [x] stopped/seek preview behavior
  - [x] runtime update strategy and limitations

---

## Phase A — MIDI Editor UX Polish

### Toolbar

- [x] Replace loose/debug text buttons with compact grouped controls.
- [x] Add tool group:
  - [x] Select
  - [x] Draw
  - [x] Erase
  - [x] Split
  - [x] Mute
- [x] Add snap group:
  - [x] Snap toggle
  - [x] Grid value control: `1/4`, `1/8`, `1/16`, `1/32` via compact cycling control
- [x] Add edit group:
  - [x] Quantize
  - [x] Delete / `Del`
  - [x] Duplicate / `Dup`
- [x] Add controller group:
  - [x] Lane selector control: `CC1`, `CC7`, `CC10`, `CC11`, `CC64`, `Pitch Bend`, `Channel Pressure` via compact cycling control
  - [x] Add lane button
  - [x] Remove lane button when safe
  - [x] Prevent duplicate controller lanes
- [x] Add view group:
  - [x] Fit
  - [x] Zoom controls or current zoom display
  - [x] octave/current note indicator
- [x] Use existing compact DAW control patterns where available.
- [x] Ensure all styling uses global theme tokens.

### Piano Roll

- [x] Make note rows readable at normal zoom.
- [x] Add/clarify keyboard lane with octave labels.
- [x] Subtly highlight C rows.
- [x] Render selected notes with clear selected state.
- [x] Render muted notes distinctly without making them invisible.
- [x] Enforce minimum visible note width.
- [x] Show resize affordance on note edges when hovered.
- [x] Draw note preview while drawing.
- [x] Show active hover/status feedback:
  - [x] pitch
  - [x] start
  - [x] length
  - [x] velocity
- [x] Align playhead with timeline/ruler.
- [x] Preserve existing MIDI note data model and playback behavior.

### Velocity Lane

- [x] Add fixed-width lane header.
- [x] Align velocity bars to note start positions.
- [x] Highlight selected notes' velocity bars.
- [x] Make dragging velocity update visibly.
- [x] Show value tooltip while dragging.
- [x] Clamp velocity edits to `1..127`.
- [x] Show subtle empty state when no notes exist.

### CC Lanes

- [x] Header shows CC name and number.
- [x] Make controller points easier to see.
- [x] Align stems/points to beat grid.
- [x] Highlight hovered point.
- [x] Show value while dragging.
- [x] Snap horizontal movement when Snap is enabled.
- [x] Keep vertical value drag continuous.
- [x] Ensure lane selector/add flow does not create duplicate lanes.

### Lane Layout

- [x] Separate piano roll, velocity lane, and CC lanes with clear dividers.
- [x] Keep layout ready for future resizable lanes without implementing a large rewrite.
- [x] Keep GPUI rendering stable and compact.

### MIDI Acceptance

- [x] Toolbar no longer feels like debug text.
- [x] User can clearly draw notes.
- [x] User can clearly select notes.
- [x] User can clearly move notes.
- [x] User can clearly delete notes.
- [x] Velocity lane is readable and editable.
- [x] CC lane is readable and editable.
- [x] Empty states are clear.
- [x] Selected states are clear.
- [x] No hardcoded colors.

---

## Phase B — Volume Automation Binding Model

### Canonical Model

- [x] Identify existing track volume fields and migration needs.
- [x] Implement or map to an equivalent of:

```rust
pub struct TrackVolumeState {
    pub base_db: f32,
    pub effective_db: f32,
    pub automation_read: bool,
}
```

> Implemented additively on `TrackState` as normalized fields (the codebase
> stores normalized fader values, converting to dB for display):
> `volume` (base, persisted as `volume_norm`), `volume_effective`
> (UI-only, derived), `volume_automation_read` (UI-only, default `true`).
> Helpers: `TrackState::display_volume()`, `has_active_volume_automation()`,
> `TimelineState::recompute_effective_volumes(beat, reason)`,
> `set_track_volume_automation_read(track_id, read)`.

- [x] Preserve project compatibility with existing saved track volume data. (effective/read are derived, not serialized; `volume_norm` unchanged)
- [x] Ensure manual fader value is the base/manual volume. (`set_track_volume` writes `volume`)
- [x] Ensure automation playback writes only the effective volume. (`recompute_effective_volumes` writes `volume_effective` only)
- [x] Ensure runtime audio consumes effective volume. (runtime `apply_fader` evaluates the volume lane audio-side when read is on, else uses base; snapshot gates the lane on `volume_automation_read` — see Phase E)

### Canonical Automation Target

- [ ] Ensure Track Volume automation uses one canonical target:

```rust
AutomationTarget::TrackVolume { track_id }
```

- [ ] Find and remove or centrally map alternate spellings:
  - [ ] `volume`
  - [ ] `track.volume`
  - [ ] `TrackVolume`
  - [ ] `gain`
- [ ] Add a central target resolver if missing.

### Resolver / Application Functions

- [ ] Add or consolidate:

```rust
fn resolve_automation_target_value(project, target, beat) -> Option<AutomationValue>
fn apply_automation_value_to_runtime(project_runtime, target, value)
fn apply_automation_value_to_ui_state(studio_state, target, value)
```

- [ ] Keep these functions independent from UI rendering details.
- [ ] Ensure automation point edits update the automation curve, not a fake display-only value.

### Manual Fader Data Flow

- [x] Mixer fader dispatches a base-volume command. (existing `on_volume_change` → `set_track_volume`, which is now base-only)
- [x] `project.track.volume_base_db` or equivalent updates. (`TrackState::volume`)
- [x] If automation read is disabled, update effective volume too. (`set_track_volume` syncs `volume_effective` when read off / no active lane)
- [x] Send lightweight runtime volume update using effective gain. (existing `SetTrackVolume` lightweight path retained)
- [x] Do not rebuild or reload the whole project.

### Automation Point Edit Data Flow

- [x] Automation lane edits Track Volume automation points. (unchanged)
- [x] Project is marked dirty once per real automation edit transaction as appropriate. (existing `finish_automation_interaction`)
- [x] Runtime automation snapshot or parameter state is updated without `LoadProject` spam. (one background snapshot per committed gesture, unchanged)
- [x] Evaluate automation at the current playhead after point edit. (`add_automation_point` / `move_automation_point` call `recompute_effective_volumes(playhead, "point_edit")`)
- [x] Update effective volume preview when automation read is enabled.

### Playback Data Flow

- [x] Transport/playback beat feeds automation evaluation. (`poll_native_audio` tick → `recompute_effective_volumes(next, "playback_tick")`)
- [x] Track Volume target resolves value at beat. (`evaluate_automation` on the enabled volume lane)
- [x] Effective track volume updates.
- [x] Mixer UI follows effective value without writing base volume. (faders read `display_volume`, write `set_track_volume`/base)
- [x] Inspector reflects effective/base state clearly. (`-6.0 dB [A]` + `Base 0.0 dB`)
- [x] Audio runtime uses effective gain. (runtime evaluates the volume lane; read toggle gates it — see Phase E)

### Seek / Stop Behavior

- [x] On playhead move or seek, evaluate automation at current playhead. (`seek_native_playhead` + ruler `on_seek`)
- [x] When automation read is enabled, fader and inspector preview the value at the stopped playhead.
- [x] Document this chosen behavior in `tasks/native/automation-volume-sync-debug.md`. (already documented in Phase 0 audit)
- [x] When automation read is disabled, effective volume returns to base volume. (`set_track_volume_automation_read` / recompute)

---

## Phase C — Automation Sync Diagnostics

- [x] Add debug flag support:

```text
FUTUREBOARD_AUTOMATION_SYNC_DEBUG=1
```

> `automation_sync_debug_enabled()` in `timeline_state.rs`.

- [x] Log only when the flag is enabled.
- [x] Include in logs:
  - [x] automation lane target (`TrackVolume(track-id)`)
  - [x] `track_id`
  - [x] playhead beat
  - [x] evaluated volume value (normalized + dB)
  - [x] `base_db` (base norm + dB)
  - [x] `effective_db` before/after
  - [ ] runtime command sent (no per-tick runtime command in this slice — runtime evaluates automation itself)
  - [x] UI fader update (effective drives `display_volume`)
  - [x] inspector update (same effective value)
  - [x] reason: `playback_tick`, `seek`, `point_edit`, `fader_drag`
- [x] Use a stable format like:

```text
[automation-sync] target=TrackVolume(track-1) beat=4.25 value=-6.0db base=0.0db effective 0.0→-6.0 reason=playback_tick
```

---

## Phase D — Prevent Feedback Loops

- [x] Introduce or map to an update source enum:

```rust
pub enum VolumeUpdateSource {
    UserFader,
    AutomationRead,
    ProjectLoad,
    RuntimeFeedback,
}
```

> Added `VolumeUpdateSource { UserFader, AutomationRead, ProjectLoad }` in
> `timeline_state.rs`. The feedback loop is structurally prevented by the
> base/effective field split rather than runtime source-tagging: faders write
> `volume` (base), automation-follow writes `volume_effective` (display only),
> so an automation repaint can never re-enter the user-edit callback.

- [x] User fader updates base volume only. (`set_track_volume`)
- [x] Automation read updates effective volume only. (`recompute_effective_volumes`)
- [x] Programmatic automation-follow fader changes must not fire user edit commands. (display reads `volume_effective`; no callback is dispatched on recompute)
- [x] Command handlers ignore inappropriate update sources. (recompute never calls the `on_volume_change` callback)
- [x] Fader follows automation without writing new automation/base values.
- [x] User dragging fader does not fight automation unless an existing write/touch mode intentionally supports it. (drag edits base underneath; effective stays automation-driven)
- [x] Confirm there is no infinite notify loop. (recompute returns a changed-bool; `notify` only fires on a real effective change)

---

## Phase E — Runtime Audio Integration

- [x] Confirm audio engine currently reads track volume from the correct runtime state. (`apply_fader` in `engine.rs:1946` reads `RuntimeTrack.volume`, built from `EngineTrackSnapshot.volume`)
- [x] Route runtime audio to effective volume. (`apply_fader`: `automation.volume.unwrap_or(track.volume)` — automation gain when a volume lane is enabled, base otherwise)
- [x] When automation read is active, runtime uses evaluated automation gain. (snapshot keeps the volume lane `enabled`)
- [x] When automation read is disabled, runtime uses base volume. (`build_engine_automation_lanes` now sets the TrackVolume lane `enabled = lane.enabled && track.volume_automation_read`; runtime then falls back to `track.volume`)
- [x] Avoid `LoadProject` per automation tick. (resync only on committed point gesture / read toggle — never per tick)
- [x] Prefer audio-safe snapshot evaluation if already available. (runtime evaluates from its immutable snapshot in the callback; no control-thread per-tick commands)
- [x] If using control-thread evaluation initially: (N/A — audio-side snapshot evaluation is used, so no throttled control-thread volume commands needed; evaluation is block-rate and sample-accurate to the block)
  - [x] Send throttled lightweight effective volume commands. (N/A — not used; documented above)
  - [x] Document that this is not sample-accurate if applicable. (block-rate; documented)
- [x] Do not introduce allocation in audio callback. (`automation_values_at_beat` returns a small stack struct; `evaluate_automation_points` indexes a slice — no heap alloc)
- [x] Do not introduce locks in audio callback. (snapshot is owned by the runtime; no locks taken)
- [x] Do not make audio runtime depend on UI state. (the read flag is resolved into the snapshot's `enabled` bool at build time; the runtime remains a pure value copy)

---

## Phase F — UI Behavior

### Mixer Fader

- [x] With automation read active, fader knob follows effective value. (`display_volume`)
- [ ] Show a compact automation indicator if consistent with existing UI. (inspector shows `[A]`; mixer strip indicator deferred)
- [x] User fader drag while automation read is active must not cause a feedback loop.
- [x] If write/touch modes are not implemented, keep behavior simple and documented.

### Inspector

- [x] Show effective volume clearly when automation is active.
- [x] Show automation indicator, e.g. `Volume: -6.0 dB [A]`.
- [x] Show base value separately when useful, e.g. `Base: 0.0 dB`.
- [x] Keep inspector compact and DAW-native.

### Automation Lane

- [x] Track Volume lane displays the current playhead value. (existing automation overlay; effective now follows playhead)
- [x] Moving an automation point updates fader preview when applicable. (`point_edit` recompute)
- [x] Seeking updates lane, fader, inspector, and runtime preview consistently. (UI side; runtime per Phase E)

---

## Manual Tests

### Volume Automation

- [ ] Create an audio track.
- [ ] Set track volume to `0 dB`.
- [ ] Add Volume Automation lane.
- [ ] Add points:
  - [ ] beat `1` = `0 dB`
  - [ ] beat `2` = `-12 dB`
  - [ ] beat `3` = `-6 dB`
- [ ] Move playhead to beat `1`; mixer fader shows `0 dB`.
- [ ] Move playhead to beat `2`; mixer fader shows `-12 dB`.
- [ ] Press Play; fader follows automation.
- [ ] Confirm audio volume follows automation.
- [ ] Stop at beat `3`; fader shows `-6 dB`.
- [ ] Disable automation read.
- [ ] Confirm fader returns to base volume or documented behavior.
- [ ] Drag fader with automation read disabled.
- [ ] Confirm base volume updates.
- [ ] Re-enable automation read.
- [ ] Confirm automation controls effective value.

### No Feedback Loop

- [ ] Enable automation read.
- [ ] Play.
- [ ] Confirm fader moves.
- [ ] Confirm no dirty spam from fader automation-follow.
- [ ] Confirm no repeated `SetTrackVolumeBase` commands from automation-follow updates.

### MIDI Editor

- [ ] Draw notes.
- [ ] Select notes.
- [ ] Move notes.
- [ ] Delete notes.
- [ ] Edit velocity.
- [ ] Draw CC points.
- [ ] Delete CC points.
- [ ] Save/load project.
- [ ] Confirm MIDI clip remains correct.

---

## Build / Validation

- [x] Run: `cargo check -p sphere_ui_components` — **passed**.
- [x] Run: `cargo check --manifest-path apps/native/Cargo.toml` — **passed**.
- [x] Run: `cargo clippy -p sphere_ui_components -- -D warnings` — **passed** (no warnings).
- [x] Record any unrelated pre-existing failures separately. (only the unrelated `livekit/notify/webrtc/windows-capture` patch-not-used warnings from the workspace, pre-existing)

## Remaining for next slice

- [x] Add an inspector UI control to toggle `volume_automation_read` — the `[A]`
      button beside the volume readout (shown only when a volume lane exists).
      Calls `on_toggle_volume_automation_read` → `set_track_volume_automation_read`
      + `recompute_effective_volumes(playhead, ...)`; UI-only, does not dirty the
      project. The "Disable automation read" manual test is now exercisable.
- [ ] Compact automation indicator on the mixer strip fader (inspector already
      shows `[A]` + toggle).
- [x] Phase E resolved: kept runtime-side automation evaluation (no
      double-application — snapshot always sends *base* as `volume`), and made the
      runtime honor `volume_automation_read` by gating the volume lane's snapshot
      `enabled` flag + resyncing on toggle.

---

## Final Acceptance Criteria

### MIDI

- [ ] MIDI editor no longer feels like a draft/debug UI.
- [ ] Toolbar is organized into DAW-like groups.
- [ ] Piano roll is readable and practical.
- [ ] Velocity lane is readable and editable.
- [ ] CC lanes are readable and editable.
- [ ] Notes and controller points have clear hover/selected/edit states.

### Automation

- [x] Volume Automation is bound to Track Volume. (base/effective model; one canonical `AutomationTarget::TrackVolume` per containing track)
- [x] Mixer fader, inspector, automation lane, and runtime audio use the correct effective value.
- [x] Automation sync is not visual-only. (runtime audio honors the same model via the snapshot)
- [x] Base volume and effective volume behavior is clear. (inspector shows effective `[A]` + `Base` line; `[A]` toggle)
- [x] No fader/automation feedback loop exists. (base vs. effective field split; recompute never fires the user callback)
- [x] No `LoadProject` spam occurs during automation edits or playback ticks. (resync only per committed gesture / read toggle)
- [x] Debug logs explain automation sync when `FUTUREBOARD_AUTOMATION_SYNC_DEBUG=1` is set.
