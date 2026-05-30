# MIDI Phase 2 — DAUx Engine MIDI Playback / Scheduling

## PART A — findings (WebUI / existing engine reference)

### WebUI / WASM MIDI playback
The WebUI (`apps/web`) does **not** have a real MIDI instrument playback path.
Transport (`apps/web/src/engine/Transport.ts`) drives a `ClipScheduler` and a
`MetronomeScheduler` over WebAudio. `ClipScheduler` bulk-schedules **audio**
clips on `play()`; there is no MIDI note scheduler, no instrument host, and the
`MidiEditorPanel` only edits clip note data — it never sounds. So WebUI is a
reference for the **MIDI clip/note model** (pitch / start-beat / length-beat /
velocity, notes stored relative to clip start) but **not** for engine playback.

### Existing native engine (DAUx = `sphere-direct-audio-engine`)
- `EngineProjectSnapshot` (types.rs) carried only audio `clips`; MIDI was absent.
- `RuntimeProject::build` (runtime.rs) converts snapshot → runtime on the
  control/worker thread (decodes audio, builds VST3 processors). Audio clips
  resolve **beats → samples at build time** using the snapshot BPM
  (`build_clip_runtime`): `start_sample = round(start_beat / bps * sr)`.
- The audio callback (engine.rs) owns a local `RuntimeProject`, advances
  `shared.position_samples` by `frames` each block, and renders via
  `render_project_block_interleaved(runtime, base_sample, ...)`.
- Transport commands (`StartTransport`/`StopTransport`/`Seek`) are handled
  inside the callback command drain.
- VST3 C-ABI bridge (`vst3_processor.rs`) exposes audio process + param changes
  **only** — there is **no event/IEventList input** function.

### Timing units / assumptions Native mirrors
- Constant tempo. `samples_per_beat = sample_rate * 60 / bpm`.
- Notes are clip-relative; absolute beat = `clip.start_beat + note.start_beat`.
- To match audio clips (and stay lock-free in the callback) MIDI events are
  **resolved to absolute project samples at build time**, not re-derived from
  beats every block.
- Block-accurate scheduling (events placed at a sample offset within the block)
  — sample-accurate within one block is the offset granularity.

## What was implemented (Phase 2A — scheduler + routing scaffold)

- **Snapshot** (types.rs): `EngineMidiClipSnapshot` / `EngineMidiNoteSnapshot`;
  `EngineProjectSnapshot.midi_clips` (serde `default`). UI fills it in
  `build_engine_project_snapshot` (layout.rs) — note edits now change the
  snapshot signature, so a committed edit triggers a rebuild (no live-drag spam;
  Phase 1 already commits once on release).
- **Runtime** (runtime.rs): `RuntimeMidiEvent` (precomputed `sample` + `beat`),
  `RuntimeMidiClip`, `RuntimeMidiTrack` (merged sorted events + `cursor` +
  `active` notes). `build_midi_runtime` clamps pitch/velocity/channel, skips
  length ≤ 0, sorts by sample with **NoteOff before NoteOn** at the same sample.
- **Callback** (engine.rs): `schedule_midi_block(base_sample, frames)` runs once
  per block, advances the cursor, emits note on/off (debug-logged) with a
  within-block sample offset, and tracks active notes. `reset_midi_playback`
  (binary-search cursor + flush) on Seek / StartTransport / LoadProject;
  `all_notes_off` on StopTransport. No allocation on the steady path; `active`
  is reserved (128) at build.
- **Debug**: `FUTUREBOARD_MIDI_ENGINE_DEBUG=1` (engine) +
  `FUTUREBOARD_MIDI_DEBUG=1` (UI, Phase 1). Logs snapshot/runtime counts, block
  beat range, per-event note on/off, all-notes-off.
- **Tests**: `runtime.rs#midi_tests` (6) prove absolute-sample resolution,
  off-before-on, zero-length skip, on/off firing + active tracking, seek-before
  fires, seek-after does not, all-notes-off clears.

## TODO (Phase 2B and beyond)
- **VST3 instrument event input**: the C++ bridge has no IEventList input. Add a
  minimal C-ABI (`sphere_daux_vst3_send_note_on/off` filling `processData.
  inputEvents` for the next process call), then route `RuntimeMidiEvent` →
  instrument insert at `schedule_midi_block`'s marked hook. Audio effects keep
  working (additive API).
- **Instrument track routing**: MIDI track / instrument insert → audio out.
- **Tempo automation**: scheduling is constant-BPM; events are sample-resolved
  at build time. Tempo changes currently require a project rebuild.
- **Loop**: re-arm cursors / flush at loop boundaries when transport loop lands.
- Optional built-in test synth so notes are audible before a VST3 instrument is
  loaded.
