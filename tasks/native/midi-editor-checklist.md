# Futureboard MIDI Editor Checklist

Planning status: checklist only. No implementation code is implied.

## Data Model

- [ ] Define stable persisted MIDI note IDs.
- [ ] Decide runtime compact note handle strategy.
- [ ] Store note pitch, start, duration, velocity, muted state.
- [ ] Decide whether selected state is persisted or transient only.
- [ ] Clamp pitch to `0..=127`.
- [ ] Clamp velocity to `1..=127`.
- [ ] Clamp start to `>= 0`.
- [ ] Clamp duration to `>= MIN_NOTE_BEATS`.
- [ ] Sort notes by `(start_beat, pitch, id)`.
- [ ] Auto-expand MIDI clip when note edits exceed clip end.
- [ ] Preserve notes when trimming clips.
- [ ] Add MIDI controller lane model.
- [ ] Add CC, pitch bend, channel pressure kinds.
- [ ] Add lane visible/collapsed/height state.
- [ ] Add migration path from current note model.

## Project Save/Load

- [ ] Serialize notes with stable IDs.
- [ ] Deserialize notes with validation and clamping.
- [ ] Serialize note muted state.
- [ ] Serialize MIDI controller lanes.
- [ ] Serialize controller points.
- [ ] Preserve empty visible controller lanes if chosen as project state.
- [ ] Migrate old projects with transient note IDs.
- [ ] Roundtrip MIDI clips without data loss.
- [ ] Warn on invalid/out-of-range data instead of panicking.

## Editor Shell

- [ ] Bottom-panel MIDI editor opens selected MIDI clip.
- [ ] Floating MIDI editor opens the same selected clip.
- [ ] Editor focus prevents Space from toggling transport while typing.
- [ ] Toolbar exposes active tool.
- [ ] Toolbar exposes snap toggle/value.
- [ ] Toolbar exposes quantize value.
- [ ] Toolbar exposes fit/zoom controls.
- [ ] Toolbar avoids fake enabled actions.
- [ ] UI uses Futureboard dark compact theme tokens.
- [ ] No Bootstrap/web form styling.

## Piano Roll Rendering

- [ ] Render ruler aligned to arrangement beat math.
- [ ] Render piano keyboard lane.
- [ ] Render pitch rows.
- [ ] Render beat/bar/subdivision grid.
- [ ] Render clip bounds.
- [ ] Render playhead synced to transport.
- [ ] Render loop region awareness.
- [ ] Render selected notes distinctly.
- [ ] Render muted notes distinctly.
- [ ] Render note labels only when readable.
- [ ] Cull notes outside visible beat/pitch range.
- [ ] Avoid DOM/element spam for dense clips.
- [ ] Prepare WGPU render snapshot shape.

## Note Editing

- [ ] Draw note.
- [ ] Select note.
- [ ] Multi-select notes.
- [ ] Marquee select notes.
- [ ] Move notes horizontally.
- [ ] Move notes vertically by pitch.
- [ ] Resize note length.
- [ ] Delete notes.
- [ ] Duplicate notes.
- [ ] Copy/paste notes.
- [ ] Quantize notes.
- [ ] Quantize preview.
- [ ] Transpose notes.
- [ ] Edit note length numerically.
- [ ] Split notes.
- [ ] Mute/unmute notes.
- [ ] Audition note on click.
- [ ] Audition note during drag where safe.
- [ ] Respect snap/grid.
- [ ] Support modifier behavior for additive selection and constrained edits.
- [ ] Mark project dirty once per committed edit.
- [ ] Do not mark dirty for selection-only changes.

## Velocity Lane

- [ ] Render velocity bars under piano roll.
- [ ] Highlight selected notes' velocity bars.
- [ ] Drag one note velocity.
- [ ] Drag multiple selected note velocities.
- [ ] Support velocity scaling.
- [ ] Support velocity ramp tool.
- [ ] Keep vertical drag snap-free.
- [ ] Clamp velocity to `1..=127`.
- [ ] Update note rendering immediately after velocity change.
- [ ] Add humanize/randomize as disabled future actions until implemented.
- [ ] Save/load velocity values.
- [ ] Undo/redo velocity edits.

## CC Control Lanes

- [ ] Add CC1 Mod Wheel lane.
- [ ] Add CC7 Volume lane.
- [ ] Add CC10 Pan lane.
- [ ] Add CC11 Expression lane.
- [ ] Add CC64 Sustain lane.
- [ ] Add custom CC number lane.
- [ ] Add Pitch Bend lane.
- [ ] Add Channel Pressure lane.
- [ ] Defer Poly Pressure with clear data-model note.
- [ ] Draw CC points.
- [ ] Draw ramps/lines.
- [ ] Erase points.
- [ ] Select points.
- [ ] Marquee select points.
- [ ] Move points.
- [ ] Delete points.
- [ ] Resize lane height.
- [ ] Collapse/expand lane.
- [ ] Remove lane.
- [ ] Per-lane value scale.
- [ ] Snap horizontally when snap is enabled.
- [ ] Keep value drag continuous vertically.
- [ ] Save/load lanes and points.
- [ ] Undo/redo CC edits.

## MIDI Tools

- [ ] Draw tool.
- [ ] Select tool.
- [ ] Erase tool.
- [ ] Split tool.
- [ ] Velocity tool.
- [ ] CC draw tool.
- [ ] Audition tool.
- [ ] Mute tool.
- [ ] Quantize command.
- [ ] Humanize command later.
- [ ] Legato command later.
- [ ] Transpose command.
- [ ] Duplicate command.
- [ ] Scale/key guide later.

## Clipboard

- [ ] Copy selected notes.
- [ ] Paste notes at playhead.
- [ ] Paste notes at mouse beat.
- [ ] Preserve relative timing.
- [ ] Preserve pitch and velocity.
- [ ] Copy selected CC points.
- [ ] Paste CC points into compatible lane.
- [ ] Reject incompatible paste with clear status text.
- [ ] Clipboard data versioned internally.

## Undo/Redo

- [ ] Note create command.
- [ ] Note delete command.
- [ ] Note move command.
- [ ] Note resize command.
- [ ] Note velocity command.
- [ ] Note mute command.
- [ ] Note split command.
- [ ] Note duplicate command.
- [ ] Quantize batch command.
- [ ] CC point create command.
- [ ] CC point delete command.
- [ ] CC point move command.
- [ ] CC ramp command.
- [ ] Batch drag gestures into one undo entry.

## MIDI Playback

- [ ] Convert notes to runtime note on/off events.
- [ ] Skip muted notes.
- [ ] Skip muted clips/tracks.
- [ ] Respect clip bounds.
- [ ] Handle looped clip playback once clip loops exist.
- [ ] Send notes to instrument track plugin.
- [ ] Add external MIDI output placeholder.
- [ ] Convert CC points to runtime MIDI controller events.
- [ ] Initial block-level scheduling documented.
- [ ] Sample-accurate scheduling planned.
- [ ] No audio-thread allocation.

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

