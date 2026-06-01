# Futureboard Automation System Checklist

Planning status: checklist only. No implementation code is implied.

## Target Model

- [ ] Define `AutomationTarget`.
- [ ] Track volume target.
- [ ] Track pan target.
- [ ] Track mute target.
- [ ] Send gain target.
- [ ] Plugin parameter target.
- [ ] Master volume target.
- [ ] Master pan target.
- [ ] Master plugin parameter target.
- [ ] Tempo target.
- [ ] Time signature target placeholder.
- [ ] Stable track IDs.
- [ ] Stable send IDs.
- [ ] Stable insert IDs.
- [ ] Stable plugin parameter IDs.
- [ ] Unresolved target preservation.
- [ ] Target display labels.
- [ ] Target value ranges.
- [ ] Target value formatting.

## Lane Data

- [ ] Define `AutomationLane`.
- [ ] Define `AutomationPoint`.
- [ ] Define `AutomationCurveKind`.
- [ ] Stable point IDs.
- [ ] Points sorted by beat.
- [ ] Point beat clamping.
- [ ] Point value clamping.
- [ ] Epsilon duplicate handling.
- [ ] Lane visibility.
- [ ] Lane height.
- [ ] Lane collapsed state if needed.
- [ ] Lane read enabled.
- [ ] Lane write enabled.
- [ ] Lane armed state.
- [ ] Project format migration.
- [ ] Save/load roundtrip.

## Automation Modes

- [ ] Off mode.
- [ ] Read mode.
- [ ] Touch mode planned and disabled until implemented.
- [ ] Latch mode planned and disabled until implemented.
- [ ] Write mode planned and disabled until implemented.
- [ ] Trim mode planned and disabled until implemented.
- [ ] UI labels do not imply unsupported recording.
- [ ] Mode state saved where appropriate.

## Lane UI

- [ ] Track automation lane reveal.
- [ ] Track target picker.
- [ ] Master automation lane reveal.
- [ ] Master target picker.
- [ ] Plugin parameter target picker.
- [ ] Tempo lane placement.
- [ ] Lane header.
- [ ] Target label.
- [ ] Current value display.
- [ ] Read/off control.
- [ ] Collapse control.
- [ ] Height resize.
- [ ] Empty lane state.
- [ ] Unresolved target state.
- [ ] Compact dark DAW styling.
- [ ] No generic web form styling.

## Point Editing

- [ ] Add point.
- [ ] Move point.
- [ ] Delete point.
- [ ] Select point.
- [ ] Multi-select points.
- [ ] Marquee select points.
- [ ] Drag selected points.
- [ ] Drag segment.
- [ ] Create ramp.
- [ ] Copy points.
- [ ] Paste points.
- [ ] Quantize points.
- [ ] Simplify points later.
- [ ] Curve handles later.
- [ ] Snap horizontally.
- [ ] Continuous vertical drag for continuous targets.
- [ ] Discrete target editing semantics.
- [ ] Project dirty once per committed edit.
- [ ] Selection-only changes do not dirty project.

## Rendering

- [ ] Lane background.
- [ ] Center/reference line.
- [ ] Points.
- [ ] Segment/curve lines.
- [ ] Selected point state.
- [ ] Hover point state.
- [ ] Drag preview.
- [ ] Value tooltip.
- [ ] Visible range culling.
- [ ] Hit-test cache.
- [ ] GPUI paint fallback.
- [ ] WGPU render snapshot planned.
- [ ] No project mutation during render.
- [ ] No plugin metadata lookup during render.

## Playback Read Mode

- [ ] Build runtime automation lane snapshot.
- [ ] Resolve target IDs to runtime handles.
- [ ] Evaluate hold curves.
- [ ] Evaluate linear curves.
- [ ] Smooth/bezier/exponential/log curves planned.
- [ ] Apply track volume.
- [ ] Apply track pan.
- [ ] Apply track mute.
- [ ] Apply send gain.
- [ ] Apply master volume.
- [ ] Apply master pan.
- [ ] Apply plugin parameter values.
- [ ] Skip unresolved targets.
- [ ] Skip read-disabled lanes.
- [ ] No audio-thread allocation.
- [ ] No audio-thread locks.
- [ ] No audio-thread string lookup.
- [ ] No audio-thread logging.

