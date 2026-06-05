# Futureboard Timeline Stage 1-2 Checklist

Planning status: checklist only. No implementation code is implied.

Source roadmaps:

- `tasks/timeline/roadmap.md`
- `tasks/timeline/roadmap-stage2.md`

## Global Safety

- [ ] Do not rewrite the whole Timeline.
- [ ] Do not rewrite the whole DAW.
- [ ] Do not break current clip editing.
- [ ] Do not break MIDI editor behavior.
- [ ] Do not break audio/plugin runtime.
- [ ] Do not call `LoadProject` on every small edit.
- [ ] Do not mutate realtime audio state directly from UI.
- [ ] Do not allocate in the audio callback.
- [ ] Do not lock in the audio callback.
- [ ] Do not log in the audio callback.
- [ ] Do not evaluate tempo/automation from UI state inside audio callback.
- [ ] Do not create nested GPUI entity updates.
- [ ] Do not make WGPU required.
- [ ] Do not fake unfinished advanced features.
- [ ] Do not implement destructive ripple/arranger edits without undo.
- [ ] Do not implement destructive ripple/arranger edits without preview or clear status.
- [ ] Use project theme tokens only.
- [ ] Keep UI compact, dark, and DAW-native.
- [ ] Mark persistent edits dirty through `StudioLayout` only.
- [ ] Return command outcomes from child components instead of updating parent state synchronously.

## Stage 1 - Composition / Arrangement

### Phase 1A - Audit And Data Model

- [ ] Create `tasks/native/timeline-stage-1-audit.md`.
- [ ] Audit timeline state ownership.
- [ ] Audit track model.
- [ ] Audit clip model.
- [ ] Audit selection model.
- [ ] Audit edit command flow.
- [ ] Audit automation model.
- [ ] Audit existing marker/region code.
- [ ] Audit files likely to change.
- [ ] Audit nested GPUI update risks.
- [ ] Audit save/load gaps.
- [ ] Identify smallest safe Stage 1 slices.
- [ ] Add or confirm `TimelineMarker`.
- [ ] Add or confirm `MarkerKind`.
- [ ] Add or confirm `ArrangementRegion`.
- [ ] Add or confirm `AutomationLane`.
- [ ] Add or confirm `AutomationTarget`.
- [ ] Add or confirm `AutomationPoint`.
- [ ] Add or confirm `AutomationCurveKind`.
- [ ] Add or confirm `TrackKind::Group`.
- [ ] Add or confirm `TrackKind::Folder`.
- [ ] Add or confirm track `parent_id`.
- [ ] Add or confirm group/folder collapse state.
- [ ] Add project defaults for new fields.
- [ ] Add migration scaffold.
- [ ] Confirm old projects load with defaults.
- [ ] Confirm save/load roundtrip preserves default Stage 1 state.
- [ ] Keep visible UI optional/disabled in this phase.
- [ ] Run `cargo check -p sphere_ui_components`.
- [ ] Run `cargo check --manifest-path apps/native/Cargo.toml`.

### Phase 1B - Marker System

- [ ] Render marker lane or ruler marker area.
- [ ] Render marker flags/triangles.
- [ ] Render marker labels when zoom allows.
- [ ] Render selected marker accent outline.
- [ ] Render marker tooltip with name, bar/beat, and kind.
- [ ] Add marker at playhead.
- [ ] Add marker at mouse position.
- [ ] Rename marker.
- [ ] Delete marker.
- [ ] Move marker.
- [ ] Select marker.
- [ ] Navigate to next marker.
- [ ] Navigate to previous marker.
- [ ] Optionally route `M` to add marker when timeline is focused.
- [ ] Route `Delete` to selected marker when marker selection is active.
- [ ] Add marker inspector name field.
- [ ] Add marker inspector position field.
- [ ] Add marker inspector color field.
- [ ] Add marker inspector kind field.
- [ ] Clamp marker beat to `>= 0`.
- [ ] Snap marker move when snap is enabled.
- [ ] Allow duplicate marker positions.
- [ ] Avoid duplicate marker names where easy.
- [ ] Dirty project on marker edits.
- [ ] Do not dirty project on marker selection only.
- [ ] Save markers.
- [ ] Load markers.
- [ ] Roundtrip markers through save/load.

