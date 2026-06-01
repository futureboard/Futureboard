# Futureboard MIDI Editor + Automation System Roadmap

Planning status: design-only pass. Do not treat this document as implemented behavior.

This roadmap defines the production direction for Futureboard's real MIDI editor, MIDI control lanes, and the unified automation system that will eventually drive track, master, tempo, and plugin parameter automation. The implementation should remain incremental, buildable, and compatible with the current GPUI native shell, the Rust audio engine / DAUx direction, the plugin host roadmap, and a future WGPU dense-viewport renderer.

## 1. Overview

Futureboard needs two closely related editing systems:

- MIDI editing: clip-local notes, velocity, and MIDI controller data.
- Automation editing: project / track / master / plugin / tempo parameter changes over musical time.

Both systems share core concepts:

- Musical timeline coordinates in beats.
- Selection, marquee, edit commands, undo / redo, clipboard, and snapping.
- Dense viewport rendering with visible-range culling.
- Immutable runtime snapshots for audio/MIDI playback.
- Project persistence with migrations.

They should not share the exact same data model. MIDI CC data belongs to MIDI clips and travels with musical MIDI content. DAW automation belongs to project / track / master / plugin state and controls mixer, transport, tempo, and plugin parameters.

## 2. Current Assumptions

- Futureboard native UI uses GPUI today.
- WGPU is planned for dense viewport rendering, but Phase 1 must work in GPUI paint/layout.
- Timeline state already has tracks, clips, selection, transport, mixer-ish state, MIDI clip notes, and early automation helpers.
- MIDI notes currently exist clip-local in timeline state and are rendered by the piano roll.
- Velocity lane exists in an early form and can mutate real note velocity.
- Automation should remain a single source of truth in project state, not duplicate local UI state.
- Audio engine integration must be snapshot-driven. UI edits may allocate; audio callback must not.
- Tempo automation is special because it changes beat/time mapping. It must not be treated as just another normalized lane in the runtime.
- Initial automation playback can support Read mode only. Write, Touch, Latch, and Trim must be visible only when honestly disabled or marked experimental.

## 3. Architecture

### 3.1 Ownership Boundaries

MIDI clip data:

- Owned by MIDI clips inside project/timeline state.
- Contains notes and controller lanes that move with the clip.
- Uses clip-local beat coordinates.
- Saved in project file.
- Exportable as MIDI later.

Automation data:

- Owned by tracks, master/project state, or global automation collections.
- Targets project entities by stable IDs.
- Uses project-global beat coordinates unless explicitly stored inside future automation clips.
- Saved in project file.
- Evaluated by runtime snapshot during playback.

Transient UI state:

- Active tool, hover point/note, marquee bounds, drag state, scroll, zoom, focused lane, and editor-local selection caches.
- Must not be the source of persisted musical data.
- Selection may be stored transiently on UI side or in state, but must not be serialized unless explicitly required.

Runtime state:

- Immutable snapshots built from project state.
- Prepared on the UI/control thread.
- Passed to audio/MIDI engine through a safe command queue.
- Audio thread reads preallocated, sorted, stable data only.

### 3.2 Editor Layers

MIDI editor:

- `MidiEditor`: container, toolbar, clip binding, focus routing.
- `PianoRoll`: note grid, piano keyboard, note interaction.
- `VelocityLane`: note velocity bars and ramp/scaling tools.
- `CcLane`: one lane per MIDI controller kind.
- `MidiClipInspector`: selected clip and note summary/edit panel.

Automation editor:

- Track automation lanes inside arrangement lanes.
- Master/global automation section.
- Tempo lane with tempo-specific rendering and value scale.
- Plugin parameter lane creation from device/editor parameter lists.
- Automation inspector for selected points/lane/target.

Shared editor infrastructure:

- Beat/grid coordinate math.
- Snap and quantize.
- Selection and marquee.
- Clipboard.
- Command/undo batching.
- Render snapshots.

