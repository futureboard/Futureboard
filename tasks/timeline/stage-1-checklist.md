# Futureboard Timeline Stage 1 Checklist

Planning status: checklist only. No implementation code is implied.

Source roadmap: `tasks/timeline/roadmap.md`

## Scope Guard

- [ ] Keep Stage 1 limited to Timeline Composition / Arrangement.
- [ ] Do not implement Stage 2 engine work.
- [ ] Do not rewrite the whole Timeline.
- [ ] Do not break current clip editing.
- [ ] Do not break MIDI editor behavior.
- [ ] Do not break audio/plugin runtime.
- [ ] Do not call `LoadProject` on every small edit.
- [ ] Do not create nested GPUI entity updates / double lease panic.
- [ ] Do not add fake UI actions that claim to work but do nothing.
- [ ] Use global theme tokens only.
- [ ] Keep UI compact and DAW-like.
- [ ] Mark persistent project edits dirty through `StudioLayout` only.
- [ ] Make Timeline/MIDI/child components return command outcomes instead of updating `StudioLayout` directly.

## Phase 1A - Audit And Data Model

- [ ] Audit current timeline architecture before code changes.
- [ ] Search timeline, track lane, clip, marker, automation, group, region, selection, inspector, dirty, and command flow symbols.
- [ ] Create `tasks/native/timeline-stage-1-audit.md`.
- [ ] Document current timeline state ownership.
- [ ] Document current track model.
- [ ] Document current clip model.
- [ ] Document current selection model.
- [ ] Document current edit command flow.
- [ ] Document current automation model.
- [ ] Document existing marker/region code if any.
- [ ] Document files likely to change.
- [ ] Document nested GPUI update risks.
- [ ] Document save/load gaps.
- [ ] Identify smallest safe implementation slices.
- [ ] Add or confirm `TimelineMarker` persistent model.
- [ ] Add or confirm `MarkerKind`.
- [ ] Add or confirm `ArrangementRegion` persistent model.
- [ ] Add or confirm `AutomationLane` persistent model.
- [ ] Add or confirm `AutomationTarget`.
- [ ] Add or confirm `AutomationPoint`.
- [ ] Add or confirm `AutomationCurveKind`.
- [ ] Add or confirm `TrackKind::Group`.
- [ ] Add or confirm `TrackKind::Folder`.
- [ ] Add or confirm track `parent_id`.
- [ ] Add or confirm group/folder collapse state.
- [ ] Add project defaults for new fields.
- [ ] Add project migration scaffold if project format supports migration/versioning.
- [ ] Ensure old projects without new fields load safely.
- [ ] Ensure save/load roundtrip preserves new empty/default state.
- [ ] Keep Phase 1A free of required visible UI.
- [ ] Keep unfinished menu entries disabled if added.
- [ ] Run `cargo check -p sphere_ui_components`.
- [ ] Run `cargo check --manifest-path apps/native/Cargo.toml`.

## Phase 1B - Marker System

- [ ] Render marker lane above timeline ruler or inside ruler header.
- [ ] Render marker flags/triangles on ruler.
- [ ] Show marker labels when zoom allows.
- [ ] Show selected marker accent outline.
- [ ] Show marker hover tooltip with name, bar/beat, and kind.
- [ ] Add command: Add Marker at Playhead.
- [ ] Add command: Add Marker at Mouse Position.
- [ ] Add command: Rename Marker.
- [ ] Add command: Delete Marker.
- [ ] Add command: Move Marker.
- [ ] Add command: Select Marker.
- [ ] Add command: Next Marker.
- [ ] Add command: Previous Marker.
- [ ] Optionally route `M` to add marker at playhead when timeline has focus.
- [ ] Route `Delete` to selected marker only when appropriate.
- [ ] Add marker inspector fields for name, position, color, and kind.
- [ ] Clamp marker beat to `>= 0`.
- [ ] Snap marker moves when snap is enabled.
- [ ] Allow duplicate marker positions.
- [ ] Avoid exact duplicate marker names if easy.
- [ ] Mark project dirty for marker edits.
- [ ] Do not mark project dirty for marker selection only.
- [ ] Persist markers in project save.
- [ ] Load markers from project.
- [ ] Roundtrip markers through save/load.

## Phase 1C - Arrangement Regions