### Phase 1C - Arrangement Regions

- [ ] Render arrangement region lane.
- [ ] Render region blocks with name.
- [ ] Render region color.
- [ ] Drag region body to move.
- [ ] Drag region edges to resize.
- [ ] Double-click region to rename.
- [ ] Add region context menu.
- [ ] Add region from selection.
- [ ] Add region at playhead.
- [ ] Rename region.
- [ ] Delete region.
- [ ] Move region.
- [ ] Resize region.
- [ ] Select region.
- [ ] Add region inspector name field.
- [ ] Add region inspector start field.
- [ ] Add region inspector end field.
- [ ] Add region inspector length display.
- [ ] Add region inspector color field.
- [ ] Enforce `end_beat > start_beat`.
- [ ] Snap region start/end when snap is enabled.
- [ ] Allow overlapping regions in Stage 1.
- [ ] Defer arranger reorder/copy/delete-section operations to Stage 2.
- [ ] Dirty project on region edits.
- [ ] Do not dirty project on region selection only.
- [ ] Save regions.
- [ ] Load regions.
- [ ] Roundtrip regions through save/load.

### Phase 1D - Automation Lanes UI

- [ ] Expand automation lanes under track lanes.
- [ ] Render automation lane header.
- [ ] Render automation target name.
- [ ] Render automation read toggle.
- [ ] Render lane height behavior.
- [ ] Render close/hide control.
- [ ] Render automation line/curve.
- [ ] Render automation points.
- [ ] Render hover value.
- [ ] Render selected point state.
- [ ] Support Track Volume target.
- [ ] Support Track Pan target.
- [ ] Support Master Volume target.
- [ ] Scaffold plugin parameter target if plugin parameters exist.
- [ ] Scaffold Tempo target as data-only.
- [ ] Show automation lane.
- [ ] Hide automation lane.
- [ ] Add automation point.
- [ ] Move automation point.
- [ ] Delete automation point.
- [ ] Select automation point.
- [ ] Clear automation lane.
- [ ] Set automation target.
- [ ] Click line to add point.
- [ ] Drag point to move.
- [ ] Support additive point selection.
- [ ] Apply snap horizontally only.
- [ ] Clamp vertical values by target range.
- [ ] Use volume range `-60 dB` to `+12 dB` or supported `-inf`.
- [ ] Use pan range `-100..+100`.
- [ ] Use plugin parameter range `0.0..1.0`.
- [ ] Use tempo range `20..300 BPM` for data scaffold.
- [ ] Dirty project on point edits.
- [ ] Do not dirty project on point selection only.
- [ ] Safely sync fader/effective value only if existing model supports it.
- [ ] Do not implement sample-accurate automation engine in Stage 1.
- [ ] Save automation lanes and points.
- [ ] Load automation lanes and points.
- [ ] Roundtrip automation through save/load.

### Phase 1E - Track Group / Folder Tracks

- [ ] Create folder track.
- [ ] Create group track.
- [ ] Assign selected tracks to folder/group.
- [ ] Collapse group/folder.
- [ ] Expand group/folder.
- [ ] Render folder/group track header.
- [ ] Render disclosure arrow.
- [ ] Render children indented.
- [ ] Render group color strip.
- [ ] Hide children when collapsed.
- [ ] Preserve child track order.
- [ ] Keep folder tracks organizational only.
- [ ] Keep group tracks organizational unless real routing exists.
- [ ] Group selected tracks.
- [ ] Ungroup tracks.
- [ ] Move track into group.
- [ ] Move track out of group.
- [ ] Prevent parent cycles.
- [ ] Prevent self-parenting.
- [ ] Prevent Master from becoming a child.
- [ ] Allow bus/return grouping only if safe.
- [ ] Delete folder/group by ungrouping children unless explicit delete-children exists.
- [ ] Save hierarchy.
- [ ] Save collapse state.
- [ ] Load hierarchy safely.
- [ ] Roundtrip hierarchy through save/load.
- [ ] Confirm no routing/audio break.