## 4. Data Models

These are proposed target models, not an instruction to rewrite existing code in one patch.

### 4.1 MIDI Note Model

```rust
pub struct MidiNote {
    pub id: String,
    pub pitch: u8,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub velocity: u8,
    pub muted: bool,
    pub selected: bool,
}
```

Rules:

- `id` must be stable within the project, not a transient render-only number, once save/load/undo is complete.
- `pitch` clamps to `0..=127`.
- `velocity` clamps to `1..=127`.
- `start_beat >= 0.0`.
- `duration_beats >= MIN_NOTE_BEATS`.
- Notes should be sorted by `(start_beat, pitch, id)` for playback and binary-search rendering, but selection/order commands may preserve a separate edit order where needed.
- `selected` is convenient for editor commands, but persistence should default to not saving selection.
- Muted notes remain in data but do not emit runtime note events.

### 4.2 MIDI Clip Model

```rust
pub struct MidiClipData {
    pub notes: Vec<MidiNote>,
    pub controller_lanes: Vec<MidiControllerLane>,
    pub channel: Option<u8>,
}
```

Rules:

- Clip-local beat coordinates.
- Clip duration must auto-expand when note/CC edits exceed the current right edge unless the user is explicitly trimming the clip.
- Clip trimming must never silently delete notes or controller data.
- Loop playback must define whether clip data repeats or clip bounds truncate events.

### 4.3 MIDI Controller Model

```rust
pub enum MidiControllerKind {
    CC(u8),
    PitchBend,
    ChannelPressure,
    PolyPressure,
}

pub struct MidiControllerPoint {
    pub id: String,
    pub beat: f32,
    pub value: f32,
    pub selected: bool,
}

pub struct MidiControllerLane {
    pub id: String,
    pub kind: MidiControllerKind,
    pub points: Vec<MidiControllerPoint>,
    pub visible: bool,
    pub height: f32,
    pub collapsed: bool,
}
```

Controller value rules:

- CC values use normalized `0.0..=1.0` in project state, with UI display as `0..127`.
- Pitch bend uses normalized `0.0..=1.0` in storage, mapped to signed bend range at runtime.
- Channel pressure uses normalized `0.0..=1.0`.
- Poly pressure will need pitch or note association later; initial model may defer it.

### 4.4 Automation Target Model

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
```

Target ID rules:

- Track targets use stable track IDs.
- Send targets use stable send IDs.
- Plugin parameter targets use stable insert ID plus plugin-provided stable parameter ID.
- If a plugin parameter ID changes after plugin update, the project loader must report unresolved automation rather than silently remapping to the wrong parameter.

### 4.5 Automation Lane Model

```rust
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

Rules:

- Values are normalized unless the target has a dedicated data type, such as tempo BPM.
- Points sorted by `(beat, id)`.
- Duplicate beats are either merged by epsilon or allowed only if the curve semantics are explicit. Initial implementation should merge/replace within epsilon.
- `read_enabled` controls playback evaluation.
- `write_enabled` must not imply Touch/Latch/Write support until those modes exist.

### 4.6 Tempo Map Model

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

Rules:

- `bpm` clamps to a sane project range, initially `20.0..=300.0`.
- Tempo at beat must be queryable without allocation.
- Seconds at beat and beat at seconds require cached segments.
- Tempo map must preserve stable clip beat positions. Audio clip stretching is a later feature and must not be implied by tempo lane UI.
- Initial phase may support static tempo plus serialized tempo points, with full tempo-map playback delayed.

## 5. UI/UX Plan

### 5.1 MIDI Editor

Editor shell:

- Compact GPUI editor panel in bottom editor and optional floating window.
- Toolbar height around 30-36 px.
- Dark panel surfaces, subtle borders, tabular numeric labels.
- No generic web form controls.
- Keyboard focus must be explicit so Space does not toggle transport while editing text/numeric fields.

Toolbar:

- Tool selector: Draw, Select, Erase, Split, Velocity, CC Draw, Audition, Mute.
- Snap toggle and snap value selector.
- Quantize selector.
- Duplicate, transpose, delete, fit, zoom controls.
- Note color mode: velocity, pitch, channel, track color.
- Future: scale/key guide toggle.

Piano roll:

- Left piano keyboard lane.
- Ruler/grid aligned to arrangement beat math.
- Clip bounds shading.
- Playhead line synced to transport.
- Loop region awareness.
- Notes visible only in current pitch/beat viewport.
- Note preview/audition on click and drag, routed through a non-blocking preview path.

Velocity lane:

- Bars below notes.
- Selected notes highlighted.
- Drag per note with snap-free vertical movement.
- Multi-note velocity scale when multiple notes selected.
- Ramp tool draws progressive velocities across selected range or drag span.
- Humanize/randomize later, clearly disabled until implemented.

CC lanes:

- Add/remove lane menu.
- Default lane presets: CC1, CC7, CC10, CC11, CC64, Pitch Bend, Channel Pressure.
- Custom CC number entry.
- Lane header with kind, value scale, collapse, remove, height resize.
- Draw points, line/ramp edit, erase, marquee select, move points.
- Per-lane snap behavior: beat snaps horizontally when enabled; values are continuous vertically.

MIDI note inspector:

- Single note: pitch, note name, start beat, length, end beat, velocity, muted, future channel.
- Multi-note: selected count, pitch mixed/same, beat range, length mixed/same, velocity mixed/same.
- Bulk actions: transpose, quantize, delete, set velocity, legato, mute/unmute.

### 5.2 Automation UI

Track automation:

- Track lane mode toggles between Clips and Automation or shows automation lanes under clips.
- Lane header identifies target and current value.
- Volume/pan lanes should be first-class and easy to reveal.
- Send and plugin parameter lanes created from target picker.

Master/global automation:

- Master track can expose master volume/pan/plugin automation.
- Global automation section may live above tracks or in a dedicated global lane group.
- Tempo lane should be visually distinct and not hidden behind track automation.

Automation lane viewport:

- Lane background.
- Center/reference line.
- Points.
- Curve/segment lines.
- Selected/hover states.
- Value tooltip.
- Snap-to-grid horizontally.
- Lane resize/collapse.
- Curve handles later.

Automation tools:

- Add point.
- Move point.
- Delete point.
- Select/marquee.
- Drag segment.
- Create ramp.
- Copy/paste.
- Quantize points.
- Simplify later.
- Convert MIDI CC to automation later.

Read/write modes:

- Initial: Off and Read.
- Later: Touch, Latch, Write.
- Future: Trim.
- UI must not pretend write automation works before runtime support exists.

## 6. Event Flow

### 6.1 MIDI Note Edit Flow

1. User interacts with note/grid/lane.
2. Editor captures focus and starts a transient drag/command.
3. During drag, preview state may repaint without marking project dirty on every pointer move.
4. On commit, command validates/clamps data and writes to project state once.
5. Command enters undo stack as a single batch.
6. Project dirty flag set once.
7. Runtime MIDI snapshot invalidated or updated through a command queue.
8. Editor and arrangement preview repaint from project state/snapshot.

### 6.2 Velocity Edit Flow

1. User grabs velocity bar or selects velocity tool.
2. Editor resolves affected note IDs.
3. Live preview may update visually.
4. Commit writes velocity values once, clamps `1..=127`, and batches undo.
5. Runtime note velocity events update through snapshot rebuild or incremental command.

### 6.3 MIDI CC Edit Flow

1. User chooses CC lane and tool.
2. Draw creates points or ramp samples.
3. Move edits selected point beats/values.
4. Commit sorts lane points, merges near-duplicates, marks project dirty once.
5. Runtime MIDI controller event stream updates.

### 6.4 Automation Edit Flow

