# Futureboard Studio — Timeline System Stage 2 Full System Roadmap

## Purpose

Design and implement **Stage 2** of the Futureboard Timeline system.

Stage 1 focused on the composition / arrangement layer:

- Markers
- Arrangement regions
- Automation lanes UI
- Track groups / folder tracks
- Selection / shortcut cleanup
- Inspector integration
- Save/load persistence
- Timeline layer contract

Stage 2 is the heavy full-system timeline engine layer.

Stage 2 turns the Timeline from a visual arrangement surface into a deep DAW timeline system that can handle:

- Full tempo map
- Time signature map
- Beat/time/sample conversion
- Sample-accurate playback scheduling
- Sample-accurate automation playback
- Audio warping / time-stretch foundations
- Clip slip/edit/stretch tools
- Comping and take lanes
- Ripple edit
- Arranger track operations
- Advanced loop/punch/cycle workflows
- Multi-track edit groups
- WGPU dense viewport rendering
- Large-session virtualization
- Timeline engine/runtime integration

This is a **full system plan**, not a single patch.

Do not implement everything in one pass.

---

## Stage 2 Summary

Stage 2 is:

```txt
Tempo Map + Time Signature Map + Sample Timeline + Advanced Clip Editing + Automation Runtime + Comping/Takes + Arranger Track + Ripple Edit + WGPU Timeline Engine + Large Session Performance.
```

Stage 2 must preserve Stage 1 behavior while deepening the timeline engine.

---

## Hard Rules

- Do not rewrite the whole DAW in one patch.
- Do not break Stage 1 markers/regions/automation/groups.
- Do not break current audio playback.
- Do not break MIDI editor.
- Do not call `LoadProject` on every timeline edit.
- Do not mutate realtime audio state directly from UI.
- Do not allocate, lock, or log in the audio callback.
- Do not evaluate tempo/automation using UI state inside audio callback.
- Do not use floating-point beat conversions casually in realtime paths without a clear model.
- Do not create nested GPUI entity updates.
- Do not make WGPU required if CPU fallback exists.
- Do not fake advanced features visually only.
- Do not implement destructive ripple/arranger edits without undo/preview.
- Use project theme tokens only.
- Keep the UI compact, DAW-native, and performance-aware.

---

## Core Design Principles

1. **One musical timeline model**
   - Bars/beats/ticks are stable.
   - Tempo changes map musical time to seconds/samples.
   - Time signature changes affect ruler/grid/bar numbering.
   - Clips store musical positions where appropriate.

2. **Runtime snapshots, not UI reads**
   - Audio runtime consumes immutable timeline/tempo/automation snapshots.
   - UI edits produce commands.
   - Runtime swaps snapshots safely.

3. **Sample-accurate where it matters**
   - Transport clock
   - Automation evaluation
   - MIDI scheduling
   - Audio clip playback start/stop
   - Punch recording boundaries
   - Export rendering

4. **Visual timeline is not audio timeline**
   - UI can render approximate preview.
   - Audio engine needs deterministic runtime data.

5. **Everything must be undoable**
   - Ripple edits
   - Arranger operations
   - Warp edits
   - Comp edits
   - Automation edits
   - Tempo map edits

6. **Stage 2 must be phased**
   - Start with data model and conversion engine.
   - Then runtime integration.
   - Then editing tools.
   - Then rendering optimization.

---

## Part A — Stage 2 Audit

Before implementation, create:

```txt
tasks/native/timeline-stage-2-audit.md
```

Search:

```bash
rg -n "Timeline|Tempo|TimeSignature|Transport|Playhead|Sample|Beat|Bar|Tick|Automation|Warp|Stretch|Take|Comp|Ripple|Arranger|Loop|Punch|Grid|Ruler|WGPU|Viewport|RenderSnapshot|AudioClip|MidiClip|RuntimeProject" crates apps
```

Document:

- Current beat/time conversion code
- Current transport clock ownership
- Current ruler/grid implementation
- Current clip position representation
- Current automation evaluation
- Current audio runtime snapshot model
- Current MIDI runtime scheduling
- Current project save/load format
- Current WGPU/CPU render split
- Current undo/command infrastructure
- Known nested update risks
- Files likely to change
- Gaps before Stage 2 can begin safely

### Acceptance

- Audit exists.
- Stage 2 dependencies/gaps are listed.
- First safe patch is identified.