- [ ] Render arrangement region lane above tracks and near ruler.
- [ ] Render region blocks with name and color.
- [ ] Support dragging region body to move.
- [ ] Support dragging region edges to resize.
- [ ] Support double-click rename.
- [ ] Support region context menu.
- [ ] Add command: Add Region from Selection.
- [ ] Add command: Add Region at Playhead.
- [ ] Add command: Rename Region.
- [ ] Add command: Delete Region.
- [ ] Add command: Move Region.
- [ ] Add command: Resize Region.
- [ ] Add command: Select Region.
- [ ] Add region inspector fields for name, start, end, length, and color.
- [ ] Enforce `end_beat > start_beat`.
- [ ] Snap region start/end when snap is enabled.
- [ ] Allow overlapping regions in Stage 1.
- [ ] Defer arranger reorder/copy song-section workflow to Stage 2.
- [ ] Mark project dirty for persistent region edits.
- [ ] Do not mark project dirty for region selection only.
- [ ] Persist regions in project save.
- [ ] Load regions from project.
- [ ] Roundtrip regions through save/load.

## Phase 1D - Automation Lanes UI

- [ ] Support track automation lane expansion under the main track lane.
- [ ] Render lane header.
- [ ] Render target name.
- [ ] Render read toggle.
- [ ] Render lane height control or stored lane height behavior.
- [ ] Render close/hide control.
- [ ] Render lane body line/curve.
- [ ] Render automation points.
- [ ] Render point hover value.
- [ ] Render selected point state.
- [ ] Add Stage 1 Track Volume target.
- [ ] Add Stage 1 Track Pan target.
- [ ] Add Stage 1 Master Volume target.
- [ ] Add plugin parameter scaffold if plugin parameter list exists.
- [ ] Add Tempo target as data scaffold only.
- [ ] Add command: Show Automation Lane.
- [ ] Add command: Hide Automation Lane.
- [ ] Add command: Add Automation Point.
- [ ] Add command: Move Automation Point.
- [ ] Add command: Delete Automation Point.
- [ ] Add command: Select Automation Point.
- [ ] Add command: Clear Automation Lane.
- [ ] Add command: Set Automation Target.
- [ ] Add point by clicking automation line.
- [ ] Move point by dragging.
- [ ] Support additive point selection with Shift/Ctrl where established by app conventions.
- [ ] Delete selected automation points.
- [ ] Apply snap to horizontal beat only.
- [ ] Clamp vertical value by target range.
- [ ] Use volume range `-60 dB` to `+12 dB`, or `-inf` if supported.
- [ ] Use pan range `-100..+100`.
- [ ] Use plugin parameter range `0.0..1.0`.
- [ ] Use tempo data range `20..300 BPM`.
- [ ] Mark project dirty for point edits.
- [ ] Do not mark project dirty for point selection only.
- [ ] Sync track volume automation with effective volume only if the current model supports it safely.
- [ ] Do not implement sample-accurate automation engine unless already available.
- [ ] Persist automation lanes and points.
- [ ] Load automation lanes and points.
- [ ] Roundtrip automation through save/load.

## Phase 1E - Track Group / Folder Tracks

- [ ] Add Create Folder Track behavior.
- [ ] Add Create Group Track behavior.
- [ ] Assign selected tracks to folder/group.
- [ ] Collapse group/folder track.
- [ ] Expand group/folder track.
- [ ] Render folder/group track header.
- [ ] Render disclosure arrow.
- [ ] Render children indented.
- [ ] Render group color strip.
- [ ] Hide children when collapsed.
- [ ] Preserve child track order.
- [ ] Keep folder tracks organizational only.
- [ ] Keep group tracks organizational in Stage 1 unless safe bus routing already exists.
- [ ] Add command: Create Folder Track.
- [ ] Add command: Create Group Track.
- [ ] Add command: Group Selected Tracks.
- [ ] Add command: Ungroup Tracks.
- [ ] Add command: Collapse/Expand Group.
- [ ] Add command: Move Track Into Group.
- [ ] Add command: Move Track Out Of Group.
- [ ] Prevent parent cycles.
- [ ] Prevent track from parenting itself.
- [ ] Prevent Master from becoming a child track.
- [ ] Allow bus/return grouping only if safe.
- [ ] Deleting folder/group ungroups children by default unless explicit delete-children action exists.
- [ ] Persist track hierarchy.
- [ ] Persist collapse state.
- [ ] Load hierarchy safely.
- [ ] Roundtrip hierarchy through save/load.
- [ ] Confirm no routing/audio behavior breaks.

## Phase 1F - Selection And Shortcut Cleanup

