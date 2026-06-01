# Futureboard Global Automation System Plan

Planning status: design-only pass. Do not implement code from this document without a separate task.

This document focuses on DAW automation: track automation, master automation, tempo automation, plugin parameter automation, and the future automation clip/lane architecture. MIDI CC lanes are related but remain MIDI clip data unless explicitly converted or linked.

## 1. Scope

The automation system must support:

- Track volume.
- Track pan.
- Track mute.
- Send amount.
- Plugin parameters.
- Master volume.
- Master pan.
- Master plugin parameters.
- Tempo.
- Time signature later.
- Global transport/project-level automation later.
- Future automation clips.

Initial implementation should prioritize manual lane editing and Read playback. Write, Touch, Latch, and Trim modes should be planned but not presented as working until runtime recording exists.

## 2. Architecture

### 2.1 Data Ownership

Track automation:

- Stored on tracks or in a project automation collection keyed by track target.
- Uses project-global beat coordinates.
- Targets stable track IDs, send IDs, insert IDs, and parameter IDs.

Master automation:

- Stored in master/project state.
- Targets master mixer and master insert chain.
- Uses project-global beat coordinates.

Tempo automation:

- Stored in a dedicated `TempoMap`.
- Uses beat/BPM points, not normalized automation values.
- Feeds transport, ruler, MIDI scheduling, and later audio clip tempo behavior.

Global automation:

- Stored in project-global lane groups.
- Covers transport/project targets that are not owned by one track.
- Includes tempo and time signature, and can later include global scene/macro controls.

Runtime automation:

- Built as immutable snapshots.
- Uses compact target handles.
- Evaluated in Read mode without allocation.
- Dispatches smoothed values to mixer/plugin/transport runtime.

### 2.2 Target Resolution

Project state stores descriptive targets. Runtime snapshots resolve them to compact handles:

- Track volume -> runtime mixer channel handle.
- Track pan -> runtime mixer channel handle.
- Send gain -> runtime send handle.
- Plugin parameter -> plugin instance handle + parameter handle.
- Master volume/pan -> master bus handle.
- Tempo -> tempo map snapshot.

Unresolved targets must be preserved in project data and surfaced in UI. They must not be deleted or silently rebound.

## 3. Data Model Proposal

```rust
pub enum AutomationTarget {
    TrackVolume { track_id: String },
    TrackPan { track_id: String },
    TrackMute { track_id: String },
    SendGain { track_id: String, send_id: String },
    PluginParameter {
        track_id: String,
        insert_id: String,
        parameter_id: String,
    },
    MasterVolume,
    MasterPan,
    MasterPluginParameter {
        insert_id: String,
        parameter_id: String,
    },
    Tempo,
    TimeSignature,
}

pub enum AutomationCurveKind {
    Hold,
    Linear,
    Bezier,
    Smooth,
    Exponential,
    Logarithmic,
}

pub struct AutomationPoint {
    pub id: String,
    pub beat: f32,
    pub value: f32,
    pub curve: AutomationCurveKind,
    pub selected: bool,
}

pub struct AutomationLane {
    pub id: String,
    pub target: AutomationTarget,
    pub points: Vec<AutomationPoint>,
    pub visible: bool,
    pub height: f32,
    pub armed: bool,
    pub read_enabled: bool,
    pub write_enabled: bool,
}
```

### 3.1 Automation Modes

```rust
pub enum AutomationMode {
    Off,
    Read,
    Touch,
    Latch,
    Write,
    Trim,
}
```

Initial mode support:

- Off: lane exists but does not affect runtime.
- Read: lane evaluates during playback.

Later mode support:

- Touch: records while parameter is touched, returns to previous automation when released.
- Latch: records after touch until stopped or mode changed.
- Write: overwrites continuously during playback.
- Trim: offsets existing automation without replacing shape.

### 3.2 Tempo Model

