# Futureboard Timeline Stage 1-2 Plan

Planning status: plan only. No implementation code is implied.

Source roadmaps:

- `tasks/timeline/roadmap.md`
- `tasks/timeline/roadmap-stage2.md`

## Purpose

Build the Futureboard Timeline in two controlled stages:

1. Stage 1 makes the timeline a real DAW arrangement surface.
2. Stage 2 turns that surface into a runtime-capable timeline engine.

The plan is intentionally phased. Each phase should be implemented as the
smallest buildable patch, validated, reviewed, and then followed by the next
phase.

## Non-Negotiable Rules

- Do not rewrite the whole Timeline or DAW in one patch.
- Do not break existing clip editing.
- Do not break MIDI editor behavior.
- Do not break audio/plugin runtime.
- Do not call `LoadProject` on every small edit.
- Do not mutate realtime audio state directly from UI.
- Do not allocate, lock, or log in the audio callback.
- Do not create nested GPUI entity updates.
- Do not make WGPU required if CPU fallback exists.
- Do not fake advanced features visually only.
- Do not implement destructive ripple/arranger edits without undo and preview.
- Use project theme tokens only.
- Keep UI compact, dark, DAW-native, and performance-aware.
- Persistent edits must mark dirty through `StudioLayout` only.
- Timeline/MIDI/child components must return command outcomes instead of synchronously updating parent state.

## Stage Boundary

Stage 1 is composition and arrangement:

- Markers
- Arrangement regions
- Automation lanes UI and persistence
- Track groups/folder tracks
- Lane expand/collapse
- Clip grouping foundations
- Unified timeline selection
- Safe edit command routing
- Inspector integration
- Project save/load migration
- Timeline layer/z-order polish
- Debug flags and QA

Stage 2 is the deep engine and advanced editing layer:

- Canonical time model
- Tempo map
- Time signature map
- Beat/time/sample conversion
- Unified grid/snap engine
- Runtime timeline snapshots
- Sample-accurate automation runtime
- Advanced clip tools
- Audio warp/time-stretch scaffold
- Loop/punch/range system
- Comping/take lanes
- Ripple editing
- Arranger operations
- Multi-track edit groups
- WGPU timeline renderer with CPU fallback
- Large-session virtualization
- Export/bounce range integration
- Full undo/redo command coverage

Stage 2 should not begin until Stage 1 acceptance is met, except for purely
diagnostic audit work that does not change behavior.

## Stage 1 Implementation Plan

### Phase 1A - Audit and Data Model

Create `tasks/native/timeline-stage-1-audit.md`, audit current ownership and
command flow, then add or confirm persistent marker, region, automation,
group/folder, and migration defaults. This phase should have no required visible
UI.

Deliverables:

- Stage 1 audit doc.
- Smallest safe implementation slices listed.
- Persistent data model scaffold.
- Old-project defaults/migration scaffold.
- Build green.

### Phase 1B - Marker System

Implement marker creation, selection, movement, rename, delete, rendering,
inspector editing, command routing, and save/load.

Deliverables:

- Marker lane/ruler rendering.
- Add/move/rename/delete/select commands.
- Marker inspector.
- Marker persistence.
- Dirty behavior correct.

### Phase 1C - Arrangement Regions

Implement visual arrangement regions/sections and basic editing. Keep arranger
reorder/copy/delete-section operations deferred to Stage 2.

Deliverables:

- Region lane rendering.
- Add/move/resize/rename/delete/select commands.
- Region inspector.
- Region persistence.
- No ripple edit.

### Phase 1D - Automation Lanes UI

Make project-layer automation lanes usable for visible targets. Runtime
sample-accurate playback remains Stage 2 unless already safely available.

Deliverables:

- Show/hide automation lanes.
- Track volume/pan and master volume targets.
- Add/move/delete/select points.
- Inspector and save/load.
- Safe fader/effective-value sync only where existing model supports it.

### Phase 1E - Track Group / Folder Tracks

Add organizational group/folder tracks with hierarchy, indentation, collapse,
expand, and persistence. Do not fake bus routing.

Deliverables:

- Folder/group track kinds.
- `parent_id` and collapse state.
- Group/ungroup/move-in/move-out commands.
- Hierarchical rendering.
- Save/load hierarchy.
- No audio routing break.

### Phase 1F - Selection and Shortcuts