### Phase 1F - Selection And Shortcuts

- [ ] Define or confirm selection item for tracks.
- [ ] Define or confirm selection item for clips.
- [ ] Define or confirm selection item for markers.
- [ ] Define or confirm selection item for regions.
- [ ] Define or confirm selection item for automation points.
- [ ] Keep MIDI editor note selection separate.
- [ ] Store selected persistent items by ID.
- [ ] Keep marquee rect transient.
- [ ] Allow only one active gesture at a time.
- [ ] Do not dirty project for selection-only changes.
- [ ] Route delete by focus and selected item priority.
- [ ] Let modal/text input shortcuts win.
- [ ] Let MIDI editor shortcuts win when MIDI editor is focused.
- [ ] Route automation point edits when automation lane is focused.
- [ ] Route clip/region/marker selection from hit tests.
- [ ] Route track selection from track header focus.
- [ ] Add or confirm `CommandOutcome`.
- [ ] Apply dirty state in `StudioLayout` after child update returns.
- [ ] Avoid `StudioLayout.update` inside `Timeline.update`.
- [ ] Avoid child-to-parent synchronous dirty update.
- [ ] Support Select All.
- [ ] Support Copy.
- [ ] Support Paste.
- [ ] Support Cut.
- [ ] Support Delete.
- [ ] Support Duplicate.
- [ ] Support Rename.
- [ ] Support Split where already supported.
- [ ] Support Mute where already supported.
- [ ] Support Group/Ungroup routing.
- [ ] Confirm Ctrl/Cmd+A/C/V/X/Delete are safe.
- [ ] Confirm no GPUI double-lease panic.

### Phase 1G - Layer Contract Polish

- [ ] Document timeline render layer order.
- [ ] Render background below all content.
- [ ] Render grid behind clips.
- [ ] Render arrangement region lane background below content labels.
- [ ] Render track lane backgrounds below clips.
- [ ] Render clips above grid.
- [ ] Render automation curves/points above lanes.
- [ ] Render markers and region labels above relevant lanes.
- [ ] Render selection/marquee above content.
- [ ] Render playhead above clips/automation.
- [ ] Render hover handles/tooltips above playhead where appropriate.
- [ ] Render floating toolbar/overlays on top.
- [ ] Ensure region lane does not cover controls.
- [ ] Ensure track headers stay visually stable.
- [ ] Ensure scrollbars remain usable.
- [ ] Ensure marquee clears after mouse up.

### Phase 1H - QA And Stabilization

- [ ] Save/load markers.
- [ ] Save/load regions.
- [ ] Save/load automation lanes/points.
- [ ] Save/load group/folder hierarchy.
- [ ] Load old project without Stage 1 fields.
- [ ] Ignore or repair invalid IDs.
- [ ] Convert orphan child tracks to root tracks.
- [ ] Mark missing automation targets disabled/missing without crash.
- [ ] Clamp invalid marker/region beats.
- [ ] Add or confirm Stage 1 debug flags.
- [ ] Throttle debug logs.
- [ ] Complete manual marker QA.
- [ ] Complete manual region QA.
- [ ] Complete manual automation QA.
- [ ] Complete manual group QA.
- [ ] Complete manual selection QA.
- [ ] Complete manual shortcut QA.
- [ ] Complete manual layering QA.
- [ ] Run `cargo check -p sphere_ui_components`.
- [ ] Run `cargo check --manifest-path apps/native/Cargo.toml`.
- [ ] Run `cargo clippy -p sphere_ui_components -- -D warnings`.