```rust
pub struct TempoPoint {
    pub id: String,
    pub beat: f32,
    pub bpm: f32,
    pub curve: AutomationCurveKind,
}

pub struct TempoMap {
    pub points: Vec<TempoPoint>,
}
```

Tempo map APIs:

- `tempo_at_beat(beat) -> f64`.
- `seconds_at_beat(beat) -> f64`.
- `beat_at_seconds(seconds) -> f64`.
- `segments() -> &[TempoSegment]`.

Tempo segment cache:

- Built outside audio callback.
- Sorted by beat.
- Contains start beat, start seconds, BPM/curve coefficients.
- Supports binary search.

### 3.3 Future Automation Clips

Automation clips are a later architecture. They should not block lane automation.

Proposed model:

```rust
pub struct AutomationClip {
    pub id: String,
    pub target: AutomationTarget,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub points: Vec<AutomationPoint>,
    pub muted: bool,
}
```

Rules:

- Lane automation remains the base system.
- Automation clips can be layered later as clip-local point data.
- Conflict resolution must be explicit: clip overrides lane, lane sums with clip, or clip is just an editing container.
- Do not add automation clips until lane read/edit/save/load is stable.

## 4. UI/UX Plan

### 4.1 Track Automation

- Track header exposes automation lane toggle.
- Lane target picker offers Volume, Pan, Mute, Sends, and Plugin Parameters.
- Lanes render under the owning track or replace clip lane in automation mode.
- Lane headers show target display name, current value, Read state, visibility/collapse, height drag.
- Empty lanes show a subtle center/reference line and target hint.

### 4.2 Master Automation

- Master track or master panel exposes automation lanes.
- Master Volume and Master Pan are built-in targets.
- Master plugin parameter lanes come from master insert chain.
- Master lanes should remain easy to find even when arrangement is scrolled deep into track list.

### 4.3 Tempo Automation

- Tempo lane should be global and visually distinct.
- It may live above tracks, in global lanes, or as a pinned top lane.
- BPM scale range initially `20..300`.
- Ruler can display tempo markers.
- Until full tempo map playback exists, tempo lane UI must say whether it is display/persistence only.

### 4.4 Plugin Parameter Automation

Lane creation sources:

- Plugin device parameter context menu: "Show Automation".
- Track automation target picker.
- Command palette target search.
- Later: touch parameter then create lane.

UI requirements:

- Display plugin name + parameter name.
- Show unit/display value when metadata exists.
- Store normalized values.
- Show unresolved parameter state if plugin/parameter missing.
- Avoid scanning plugins from the UI render path.

### 4.5 Automation Editing

Core interactions:

- Click lane to add point.
- Drag point to move.
- Shift/Cmd click to multi-select.
- Marquee select points.
- Delete selected points.
- Drag segment to create/move ramp.
- Modifier to constrain horizontal/vertical movement.
- Snap horizontally to grid.
- Values remain continuous vertically unless target is discrete.

Future interactions:

- Bezier handles.
- Smooth/exponential/log curves.
- Simplify selected range.
- Copy/paste points.
- Convert MIDI CC lane to automation lane.

## 5. Event Flow

### 5.1 Reveal/Create Lane

1. User chooses target.
2. System finds existing lane for target or creates a new lane.
3. Lane becomes visible and selected.
4. No runtime change occurs until points exist and Read is enabled.
5. Project dirty flag is set if lane visibility/creation is persisted.

### 5.2 Add Point

1. Pointer down in lane.
2. Beat/value resolved from lane transform.
3. Snap applied to beat if enabled.
4. Value normalized/clamped for target.
5. Point inserted or replaces existing epsilon-near point.
6. Points sorted.
7. Undo command stored.
8. Runtime automation snapshot invalidated.

### 5.3 Move Points

1. User selects one or more points.
2. Drag preview updates visually.
3. On release, point beats/values are committed as one batch.
4. Points sorted and clamped.
5. Project dirty and runtime snapshot dirty set once.