---

## Part B — Timeline Time Model

## Goal

Create a canonical timeline time model.

Stage 2 needs clear representations for:

- Beat
- Tick
- Bar/beat display
- Seconds
- Samples
- Musical position
- Absolute timeline position
- Tempo map segments
- Time signature segments

### Recommended Types

```rust
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Beat(pub f64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tick(pub i64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seconds(pub f64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SampleFrame(pub i64);
```

### Musical Position

```rust
pub struct MusicalPosition {
    pub bar: i64,
    pub beat: u32,
    pub tick: u32,
}
```

### Timeline Position

```rust
pub enum TimelinePosition {
    Beat(Beat),
    Tick(Tick),
    Seconds(Seconds),
    Sample(SampleFrame),
}
```

### Rules

- Persistent project data may store beats/ticks.
- Runtime audio should use samples and precomputed map segments.
- UI can display bars/beats using time signature map.
- Avoid ambiguous `f32` for long-session timeline positions.
- Prefer `f64` or integer ticks for musical data.
- Audio runtime should use integer sample frames where possible.

### Acceptance

- Time conversion types exist.
- Existing timeline math begins migrating to canonical helpers.
- No widespread ad-hoc `beat * px_per_beat` without helper wrappers in new code.

---

## Part C — Tempo Map Engine

## Goal

Support tempo changes across the project.

Tempo map is not just automation. It changes beat/time/sample conversion.

### Tempo Point

```rust
pub struct TempoPoint {
    pub id: String,
    pub beat: Beat,
    pub bpm: f64,
    pub curve: TempoCurveKind,
}
```

```rust
pub enum TempoCurveKind {
    Hold,
    Linear,
}
```

Stage 2 can start with `Hold` only and scaffold `Linear`.

### Tempo Map

```rust
pub struct TempoMap {
    pub points: Vec<TempoPoint>,
    pub default_bpm: f64,
}
```

### Runtime Tempo Segment

```rust
pub struct TempoSegment {
    pub start_beat: Beat,
    pub end_beat: Option<Beat>,
    pub start_seconds: Seconds,
    pub start_sample: SampleFrame,
    pub bpm: f64,
    pub samples_per_beat: f64,
}
```

### Required APIs

```rust
impl TempoMap {
    pub fn tempo_at_beat(&self, beat: Beat) -> f64;
    pub fn seconds_at_beat(&self, beat: Beat) -> Seconds;
    pub fn beat_at_seconds(&self, seconds: Seconds) -> Beat;
    pub fn sample_at_beat(&self, beat: Beat, sample_rate: u32) -> SampleFrame;
    pub fn beat_at_sample(&self, sample: SampleFrame, sample_rate: u32) -> Beat;
    pub fn build_runtime_segments(&self, sample_rate: u32) -> RuntimeTempoMap;
}
```

### Editing

- Add tempo point
- Move tempo point
- Delete tempo point
- Edit BPM
- Snap tempo point to grid
- Tempo lane display
- Tempo point inspector

### UI

Tempo lane can live:

- above track lanes
- in global automation area
- in ruler area

Display:

- BPM labels
- tempo point markers
- optional curve line
- current tempo in transport follows playhead

### Runtime

- Transport must use tempo map to advance beat position.
- Audio runtime must know sample position and beat position.
- MIDI scheduling must use tempo map.
- Automation evaluation by beat must be tied to runtime beat position.

### Acceptance

- Static tempo still works.
- Tempo points persist.
- Ruler/grid reflects tempo map enough for Stage 2.
- Runtime conversion APIs are tested.
- No old projects break.

---

## Part D — Time Signature Map

## Goal

Support time signature changes.

Time signature affects:

- bar numbering
- ruler labels
- grid accent lines
- metronome
- musical position display
- region/marker display
- future score editor

### Time Signature Point

```rust
pub struct TimeSignaturePoint {
    pub id: String,
    pub beat: Beat,
    pub numerator: u8,
    pub denominator: u8,
}
```

### Time Signature Map

```rust
pub struct TimeSignatureMap {
    pub points: Vec<TimeSignaturePoint>,
    pub default_numerator: u8,
    pub default_denominator: u8,
}
```

### APIs