### Stage 1 Acceptance

- [ ] Markers work and persist.
- [ ] Arrangement regions work and persist.
- [ ] Track volume automation lane can be edited and persists.
- [ ] Track groups/folders can collapse/expand and persist.
- [ ] Selection model handles clips/markers/regions/automation points safely.
- [ ] Ctrl/Cmd+A/C/V/X/Delete work without GPUI nested update panic.
- [ ] Inspector edits selected timeline items.
- [ ] Grid/playhead/regions/markers/automation layers render in correct order.
- [ ] Old projects load safely.
- [ ] Build/check passes.

## Stage 2 - Engine / Advanced Editing

### Phase 2A - Audit And Time Model

- [ ] Create `tasks/native/timeline-stage-2-audit.md`.
- [ ] Audit beat/time conversion code.
- [ ] Audit transport clock ownership.
- [ ] Audit ruler/grid implementation.
- [ ] Audit clip position representation.
- [ ] Audit automation evaluation.
- [ ] Audit audio runtime snapshot model.
- [ ] Audit MIDI runtime scheduling.
- [ ] Audit project save/load format.
- [ ] Audit WGPU/CPU render split.
- [ ] Audit undo/command infrastructure.
- [ ] Audit nested update risks.
- [ ] List files likely to change.
- [ ] List gaps before Stage 2 can begin safely.
- [ ] Identify first safe Stage 2 patch.
- [ ] Add or confirm `Beat`.
- [ ] Add or confirm `Tick`.
- [ ] Add or confirm `Seconds`.
- [ ] Add or confirm `SampleFrame`.
- [ ] Add or confirm `MusicalPosition`.
- [ ] Add or confirm `TimelinePosition`.
- [ ] Add conversion helper skeleton.
- [ ] Avoid ambiguous `f32` for long-session positions in new code.
- [ ] Prefer integer sample frames in runtime code.
- [ ] Add initial conversion tests or placeholders.
- [ ] Keep this phase free of UI rewrite.
- [ ] Run relevant cargo check.

### Phase 2B - Tempo Map Core

- [ ] Add `TempoPoint`.
- [ ] Add `TempoCurveKind`.
- [ ] Add `TempoMap`.
- [ ] Add runtime tempo segments.
- [ ] Support Hold tempo points first.
- [ ] Scaffold Linear tempo curve if needed.
- [ ] Implement `tempo_at_beat`.
- [ ] Implement `seconds_at_beat`.
- [ ] Implement `beat_at_seconds`.
- [ ] Implement `sample_at_beat`.
- [ ] Implement `beat_at_sample`.
- [ ] Implement runtime segment builder.
- [ ] Add tempo point.
- [ ] Move tempo point.
- [ ] Delete tempo point.
- [ ] Edit BPM.
- [ ] Snap tempo point to grid.
- [ ] Render tempo lane or global marker scaffold.
- [ ] Add tempo point inspector.
- [ ] Keep static tempo compatibility.
- [ ] Persist tempo points.
- [ ] Load tempo points.
- [ ] Test beat-to-seconds conversion.
- [ ] Test seconds-to-beat conversion.
- [ ] Test beat-to-sample conversion.
- [ ] Test sample-to-beat conversion.
- [ ] Test multiple tempo points.
- [ ] Confirm old projects do not break.

### Phase 2C - Time Signature Map

- [ ] Add `TimeSignaturePoint`.
- [ ] Add `TimeSignatureMap`.
- [ ] Add `signature_at_beat`.
- [ ] Add `musical_position_at_beat`.
- [ ] Add `beat_at_musical_position`.
- [ ] Add `next_bar_beat`.
- [ ] Add `bar_start_before_or_at`.
- [ ] Render time signature markers.
- [ ] Add time signature inspector.
- [ ] Update ruler bar labels.
- [ ] Update grid accent lines.
- [ ] Update transport signature display.
- [ ] Persist time signature points.
- [ ] Load time signature points.
- [ ] Test beat-to-bar/beat/tick conversion.
- [ ] Test bar/beat/tick-to-beat conversion.
- [ ] Test signature changes.
- [ ] Do not add polymeter per track.
- [ ] Do not add complex additive meters.