### 5.4 Automation Playback

1. Transport provides current playhead beat or beat range.
2. Runtime automation evaluator finds active points/segments.
3. Value is interpolated by curve kind.
4. Value is mapped to target domain.
5. Smoothed runtime parameter receives value.
6. Plugin/mixer/transport applies value without allocation.

### 5.5 Write Automation Later

1. Parameter touch starts recording candidate.
2. Runtime/control thread streams parameter changes with timestamps/beats.
3. Recorder decimates/simplifies points outside audio callback.
4. Mode semantics decide when recording stops.
5. Undo stores one automation recording pass.

## 6. Engine Integration Notes

### 6.1 Runtime Snapshot

```rust
pub struct RuntimeAutomationLaneSnapshot {
    pub lane_id: String,
    pub target: RuntimeAutomationTarget,
    pub points: Vec<RuntimeAutomationPoint>,
    pub read_enabled: bool,
}

pub struct RuntimeAutomationPoint {
    pub beat: f64,
    pub value: f32,
    pub curve: RuntimeAutomationCurve,
}
```

Rules:

- Build snapshots on UI/control thread.
- Resolve target strings to runtime handles before audio callback.
- Sort and validate points in snapshot builder.
- Precompute curve coefficients where useful.
- Atomic/safe snapshot swap.

### 6.2 Mixer Automation

- Volume and pan use smoothing.
- Mute uses hold/discrete semantics.
- Send gain uses smoothing.
- Automation should be applied after manual parameter value resolution according to mode.
- Initial Read mode can override manual value while playing and lane read is enabled.

### 6.3 Plugin Automation

- Plugin host exposes parameter metadata and stable IDs.
- Runtime resolves `(track_id, insert_id, parameter_id)` to plugin parameter handle.
- Parameter changes are sent through realtime-safe queue or parameter buffer.
- Normalized value is primary.
- Plain/display values are UI only.
- Missing target should show unresolved lane and skip runtime dispatch.

### 6.4 Tempo Automation

Tempo is a transport concern:

- Playback scheduling reads tempo map.
- MIDI note scheduling converts beat to sample time through tempo map.
- Ruler/grid labels read tempo map.
- Audio clip stretching is future work and must be documented separately.

Initial safe staging:

- Persist tempo map.
- Render tempo points.
- Keep static tempo as runtime source.
- Add full tempo map playback after conversion APIs and tests exist.

## 7. Rendering Strategy

Phase 1:

- GPUI lane headers and controls.
- GPUI/canvas paint fallback for automation segments and points.
- Visible range filtering.
- Point hit testing using cached geometry.

Later:

- WGPU renderer for dense point/curve rendering.
- GPUI remains for headers, menus, inspectors, and target picker.

Render snapshot:

```rust
pub struct AutomationRenderSnapshot {
    pub visible_beat_range: (f32, f32),
    pub lanes: Vec<AutomationLaneRenderItem>,
}

pub struct AutomationLaneRenderItem {
    pub lane_id: String,
    pub target_label: String,
    pub height: f32,
    pub points: Vec<AutomationPointRenderItem>,
    pub segments: Vec<AutomationSegmentRenderItem>,
}
```

Rendering rules:

- No project mutation during render.
- No plugin metadata queries during render.
- No per-frame allocation proportional to entire project when only a viewport is visible.
- Hit-test cache must be invalidated when zoom/scroll/lane data changes.

## 8. Phase A-Z Roadmap

### Phase A - Audit Current MIDI/Automation Code

Goals:

- Understand existing timeline MIDI clips, piano roll, velocity lane, automation helpers, project format, and engine snapshot code.

Tasks:

- Inspect current `timeline_state`, project format, piano roll, inspector, engine snapshot, and direct audio engine integration.
- Inspect any Electron/web MIDI editor behavior as product reference.
- Document gaps and current invariants.