```rust
impl TimeSignatureMap {
    pub fn signature_at_beat(&self, beat: Beat) -> TimeSignature;
    pub fn musical_position_at_beat(&self, beat: Beat) -> MusicalPosition;
    pub fn beat_at_musical_position(&self, pos: MusicalPosition) -> Beat;
    pub fn next_bar_beat(&self, beat: Beat) -> Beat;
    pub fn bar_start_before_or_at(&self, beat: Beat) -> Beat;
}
```

### UI

- Time signature markers in ruler/global lane
- Inspector edits numerator/denominator
- Grid/ruler bar labels update
- Transport display updates

### Stage 2 Limits

- No polymeter per track yet.
- No complex additive meters yet.
- Use standard numerator/denominator.

### Acceptance

- Time signature points persist.
- Ruler bar labels respond to time signature changes.
- Grid accents adjust.
- Transport can display current signature.

---

## Part E — Unified Grid / Snap Engine

## Goal

Create a shared grid/snap engine used by:

- Timeline
- MIDI editor
- Automation lanes
- Marker/region editing
- Clip drawing
- Arrangement editing
- WGPU render snapshots

### Grid Resolution

```rust
pub enum GridResolution {
    Bar,
    Beat,
    Division(u32), // 1/4, 1/8, 1/16, 1/32, etc.
    Triplet(u32),
    Dotted(u32),
    Samples(u32),
    Off,
}
```

### Snap Settings

```rust
pub struct SnapSettings {
    pub enabled: bool,
    pub resolution: GridResolution,
    pub adaptive: bool,
    pub snap_to_markers: bool,
    pub snap_to_regions: bool,
    pub snap_to_clip_edges: bool,
    pub snap_to_playhead: bool,
}
```

### APIs

```rust
pub fn snap_beat(beat: Beat, settings: &SnapSettings, maps: &TimelineMaps) -> Beat;
pub fn grid_lines_for_view(view: TimelineViewRange, maps: &TimelineMaps, settings: &GridSettings) -> Vec<GridLine>;
pub fn nearest_snap_target(beat: Beat, context: SnapContext) -> Option<SnapTarget>;
```

### Acceptance

- Timeline and MIDI editor share grid/snap logic where possible.
- Clip draw preview uses same snap logic.
- Markers/regions/automation points use same snap logic.
- Grid lines remain stable under zoom.

---

## Part F — Advanced Clip Editing Foundation

## Goal

Build advanced clip editing tools on top of stable model.

### Tools

- Select
- Draw
- Split
- Slip
- Stretch
- Fade
- Mute
- Glue/Join later
- Razor/range later

### Clip Operations

- Move clip
- Resize left/right
- Slip contents inside clip
- Split at playhead/cursor
- Duplicate
- Mute/unmute
- Change gain
- Fade in/out
- Crossfade later
- Stretch audio later

### Audio Clip Extended Model

```rust
pub struct AudioClip {
    pub id: String,
    pub track_id: String,
    pub source_asset_id: String,
    pub start_beat: Beat,
    pub duration_beats: Beat,
    pub source_offset_seconds: Seconds,
    pub source_duration_seconds: Seconds,
    pub gain_db: f64,
    pub muted: bool,
    pub fade_in: Option<ClipFade>,
    pub fade_out: Option<ClipFade>,
    pub stretch: Option<ClipStretchState>,
}
```

### Clip Fade

```rust
pub struct ClipFade {
    pub length_beats: Beat,
    pub curve: FadeCurveKind,
}
```

```rust
pub enum FadeCurveKind {
    Linear,
    EqualPower,
    Slow,
    Fast,
}
```

### Slip Editing

Slip editing changes source offset without changing clip timeline bounds.

Rules:

- Drag inside clip with Slip tool.
- Clip start/duration remain unchanged.
- `source_offset` changes.
- Clamp to source media boundaries.
- Show waveform preview shifting inside clip.

### Stretch Editing

Stretch editing changes clip duration and playback rate/time-stretch state.

Stage 2 may scaffold before full DSP:

```rust
pub struct ClipStretchState {
    pub mode: StretchMode,
    pub original_duration_seconds: Seconds,
    pub stretched_duration_beats: Beat,
    pub playback_rate: f64,
}
```

```rust
pub enum StretchMode {
    Resample,
    ElastiqueLikePlaceholder,
    OfflineRendered,
}
```

Initial Stage 2:
- support playback-rate stretch for prototype
- mark high-quality stretch as future
- no fake high-quality claim

### Acceptance

