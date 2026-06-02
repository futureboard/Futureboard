# Futureboard MIDI Editor Checklist

Planning status: checklist only. No implementation code is implied.

## Data Model

- [ ] Define stable persisted MIDI note IDs.
- [ ] Decide runtime compact note handle strategy.
- [x] Store note pitch, start, duration, velocity, muted state.
- [ ] Decide whether selected state is persisted or transient only.
- [ ] Clamp pitch to `0..=127`.
- [ ] Clamp velocity to `1..=127`.
- [ ] Clamp start to `>= 0`.
- [ ] Clamp duration to `>= MIN_NOTE_BEATS`.
- [ ] Sort notes by `(start_beat, pitch, id)`.
- [ ] Auto-expand MIDI clip when note edits exceed clip end.
- [ ] Preserve notes when trimming clips.
- [x] Add MIDI controller lane model.
- [x] Add CC, pitch bend, channel pressure kinds.
- [x] Add lane visible/collapsed/height state.
- [ ] Add migration path from current note model.

## Project Save/Load

- [ ] Serialize notes with stable IDs.
- [ ] Deserialize notes with validation and clamping.
- [x] Serialize note muted state.
- [x] Serialize MIDI controller lanes.
- [x] Serialize controller points.
- [x] Preserve empty visible controller lanes if chosen as project state.
- [ ] Migrate old projects with transient note IDs.
- [ ] Roundtrip MIDI clips without data loss.
- [ ] Warn on invalid/out-of-range data instead of panicking.

## Editor Shell

- [x] Bottom-panel MIDI editor opens selected MIDI clip.
- [x] Floating MIDI editor opens the same selected clip.
- [x] Editor focus prevents Space from toggling transport while typing.
- [x] Toolbar exposes active tool.
- [x] Toolbar exposes snap toggle/value.
- [x] Toolbar exposes quantize value.
- [x] Toolbar exposes fit/zoom controls.
- [x] Toolbar avoids fake enabled actions.
- [x] UI uses Futureboard dark compact theme tokens.
- [x] No Bootstrap/web form styling.

## Piano Roll Rendering

- [x] Render ruler aligned to arrangement beat math.
- [x] Render piano keyboard lane.
- [x] Render pitch rows.
- [x] Render beat/bar/subdivision grid.
- [x] Render clip bounds.
- [x] Render playhead synced to transport.
- [x] Render loop region awareness.
- [x] Render selected notes distinctly.
- [x] Render muted notes distinctly.
- [x] Render note labels only when readable.
- [x] Cull notes outside visible beat/pitch range.
- [x] Avoid DOM/element spam for dense clips.
- [x] Prepare WGPU render snapshot shape.

## Note Editing

- [x] Draw note.
- [x] Select note.
- [x] Multi-select notes.
- [x] Marquee select notes.
- [x] Move notes horizontally.
- [x] Move notes vertically by pitch.
- [x] Resize note length.
- [x] Delete notes.
- [x] Duplicate notes.
- [x] Copy/paste notes.
- [x] Quantize notes.
- [x] Quantize preview.
- [x] Transpose notes.
- [x] Edit note length numerically.
- [x] Split notes.
- [x] Mute/unmute notes.
- [ ] Audition note on click.
- [ ] Audition note during drag where safe.
- [x] Respect snap/grid.
- [x] Support modifier behavior for additive selection and constrained edits.
- [x] Mark project dirty once per committed edit.
- [x] Do not mark dirty for selection-only changes.

## Velocity Lane

- [ ] Render velocity bars under piano roll.
- [ ] Highlight selected notes' velocity bars.
- [x] Drag one note velocity.
- [x] Drag multiple selected note velocities.
- [ ] Support velocity scaling.
- [ ] Support velocity ramp tool.
- [ ] Keep vertical drag snap-free.
- [ ] Clamp velocity to `1..=127`.
- [ ] Update note rendering immediately after velocity change.
- [ ] Add humanize/randomize as disabled future actions until implemented.
- [ ] Save/load velocity values.
- [ ] Undo/redo velocity edits.

## CC Control Lanes