### Phase 2D - Unified Grid / Snap Engine

- [ ] Add `GridResolution`.
- [ ] Add `SnapSettings`.
- [ ] Add `snap_beat`.
- [ ] Add `grid_lines_for_view`.
- [ ] Add `nearest_snap_target`.
- [ ] Support bar snap.
- [ ] Support beat snap.
- [ ] Support division snap.
- [ ] Support triplet snap.
- [ ] Support dotted snap.
- [ ] Support sample snap if needed.
- [ ] Support snap off.
- [ ] Support snap to markers.
- [ ] Support snap to regions.
- [ ] Support snap to clip edges.
- [ ] Support snap to playhead.
- [ ] Share snap logic with timeline.
- [ ] Share snap logic with MIDI editor where possible.
- [ ] Share snap logic with automation lanes.
- [ ] Share snap logic with marker/region editing.
- [ ] Keep grid lines stable under zoom.
- [ ] Add snap unit tests.

### Phase 2E - Runtime Timeline Snapshot

- [ ] Add runtime timeline snapshot model.
- [ ] Add runtime track timeline model.
- [ ] Add runtime audio clip model.
- [ ] Add runtime MIDI clip model.
- [ ] Convert audio clip start/end to samples.
- [ ] Convert audio clip source offset to samples.
- [ ] Convert MIDI events to sample positions.
- [ ] Convert loop region to runtime sample range.
- [ ] Convert punch region to runtime sample range if available.
- [ ] Include runtime tempo map.
- [ ] Include runtime time signature map.
- [ ] Include runtime automation lanes.
- [ ] Build snapshot from project edits, not every frame.
- [ ] Swap snapshot at safe audio boundary.
- [ ] Ensure audio runtime does not read UI state.
- [ ] Add runtime snapshot integration tests.

### Phase 2F - Sample-Accurate Automation Runtime

- [ ] Add runtime automation lane model.
- [ ] Add runtime automation point model.
- [ ] Convert automation point beats to samples.
- [ ] Add constant block value.
- [ ] Add linear ramp block value.
- [ ] Add reusable per-sample buffer path if needed.
- [ ] Evaluate track volume automation.
- [ ] Evaluate track pan automation.
- [ ] Scaffold send gain automation.
- [ ] Scaffold master volume automation.
- [ ] Scaffold plugin parameter automation if host supports safe dispatch.
- [ ] Keep tempo handled by tempo map, not generic automation.
- [ ] Rebuild runtime automation snapshot on edit.
- [ ] Avoid callback allocation.
- [ ] Avoid callback locks.
- [ ] Avoid callback logging.
- [ ] Sync effective mixer/inspector values safely.
- [ ] Test hold automation.
- [ ] Test linear automation.
- [ ] Test block crossing point.
- [ ] Test constant block.
- [ ] Test ramp block.

### Phase 2G - Advanced Clip Tools

- [ ] Add slip tool.
- [ ] Slip audio contents without changing clip bounds.
- [ ] Clamp slip to source boundaries.
- [ ] Show shifted waveform preview for slip.
- [ ] Add fade data model.
- [ ] Add fade in.
- [ ] Add fade out.
- [ ] Render basic fade handles.
- [ ] Add fade inspector or status display if needed.
- [ ] Improve split behavior.
- [ ] Keep mute stable.
- [ ] Keep duplicate stable.
- [ ] Add stretch data scaffold.
- [ ] Support playback-rate stretch prototype only if safe.
- [ ] Label high-quality stretch as future until DSP exists.
- [ ] Do not claim fake high-quality time-stretch.

### Phase 2H - Warp Scaffold