- Slip tool works for audio clips.
- Split/mute/duplicate remain stable.
- Fade data model exists and basic render handles show.
- Stretch scaffold exists without fake DSP claim.

---

## Part G — Audio Warping / Time-Stretch System

## Goal

Design audio warping in a way that can become production-grade.

Stage 2 should scaffold warping carefully.

### Warp Concepts

- Clip source time
- Timeline beat time
- Warp markers
- Stretch segments
- Transient markers
- Offline render cache
- Realtime preview quality vs final quality

### Warp Marker

```rust
pub struct WarpMarker {
    pub id: String,
    pub source_seconds: Seconds,
    pub target_beat: Beat,
    pub locked: bool,
}
```

### Warp State

```rust
pub struct AudioWarpState {
    pub enabled: bool,
    pub markers: Vec<WarpMarker>,
    pub algorithm: WarpAlgorithm,
    pub preserve_pitch: bool,
    pub cache_status: WarpCacheStatus,
}
```

```rust
pub enum WarpAlgorithm {
    RealtimePreview,
    OfflineHighQuality,
    ResampleOnly,
}
```

```rust
pub enum WarpCacheStatus {
    Clean,
    Dirty,
    Rendering,
    Failed(String),
}
```

### Stage 2 Phases

1. Data model only
2. UI markers
3. Playback-rate stretch preview
4. Offline render cache
5. High-quality algorithm integration later

### Rules

- Do not claim high-quality warp exists before DSP exists.
- Do not block UI while rendering warp cache.
- Do not do time-stretch heavy work in audio callback.
- Do not mutate source audio.

### Acceptance

- Warp state persists.
- Warp markers can be edited visually if implemented.
- Audio engine can ignore advanced warp until supported.
- UI clearly marks unsupported/preview behavior.

---

## Part H — Sample-Accurate Automation Runtime

## Goal

Move automation from UI-only lanes to runtime-safe sample-accurate evaluation.

Stage 1 created automation lanes and points.

Stage 2 makes automation real.

### Runtime Automation Lane

```rust
pub struct RuntimeAutomationLane {
    pub target: AutomationTarget,
    pub points: Vec<RuntimeAutomationPoint>,
    pub interpolation: AutomationInterpolation,
}
```

```rust
pub struct RuntimeAutomationPoint {
    pub sample: SampleFrame,
    pub value: f64,
    pub curve: AutomationCurveKind,
}
```

### Automation Block Evaluation

```rust
pub struct AutomationBlock {
    pub target: AutomationTarget,
    pub values: AutomationValueBuffer,
}
```

Options:

1. Per-block scalar if no change inside block.
2. Per-sample buffer if automation changes inside block.
3. Hybrid: scalar + ramp.

Recommended initial runtime model:

```rust
pub enum AutomationBlockValue {
    Constant(f64),
    LinearRamp { start: f64, end: f64 },
    PerSample(Vec<f32>), // preallocated/reused, not allocated in callback
}
```

### Targets

- Track volume
- Track pan
- Send gain
- Plugin parameter
- Master volume
- Master plugin parameter
- Tempo handled separately by tempo map

### Rules

- No allocation in callback.
- Prebuild runtime points as sample positions.
- Use immutable runtime automation snapshot.
- Rebuild snapshot on automation edit, not in callback.
- Plugin parameter automation must use realtime-safe parameter queue or direct safe host API.
- Do not call UI from audio runtime.

### Acceptance

- Volume automation affects audio.
- Pan automation affects audio.
- Plugin param automation scaffold works if plugin host supports it.
- Automation remains synced with mixer/inspector effective values.
- No audio-thread allocation.

---

## Part I — Clip / Timeline Runtime Snapshot

## Goal

Create runtime timeline snapshot consumed by audio engine.

### Runtime Timeline Snapshot

```rust
pub struct RuntimeTimelineSnapshot {
    pub sample_rate: u32,
    pub tempo_map: RuntimeTempoMap,
    pub time_signature_map: RuntimeTimeSignatureMap,
    pub tracks: Vec<RuntimeTrackTimeline>,
    pub automation: Vec<RuntimeAutomationLane>,
    pub loop_region: Option<RuntimeLoopRegion>,
    pub punch_region: Option<RuntimePunchRegion>,
}
```

### Runtime Clip