Files:

- `crates/SphereUIComponents/src/components/timeline/timeline_state.rs`
- `crates/SphereUIComponents/src/components/piano_roll.rs`
- Project serialization modules.
- Engine snapshot modules.

Risks:

- Existing transient note IDs may conflict with persistence goals.

Acceptance criteria:

- Gap list and migration notes exist before data-model changes.

### Phase B - Data Model Foundation

Goals:

- Establish MIDI note, MIDI controller, automation target, automation lane, automation point, and tempo map data models.

Tasks:

- Add stable IDs.
- Add MIDI note muted state.
- Add controller lane model.
- Add automation target model.
- Add tempo map model.
- Add validation/clamping helpers.

Files:

- Timeline/project state modules.
- Shared model modules if split from `timeline_state`.

Risks:

- Large model changes can break save/load.

Acceptance criteria:

- Models compile, have defaults, and preserve current projects through migration.

### Phase C - Project Save/Load

Goals:

- Persist MIDI notes, CC lanes, automation lanes, and tempo map.

Tasks:

- Bump project format version.
- Add backward migration.
- Add roundtrip tests.

Files:

- Project format modules.
- Project serializer/deserializer.

Risks:

- Invalid old data can panic loaders if validation is not centralized.

Acceptance criteria:

- Old projects load.
- New fields roundtrip.
- Missing fields default safely.

### Phase D - MIDI Editor Shell

Goals:

- Open and focus MIDI editor for selected clip.

Tasks:

- Build editor shell, toolbar, ruler/grid, keyboard lane, clip bounds, focus routing.

Files:

- `midi_editor.rs`
- `piano_roll.rs`
- `piano_keyboard.rs`
- Layout/editor host files.

Risks:

- Keyboard focus conflicts with transport shortcuts.

Acceptance criteria:

- MIDI clip opens in bottom and floating editor with stable focus behavior.

### Phase E - Note Rendering

Goals:

- Render visible notes clearly and efficiently.

Tasks:

- Render notes, selected/muted states, pitch rows, optional labels, and playhead.
- Cull by visible range.

Files:

- `piano_roll.rs`
- `midi_note_canvas.rs` or render snapshot module.

Risks:

- Dense clips may create too many GPUI children.

Acceptance criteria:

- 100 notes smooth; 10k notes has a documented fallback/perf baseline.

### Phase F - Note Editing

Goals:

- Complete basic note editing workflow.

Tasks:

- Draw, select, multi-select, move, resize, delete, duplicate, copy/paste, quantize, transpose, split, mute.
- Batch undo commands.

Files:

- `piano_roll.rs`
- `midi_tools.rs`
- `command.rs`
- `undo.rs`

Risks:

- Drag gestures can dirty project too often.

Acceptance criteria:

- Each committed edit marks dirty once and has undo/redo.

### Phase G - Velocity Lane

Goals:

- Real velocity editing under piano roll.

Tasks:

- Render bars, drag values, multi-edit, ramp tool, selected sync.

Files:

- `velocity_lane.rs`
- `piano_roll.rs`

Risks:

- Multi-edit semantics may be unclear.

Acceptance criteria:

- Velocity changes affect playback and save/load.

### Phase H - CC Lanes

Goals:

- Add MIDI controller lanes.

Tasks:

- Lane add/remove, CC kind picker, point draw/move/delete, ramps, lane resize/collapse.

Files:

- `cc_lane.rs`
- `midi_editor_state.rs`

Risks:

- Pitch bend and pressure need different display scales.

Acceptance criteria:

- CC1/7/10/11/64 and Pitch Bend lanes save/load and render.

### Phase I - MIDI Playback Runtime

Goals:

- Convert MIDI clips to runtime events.

Tasks:

- Build runtime snapshots, schedule note on/off, schedule CC events, route to instrument/plugin placeholder.

Files:

- Engine snapshot modules.
- Direct audio engine MIDI runtime.