- [ ] Define or confirm unified timeline selection item for tracks.
- [ ] Define or confirm unified timeline selection item for clips.
- [ ] Define or confirm unified timeline selection item for markers.
- [ ] Define or confirm unified timeline selection item for regions.
- [ ] Define or confirm unified timeline selection item for automation points.
- [ ] Keep MIDI note selection separate inside MIDI editor.
- [ ] Store selected persistent items by ID.
- [ ] Keep marquee rect transient.
- [ ] Allow only one active gesture at a time.
- [ ] Do not dirty project for selection-only changes.
- [ ] Route delete by focused context and selected item priority.
- [ ] Ensure modal/text input wins over DAW shortcuts.
- [ ] Ensure MIDI editor selection wins when MIDI editor is focused.
- [ ] Ensure automation lane focus routes point edits correctly.
- [ ] Ensure clip/region/marker hit tests route selection correctly.
- [ ] Ensure track header focus routes track selection correctly.
- [ ] Add or confirm `CommandOutcome` with `changed`, `project_dirty`, and `status`.
- [ ] Make child components return command outcomes for edit commands.
- [ ] Apply project dirty in `StudioLayout` after child update finishes.
- [ ] Do not call `StudioLayout.update` from `Timeline.update`.
- [ ] Do not mark dirty by synchronously updating parent from child.
- [ ] Do not call `LoadProject` for every edit.
- [ ] Support Select All in timeline.
- [ ] Support Copy in timeline.
- [ ] Support Paste in timeline.
- [ ] Support Cut in timeline.
- [ ] Support Delete in timeline.
- [ ] Support Duplicate in timeline.
- [ ] Support Rename in timeline.
- [ ] Support Split in timeline where existing clip editing supports it.
- [ ] Support Mute in timeline where existing clip editing supports it.
- [ ] Support Group/Ungroup command routing.
- [ ] Confirm Ctrl/Cmd+A/C/V/X/Delete are safe.
- [ ] Confirm no GPUI double lease panic.

## Phase 1G - Layer Contract Polish

- [ ] Define or document timeline render layer order.
- [ ] Render timeline background at layer 0.
- [ ] Render grid background at layer 1.
- [ ] Render arrangement region lane background at layer 2.
- [ ] Render track lane backgrounds at layer 3.
- [ ] Render clips at layer 4.
- [ ] Render automation lanes/curves/points at layer 5.
- [ ] Render markers and region labels at layer 6.
- [ ] Render selection/marquee overlay at layer 7.
- [ ] Render playhead at layer 8.
- [ ] Render hover handles/tooltips at layer 9.
- [ ] Render floating toolbar/overlays at layer 10.
- [ ] Ensure grid never draws above clips.
- [ ] Ensure playhead draws above clips and automation.
- [ ] Ensure marker flags are visible above ruler.
- [ ] Ensure region lane does not cover controls.
- [ ] Ensure track headers stay above timeline body if needed.
- [ ] Ensure scrollbars and floating tools stay on top.
- [ ] Ensure marquee clears after mouse up.

## Phase 1H - QA And Stabilization

- [ ] Save/load roundtrip markers.
- [ ] Save/load roundtrip regions.
- [ ] Save/load roundtrip automation lanes/points.
- [ ] Save/load roundtrip group/folder hierarchy.
- [ ] Load old project without Stage 1 fields.
- [ ] Ignore or repair invalid IDs on load.
- [ ] Convert orphan child tracks to root tracks on load.
- [ ] Disable or mark missing automation targets without crashing.
- [ ] Clamp invalid marker/region beats on load.
- [ ] Add or confirm timeline debug flags.
- [ ] Add or confirm selection debug flag.
- [ ] Add or confirm marker debug flag.
- [ ] Add or confirm region debug flag.
- [ ] Add or confirm automation debug flag.
- [ ] Add or confirm track group debug flag.
- [ ] Add or confirm edit command debug flag.
- [ ] Log command dispatch when debug is enabled.
- [ ] Log selection target when debug is enabled.
- [ ] Log marker add/move/delete when debug is enabled.
- [ ] Log region add/move/resize/delete when debug is enabled.
- [ ] Log automation point edits when debug is enabled.
- [ ] Log group/ungroup when debug is enabled.
- [ ] Log dirty outcome when debug is enabled.
- [ ] Log save/load migration warnings when debug is enabled.
- [ ] Throttle or avoid per-frame debug logs.
- [ ] Run `cargo check -p sphere_ui_components`.
- [ ] Run `cargo check --manifest-path apps/native/Cargo.toml`.
- [ ] Run `cargo clippy -p sphere_ui_components -- -D warnings`.

## Toolbar And Context Menus

- [ ] Add marker button/menu.
- [ ] Add region button/menu.
- [ ] Add automation toggle.
- [ ] Add group selected menu.
- [ ] Keep snap/grid controls working.
- [ ] Timeline empty context menu includes Add Marker.
- [ ] Timeline empty context menu includes Add Region.
- [ ] Timeline empty context menu includes Paste.
- [ ] Clip context menu includes Cut.
- [ ] Clip context menu includes Copy.
- [ ] Clip context menu includes Delete.
- [ ] Clip context menu includes Duplicate.
- [ ] Clip context menu includes Split.
- [ ] Clip context menu includes Mute.
- [ ] Marker context menu includes Rename.
- [ ] Marker context menu includes Delete.
- [ ] Marker context menu includes Color.
- [ ] Region context menu includes Rename.
- [ ] Region context menu includes Delete.
- [ ] Region context menu includes Color.
- [ ] Track header context menu includes Add Automation Lane.
- [ ] Track header context menu includes Group Selected Tracks.
- [ ] Track header context menu includes Create Folder.
- [ ] Track header context menu includes Collapse/Expand.
- [ ] Automation lane context menu includes Add Point.
- [ ] Automation lane context menu includes Clear Lane.
- [ ] Automation lane context menu includes Hide Lane.
- [ ] Unavailable actions are visibly disabled.
- [ ] No action reports success unless it works.