```rust
pub struct RuntimeAudioClip {
    pub clip_id: String,
    pub track_id: String,
    pub source_handle: AudioSourceHandle,
    pub start_sample: SampleFrame,
    pub end_sample: SampleFrame,
    pub source_offset_samples: SampleFrame,
    pub gain: f32,
    pub fade_in: RuntimeFade,
    pub fade_out: RuntimeFade,
    pub muted: bool,
    pub stretch: Option<RuntimeStretchState>,
}
```

### Runtime MIDI Clip

```rust
pub struct RuntimeMidiClip {
    pub clip_id: String,
    pub track_id: String,
    pub start_sample: SampleFrame,
    pub end_sample: SampleFrame,
    pub events: Vec<RuntimeMidiEvent>,
}
```

### Rules

- Runtime snapshot is immutable.
- UI/project edits build new snapshot.
- Audio engine swaps snapshot at safe boundary.
- Runtime snapshot uses sample positions.
- Project model can remain beat-based.

### Acceptance

- Timeline runtime snapshot can be built.
- Audio engine can consume it without UI state.
- Snapshot build is not done every frame.

---

## Part J — Comping and Take Lanes

## Goal

Support recording/editing multiple takes and comping.

### Take Lane Model

```rust
pub struct TakeLane {
    pub id: String,
    pub name: String,
    pub clips: Vec<String>,
    pub muted: bool,
    pub color: Option<ProjectColor>,
}
```

### Comp Region

```rust
pub struct CompSegment {
    pub id: String,
    pub take_lane_id: String,
    pub source_clip_id: String,
    pub start_beat: Beat,
    pub end_beat: Beat,
}
```

### Track Take State

```rust
pub struct TrackTakeState {
    pub take_lanes: Vec<TakeLane>,
    pub comp_segments: Vec<CompSegment>,
    pub show_takes: bool,
}
```

### UI

- Expand take lanes under parent track.
- Show recorded takes as lanes.
- Active comp lane renders final comp.
- Drag/select comp regions.
- Audition take lane.
- Promote selection to comp.

### Commands

- Show Takes
- Hide Takes
- Create Take Lane
- Delete Take Lane
- Promote Take Range to Comp
- Split Comp Segment
- Clear Comp
- Flatten Comp later

### Rules

- Comping should be non-destructive.
- Source recordings remain intact.
- Flatten/render comp is later.
- Editing comp marks project dirty.
- Take recording integration can come after audio recording foundation.

### Acceptance

- Take lane data model exists.
- Basic UI can show/hide take lanes.
- Comp segment model persists.
- Full comp workflow can be phased later.

---

## Part K — Ripple Edit System

## Goal

Add timeline operations that move later content automatically.

Ripple editing is dangerous and must be explicit.

### Ripple Modes

```rust
pub enum RippleMode {
    Off,
    RippleTrack,
    RippleAll,
}
```

### Ripple Operations

- Delete time range
- Insert time
- Delete clip with ripple
- Paste with ripple
- Move region with ripple later

### Time Range

```rust
pub struct TimelineRange {
    pub start: Beat,
    pub end: Beat,
}
```

### Rules

- Ripple must be visibly enabled.
- Never surprise-ripple when mode is off.
- Preview affected range if possible.
- Undo must restore all affected items.
- Markers/regions/automation should move with content in RippleAll.
- Track-only ripple affects selected track lanes only.
- Tempo/time signature points should not move unless explicit.

### Acceptance

- Ripple mode toggle exists.
- Delete range with ripple works for clips first.
- Markers/automation movement can be staged.
- Undo works.

---

## Part L — Arranger Track Advanced Operations

## Goal

Stage 1 arrangement regions were visual.

Stage 2 makes arrangement regions operational.

### Arranger Operations

- Duplicate section
- Move section
- Delete section
- Insert section
- Rename section
- Reorder sections
- Export section
- Loop section

### Affected Data

Operations may affect:

- Clips
- MIDI clips
- Automation points
- Markers
- Regions
- Tempo/time signature points optionally
- Take lanes
- Warp markers maybe later

### Rules

- Arranger operations must be undoable.
- Show preview before destructive operations.
- Avoid moving tempo/time signature by default unless user chooses.
- Preserve relative positions inside region.
- Handle overlapping clips carefully.
- Do not destroy hidden/collapsed group contents.

### Acceptance

- Duplicate arrangement region works for clips and automation within region.
- Move section works with preview or clear behavior.
- Delete section can use ripple system.
- Undo works.

