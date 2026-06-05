# Futureboard Studio — Timeline System Stage 1 Roadmap

## Purpose

Design and implement **Stage 1** of the Futureboard Timeline system.

Stage 1 focuses on the **Timeline Composition / Arrangement layer**:

- Automation lanes
- Marker system
- Arrangement regions
- Track groups / folder tracks
- Track lanes and lane expansion
- Timeline selection and edit command foundation
- Inspector integration
- Save/load persistence
- UI rendering polish
- Debug tools and QA checklist

Stage 1 should make the timeline feel like a real DAW arrangement surface without jumping too early into deep engine features.

---

## Stage Split

### Stage 1 — Timeline Composition + Arrangement System

Stage 1 includes:

- Markers
- Arrangement regions
- Automation lanes
- Track groups and folder tracks
- Lane expand/collapse
- Clip grouping foundations
- Selection model cleanup
- Timeline edit command routing
- Inspector integration
- Project save/load model
- Layer/z-order polish

### Stage 2 — Timeline Engine + Advanced Editing

Stage 2 is deferred:

- Full tempo map engine
- Sample-accurate automation playback
- Audio warping/time-stretch
- Comping/take lanes
- Ripple edit
- Arranger track advanced workflow
- Full WGPU viewport rewrite
- Large-session virtualization beyond safe UI culling
- Deep audio engine scheduling changes

---

## Hard Rules

- Do not rewrite the whole Timeline.
- Do not break current clip editing.
- Do not break MIDI editor.
- Do not break audio/plugin runtime.
- Do not call `LoadProject` on every small edit.
- Do not create nested GPUI entity update / double lease panic.
- Do not add fake UI actions that claim to work but do nothing.
- Use global theme tokens only.
- Keep UI compact and DAW-like.
- Every persistent edit must mark project dirty through `StudioLayout` only.
- Timeline/MIDI/child components must return `CommandOutcome`; they must not update `StudioLayout` directly.

---

## Stage 1 Goals

Stage 1 should implement or scaffold safely:

1. Marker system
2. Arrangement regions
3. Automation lanes
4. Track group / folder tracks
5. Lane expand/collapse
6. Clip grouping foundations
7. Selection model cleanup
8. Timeline edit command routing
9. Inspector integration
10. Save/load persistence
11. UI rendering polish
12. Debug tools and QA checklist

---

## Part A — Audit Current Timeline Architecture

Before changing code, audit the current implementation.

Search:

```bash
rg -n "Timeline|TrackLane|Clip|Marker|Automation|Group|Folder|Region|Selection|EditCommand|CommandOutcome|track group|marker|lane|Inspector|project dirty|mark_project_changed|dispatch_command" crates apps
```

Create:

```txt
tasks/native/timeline-stage-1-audit.md
```

Document:

- Current timeline state ownership
- Current track model
- Current clip model
- Current selection model
- Current edit command flow
- Current automation model
- Existing marker/region code if any
- Files likely to change
- Risks for nested GPUI updates
- Save/load gaps

### Acceptance

- Audit file exists.
- Audit identifies smallest safe implementation slices.

---

## Part B — Timeline Stage 1 Data Model

Add or confirm persistent models.

### Marker

```rust
pub struct TimelineMarker {
    pub id: String,
    pub name: String,
    pub beat: f64,
    pub color: Option<ProjectColor>,
    pub kind: MarkerKind,
}

pub enum MarkerKind {
    Marker,
    Cue,
    SectionStart,
}
```

### Arrangement Region

```rust
pub struct ArrangementRegion {
    pub id: String,
    pub name: String,
    pub start_beat: f64,
    pub end_beat: f64,
    pub color: Option<ProjectColor>,
}
```

### Automation Lane

```rust
pub struct AutomationLane {
    pub id: String,
    pub target: AutomationTarget,
    pub points: Vec<AutomationPoint>,
    pub visible: bool,
    pub height: f32,
    pub read_enabled: bool,
    pub armed: bool,
}
```