- [x] Add CC1 Mod Wheel lane.
- [x] Add CC7 Volume lane.
- [x] Add CC10 Pan lane.
- [x] Add CC11 Expression lane.
- [x] Add CC64 Sustain lane.
- [ ] Add custom CC number lane.
- [x] Add Pitch Bend lane.
- [x] Add Channel Pressure lane.
- [x] Defer Poly Pressure with clear data-model note.
- [x] Draw CC points.
- [x] Draw ramps/lines.
- [x] Erase points.
- [ ] Select points.
- [ ] Marquee select points.
- [x] Move points.
- [x] Delete points.
- [ ] Resize lane height.
- [ ] Collapse/expand lane.
- [ ] Remove lane.
- [ ] Per-lane value scale.
- [x] Snap horizontally when snap is enabled.
- [x] Keep value drag continuous vertically.
- [x] Save/load lanes and points.
- [x] Undo/redo CC edits.

## MIDI Tools

- [x] Draw tool.
- [x] Select tool.
- [x] Erase tool.
- [x] Split tool.
- [x] Velocity tool.
- [x] CC draw tool.
- [ ] Audition tool.
- [x] Mute tool.
- [x] Quantize command.
- [ ] Humanize command later.
- [ ] Legato command later.
- [x] Transpose command.
- [x] Duplicate command.
- [ ] Scale/key guide later.

## Clipboard

- [x] Copy selected notes.
- [x] Paste notes at playhead.
- [x] Paste notes at mouse beat.
- [x] Preserve relative timing.
- [x] Preserve pitch and velocity.
- [ ] Copy selected CC points.
- [ ] Paste CC points into compatible lane.
- [ ] Reject incompatible paste with clear status text.
- [x] Clipboard data versioned internally.

## Undo/Redo

- [x] Note create command.
- [x] Note delete command.
- [x] Note move command.
- [x] Note resize command.
- [x] Note velocity command.
- [x] Note mute command.
- [x] Note split command.
- [x] Note duplicate command.
- [x] Quantize batch command.
- [x] CC point create command.
- [x] CC point delete command.
- [x] CC point move command.
- [x] CC ramp command.
- [x] Batch drag gestures into one undo entry.

## MIDI Playback

- [x] Convert notes to runtime note on/off events.
- [x] Skip muted notes.
- [x] Skip muted clips/tracks.
- [ ] Respect clip bounds.
- [ ] Handle looped clip playback once clip loops exist.
- [x] Send notes to instrument track plugin.
- [ ] Add external MIDI output placeholder.
- [x] Convert CC points to runtime MIDI controller events.
- [x] Initial block-level scheduling documented.
- [ ] Sample-accurate scheduling planned.
- [x] No audio-thread allocation.

## Keyboard Shortcuts

- [ ] Delete/backspace deletes selected notes/points.
- [ ] Cmd/Ctrl+A selects all in focused editor context.
- [ ] Escape cancels drag/clears selection.
- [ ] Duplicates use standard shortcut.
- [ ] Quantize shortcut routes to MIDI editor when focused.
- [ ] Transpose shortcuts route to MIDI editor when focused.
- [ ] Space behavior respects text/numeric input focus.
- [ ] Shortcuts are registered in command registry, not duplicated ad hoc.

## Performance

- [ ] 1 note responsive.
- [ ] 100 notes responsive.
- [ ] 10k notes scrolls/zooms acceptably.
- [ ] Dense velocity bars cull by visible range.
- [ ] Dense CC points cull by visible range.
- [ ] No per-frame project cloning for large clips.
- [ ] No layout-dependent mutation during render.
- [ ] WGPU migration plan documented.

## QA

- [ ] Save/load roundtrip for one MIDI clip.
- [ ] Save/load roundtrip for large MIDI clip.
- [ ] Undo/redo every note edit type.
- [ ] Undo/redo every velocity edit type.
- [ ] Undo/redo CC edits.
- [ ] Playback emits expected note timings.
- [ ] Playback emits expected velocity values.
- [ ] Playback emits expected CC values.
- [ ] Selection-only changes do not dirty project.
- [ ] Clip auto-expands on note beyond right edge.
- [ ] No crash with empty MIDI clip.
- [ ] No crash with invalid migrated data.