---

## Part M — Loop, Cycle, Punch, and Range System

## Goal

Unify loop/cycle/punch/range editing.

### Loop Region

```rust
pub struct LoopRegion {
    pub enabled: bool,
    pub start_beat: Beat,
    pub end_beat: Beat,
}
```

### Punch Region

```rust
pub struct PunchRegion {
    pub enabled: bool,
    pub start_beat: Beat,
    pub end_beat: Beat,
}
```

### Time Selection

```rust
pub struct TimeSelection {
    pub start_beat: Beat,
    pub end_beat: Beat,
    pub tracks: Option<Vec<String>>,
}
```

### UI

- Loop range in ruler.
- Punch range distinct from loop.
- Time selection overlay.
- Drag handles.
- Inspector/status display.

### Runtime

- Loop playback wraps at sample-accurate boundary.
- Punch recording starts/stops at correct sample.
- Export selected range uses same range model.

### Acceptance

- Loop region edits persist.
- Transport respects loop region.
- Punch region scaffold exists.
- Time selection can be used by edit/export operations.

---

## Part N — Multi-track Edit Groups

## Goal

Allow editing groups of tracks together.

### Edit Group

```rust
pub struct EditGroup {
    pub id: String,
    pub name: String,
    pub track_ids: Vec<String>,
    pub enabled: bool,
    pub color: Option<ProjectColor>,
}
```

### Grouped Operations

- Move clips together
- Split clips together
- Resize related clips
- Mute/solo optional
- Automation link optional later

### Rules

- Edit groups are not the same as folder/group tracks.
- Track folder affects organization.
- Edit group affects editing behavior.
- User must explicitly enable edit group behavior.
- Avoid surprising multi-track edits.

### Acceptance

- Edit group model exists.
- Basic grouped clip move/split can be phased.
- UI shows edit group membership.

---

## Part O — WGPU Timeline Viewport Full System

## Goal

Move dense timeline rendering to WGPU while keeping GPUI as shell.

### GPUI Responsibilities

- App chrome
- Menus
- Panels
- Inspector
- Track headers where useful
- Floating controls
- Inputs/text
- Popovers/dialogs

### WGPU Responsibilities

- Timeline grid
- Clips
- Waveforms
- MIDI previews
- Automation curves
- Markers/regions background
- Playhead
- Selection overlays
- Dense visible lanes
- Large-session rendering

### Render Snapshot

```rust
pub struct TimelineRenderSnapshot {
    pub viewport: TimelineViewport,
    pub tracks: Vec<RenderTrack>,
    pub clips: Vec<RenderClip>,
    pub automation: Vec<RenderAutomationLane>,
    pub markers: Vec<RenderMarker>,
    pub regions: Vec<RenderRegion>,
    pub playhead: RenderPlayhead,
    pub selection: RenderSelection,
    pub theme: TimelineRenderTheme,
}
```

### Rules

- WGPU render snapshot must be built outside render hot path where possible.
- No project mutation during WGPU render.
- No UI state read from GPU renderer.
- CPU fallback must remain.
- Use global theme tokens converted into render theme.
- Handle old/weak GPUs with fallback/quality levels.

### GPU Device Support

Support:

- NVIDIA
- AMD via DX/Vulkan/Metal where applicable
- Intel iGPU

Settings:

- Renderer: GPU Acceleration / CPU Render
- GPU Device: Auto / device list
- Timeline Quality: High / Balanced / Low
- Restart required if backend/device changes

### Acceptance

- Timeline can render through WGPU snapshot.
- CPU fallback works.
- Viewport fills full available size.
- Old laptop mode reduces density/effects.
- No WGPU dependency in audio runtime.

---

## Part P — Large Session Virtualization

## Goal

Handle large projects.

Target examples:

- 1000 tracks idle
- 200 tracks active
- 10,000 clips
- dense automation
- long 3-hour audio file
- many waveform chunks

### Virtualization

- Only layout visible tracks.
- Only render visible clips.
- Only fetch visible waveform chunks.
- Only render visible automation points/segments.
- Use LOD for grid/waveforms.
- Avoid per-frame allocations.
- Avoid full project scans every paint.

### Data Structures

- Track visibility tree
- Clip interval index
- Marker/region interval index
- Automation point range query
- Waveform chunk cache
- Render snapshot cache

### Acceptance