Unify timeline selection for clips, tracks, markers, regions, and automation
points while keeping MIDI editor selection separate. Route edit commands through
safe command outcomes.

Deliverables:

- Unified timeline selection item.
- Safe Delete/Cut/Copy/Paste/SelectAll routing.
- `CommandOutcome` flow or equivalent safe result flow.
- No nested `StudioLayout` update.
- No GPUI double-lease panic.

### Phase 1G - Layer Contract Polish

Stabilize timeline rendering order so grid, clips, automation, markers, regions,
selection, playhead, handles, and overlays never fight each other.

Deliverables:

- Documented render layer order.
- Grid behind clips.
- Playhead above clips/automation.
- Markers/regions visible.
- Marquee cleanup.

### Phase 1H - QA and Stabilization

Validate save/load, migration, selection, shortcuts, rendering, and debug tools.

Deliverables:

- Stage 1 manual QA complete.
- Old projects load safely.
- Debug flags available and throttled.
- `cargo check -p sphere_ui_components`.
- `cargo check --manifest-path apps/native/Cargo.toml`.
- `cargo clippy -p sphere_ui_components -- -D warnings`.

## Stage 1 Gate

Stage 1 is complete when:

- Markers work and persist.
- Arrangement regions work and persist.
- Track volume automation lane can be edited and persists.
- Track groups/folders can collapse/expand and persist.
- Selection handles clips/markers/regions/automation points safely.
- Ctrl/Cmd+A/C/V/X/Delete work without nested GPUI update panic.
- Inspector edits selected timeline items.
- Grid/playhead/regions/markers/automation layers render in correct order.
- Old projects load safely.
- Build/check passes.

## Stage 2 Implementation Plan

### Phase 2A - Audit and Time Model

Create `tasks/native/timeline-stage-2-audit.md`, audit time/transport/runtime
dependencies, and introduce canonical time types and conversion helper skeleton.
No UI rewrite.

Deliverables:

- Stage 2 audit doc.
- `Beat`, `Tick`, `Seconds`, and `SampleFrame` types or equivalent.
- Conversion helper skeleton.
- Initial tests or placeholders.
- Build green.

### Phase 2B - Tempo Map Core

Add tempo points, tempo map, runtime tempo segments, conversion APIs, persistence,
and static tempo compatibility.

Deliverables:

- `tempo_at_beat`.
- `seconds_at_beat`.
- `beat_at_seconds`.
- `sample_at_beat`.
- `beat_at_sample`.
- Runtime tempo segment builder.
- Save/load and tests.

### Phase 2C - Time Signature Map

Add time signature points and conversion between beat and musical bar/beat/tick
positions.

Deliverables:

- Time signature map model.
- Musical position conversion.
- Ruler/grid accents update.
- Transport display support.
- Save/load and tests.

### Phase 2D - Unified Grid/Snap Engine

Create shared grid and snap logic for timeline, MIDI editor, automation lanes,
markers, regions, clip drawing, arrangement editing, and render snapshots.

Deliverables:

- Grid resolution model.
- Snap settings model.
- `snap_beat` helper.
- Grid line generator.
- Nearest target helper.
- MIDI/timeline compatibility.

### Phase 2E - Runtime Timeline Snapshot

Build immutable runtime timeline snapshots consumed by the audio engine.

Deliverables:

- Runtime clip sample positions.
- Runtime MIDI event sample positions.
- Runtime loop region conversion.
- Snapshot handoff to audio engine.
- No UI state read by audio runtime.

### Phase 2F - Sample-Accurate Automation Runtime

Move automation lanes from UI/project data into runtime-safe evaluation.

Deliverables:

- Runtime automation lanes.
- Runtime points converted to samples.
- Constant/ramp/per-sample block model with no callback allocation.
- Track volume/pan runtime evaluation.
- Plugin parameter scaffold if host supports it.

### Phase 2G - Advanced Clip Tools

Add advanced clip edit foundations after time conversion and runtime snapshots
are stable.

Deliverables:

- Slip tool for audio clips.
- Fade model and basic handles.
- Split improvements.
- Stretch scaffold with honest quality labeling.
- Existing split/mute/duplicate remain stable.

### Phase 2H - Warp Scaffold

Add non-destructive warp state and marker model. Keep heavy time-stretch DSP out
of the audio callback.

Deliverables:

- Warp marker model.
- Warp state persistence.
- Warp UI scaffold.
- Playback-rate preview only if safe.
- Offline cache plan.

