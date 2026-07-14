# Export Stems / Multitrack handoff

## Goal

Finish and validate Futureboard's native Export Arrangement flow, especially a true single-pass Stem/Multitrack render that includes VSTi multi-output channels, buses, and returns without producing silent files.

## Current state

- Mixer/UI interaction fixes were committed earlier as `5de84d5d Fix mixer routing and native UI interactions`.
- The current commit adds the export dialog modes and the engine-side single-pass channel-tap renderer.
- Mixdown uses one offline graph pass and one encoder.
- Stem/Multitrack uses one offline graph/timeline pass, captures all requested mixer-channel taps, and streams each tap to a separate encoder concurrently.
- Stem mode targets every non-master mixer channel, including normal tracks, Bus, Return, and VSTi multi-out child strips.
- Multitrack targets normal source tracks plus VSTi multi-out child strips; ordinary Bus/Return tracks are excluded.
- Batch normalization is intentionally disabled because independent peak normalization would require an analysis/storage pass and violate the current single-pass contract.

## Silence investigation and bridge changes

The first single-pass implementation generated valid WAV files containing silence for Addictive Drums 2. Root cause: `render_offline` built a fresh `RuntimeProject` with no external `PluginBridgeSink`; bridged VST/VSTi inserts therefore skipped DSP.

The current implementation:

1. Keeps a control-side registry of live plugin bridge sinks in `EngineInner`.
2. Captures that registry when the Export window opens.
3. Temporarily removes those sinks from the realtime engine when export starts, waits 25 ms for the command to cross to the callback, and restores them after success/error/cancellation.
4. Installs export-only wrappers in the offline runtime.
5. Makes those wrappers wait for the external host's fresh output block (up to 5 seconds) so an offline worker cannot outrun the asynchronous bridge producer.
6. Preserves the realtime path as wait-free; blocking occurs only in the export worker.

Important: the user's latest screenshot still shows imported clips with no visible waveform. It is not confirmed from an actual Addictive Drums export that this bridge handoff now produces non-zero samples. Treat end-to-end VSTi validation as the first task, not as finished work.

## Main files

- `crates/SphereDirectAudioEngine/src/engine/render.rs`
  - `render_project_block_interleaved_with_taps`
  - captures post-insert/post-fader track blocks in runtime-track order.
- `crates/SphereDirectAudioEngine/src/export/offline_renderer.rs`
  - bridge-aware render entry points.
  - `OfflineBridgeSink` blocking export adapter.
  - installs live bridge endpoints into the offline runtime.
- `crates/SphereDirectAudioEngine/src/export/exporter.rs`
  - `export_tracks_single_pass_with_bridges`.
  - opens all encoders, renders once, writes per-target taps, and calculates per-file peaks.
- `crates/SphereDirectAudioEngine/src/engine.rs`
  - control-side plugin bridge sink registry and getter.
- `crates/SphereUIComponents/src/export/export_window.rs`
  - polished dialog, Mixdown/Stem/Multitrack modes, worker ownership transfer, progress UI.
- `crates/SphereUIComponents/src/layout/export_ops.rs`
  - builds the export snapshot/targets and passes live bridge handles plus the audio engine.

## Validation already run

- `cargo check -p sphere_directaudioengine`
- `cargo check -p sphere_ui_components`
- `cargo test -p sphere_directaudioengine export` — 15 passed
- `cargo test -p sphere_ui_components export::tests` — 13 passed
- `cargo fmt`
- `git diff --check`

The automated single-pass test uses a silent synthetic snapshot and verifies multiple WAV outputs and frame counts. It does not prove external VST/VSTi audio is non-zero.

## Recommended next steps

1. Export a short MIDI region from Addictive Drums 2 and inspect sample peaks of every WAV, not only waveform thumbnails.
2. Enable `FUTUREBOARD_PLUGIN_BRIDGE_DEBUG=1` and plugin restore/debug logging if files remain silent. Confirm:
   - the captured bridge map contains the Addictive Drums insert ID;
   - the offline runtime resolves that same ID onto `RuntimeInsert.bridge_sink`;
   - `request_seq` and `done_seq` advance for every block;
   - `read_output_multichannel` returns non-zero frames/channels;
   - `scratch_multi`, child `recv_l/recv_r`, processed child `block_l/block_r`, and export taps have non-zero peaks.
3. ~~Replace the 25 ms ownership-transfer delay with an acknowledged engine command/barrier.~~ DONE (2026-07-15): `EngineCommand::CommandBarrier` acked by both callback drain loops (legacy cpal + DAUx); `wait_for_command_barrier(500ms)` in `BridgeSinkHandoff::detach`, with the 25 ms sleep kept only as the timeout fallback (no open stream / stalled callback).
4. ~~Make the blocking bridge wait cancellation-aware.~~ DONE: `OfflineBridgeSink` carries the `ExportCancelToken`; a cancelled export aborts a pending bridge wait immediately instead of stalling out the 5 s per-read deadline.
5. ~~Add a deterministic non-silent fake `PluginBridgeSink` integration test.~~ DONE: `single_pass_bridged_multiout_stems_are_not_silent` in `export/exporter.rs` — freshness-guarded 4-channel constant sink, parent instrument + two `vsti-out:` child bus strips through `export_tracks_single_pass_with_bridges`; asserts child A peaks at plugin channels 1/2 and child B at 3/4, parent silent (no fallback downmix).
6. Verify bridge latency alignment. The bridge is one block pipelined; warmup/PDC should discard that latency, but this needs a known impulse test.
7. ~~Check error cleanup when one of several encoder finalizations fails.~~ DONE: both the encoder-open loop and the finalize/rename loop in `export_tracks_single_pass_with_bridges` now close remaining encoder handles and remove every not-yet-finalized `.partial` on error (already-renamed outputs are kept — they are complete stems). Live sink restore moved into the `BridgeSinkHandoff` Drop guard, so a worker panic can no longer leave bridged inserts detached/silent.

Steps 1, 2 (manual Addictive Drums validation) and 6 (impulse latency test) remain open.

## Constraints

- Follow `AGENTS.md` -> `CLAUDE.md` and `tasks/SKILL.md`.
- Do not make the realtime callback block or allocate.
- Keep Stem/Multitrack as a single graph/timeline render pass; do not revert to solo-and-render loops.
- GPUI child callbacks must not synchronously update parent entities.