- Large session scroll remains responsive.
- Zoom does not regenerate everything.
- Waveform chunks stream/cache.
- Timeline render cost tracks visible content, not full project size.

---

## Part Q — Timeline Undo/Redo Full Command System

## Goal

All Stage 2 edits must be undoable.

### Command Trait

```rust
pub trait TimelineCommand {
    fn apply(&mut self, project: &mut ProjectState) -> CommandOutcome;
    fn undo(&mut self, project: &mut ProjectState) -> CommandOutcome;
    fn label(&self) -> &'static str;
}
```

### Batch Command

```rust
pub struct BatchTimelineCommand {
    pub label: String,
    pub commands: Vec<Box<dyn TimelineCommand>>,
}
```

### Required Undo Coverage

- Tempo points
- Time signature points
- Markers
- Regions
- Automation points
- Clip move/resize/split/slip/stretch
- Ripple operations
- Arranger operations
- Group/ungroup
- Take lane edits
- Loop/punch region edits

### Rules

- Drag gesture creates one undo item, not one per mouse move.
- Batch operations are atomic.
- Failed apply should not partially corrupt project.
- Undo/redo must not call audio runtime directly; project changes trigger snapshot rebuild.

### Acceptance

- Core Stage 2 operations undo/redo.
- No dirty state false positives.
- No undo spam from drag.

---

## Part R — Export / Render Range Integration

## Goal

Timeline ranges drive export/render.

Use the same range model for:

- Export selected range
- Export arrangement region
- Export full song
- Bounce selected clips
- Freeze track
- Render comp
- Render warped audio cache

### Export Range

```rust
pub enum ExportRange {
    FullSong,
    TimeSelection(TimelineRange),
    ArrangementRegion(String),
    LoopRegion,
}
```

### Acceptance

- Timeline range model can feed export system.
- Arrangement region can be selected for export later.
- No duplicate range systems.

---

## Part S — Timeline Diagnostics

Add debug flags:

```txt
FUTUREBOARD_TIMELINE_STAGE2_DEBUG=1
FUTUREBOARD_TEMPO_MAP_DEBUG=1
FUTUREBOARD_TIME_SIGNATURE_DEBUG=1
FUTUREBOARD_SNAP_DEBUG=1
FUTUREBOARD_RUNTIME_TIMELINE_DEBUG=1
FUTUREBOARD_AUTOMATION_RUNTIME_DEBUG=1
FUTUREBOARD_WGPU_TIMELINE_DEBUG=1
FUTUREBOARD_TIMELINE_PERF_DEBUG=1
FUTUREBOARD_RIPPLE_DEBUG=1
FUTUREBOARD_ARRANGER_DEBUG=1
FUTUREBOARD_COMPING_DEBUG=1
```

Log:

- tempo map build
- beat/sample conversion
- time signature conversion
- snap decisions
- runtime snapshot build
- automation runtime segment count
- WGPU snapshot stats
- visible track count
- visible clip count
- ripple affected items
- arranger affected items

Do not log per frame unless throttled.

---

## Part T — Tests

### Unit Tests

Tempo map:

- beat → seconds
- seconds → beat
- beat → sample
- sample → beat
- multiple tempo points
- tempo point at beat 0
- invalid tempo rejected

Time signature:

- beat → bar/beat/tick
- bar/beat/tick → beat
- signature changes
- grid accents

Snap:

- snap to bar
- snap to beat
- snap to 1/16
- snap to markers
- snap off

Automation:

- hold
- linear
- block crossing point
- constant block
- ramp block
- target lookup

Ripple:

- delete range
- insert time
- move affected clips
- markers/automation policy

Undo:

- drag command single undo
- batch command undo

### Integration Tests

- save/load tempo map
- save/load regions/markers/automation/takes
- runtime snapshot build
- audio clip sample positions
- MIDI event sample positions
- loop region conversion
- export range conversion

### Manual Tests

- add tempo point
- move tempo point
- grid changes correctly
- add time signature change
- bar labels update
- slip audio clip
- split clips across tracks
- draw automation and hear it
- duplicate arrangement section
- ripple delete range
- show take lanes
- WGPU/CPU renderer switch
- large project scroll test

---

## Stage 2 Phases

## Phase 2A — Audit and Time Model

- Stage 2 audit
- canonical Beat/Tick/Seconds/SampleFrame types
- conversion helper skeleton
- no UI rewrite