### Phase 2I - Loop/Punch/Range System

Unify loop, punch, and time selection so transport, editing, export, and recording
can use one range model.

Deliverables:

- Loop region model.
- Punch region scaffold.
- Time selection model.
- Transport loop integration.
- Export/edit range compatibility.

### Phase 2J - Comping / Take Lanes

Introduce non-destructive take lane and comp segment models with basic UI.

Deliverables:

- Take lane model.
- Show/hide take lanes.
- Comp segment model.
- Basic promote/split/clear command scaffolds.
- Recording integration deferred until safe.

### Phase 2K - Ripple Edit

Add explicit ripple mode and safe first ripple operations. Ripple must never
surprise users.

Deliverables:

- Ripple mode toggle.
- Delete range with ripple for clips first.
- Preview/status of affected items.
- Undo support.
- Marker/automation movement staged by policy.

### Phase 2L - Arranger Operations

Turn Stage 1 regions into operational arrangement sections.

Deliverables:

- Duplicate region contents.
- Move region contents.
- Delete section through ripple where appropriate.
- Undo support.
- Preview or clear behavior for destructive edits.

### Phase 2M - Multi-track Edit Groups

Add edit groups that link editing behavior separately from folder/group track
organization.

Deliverables:

- Edit group model.
- Enabled/disabled group behavior.
- UI indicators.
- Grouped clip operation scaffold.

### Phase 2N - WGPU Timeline Renderer

Move dense timeline visuals to WGPU through render snapshots while keeping GPUI
as shell and CPU fallback as a first-class path.

Deliverables:

- Timeline render snapshot.
- WGPU grid/clips/playhead path.
- Theme-token-to-render-theme bridge.
- CPU fallback.
- Device/settings integration.

### Phase 2O - Large Session Performance

Make timeline cost scale with visible content, not full project size.

Deliverables:

- Track visibility tree.
- Clip/marker/region interval indexes.
- Automation range queries.
- Waveform chunk cache and LOD.
- Render snapshot cache.
- Large-session manual perf pass.

### Phase 2P - Export/Bounce Range Integration

Use timeline ranges as the source for export, bounce, freeze, comp rendering, and
warp cache rendering.

Deliverables:

- Export range model.
- Full song range.
- Time selection range.
- Arrangement region range.
- Loop region range.
- No duplicate range systems.

### Phase 2Q - QA / Stabilization

Stabilize Stage 2 with unit, integration, manual, performance, migration, and
panic checks.

Deliverables:

- Tempo/time signature/snap/automation/ripple/undo unit tests.
- Runtime snapshot integration tests.
- Save/load integration tests.
- Manual Stage 2 QA complete.
- Old project migration validated.
- Build/check passes.

## Stage 2 Gate

Stage 2 is complete when:

- Tempo map works and persists.
- Time signature map works and persists.
- Grid/ruler reflects tempo and time signature maps.
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

## Recommended Order

1. Phase 1A - Audit and Data Model
2. Phase 1B - Marker System
3. Phase 1C - Arrangement Regions
4. Phase 1D - Automation Lanes UI
5. Phase 1E - Track Group / Folder
6. Phase 1F - Selection and Shortcuts
7. Phase 1G - Layer Contract Polish
8. Phase 1H - QA and Stabilization
9. Phase 2A - Audit and Time Model
10. Phase 2B - Tempo Map Core
11. Phase 2C - Time Signature Map
12. Phase 2D - Unified Grid/Snap Engine
13. Phase 2E - Runtime Timeline Snapshot
14. Phase 2F - Sample-Accurate Automation Runtime
15. Phase 2G - Advanced Clip Tools
16. Phase 2I - Loop/Punch/Range System
17. Phase 2J - Comping / Take Lanes
18. Phase 2K - Ripple Edit
19. Phase 2L - Arranger Operations
20. Phase 2M - Multi-track Edit Groups
21. Phase 2N - WGPU Timeline Renderer
22. Phase 2O - Large Session Performance
23. Phase 2P - Export/Bounce Range Integration
24. Phase 2Q - QA / Stabilization

Phase 2H, Warp Scaffold, can begin after Phase 2G if audio clip stretch state is
stable. It should not block loop/range work unless the implementation couples
them.

## First Patch Rule

Start with Phase 1A only. After Stage 1 is accepted, start Stage 2 with Phase 2A
only. Stop after each first audit/model phase for review before continuing.