- [ ] Add `WarpMarker`.
- [ ] Add `AudioWarpState`.
- [ ] Add `WarpAlgorithm`.
- [ ] Add `WarpCacheStatus`.
- [ ] Persist warp state.
- [ ] Persist warp markers.
- [ ] Add warp marker UI scaffold.
- [ ] Support visual warp marker edit if implemented.
- [ ] Add playback-rate preview only if safe.
- [ ] Add offline cache plan.
- [ ] Do not mutate source audio.
- [ ] Do not block UI while rendering cache.
- [ ] Do not run heavy time-stretch in audio callback.
- [ ] Clearly mark unsupported/preview behavior.

### Phase 2I - Loop / Punch / Range System

- [ ] Add loop region model.
- [ ] Add punch region model.
- [ ] Add time selection model.
- [ ] Render loop range in ruler.
- [ ] Render punch range distinctly.
- [ ] Render time selection overlay.
- [ ] Add range drag handles.
- [ ] Add inspector/status display.
- [ ] Persist loop region.
- [ ] Persist punch region scaffold.
- [ ] Persist time selection if project policy requires it.
- [ ] Transport respects loop region.
- [ ] Loop wraps at sample-accurate boundary.
- [ ] Punch scaffold uses sample-accurate boundaries.
- [ ] Export/edit operations can consume time selection.

### Phase 2J - Comping / Take Lanes

- [ ] Add take lane model.
- [ ] Add comp segment model.
- [ ] Add track take state model.
- [ ] Show take lanes under parent track.
- [ ] Hide take lanes.
- [ ] Render active comp lane.
- [ ] Render source take lanes.
- [ ] Select comp region.
- [ ] Drag comp region.
- [ ] Audition take lane scaffold.
- [ ] Promote take range to comp.
- [ ] Split comp segment.
- [ ] Clear comp.
- [ ] Delete take lane.
- [ ] Keep comping non-destructive.
- [ ] Keep source recordings intact.
- [ ] Persist take lanes.
- [ ] Persist comp segments.
- [ ] Defer flatten/render comp until safe.

### Phase 2K - Ripple Edit

- [ ] Add ripple mode model.
- [ ] Add Off mode.
- [ ] Add RippleTrack mode.
- [ ] Add RippleAll mode.
- [ ] Add visible ripple mode toggle.
- [ ] Add timeline range model.
- [ ] Delete time range.
- [ ] Insert time.
- [ ] Delete clip with ripple.
- [ ] Paste with ripple.
- [ ] Preview affected range where possible.
- [ ] Undo ripple operations.
- [ ] Move markers/regions/automation in RippleAll according to policy.
- [ ] Keep tempo/time signature points fixed unless explicit.
- [ ] Never ripple when mode is off.
- [ ] Test delete range.
- [ ] Test insert time.
- [ ] Test affected clip movement.

### Phase 2L - Arranger Operations

- [ ] Duplicate arrangement section.
- [ ] Move arrangement section.
- [ ] Delete arrangement section.
- [ ] Insert arrangement section.
- [ ] Rename arrangement section.
- [ ] Reorder arrangement sections.
- [ ] Scaffold export section.
- [ ] Scaffold loop section.
- [ ] Apply operations to clips.
- [ ] Apply operations to MIDI clips.
- [ ] Apply operations to automation points.
- [ ] Apply operations to markers where policy says yes.
- [ ] Apply operations to regions.
- [ ] Avoid moving tempo/time signature by default.
- [ ] Preserve relative positions inside region.
- [ ] Handle overlapping clips clearly.
- [ ] Do not destroy hidden/collapsed group contents.
- [ ] Add preview or clear status for destructive operations.
- [ ] Undo arranger operations.

### Phase 2M - Multi-track Edit Groups