Risks:

- Audio-thread allocations or string lookups.

Acceptance criteria:

- Playback emits deterministic note/CC events without audio-thread allocation.

### Phase J - Automation Data Model

Goals:

- Add unified automation targets, lanes, points, curves.

Tasks:

- Track/master/global lane model.
- Target display and validation.
- Basic curve enum.

Files:

- `automation_state.rs`
- `automation_targets.rs`

Risks:

- Overcoupling automation to UI lane layout.

Acceptance criteria:

- Automation model supports track, master, plugin, tempo targets in project state.

### Phase K - Track Automation UI

Goals:

- Edit track volume/pan lanes.

Tasks:

- Reveal lanes, render points/segments, add/move/delete/select points.

Files:

- `automation_lane.rs`
- `automation_editor.rs`

Risks:

- Track virtualization and lane heights can conflict.

Acceptance criteria:

- Track volume/pan automation points can be edited and saved.

### Phase L - Automation Playback Read Mode

Goals:

- Evaluate automation in runtime.

Tasks:

- Runtime lane snapshots, value evaluator, mixer dispatch, smoothing.

Files:

- Engine snapshot modules.
- Direct audio engine automation evaluator.

Risks:

- Zipper noise if values are applied abruptly.

Acceptance criteria:

- Volume/pan read automation affects playback smoothly.

### Phase M - Plugin Parameter Automation

Goals:

- Automate plugin parameters.

Tasks:

- Parameter discovery, target picker, lane creation, normalized value mapping, runtime parameter dispatch.

Files:

- Plugin registry/host metadata modules.
- Automation target picker.
- Runtime plugin dispatch.

Risks:

- Parameter ID instability across plugin versions.

Acceptance criteria:

- A known parameter can be automated by stable ID and survives save/load.

### Phase N - Master Automation

Goals:

- Automate master volume/pan and master plugin parameters.

Tasks:

- Master lane UI, target picker, runtime dispatch.

Files:

- `master_automation.rs`
- Mixer/master state modules.

Risks:

- Master lanes can become hard to find in arrangement UI.

Acceptance criteria:

- Master volume automation plays back and saves.

### Phase O - Tempo Automation Data Model

Goals:

- Persist tempo points and tempo map.

Tasks:

- Tempo point model, validation, static tempo compatibility, display-only lane if needed.

Files:

- `tempo_lane.rs`
- Transport/project state modules.

Risks:

- Users may expect playback effect before runtime support.

Acceptance criteria:

- Tempo points save/load and UI clearly states runtime support status.

### Phase P - Tempo Map Playback

Goals:

- Make tempo map drive transport and scheduling.

Tasks:

- Beat/time conversion, segment cache, ruler/grid updates, MIDI scheduling integration.

Files:

- Transport.
- Direct audio engine scheduler.
- Ruler/grid code.

Risks:

- Existing beat/second assumptions break audio clips.

Acceptance criteria:

- Tempo changes affect playhead timing and MIDI scheduling predictably.

### Phase Q - Automation Write Modes

Goals:

- Add Touch/Latch/Write recording.

Tasks:

- Parameter touch events, recorder, point decimation, mode UI, undo batch.

Files:

- Automation recorder.
- Plugin/mixer parameter event paths.

Risks:

- Recording too many points can hurt performance.

Acceptance criteria:

- Touch/Latch/Write record automation honestly and undo as one pass.

### Phase R - MIDI Tools Polish

Goals:

- Add musical editing tools.

Tasks:

- Humanize, legato, advanced quantize, transpose dialog, duplicate patterns, scale guide.

Files:

- MIDI tools and command registry.

Risks:

- Tool UI can become cluttered.

Acceptance criteria:

- Tools are command-backed and keyboard accessible.

### Phase S - Automation Tools Polish

Goals:

- Improve automation editing.

Tasks:

- Ramps, simplify, curve handles, copy/paste, target conversion helpers.

Files:

- Automation tools and lane UI.

Risks:

- Curve semantics need runtime parity.

Acceptance criteria:

- Visual curves match runtime evaluation.

### Phase T - Inspector Integration

Goals:

- Inspect selected notes and automation points.

Tasks:

- Note inspector, point inspector, lane target info, bulk edit controls.

Files:

- Inspector panel modules.
- MIDI/automation editor integration.

Risks:

- Inspector can duplicate editor toolbar state.

Acceptance criteria:

- Inspector edits real state and does not create fake local values.

### Phase U - Performance Pass

Goals:

- Scale to dense sessions.

Tasks:

- Visible virtualization, render snapshots, no per-frame allocations, WGPU migration plan.

Files:

- Render snapshot modules.
- WGPU renderer prototype later.

Risks:

- Premature WGPU rewrite before interaction model stabilizes.

Acceptance criteria:

- 10k notes and dense automation points remain usable.

### Phase V - Undo/Redo Completion

Goals:

- Complete command history for MIDI and automation.

Tasks:

- Batch gestures, command serialization for tests, redo stability.

Files:

- Command/undo modules.

Risks:

- Commands may capture stale IDs after delete/recreate.

Acceptance criteria:

- All MIDI and automation edit commands undo/redo reliably.

### Phase W - Keyboard Shortcuts

Goals:

- Make focused editor shortcuts reliable.

Tasks:

- Register MIDI/automation commands, focus routing, text input capture.

Files:

- Command registry.
- Editor focus handlers.

Risks:

- Transport shortcuts conflict with editor inputs.

Acceptance criteria:

- Shortcuts route to focused editor and never corrupt text/numeric input.

### Phase X - QA and Regression Tests

Goals:

- Test editing, playback, persistence, and performance.

Tasks:

- Unit tests, roundtrip tests, runtime evaluator tests, large-data smoke tests.

Files:

- Test modules near model/runtime code.

Risks:

- UI-only behavior may lack coverage.

Acceptance criteria:

- Regression checklist passes before release.

### Phase Y - Documentation

Goals:

- Document user-facing and developer behavior.

Tasks:

- User docs, developer architecture, target metadata notes, plugin automation limitations.

Files:

- `tasks/native` and future user docs.

Risks:

- Docs can drift from implementation.

Acceptance criteria:

- Docs match current features and disabled future modes.

### Phase Z - Stabilization

Goals:

- Prepare for release-quality editing.

Tasks:

- Bug bash, profiling, UX review, file compatibility review, release checklist.

Files:

- No specific file; cross-system stabilization.

Risks:

- Cross-feature regressions across timeline, mixer, plugins, and transport.

Acceptance criteria:

- Acceptance criteria in roadmap and checklists pass on representative projects.

## 9. Risks

- Tempo automation can destabilize every beat/second assumption.
- Plugin parameter automation depends on stable metadata from every plugin format.
- Dense automation rendering can become slow if every point is a GPUI child.
- Write automation can generate excessive point density.
- Unresolved plugin/track/send IDs can cause data loss if not preserved.
- Audio-thread safety can be broken by innocent-looking target lookups.
- Automation clips can complicate conflict resolution if introduced too early.
- UI can become cluttered if every future mode is visible before it works.

## 10. Acceptance Criteria

- Track volume/pan lanes can be created, edited, saved, loaded, and played in Read mode.
- Master volume lane can be created, edited, saved, loaded, and played in Read mode.
- Plugin parameter lanes preserve target identity and unresolved states.
- Runtime automation evaluation allocates nothing in the audio callback.
- Continuous parameters are smoothed.
- Discrete parameters use deterministic hold/threshold behavior.
- Tempo points save/load and full tempo map playback is gated behind conversion tests.
- Automation write modes remain disabled until recording is implemented.
- 32 tracks with automation lanes remain responsive.