1. User reveals/selects automation target.
2. UI creates lane if missing.
3. Point/segment edit mutates automation lane through command.
4. Undo batch captures previous and next point state.
5. Runtime automation snapshot rebuilds or receives lane delta.
6. Playback evaluator applies target values in Read mode.

### 6.5 Plugin Parameter Touch Flow

1. Plugin/device UI reports parameter touched and changed.
2. UI updates live parameter value immediately through control queue.
3. If Write/Touch/Latch mode is later enabled, automation recorder creates points.
4. Initial implementation should only allow manual lane point editing and Read playback.

## 7. Engine Integration

### 7.1 MIDI Playback

Runtime note scheduling:

- Convert MIDI clips to runtime events sorted by absolute beat.
- Note on at `clip_start + note.start_beat`.
- Note off at note on plus duration.
- Skip muted notes and muted clips/tracks.
- Respect clip loop behavior once looped clips exist.
- Route instrument tracks to plugin/instrument runtime.
- External MIDI output can be added later.

Scheduling quality:

- Initial block-level scheduling is acceptable if documented.
- Target is sample-accurate scheduling using tempo map and block start/end beat mapping.
- Note preview should use a separate low-latency command path that never blocks UI or audio.

### 7.2 Automation Playback

Runtime automation:

- Evaluate automation value at playhead beat for every readable lane.
- Apply volume/pan/send/plugin values through smoothed runtime parameters.
- Avoid zipper noise with smoothing for continuous parameters.
- Mute can use hold/threshold semantics, not smoothing.
- Tempo automation is evaluated through the tempo map, not the generic normalized lane evaluator.

Audio-thread constraints:

- No allocation.
- No locks.
- No project-state traversal.
- No string lookup.
- No plugin metadata lookup.
- No logging.

### 7.3 Tempo Map Integration

Tempo map must provide:

- `tempo_at_beat(beat)`.
- `seconds_at_beat(beat)`.
- `beat_at_seconds(seconds)`.
- Segment cache built outside audio callback.
- Transport conversion for playhead and scheduling.
- Ruler/grid labels from tempo map.

Initial compatibility:

- Static tempo remains the playback source until full tempo map runtime lands.
- Tempo points can be saved and displayed before they affect playback, but UI must label this honestly.

### 7.4 Plugin Parameter Automation

Requirements:

- Plugin scanner/runtime exposes stable parameter IDs, display names, units, normalized/default values, and plain-value conversion where available.
- Automation stores normalized `0.0..=1.0` values.
- Runtime dispatch maps target to plugin instance parameter safely.
- Missing plugin or parameter produces unresolved automation warning, not silent data loss.
- Parameter updates use a lock-free/control queue compatible with plugin runtime.

### 7.5 Runtime Snapshots

```rust
pub struct RuntimeMidiClipSnapshot {
    pub clip_id: String,
    pub track_id: String,
    pub start_beat: f64,
    pub duration_beats: f64,
    pub notes: Vec<RuntimeMidiNote>,
    pub controllers: Vec<RuntimeMidiControllerLane>,
}

pub struct RuntimeAutomationLaneSnapshot {
    pub lane_id: String,
    pub target: RuntimeAutomationTarget,
    pub points: Vec<RuntimeAutomationPoint>,
    pub read_enabled: bool,
}

pub struct RuntimeTempoMapSnapshot {
    pub segments: Vec<RuntimeTempoSegment>,
}
```

Snapshot rules:

- Build immutable snapshots off the audio thread.
- Use numeric runtime target handles instead of strings in audio callback.
- Keep vectors sorted.
- Precompute curve coefficients where possible.
- Swap snapshots atomically or through a safe command queue.

## 8. Rendering Strategy

### 8.1 Phase 1 GPUI Rendering

- Render only visible notes/points.
- Precompute render geometry per frame from immutable editor snapshot.
- Use GPUI elements for interaction handles and simple drawing where current code supports it.
- Avoid thousands of persistent child elements when dense clips are loaded.
- Use canvas/paint paths for grid/segments where available.