- [ ] Add edit group model.
- [ ] Add edit group enabled state.
- [ ] Add edit group color.
- [ ] Distinguish edit groups from folder/group tracks.
- [ ] Show edit group membership in UI.
- [ ] Move grouped clips together scaffold.
- [ ] Split grouped clips scaffold.
- [ ] Resize related clips scaffold.
- [ ] Avoid surprising multi-track edits.
- [ ] Require explicit enabled edit group behavior.
- [ ] Persist edit groups.
- [ ] Load edit groups.

### Phase 2N - WGPU Timeline Renderer

- [ ] Add timeline render snapshot.
- [ ] Include viewport in snapshot.
- [ ] Include render tracks in snapshot.
- [ ] Include render clips in snapshot.
- [ ] Include render automation in snapshot.
- [ ] Include render markers in snapshot.
- [ ] Include render regions in snapshot.
- [ ] Include playhead in snapshot.
- [ ] Include selection in snapshot.
- [ ] Include render theme from theme tokens.
- [ ] Render grid through WGPU path.
- [ ] Render clips through WGPU path.
- [ ] Render playhead through WGPU path.
- [ ] Render dense automation through WGPU path.
- [ ] Keep GPUI shell responsibilities clear.
- [ ] Build snapshot outside hot render path where possible.
- [ ] Do not mutate project during WGPU render.
- [ ] Do not read UI state from GPU renderer.
- [ ] Keep CPU fallback working.
- [ ] Add GPU acceleration setting integration.
- [ ] Add GPU device auto/device list support if available.
- [ ] Add timeline quality setting if useful.
- [ ] Handle weak/old GPUs with fallback.
- [ ] Confirm no WGPU dependency in audio runtime.

### Phase 2O - Large Session Performance

- [ ] Only layout visible tracks.
- [ ] Only render visible clips.
- [ ] Only fetch visible waveform chunks.
- [ ] Only render visible automation points/segments.
- [ ] Use grid LOD.
- [ ] Use waveform LOD.
- [ ] Avoid per-frame allocations.
- [ ] Avoid full project scans every paint.
- [ ] Add track visibility tree.
- [ ] Add clip interval index.
- [ ] Add marker/region interval index.
- [ ] Add automation range query/index.
- [ ] Add waveform chunk cache.
- [ ] Add render snapshot cache.
- [ ] Test 1000-track idle scenario.
- [ ] Test 200 active tracks.
- [ ] Test 10,000 clips.
- [ ] Test dense automation.
- [ ] Test long audio file waveform chunking.
- [ ] Confirm scroll remains responsive.
- [ ] Confirm zoom does not regenerate everything.

### Phase 2P - Export / Bounce Range Integration

- [ ] Add export range model.
- [ ] Support full song range.
- [ ] Support time selection range.
- [ ] Support arrangement region range.
- [ ] Support loop region range.
- [ ] Feed export selected range.
- [ ] Feed export arrangement region.
- [ ] Feed bounce selected clips.
- [ ] Feed freeze track.
- [ ] Feed render comp.
- [ ] Feed render warped audio cache.
- [ ] Avoid duplicate range systems.

### Phase 2Q - QA / Stabilization

- [ ] Test save/load tempo map.
- [ ] Test save/load time signature map.
- [ ] Test save/load regions/markers/automation/takes.
- [ ] Test runtime snapshot build.
- [ ] Test audio clip sample positions.
- [ ] Test MIDI event sample positions.
- [ ] Test loop region conversion.
- [ ] Test export range conversion.
- [ ] Test undo drag command as single undo.
- [ ] Test batch command undo.
- [ ] Test old project migration.
- [ ] Test no nested GPUI update panic.
- [ ] Test WGPU/CPU renderer switch.
- [ ] Test large project scroll.
- [ ] Add Stage 2 diagnostics flags.
- [ ] Throttle diagnostics logs.
- [ ] Run relevant unit tests.
- [ ] Run relevant integration tests.
- [ ] Run `cargo check -p sphere_ui_components`.
- [ ] Run `cargo check --manifest-path apps/native/Cargo.toml`.
- [ ] Run `cargo clippy -p sphere_ui_components -- -D warnings`.