## Smoothing

- [ ] Volume smoothing.
- [ ] Pan smoothing.
- [ ] Send gain smoothing.
- [ ] Plugin parameter smoothing where appropriate.
- [ ] Discrete target hold behavior.
- [ ] Avoid zipper noise.
- [ ] Define smoothing time constants.
- [ ] Confirm plugin parameter smoothing ownership: host vs engine vs plugin.

## Plugin Parameter Automation

- [ ] Plugin metadata exposes stable parameter IDs.
- [ ] Plugin metadata exposes display name.
- [ ] Plugin metadata exposes unit if available.
- [ ] Plugin metadata exposes normalized default value.
- [ ] Plugin metadata exposes plain value conversion if available.
- [ ] Create lane from plugin device UI.
- [ ] Create lane from target picker.
- [ ] Store normalized values.
- [ ] Dispatch runtime parameter changes safely.
- [ ] Preserve unresolved plugin parameter lanes.
- [ ] Save/load plugin parameter automation.
- [ ] Test with missing plugin.
- [ ] Test with missing parameter.

## Master Automation

- [ ] Master volume lane.
- [ ] Master pan lane.
- [ ] Master plugin parameter lane.
- [ ] Master lane UI placement.
- [ ] Master lane runtime dispatch.
- [ ] Save/load master automation.
- [ ] Unresolved master plugin parameter handling.

## Tempo Automation

- [ ] Define `TempoPoint`.
- [ ] Define `TempoMap`.
- [ ] Clamp BPM to `20..=300`.
- [ ] Sort tempo points.
- [ ] Save/load tempo map.
- [ ] Render tempo lane.
- [ ] Tempo marker/ruler display.
- [ ] Static tempo compatibility.
- [ ] `tempo_at_beat`.
- [ ] `seconds_at_beat`.
- [ ] `beat_at_seconds`.
- [ ] Segment cache built off audio thread.
- [ ] Transport integration.
- [ ] MIDI scheduling integration.
- [ ] Ruler/grid integration.
- [ ] Audio clip stretching behavior explicitly deferred.

## Write Modes Later

- [ ] Parameter touch event path.
- [ ] Parameter release event path.
- [ ] Automation recorder.
- [ ] Point decimation/simplification.
- [ ] Touch mode recording.
- [ ] Latch mode recording.
- [ ] Write mode recording.
- [ ] Trim mode editing.
- [ ] Undo one recording pass as one command.
- [ ] Prevent excessive point density.

## Future Automation Clips

- [ ] Decide automation clip conflict model.
- [ ] Define clip-local automation point data.
- [ ] Decide lane vs clip priority.
- [ ] Define clip mute behavior.
- [ ] Define clip loop behavior.
- [ ] Define copy/paste behavior.
- [ ] Do not implement before lane automation is stable.

## Engine Checklist

- [ ] Runtime target handle table.
- [ ] Runtime automation snapshots.
- [ ] Runtime tempo map snapshot.
- [ ] Command queue for snapshot updates.
- [ ] Mixer parameter dispatch.
- [ ] Plugin parameter dispatch.
- [ ] Tempo map dispatch.
- [ ] No allocation in audio callback.
- [ ] No lock in audio callback.
- [ ] No project state read in audio callback.
- [ ] Deterministic evaluator tests.

## QA Checklist

- [ ] One automation point.
- [ ] Two points linear ramp.
- [ ] Dense automation points.
- [ ] 32 tracks with visible automation lanes.
- [ ] Track volume playback.
- [ ] Track pan playback.
- [ ] Track mute playback.
- [ ] Send gain playback.
- [ ] Master volume playback.
- [ ] Plugin parameter playback.
- [ ] Missing plugin survives load.
- [ ] Missing parameter survives load.
- [ ] Tempo point save/load.
- [ ] Tempo change playback after tempo map phase.
- [ ] Undo/redo add point.
- [ ] Undo/redo move point.
- [ ] Undo/redo delete point.
- [ ] Save/load roundtrip.
- [ ] CPU/FPS test with dense lanes.