### Automation Target

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
    Tempo,
}
```

### Automation Point

```rust
pub struct AutomationPoint {
    pub id: String,
    pub beat: f64,
    pub value: f64,
    pub curve: AutomationCurveKind,
    pub selected: bool,
}

pub enum AutomationCurveKind {
    Hold,
    Linear,
    Smooth,
}
```

### Track Group / Folder

```rust
pub enum TrackKind {
    Audio,
    Instrument,
    Midi,
    Bus,
    Return,
    Group,
    Folder,
    Master,
}
```

Recommended group model:

```rust
pub struct Track {
    pub id: String,
    pub parent_id: Option<String>,
    pub is_group_collapsed: bool,
}
```

Preferred behavior:

- Store `parent_id` on child tracks.
- Group/folder track owns collapse state.
- Compute visible track tree from flat list.
- Do not reorder tracks destructively unless user explicitly groups/reorders.
- Preserve stable track IDs.
- Save/load must roundtrip.

### Acceptance

- Models exist or current models are extended.
- Old projects without these fields load with defaults.

---

## Part C — Marker System

## Goal

Add timeline markers that can be created, selected, moved, renamed, deleted, and saved.

### UI

- Marker lane above timeline ruler or inside ruler header.
- Marker flags/triangles on ruler.
- Marker label visible when zoom allows.
- Selected marker has accent outline.
- Hover marker shows tooltip:
  - name
  - bar/beat
  - kind

### Commands

- Add Marker at Playhead
- Add Marker at Mouse Position
- Rename Marker
- Delete Marker
- Move Marker
- Select Marker
- Next Marker
- Previous Marker

### Keyboard

Optional:

- `M` = add marker at playhead if focus is timeline
- `Delete` = delete selected marker

### Inspector

When marker selected:

- Name
- Position
- Color
- Kind

### Save/load

- Markers are stored in project.

### Rules

- Marker beat clamps to `>= 0`.
- Marker move snaps if snap enabled.
- Duplicate marker positions are allowed.
- Avoid exact duplicate names if easy.
- Marker edits mark project dirty.
- Selection-only does not dirty project.

### Acceptance

- Add marker at playhead works.
- Marker renders on ruler.
- Marker can move/rename/delete.
- Save/load roundtrips.

---

## Part D — Arrangement Regions

## Goal

Add arrangement regions/sections on the timeline.

Use cases:

- Intro
- Verse
- Chorus
- Bridge
- Drop
- Outro

### UI

- Region lane above tracks, below or near ruler.
- Region blocks with name and color.
- Drag edges to resize.
- Drag body to move.
- Double click rename.
- Right click context menu.

### Commands

- Add Region from Selection
- Add Region at Playhead
- Rename Region
- Delete Region
- Move Region
- Resize Region
- Select Region

### Inspector

When region selected:

- Name
- Start
- End
- Length
- Color

### Rules

- `end_beat > start_beat`
- Snap start/end if snap enabled.
- Stage 1 can allow overlapping regions.
- No arranger reorder/copy song sections yet; that is Stage 2.

### Acceptance

- Regions render.
- Add/rename/move/resize/delete works.
- Save/load roundtrips.
- No ripple editing yet.

---

## Part E — Automation Lanes Stage 1

## Goal

Make automation lanes usable in timeline.

Stage 1 automation is UI/project-layer first:

- Show/hide automation lane
- Add points
- Move points
- Delete points
- Select points
- Track volume automation sync with effective volume if already present
- Save/load
- Inspector

Do not implement sample-accurate automation engine in Stage 1 unless already available.

### Lane UI

Track can expand automation lanes under the main lane.

Each lane has a header:

- Target name
- Read toggle
- Lane height
- Close/hide

Lane body:

- Line/curve
- Points
- Hover value
- Selected points

### Targets Stage 1

- Track Volume
- Track Pan
- Master Volume
- Plugin Parameter scaffold if plugin parameter list exists
- Tempo lane as data scaffold only; full tempo engine deferred

### Commands

- Show Automation Lane
- Hide Automation Lane
- Add Automation Point
- Move Automation Point
- Delete Automation Point
- Select Automation Point
- Clear Automation Lane
- Set Automation Target

### Editing

- Click line adds point.
- Drag point moves beat/value.
- Shift/Ctrl additive select.
- Delete removes selected points.
- Snap applies to horizontal beat only.
- Vertical value is continuous/clamped by target range.

### Value Ranges

- Volume: `-60 dB` to `+12 dB`, or `-inf` if supported.
- Pan: `-100..+100`
- Plugin parameters: `0.0..1.0` normalized
- Tempo: `20..300 BPM`, data only for Stage 1

### Dirty Behavior

- Point edit marks project dirty.
- Selection-only does not dirty project.

### Acceptance

- Track Volume automation lane is usable.
- Points can be added/moved/deleted.
- Lane can be shown/hidden.
- Save/load works.
- Fader/effective value sync does not fake or loop.

---

## Part F — Track Group / Folder Tracks Stage 1

## Goal

Support group/folder track organization.

### Stage 1 Features

- Create Folder Track
- Create Group Track
- Assign selected tracks to folder/group
- Collapse/expand group
- Render children indented
- Group color strip
- Basic group mute/solo behavior
- Save/load hierarchy

### Folder Track

- Organizational only
- No audio processing
- Can collapse children

### Group Track

- Organizational + future routing
- Stage 1 may behave like folder unless bus routing exists
- If routing graph supports group bus, optionally route child outputs to group
- Do not fake audio group processing if not implemented

### Commands

- Create Folder Track
- Create Group Track
- Group Selected Tracks
- Ungroup Tracks
- Collapse/Expand Group
- Move Track Into Group
- Move Track Out Of Group

### UI

- Folder/group track header
- Disclosure arrow
- Children indented
- Collapsed state hides children lanes
- Collapsed group may show summary lane placeholder
- Children track order preserved

### Rules

- Prevent cycles.
- Track cannot be parent of itself.
- Master cannot be child.
- Bus/return grouping allowed only if safe.
- Deleting folder/group should use safe default:
  - ungroup children when deleting folder/group unless explicit delete children action exists.

### Acceptance

- Create folder/group track.
- Group selected tracks.
- Collapse hides children.
- Expand shows children.
- Save/load hierarchy.
- No routing/audio break.

---

## Part G — Timeline Selection Model Cleanup

## Goal

One timeline selection model that supports:

- Clips
- Tracks
- Markers
- Regions
- Automation points
- MIDI notes separately in MIDI editor

### Selection Types

```rust
pub enum TimelineSelectionItem {
    Track(String),
    Clip(String),
    Marker(String),
    Region(String),
    AutomationPoint {
        lane_id: String,
        point_id: String,
    },
}
```

### Rules

- Selected persistent items stored by ID.
- Transient marquee rect is not persisted.
- Only one active gesture at a time.
- Selection-only changes do not dirty project.
- Delete command routes by focused context and selected item priority.

### Selection Priority

1. Active modal/text input wins.
2. MIDI editor selection if MIDI editor focused.
3. Automation points if automation lane focused.
4. Clips/regions/markers based on hit test.
5. Tracks if track header focused.

### Acceptance

- Select marker/region/clip/automation point works.
- Delete deletes correct selected target.
- No marquee stuck overlay.
- No nested update panic.

---

## Part H — Edit Command Routing

## Goal

Make timeline edit commands safe and central.

### Commands

- SelectAll
- Copy
- Paste
- Cut
- Delete
- Duplicate
- Rename
- Split
- Mute
- Group
- Ungroup

### Command Outcome

Use `CommandOutcome`:

```rust
pub struct CommandOutcome {
    pub changed: bool,
    pub project_dirty: bool,
    pub status: Option<String>,
}
```

Child components return outcome. `StudioLayout` applies project dirty after child update finishes.

### Do Not

- Call `StudioLayout.update` from `Timeline.update`.
- Mark dirty directly inside child by updating parent.
- Call `LoadProject` for every edit.

### Acceptance

- Ctrl/Cmd+A/C/V/X/Delete safe in timeline.
- Edit command result applies dirty correctly.
- No GPUI double lease panic.

---

## Part I — Timeline Layer Contract / Z-order

## Goal

Fix layer ordering and keep timeline visuals stable.

### Render Layers

0. Timeline background
1. Grid background
2. Arrangement region lane background
3. Track lane backgrounds
4. Clips
5. Automation lanes/curves/points
6. Markers/region labels
7. Selection/marquee overlay
8. Playhead
9. Hover handles/tooltips
10. Floating toolbar / overlays

### Rules

- Grid never draws above clips.
- Playhead always above clips/automation.
- Marker flags visible above ruler.
- Region lane does not cover controls.
- Track headers stay above timeline body if needed.
- Scrollbar and floating tools always top.

### Acceptance

- No grid/timeline region layer bug.
- Playhead visible.
- Marker/regions visible.
- Marquee clears after mouse up.

---

## Part J — Inspector Integration

Inspector must show details for selected:

- Track
- Clip
- Marker
- Region
- Automation Lane/Point
- Group/Folder Track

### Marker Inspector

- Name
- Beat/bar
- Kind
- Color

### Region Inspector

- Name
- Start
- End
- Length
- Color

### Automation Point Inspector

- Target
- Beat
- Value
- Curve

### Group Track Inspector

- Name
- Color
- Collapsed
- Children count
- Output/routing if group supports it

### Rules

- Inspector edits use command/outcome flow.
- Inspector value changes mark dirty only when actual persistent value changes.
- Use shared SettingsRow/ComboBox/TextInput components.
- Use global theme tokens.

### Acceptance

- Selecting marker/region/automation point changes inspector.
- Editing inspector updates timeline.
- No nested entity update.

---

## Part K — Save/Load / Migration

## Goal

Stage 1 data persists safely.

### Save

Persist:

- Markers
- Arrangement regions
- Automation lanes/points
- Track parent/group state
- Collapsed state
- Lane visibility/heights
- Colors

### Load

- Old projects without fields load with defaults.
- Invalid IDs are ignored or repaired.
- Orphan child tracks become root tracks.
- Automation target missing track/plugin becomes disabled/missing lane.
- Marker/region invalid beat clamped.

### Migration

- Add default empty arrays.
- Add version bump if project format has version.
- Document migration in audit or project migration notes.

### Acceptance

- Save/load roundtrip works.
- Old project still opens.
- Missing automation targets do not crash.

---

## Part L — Stage 1 UI Polish

### Toolbar

- Add marker button/menu
- Add region button/menu
- Automation toggle
- Group selected menu
- Snap/grid controls remain

### Context Menus

Timeline empty:

- Add Marker
- Add Region
- Paste

Clip:

- Cut
- Copy
- Delete
- Duplicate
- Split
- Mute

Marker:

- Rename
- Delete
- Color

Region:

- Rename
- Delete
- Color

Track header:

- Add Automation Lane
- Group Selected Tracks
- Create Folder
- Collapse/Expand

Automation lane:

- Add Point
- Clear Lane
- Hide Lane

### Acceptance

- Common actions discoverable.
- Disabled actions visibly disabled.
- No fake success.

---

## Part M — Debug Flags

Add or confirm:

```txt
FUTUREBOARD_TIMELINE_DEBUG=1
FUTUREBOARD_TIMELINE_SELECTION_DEBUG=1
FUTUREBOARD_TIMELINE_MARKER_DEBUG=1
FUTUREBOARD_TIMELINE_REGION_DEBUG=1
FUTUREBOARD_AUTOMATION_DEBUG=1
FUTUREBOARD_TRACK_GROUP_DEBUG=1
FUTUREBOARD_EDIT_COMMAND_DEBUG=1
```

Log:

- Command dispatch
- Selection target
- Marker add/move/delete
- Region add/move/resize/delete
- Automation point edit
- Group/ungroup
- Dirty outcome
- Save/load migration warnings

Do not log per-frame unless debug throttled.

---

## Part N — Manual Test Checklist

### Markers

1. Add marker at playhead.
2. Move marker.
3. Rename marker.
4. Delete marker.
5. Save/load project.
6. Marker remains.

### Regions

1. Add region from beat 1 to beat 5.
2. Rename region `Verse`.
3. Resize region.
4. Move region.
5. Delete region.
6. Save/load project.

### Automation

1. Show Track Volume automation.
2. Add points.
3. Move points.
4. Delete points.
5. Play/seek and confirm fader sync if automation read exists.
6. Save/load.

### Groups

1. Create 3 tracks.
2. Group selected tracks.
3. Collapse group.
4. Expand group.
5. Save/load.
6. Ungroup.

### Selection

1. Select clip.
2. Select marker.
3. Select region.
4. Select automation point.
5. Delete selected target.
6. No wrong target deleted.

### Shortcuts

1. Ctrl/Cmd+A in timeline.
2. Ctrl/Cmd+C/V/X clips.
3. Delete clips/markers/regions/automation points.
4. Text input shortcuts still work.

### Layering

1. Playhead above clips.
2. Grid behind clips.
3. Marker visible.
4. Region lane visible.
5. Marquee not stuck.

### Build

```bash
cargo check -p sphere_ui_components
cargo check --manifest-path apps/native/Cargo.toml
cargo clippy -p sphere_ui_components -- -D warnings
```

---

## Stage 1 Phases

## Phase 1A — Audit and Data Model

- Audit existing timeline.
- Add marker/region/group/automation model defaults.
- Add save/load migration scaffold.

## Phase 1B — Marker System

- Marker lane render.
- Add/move/rename/delete.
- Inspector.
- Save/load.

## Phase 1C — Arrangement Regions

- Region lane render.
- Add/move/resize/rename/delete.
- Inspector.
- Save/load.

## Phase 1D — Automation Lanes UI

- Track volume lane.
- Add/move/delete points.
- Lane show/hide.
- Inspector.
- Save/load.

## Phase 1E — Track Group / Folder

- Folder/group track kind.
- `parent_id` / collapse.
- Render hierarchy.
- Group/ungroup commands.
- Save/load.

## Phase 1F — Selection and Shortcuts

- Unified selection item.
- `CommandOutcome`.
- Ctrl/Cmd+A/C/V/X/Delete routing.
- No nested update.

## Phase 1G — Layer Contract Polish

- Z-order.
- Playhead/grid/regions/markers.
- Marquee cleanup.

## Phase 1H — QA and Stabilization

- Save/load roundtrip.
- Old project migration.
- No panic tests.
- Manual checklist.

---

## Recommended First Patch

Start with **Phase 1A only**.

Do not implement all Stage 1 in one patch.

### Phase 1A Deliverables

- Audit doc.
- Data model scaffold for markers/regions/group state.
- Project defaults/migration.
- No visible UI required except maybe disabled menu entries.
- Build green.

Then:

1. Phase 1B — Marker system
2. Phase 1C — Arrangement regions
3. Phase 1D — Automation lanes
4. Phase 1E — Track group/folder
5. Phase 1F — Selection/shortcut cleanup
6. Phase 1G — Layer polish
7. Phase 1H — QA and stabilization

---

## Final Stage 1 Acceptance

Stage 1 is complete when:

- Markers work and persist.
- Arrangement regions work and persist.
- Track volume automation lane can be edited and persists.
- Track groups/folders can collapse/expand and persist.
- Selection model handles clips/markers/regions/automation points safely.
- Ctrl/Cmd+A/C/V/X/Delete work without GPUI nested update panic.
- Inspector edits selected timeline items.
- Grid/playhead/regions/markers/automation layers render in correct order.
- Old projects load safely.
- Build/check passes.

---

## One-Line Summary

Timeline Stage 1 is:

```txt
Markers + Arrangement Regions + Automation Lanes + Track Group/Folder + Selection/Edit Command cleanup + Inspector + Save/Load.
```

Do not do Stage 2 yet:

```txt
tempo engine, warping, comping, ripple edit, full WGPU rewrite.
```

End of Stage 1.