## Phase 2B — Tempo Map Core

- tempo points
- runtime tempo segments
- conversion tests
- static tempo compatibility
- save/load

## Phase 2C — Time Signature Map

- time signature points
- bar/beat conversion
- ruler/grid update
- save/load

## Phase 2D — Unified Grid/Snap Engine

- shared snap helper
- grid line generator
- marker/clip edge snap scaffold
- MIDI/timeline compatibility

## Phase 2E — Runtime Timeline Snapshot

- runtime clip sample positions
- runtime MIDI event sample positions
- runtime loop region
- audio engine snapshot handoff

## Phase 2F — Sample-Accurate Automation Runtime

- runtime automation lanes
- constant/ramp blocks
- track volume/pan runtime
- plugin parameter scaffold

## Phase 2G — Advanced Clip Tools

- slip tool
- fade handles
- split improvements
- stretch data scaffold

## Phase 2H — Warp Scaffold

- warp marker model
- warp UI scaffold
- playback-rate preview only if safe
- offline cache plan

## Phase 2I — Loop/Punch/Range System

- loop region
- punch region scaffold
- time selection
- transport integration

## Phase 2J — Comping / Take Lanes

- take lane model
- show/hide takes
- comp segment model
- recording integration later

## Phase 2K — Ripple Edit

- ripple mode
- delete range clips
- undo
- preview/status

## Phase 2L — Arranger Operations

- duplicate region
- move region contents
- delete region using ripple
- undo

## Phase 2M — Multi-track Edit Groups

- edit group model
- grouped clip operation scaffold
- UI indicators

## Phase 2N — WGPU Timeline Renderer

- render snapshot
- grid/clips/playhead WGPU path
- CPU fallback
- device settings integration

## Phase 2O — Large Session Performance

- interval indexes
- visible track/clip culling
- waveform LOD
- render snapshot caching

## Phase 2P — Export/Bounce Range Integration

- export range model
- arrangement region export scaffold
- loop/time selection export scaffold

## Phase 2Q — QA / Stabilization

- old project migration
- unit tests
- manual tests
- performance pass
- crash/panic pass

---

## Recommended Implementation Order

Start with:

1. Phase 2A — Audit and Time Model
2. Phase 2B — Tempo Map Core
3. Phase 2C — Time Signature Map
4. Phase 2D — Grid/Snap Engine
5. Phase 2E — Runtime Timeline Snapshot

Then:

6. Phase 2F — Automation Runtime
7. Phase 2G — Advanced Clip Tools
8. Phase 2I — Loop/Punch/Range

Then:

9. Phase 2J — Comping/Takes
10. Phase 2K — Ripple Edit
11. Phase 2L — Arranger Operations

Finally:

12. Phase 2N — WGPU Timeline Renderer
13. Phase 2O — Large Session Performance
14. Phase 2P/Q — Export + Stabilization

Do not start with WGPU rewrite first.

Do not start with comping/ripple before time model is stable.

---

## Recommended First Patch

Start with **Phase 2A only**.

### Phase 2A Deliverables

- `tasks/native/timeline-stage-2-audit.md`
- canonical time model module
- conversion helper skeleton
- unit test placeholders or initial tests
- no visible UI rewrite
- build green

### Stop after Phase 2A.

Then proceed to Phase 2B only after review.

---

## Final Stage 2 Acceptance

Stage 2 is complete when:

- Tempo map works and persists.
- Time signature map works and persists.
- Grid/ruler reflects tempo/time signature maps.
- Timeline has stable beat/seconds/sample conversion.
- Runtime timeline snapshot feeds audio engine.
- Automation can be evaluated runtime-safely.
- Clip slip/fade/stretch scaffold works.
- Loop/punch/range model is unified.
- Take lane/comping model works at least minimally.
- Ripple edit exists with undo.
- Arranger region operations exist with undo.
- WGPU timeline renderer exists with CPU fallback.
- Large sessions remain responsive.
- Export/bounce can consume timeline ranges.
- Old projects load safely.
- No GPUI double update panic.
- Build/check passes.

---

## One-Line Summary

Stage 2 is the deep timeline engine:

```txt
Tempo Map + Time Signature Map + Sample Timeline + Automation Runtime + Advanced Clip Editing + Takes/Comping + Ripple/Arranger + WGPU Renderer + Large Session Performance.
```

End of Stage 2.
