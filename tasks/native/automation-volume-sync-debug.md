# Automation Volume Sync Debug Findings

## Scope

Phase 0 audit for `tasks/native/midi-editor-volume-automation-sync-checklist.md`.

Goal: identify the current Track Volume / Volume Automation sources of truth, where they diverge, and the fix model to implement before touching MIDI editor polish.

## Files inspected

- `crates/SphereUIComponents/src/components/timeline/timeline_state.rs`
- `crates/SphereUIComponents/src/components/timeline/timeline.rs`
- `crates/SphereUIComponents/src/components/timeline/automation_lane.rs`
- `crates/SphereUIComponents/src/components/timeline/track_header.rs`
- `crates/SphereUIComponents/src/components/mixer_panel.rs`
- `crates/SphereUIComponents/src/components/panel.rs`
- `crates/SphereUIComponents/src/layout/engine_snapshot.rs`
- `crates/SphereUIComponents/src/layout/audio_transport.rs`
- `crates/SphereUIComponents/src/project/mod.rs`
- `crates/SphereUIComponents/src/project/format.rs`
- `crates/SphereDirectAudioEngine/src/types.rs`
- `crates/SphereDirectAudioEngine/src/runtime.rs`
- `crates/SphereDirectAudioEngine/src/engine.rs`

## Current volume source of truth

### UI/project state

Current UI-side track volume is a single normalized fader value:

```rust
pub struct TrackState {
    pub volume: f32,
    pub pan: f32,
    // ...
}
```

Defined in `crates/SphereUIComponents/src/components/timeline/timeline_state.rs`.

`TrackState::volume` is documented as normalized `0.0..=1.0`, with `volume::norm_to_db` mapping it to `-60 dB..+6 dB`.

Mutation path:

```rust
pub fn set_track_volume(&mut self, track_id: &str, norm: f32) {
    if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
        t.volume = norm.clamp(0.0, 1.0);
    }
}
```

UI controls currently read/write this same field:

- Track header slider: `track.volume`
- Mixer fader: `track.volume`
- Inspector volume slider/readout: `track.volume`
- Project save/load: `ProjectTrack.volume_norm`
- Engine snapshot: `EngineTrackSnapshot.volume = volume_norm_to_linear(track.volume)`

There is no UI-side distinction between manual/base volume and automation/effective volume.

### Project persistence

Project persistence stores the same base/manual value as:

```rust
pub struct ProjectTrack {
    pub volume_norm: f32,
    // ...
}
```

`FutureboardProject::from(&TimelineState)` writes `volume_norm: t.volume`.

`apply_to_timeline` restores `TrackState { volume: pt.volume_norm, ... }`.

Project format currently persists one volume value, not separate base/effective values.

### Engine/runtime state

The audio snapshot has one per-track volume value:

```rust
pub struct EngineTrackSnapshot {
    pub volume: f32,
    // linear 0.0..2.0
}
```

The runtime track has one base runtime volume field:

```rust
pub struct RuntimeTrack {
    pub volume: f32,
    pub automation_lanes: Vec<RuntimeAutomationLane>,
    // ...
}
```

`RuntimeProject::build` initializes `RuntimeTrack.volume` from the snapshot.

Lightweight manual update path exists:

```rust
EngineCommand::SetTrackVolume { track_id, value }
```

which calls:

```rust
runtime.update_track_volume(&track_id, value);
```

This updates `RuntimeTrack.volume` only.

## Current automation source of truth

### UI/project automation state

Automation lanes are stored per track:

```rust
pub struct AutomationLaneState {
    pub id: String,
    pub name: String,
    pub target: AutomationTarget,
    pub enabled: bool,
    pub visible: bool,
    pub points: Vec<AutomationPoint>,
}
```

Automation point values are normalized `0.0..=1.0`.

Current UI automation targets are per-track by containment rather than by explicit target track id:

```rust
pub enum AutomationTarget {
    TrackVolume,
    TrackPan,
    TrackMute,
    PluginParameter { ... },
    SendLevel { ... },
}
```

Because lanes live inside `TrackState`, `AutomationTarget::TrackVolume` implicitly means the containing track’s volume.

Project persistence stores the target as a tag descriptor:

- tag `0` = Track Volume
- tag `1` = Track Pan
- tag `2` = Track Mute
- tag `3` = Plugin Parameter
- tag `4` = Send Gain

Legacy lane names map through `AutomationTarget::from_legacy_name`.

### UI automation evaluation

`evaluate_automation(points, beat, default)` exists in `timeline_state.rs`.

It is currently used by `automation_overlay` to draw the automation curve only.

Important: this UI evaluation does not update `TrackState::volume`, mixer fader, inspector, or runtime parameter state.

### Runtime automation evaluation

Runtime automation lanes are included in `EngineTrackSnapshot.automation_lanes` and converted into `RuntimeAutomationLane`.

Runtime target mapping is tag-based:

```rust
pub enum RuntimeAutomationTarget {
    TrackVolume,
    TrackPan,
    TrackMute,
    PluginParameter { ... },
    SendGain { ... },
    Unresolved,
}
```

Runtime automation evaluation exists:

```rust
pub fn automation_values_at_beat(&self, beat: f64) -> RuntimeTrackAutomationValues
```

For Track Volume, it converts normalized automation value to linear gain:

```rust
RuntimeAutomationTarget::TrackVolume => {
    values.volume = Some(volume_norm_to_linear(value));
}
```

Audio render path uses automation during fader application:

```rust
fn apply_fader(track: &mut RuntimeTrack, frames: usize, beat: f64) {
    let automation = track.automation_values_at_beat(beat);
    let volume = automation.volume.unwrap_or(track.volume);
    let pan = automation.pan.unwrap_or(track.pan);
    // ...
}
```

This means runtime audio already uses the automation curve when the runtime snapshot contains enabled volume automation lanes.

## Where the mismatch happens

The mismatch is not that runtime audio completely ignores Track Volume automation. The runtime does evaluate Track Volume automation in `apply_fader`.

The mismatch is that UI and runtime have different effective-value models:

1. `TrackState::volume` remains the manual/base fader value.
2. Automation lane points are a separate curve in `TrackState::automation_lanes`.
3. Runtime audio evaluates automation to an effective value internally during render.
4. Mixer fader, inspector, and track header continue to display `TrackState::volume`.
5. Playback/seek updates `transport.playhead_beats`, but does not evaluate automation into UI state.
6. Automation point edits mark the project dirty and eventually rebuild the runtime snapshot, but do not preview/update effective UI volume at the playhead.

So the user sees:

- Automation lane value at beat 2 says `-12 dB`.
- Audio runtime may play `-12 dB` if the runtime snapshot is current.
- Mixer/track/inspector fader still shows the base `track.volume`, for example `0 dB`.

That creates the apparent separate source-of-truth bug.

## Current `LoadProject` behavior

`Timeline::finish_automation_interaction` calls `mark_project_changed(cx)` once when an automation drag/add changed data.

`StudioLayout::poll_native_audio` sees `engine_project_dirty` or `engine_media_dirty` and calls:

```rust
schedule_audio_project_sync(cx, false, "engine_dirty_poll")
```

`schedule_audio_project_sync` builds an `EngineProjectSnapshot` and spawns a background thread that calls:

```rust
engine.load_project(snapshot)
```

This is not called on every mouse move because automation drag movement is live/silent until release. However, the current design still uses a full project snapshot reload for automation point edits after the gesture. That is acceptable for current behavior but should be replaced or supplemented by a lightweight automation snapshot/parameter update for the sync fix.

Manual fader drag currently uses the lightweight runtime path:

```rust
on_track_param_change(track_id, "volume", value)
```

which maps to `EngineCommand::SetTrackVolume`.

## Target identity finding

Current native UI target identity is implicit:

```rust
AutomationTarget::TrackVolume
```

The track id comes from the containing `TrackState` and from editor calls like:

```rust
ensure_automation_lane(track_id, target)
add_automation_point(track_id, lane_id, beat, value)
```

This is workable internally, but not canonical enough for cross-system diagnostics or explicit binding. The requested model should introduce a canonical resolved target form equivalent to:

```rust
AutomationTarget::TrackVolume { track_id }
```

without necessarily changing persisted lane storage immediately. A resolver can combine `(containing track_id, lane.target)` into a canonical resolved target.