### Stage 2 Acceptance

- [ ] Tempo map works and persists.
- [ ] Time signature map works and persists.
- [ ] Grid/ruler reflects tempo/time signature maps.
- [ ] Timeline has stable beat/seconds/sample conversion.
- [ ] Runtime timeline snapshot feeds audio engine.
- [ ] Automation can be evaluated runtime-safely.
- [ ] Clip slip/fade/stretch scaffold works.
- [ ] Loop/punch/range model is unified.
- [ ] Take lane/comping model works at least minimally.
- [ ] Ripple edit exists with undo.
- [ ] Arranger region operations exist with undo.
- [ ] WGPU timeline renderer exists with CPU fallback.
- [ ] Large sessions remain responsive.
- [ ] Export/bounce can consume timeline ranges.
- [ ] Old projects load safely.
- [ ] No GPUI double update panic.
- [ ] Build/check passes.

## Manual QA - Stage 1

- [ ] Add, move, rename, delete, save, and reload marker.
- [ ] Add, rename, resize, move, delete, save, and reload region.
- [ ] Show Track Volume automation, add/move/delete points, save, and reload.
- [ ] Create tracks, group selected tracks, collapse, expand, save, reload, and ungroup.
- [ ] Select clip, marker, region, and automation point.
- [ ] Delete selected target without deleting wrong target.
- [ ] Use Ctrl/Cmd+A/C/V/X/Delete in timeline.
- [ ] Confirm text input shortcuts still work.
- [ ] Confirm MIDI editor shortcuts still route to MIDI editor when focused.
- [ ] Confirm playhead above clips.
- [ ] Confirm grid behind clips.
- [ ] Confirm markers and region lane visible.
- [ ] Confirm marquee does not stick.

## Manual QA - Stage 2

- [ ] Add tempo point.
- [ ] Move tempo point.
- [ ] Confirm grid changes correctly.
- [ ] Add time signature change.
- [ ] Confirm bar labels update.
- [ ] Slip audio clip.
- [ ] Split clips across tracks.
- [ ] Draw automation and hear runtime-safe result where implemented.
- [ ] Duplicate arrangement section.
- [ ] Ripple delete range.
- [ ] Show take lanes.
- [ ] Switch WGPU/CPU renderer.
- [ ] Scroll large project.
- [ ] Export selected timeline range.

## Debug Flags

- [ ] `FUTUREBOARD_TIMELINE_DEBUG=1`
- [ ] `FUTUREBOARD_TIMELINE_SELECTION_DEBUG=1`
- [ ] `FUTUREBOARD_TIMELINE_MARKER_DEBUG=1`
- [ ] `FUTUREBOARD_TIMELINE_REGION_DEBUG=1`
- [ ] `FUTUREBOARD_AUTOMATION_DEBUG=1`
- [ ] `FUTUREBOARD_TRACK_GROUP_DEBUG=1`
- [ ] `FUTUREBOARD_EDIT_COMMAND_DEBUG=1`
- [ ] `FUTUREBOARD_TIMELINE_STAGE2_DEBUG=1`
- [ ] `FUTUREBOARD_TEMPO_MAP_DEBUG=1`
- [ ] `FUTUREBOARD_TIME_SIGNATURE_DEBUG=1`
- [ ] `FUTUREBOARD_SNAP_DEBUG=1`
- [ ] `FUTUREBOARD_RUNTIME_TIMELINE_DEBUG=1`
- [ ] `FUTUREBOARD_AUTOMATION_RUNTIME_DEBUG=1`
- [ ] `FUTUREBOARD_WGPU_TIMELINE_DEBUG=1`
- [ ] `FUTUREBOARD_TIMELINE_PERF_DEBUG=1`
- [ ] `FUTUREBOARD_RIPPLE_DEBUG=1`
- [ ] `FUTUREBOARD_ARRANGER_DEBUG=1`
- [ ] `FUTUREBOARD_COMPING_DEBUG=1`