## Inspector Integration

- [ ] Inspector shows selected track details.
- [ ] Inspector shows selected clip details.
- [ ] Inspector shows selected marker details.
- [ ] Inspector shows selected region details.
- [ ] Inspector shows selected automation lane/point details.
- [ ] Inspector shows selected group/folder track details.
- [ ] Marker inspector edits name.
- [ ] Marker inspector edits beat/bar.
- [ ] Marker inspector edits kind.
- [ ] Marker inspector edits color.
- [ ] Region inspector edits name.
- [ ] Region inspector edits start.
- [ ] Region inspector edits end.
- [ ] Region inspector shows length.
- [ ] Region inspector edits color.
- [ ] Automation point inspector shows target.
- [ ] Automation point inspector edits beat.
- [ ] Automation point inspector edits value.
- [ ] Automation point inspector edits curve.
- [ ] Group/folder track inspector edits name.
- [ ] Group/folder track inspector edits color.
- [ ] Group/folder track inspector edits collapsed state.
- [ ] Group/folder track inspector shows children count.
- [ ] Group track inspector shows output/routing only if group routing is supported.
- [ ] Inspector edits use command/outcome flow.
- [ ] Inspector dirty state changes only on actual persistent changes.
- [ ] Inspector uses shared settings/input components.
- [ ] Inspector uses global theme tokens.
- [ ] Inspector edits do not cause nested entity update.

## Manual QA - Markers

- [ ] Add marker at playhead.
- [ ] Move marker.
- [ ] Rename marker.
- [ ] Delete marker.
- [ ] Save project.
- [ ] Load project.
- [ ] Confirm marker remains.

## Manual QA - Regions

- [ ] Add region from beat 1 to beat 5.
- [ ] Rename region `Verse`.
- [ ] Resize region.
- [ ] Move region.
- [ ] Delete region.
- [ ] Save/load project.
- [ ] Confirm no ripple editing occurs.

## Manual QA - Automation

- [ ] Show Track Volume automation.
- [ ] Add automation points.
- [ ] Move automation points.
- [ ] Delete automation points.
- [ ] Play/seek and confirm fader sync if automation read exists.
- [ ] Save/load project.
- [ ] Confirm lane visibility persists.

## Manual QA - Groups

- [ ] Create 3 tracks.
- [ ] Group selected tracks.
- [ ] Collapse group.
- [ ] Confirm children hide.
- [ ] Expand group.
- [ ] Confirm children show.
- [ ] Save/load project.
- [ ] Ungroup tracks.
- [ ] Confirm no audio routing break.

## Manual QA - Selection

- [ ] Select clip.
- [ ] Select marker.
- [ ] Select region.
- [ ] Select automation point.
- [ ] Delete selected target.
- [ ] Confirm no wrong target deleted.
- [ ] Confirm selection-only changes do not dirty project.
- [ ] Confirm no marquee stuck overlay.

## Manual QA - Shortcuts

- [ ] Ctrl/Cmd+A in timeline.
- [ ] Ctrl/Cmd+C clips.
- [ ] Ctrl/Cmd+V clips.
- [ ] Ctrl/Cmd+X clips.
- [ ] Delete clips.
- [ ] Delete markers.
- [ ] Delete regions.
- [ ] Delete automation points.
- [ ] Confirm text input shortcuts still work.
- [ ] Confirm MIDI editor shortcuts still route to MIDI editor when focused.

## Manual QA - Layering

- [ ] Playhead appears above clips.
- [ ] Grid appears behind clips.
- [ ] Markers remain visible.
- [ ] Region lane remains visible.
- [ ] Automation points remain visible.
- [ ] Marquee clears after mouse up.
- [ ] Floating tools remain above timeline body.

## Final Stage 1 Acceptance

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

## Explicitly Deferred To Stage 2

- [ ] Full tempo map engine.
- [ ] Sample-accurate automation playback.
- [ ] Audio warping/time-stretch.
- [ ] Comping/take lanes.
- [ ] Ripple edit.
- [ ] Arranger track advanced workflow.
- [ ] Full WGPU viewport rewrite.
- [ ] Large-session virtualization beyond safe UI culling.
- [ ] Deep audio engine scheduling changes.