### 8.2 Future WGPU Rendering

WGPU should own dense visuals:

- Note rectangles.
- Velocity bars.
- CC/automation curves and points.
- Grid/ruler overlays.
- Selection/marquee overlays.

GPUI should own controls:

- Toolbar.
- Lane headers.
- Inspectors.
- Menus.
- Text/numeric inputs.

Render snapshots:

```rust
pub struct MidiEditorRenderSnapshot {
    pub clip_id: String,
    pub visible_beat_range: (f32, f32),
    pub visible_pitch_range: (u8, u8),
    pub notes: Vec<NoteRenderItem>,
    pub velocity_items: Vec<VelocityRenderItem>,
    pub controller_lanes: Vec<ControllerLaneRenderItem>,
}

pub struct AutomationRenderSnapshot {
    pub visible_beat_range: (f32, f32),
    pub lanes: Vec<AutomationLaneRenderItem>,
}
```

Rendering rules:

- No project mutation during render.
- No decode/file IO during render.
- No layout-dependent project math hidden inside draw loops.
- Stable coordinate transforms shared by notes, lanes, playhead, ruler, and arrangement.

## 9. Recommended First Implementation Slice

Recommended order:

1. Phase A: audit.
2. Phase B: data model foundations.
3. Phase C: save/load migrations.
4. Phase D/E/F: basic piano roll shell, note rendering, and note editing.
5. Phase G: velocity lane.
6. Phase H: CC lanes.
7. Automation phases after MIDI basics are stable.

Rationale:

- MIDI note editing is the tightest user-facing loop and validates beat/grid/selection/undo infrastructure.
- Velocity lane is close to notes and exercises multi-edit workflows.
- CC lanes prepare the lane editor interaction model before global automation expands target complexity.
- Automation playback should not be layered on top of unstable command, selection, and render primitives.

## 10. Phase A-Z Roadmap Summary

The detailed A-Z phase plan is maintained in `tasks/native/automation-system-plan.md` because it spans MIDI, automation, tempo, engine, QA, and stabilization work. Summary order:

| Phase | Focus |
| --- | --- |
| A | Audit current MIDI/automation code and document gaps. |
| B | Data model foundation for MIDI, CC, automation, and tempo. |
| C | Project save/load migrations and roundtrip tests. |
| D | MIDI editor shell. |
| E | Note rendering. |
| F | Note editing. |
| G | Velocity lane. |
| H | CC lanes. |
| I | MIDI playback runtime. |
| J | Automation data model. |
| K | Track automation UI. |
| L | Automation playback Read mode. |
| M | Plugin parameter automation. |
| N | Master automation. |
| O | Tempo automation data model. |
| P | Tempo map playback. |
| Q | Automation write modes. |
| R | MIDI tools polish. |
| S | Automation tools polish. |
| T | Inspector integration. |
| U | Performance pass. |
| V | Undo/redo completion. |
| W | Keyboard shortcuts. |
| X | QA and regression tests. |
| Y | Documentation. |
| Z | Stabilization. |

Implementation order should stay conservative: A, B, C, D, E, F, G, H, I, then J through automation playback. Tempo playback should not start before tempo map conversion tests exist.

## 11. Testing Plan

Model tests:

- Validate note clamp/sort behavior.
- Validate CC lane point insert/move/delete behavior.
- Validate automation target serialization.
- Validate automation point sorting and duplicate epsilon handling.
- Validate tempo map segment construction.

Project tests:

- Save/load one MIDI clip.
- Save/load MIDI clip with notes, muted notes, velocities, and CC lanes.
- Save/load track automation.
- Save/load master automation.
- Save/load plugin parameter automation, including unresolved target cases.
- Save/load tempo points.

Runtime tests:

- Convert MIDI notes to note on/off runtime events.
- Convert CC points to controller events.
- Evaluate automation hold/linear curves.
- Verify smoothing receives expected value changes.
- Verify tempo map `tempo_at_beat`, `seconds_at_beat`, and `beat_at_seconds`.
- Assert runtime snapshot build happens off the audio callback path.

UI/interaction tests:

- Selection-only edits do not dirty the project.
- Drag gestures commit one undo entry.
- Spacebar does not toggle transport while text/numeric editor fields are focused.
- Bottom and floating MIDI editors stay in sync.
- Automation lane edit updates arrangement and runtime snapshot state.

Performance tests:

- 1 note baseline.
- 100 notes normal editing.
- 10k notes scroll/zoom.
- Dense CC point lane.
- Dense automation lane.
- 32 tracks with automation lanes.
- Plugin automation lane with active playback.
- CPU/FPS smoke pass for GPUI fallback.

## 12. Checklists

Detailed execution checklists live in:

- `tasks/native/midi-editor-checklist.md`
- `tasks/native/automation-system-checklist.md`

These checklists are the phase-gate criteria before implementation moves from MIDI basics into automation playback and tempo work.

## 13. Risks

- Tempo automation changes fundamental beat/time conversion and can break transport, MIDI scheduling, ruler/grid rendering, and audio clip behavior.
- Plugin parameter automation depends on stable IDs from plugin formats and wrappers.
- Dense note/point rendering can become slow if every visible item becomes a heavy GPUI element.
- Write automation can produce excessive point density without decimation.
- Selection state can become inconsistent if stored in too many places.
- Runtime snapshots can become unsafe if they contain strings or unresolved project references in audio-thread paths.
- Automation clips can introduce priority/conflict ambiguity if added before basic lane automation stabilizes.

## 14. Open Questions

- Should selection be stored in project state, editor state, or both with a sync boundary?
- Should MIDI note IDs be UUID-like strings in project files while runtime uses compact numeric handles?
- Should MIDI CC lanes be serialized only when non-empty, or preserve empty visible lanes as editor state?
- Should automation lanes live under each track, in a global collection keyed by target, or both via view groups?
- How should unresolved plugin parameters appear in the UI?
- What is the first supported plugin parameter ID stability contract for VST3/CLAP/AU?
- Should tempo automation initially be display-only or blocked entirely until runtime tempo map support exists?
- How should audio clip playback behave when tempo automation changes after audio import?
- Should automation clips be region-based from the beginning or introduced after lane automation is stable?
- What is the exact command history boundary between arrangement, MIDI editor, and automation editor?

## 15. Global Acceptance Criteria

The MIDI editor and automation system are production-ready when:

- MIDI notes can be drawn, selected, moved, resized, deleted, duplicated, copied, pasted, quantized, transposed, muted, and velocity-edited with undo/redo.
- Velocity lane and selected note state remain synchronized.
- MIDI CC lanes support drawing, selecting, moving, deleting, and saving/loading points.
- MIDI playback emits correct note and controller events from project state.
- Automation lanes can target track volume/pan, sends, plugin parameters, master volume/pan, master plugins, and tempo data.
- Automation Read mode evaluates deterministically and updates runtime mixer/plugin parameters without audio-thread allocation.
- Tempo map APIs support beat/time conversion before full tempo automation is enabled.
- Project save/load roundtrips MIDI, CC, automation, and tempo data.
- Large sessions remain responsive: 10k notes, dense CC points, 32+ tracks, and visible-range rendering.
- UI stays compact, dark, DAW-native, and honest about disabled future modes.

## 16. Documentation Outputs

This planning pass creates four documents:

- `tasks/native/midi-editor-automation-plan.md`: combined architecture, data model, UI, event flow, engine, rendering, testing, and acceptance roadmap.
- `tasks/native/midi-editor-checklist.md`: MIDI execution checklist.
- `tasks/native/automation-system-plan.md`: detailed A-Z phase plan with automation-specific architecture.
- `tasks/native/automation-system-checklist.md`: automation, engine, and QA checklist.