Recommended bridge type:

```rust
pub enum ResolvedAutomationTarget {
    TrackVolume { track_id: String },
    TrackPan { track_id: String },
    TrackMute { track_id: String },
    PluginParameter { track_id: String, insert_id: String, parameter_id: String },
    SendLevel { track_id: String, send_id: String },
}
```

## Chosen fix model

### State model

Add explicit base/effective volume state on the UI side while preserving compatibility with existing `volume_norm` saved projects.

Preferred normalized storage in the current codebase:

```rust
pub struct TrackVolumeState {
    pub base_norm: f32,
    pub effective_norm: f32,
    pub automation_read: bool,
}
```

The checklist described `base_db` / `effective_db`; this codebase currently stores normalized fader values and converts to dB for display. Use normalized values internally, with helpers for dB display.

Migration approach:

- Existing `TrackState::volume` should become or map to `base_norm`.
- Existing project `volume_norm` loads into `base_norm`.
- Initialize `effective_norm = base_norm`.
- Existing engine snapshots should send effective linear gain for non-automated paths.

### Behavior

- Manual fader drag edits base volume only.
- If automation read is disabled, effective volume follows base volume.
- If automation read is enabled, effective volume is the evaluated Track Volume automation value at the current playhead.
- Runtime audio uses effective value or evaluates the same automation snapshot audio-safely.
- Mixer/track/inspector display effective value when automation read is enabled.
- Inspector should also show base value when automation is active.
- Automation-follow UI updates must not dispatch user fader commands.

### Stopped/seek preview behavior

Chosen behavior: DAW-like playhead preview.

When automation read is enabled, evaluate Track Volume automation on playhead move/seek even while stopped. The mixer fader and inspector show the value at the current playhead position.

When automation read is disabled, effective volume returns to base volume.

### Runtime update strategy

Current runtime already evaluates automation inside the audio callback from an immutable runtime snapshot. That is the preferred audio-side direction because it avoids UI dependency and avoids per-tick commands.

Needed changes:

- Keep or improve audio-side snapshot evaluation.
- Add UI-side evaluation for display sync on playback tick, seek, and point edit.
- Avoid full `LoadProject` on every automation point change or playback tick.
- For point edits, update runtime automation data using a lightweight automation snapshot command if available/added; otherwise only one background `LoadProject` per committed gesture remains a temporary behavior to replace.

## Feedback-loop risk

Current faders call user callbacks when interacted with. If future automation-follow updates mutate the same `TrackState::volume` field used by fader controls, the fader callback could be interpreted as a user edit and write the base volume repeatedly.

Avoid by separating sources:

```rust
pub enum VolumeUpdateSource {
    UserFader,
    AutomationRead,
    ProjectLoad,
    RuntimeFeedback,
}
```

Rules:

- `UserFader` updates base volume.
- `AutomationRead` updates effective volume only.
- Programmatic fader display changes must not call the user edit callback.
- Command handlers must ignore or route non-user sources correctly.

## Diagnostic flag to add in implementation

Add:

```text
FUTUREBOARD_AUTOMATION_SYNC_DEBUG=1
```

Log format:

```text
[automation-sync] target=TrackVolume(track-1) beat=4.25 value=-6.0db base=0.0db effective 0.0→-6.0 reason=playback_tick
```

Reasons to log:

- `playback_tick`
- `seek`
- `point_edit`
- `fader_drag`
- `project_load`
- `runtime_command`

Include:

- canonical automation target
- track id
- playhead beat
- evaluated normalized value and dB value
- base/effective before/after
- runtime command/snapshot update sent
- UI fader/inspector update reason

## Implementation notes for next phase

1. Add resolved target helpers before changing UI behavior.
2. Add base/effective volume state and migration from existing `volume`/`volume_norm`.
3. Add UI-side Track Volume automation evaluation on playback tick and seek.
4. Make mixer/inspector read effective value when automation read is active.
5. Ensure fader user callbacks update base only and cannot be triggered by automation-follow updates.
6. Keep audio runtime free of locks/allocations in the callback.
7. Add lightweight runtime automation update for committed point edits if feasible; otherwise document the temporary one-`LoadProject`-per-gesture behavior and do not call it on every point movement/tick.
