# Futureboard Audio System Checklist

Planning status: checklist only. No implementation code is implied.

## Realtime Safety

Status notes from Phase B (see `audio-system-phase-a-audit.md`):

- [~] Audio callback never allocates. — Retired graphs now dropped off-thread
  via `graveyard.rs` (was A.2.1). Remaining: lazy buffer `resize` on oversized
  blocks (A.2.3) and per-sample `String`/`Vec` clones in the non-stereo
  fallback path (A.2.4). Not fully verified by an allocator guard yet.
- [x] Audio callback never blocks on locks. — atomics + `try_recv` only.
- [x] Audio callback never opens files. — sources pre-decoded/mmapped on the
      control thread; callback only reads prepared buffers.
- [x] Audio callback never scans plugins.
- [x] Audio callback never calls UI.
- [x] Audio callback never logs per block. — all callback `eprintln!` now gated
      behind `FUTUREBOARD_AUDIO_CALLBACK_DEBUG` (was A.2.2); steady state is silent.
- [x] Audio callback never rebuilds project graph. — built on control thread,
      callback only swaps a prepared `RuntimeProject`.
- [x] Audio callback reads immutable runtime snapshot only. — graph is built and
      published by the control thread; the callback owns its copy.
- [x] Graph swaps happen at safe block boundaries. — applied at the top of the
      block in the command drain.
- [x] Failed graph build preserves current running graph. — build runs on the
      control thread and only a valid `RuntimeProject` is published; the running
      graph keeps playing until the swap.
- [~] Debug output is ring-buffered/throttled. — callback debug is now gated
  off by default (throttled to zero). A true ring buffer for the
  `*_CALLBACK_DEBUG=1` case (plan §18) is still TODO.

## Device Backend

Windows-first pass. Verified/implemented in `backend/`, `device/mod.rs`,
`engine.rs` (`open_daux`/`open_daux_safe`/`get_daux_status`/`recover_daux`).

- [x] Enumerate audio backends. — `list_available_backends` / `listDauxBackends`.
- [x] Enumerate input devices. — `device::list_input_devices` (cpal).
- [x] Enumerate output devices. — `device::list_output_devices` (cpal).
- [x] WASAPI Shared support. — cpal Auto on Windows (event-driven).
- [x] WASAPI Exclusive support. — `backend/wasapi_exclusive.rs` (+ MMCSS).
- [x] CoreAudio support plan. — cpal Auto = CoreAudio on macOS; in the backend
      abstraction + backend list.
- [~] PipeWire support plan. — works via the ALSA/PipeWire compat layer; a
  native PipeWire backend is not wired (documented future).
- [~] JACK support plan. — cpal has a JACK feature (not enabled); future.
- [x] ALSA fallback plan. — cpal Auto = ALSA on Linux; listed as a backend.
- [x] Set sample rate. — `JsDauxConfig.sample_rate` → controlled reopen.
- [x] Set buffer size. — `JsDauxConfig.buffer_size` (safe-mode floor).
- [x] Restart device safely. — `open_daux` (close→open) / `open_daux_safe`
      (restores previous working config on failure).
- [x] Test output. — `set_test_tone`.
- [x] Test input. — **added**: `start_input_test`/`get_input_test_level`/
      `stop_input_test` open a capture stream (via `recording::find_input_device`)
      whose callback tracks the running peak; the UI polls a `0.0..=1.0` level and
      stops it. Independent of recording/output. Exposed on NAPI + native facade.
- [x] Recover from device loss. — **added**: backends set `SharedState.device_lost`
      on mid-stream loss (cpal `DeviceNotAvailable`; WASAPI-exclusive abnormal exit);
      surfaced as `getDauxStatus().deviceLost` + `deviceState`; `recoverDaux()`
      reopens with the last-known-good config (also on the native facade as
      `recover_device`).
- [~] Persist device settings. — the engine keeps the active `daux_config` in
  memory; durable persistence lives in the settings/UI layer (`SphereSettings`).
- [x] Mark restart-required settings. — `daux_requires_restart(config)` /
      `dauxRequiresRestart` reports whether a change (backend/device/sample rate/
      buffer/MMCSS/safe-mode) needs a controlled restart, for the Settings UI.

## Runtime Snapshot

- [ ] Define runtime project snapshot.
- [x] Define runtime transport snapshot. — `transport::RuntimeTransportSnapshot`
      built from shared atomics + tempo map; exposed via `transport_snapshot()`,
      `EngineStats`, and `getDebugInfo().positionBeats`.
- [x] Define runtime tempo map snapshot. — `tempo_map::RuntimeTempoMapSnapshot`
      with step-hold segments; `TempoMap` provides `tempo_at_beat`,
      `seconds_at_beat`, `beat_at_seconds` (unit-tested).
- [ ] Define runtime track snapshot.
- [ ] Define runtime clip snapshot.
- [x] Define runtime routing graph. — `RuntimeAudioGraph` on `RuntimeProject`.
- [ ] Define runtime automation snapshot.
- [ ] Define runtime media/source handles.
- [ ] Resolve project string IDs to runtime handles.
- [ ] Sort clips/events for playback.
- [x] Validate routing before swap. — `RuntimeProject::build` runs
      `plan_runtime_audio_graph`; cycles reject `load_project` without swapping.
- [x] Build snapshots off audio callback. — graph plan built on control thread
      in `RuntimeProject::build`.
- [x] Atomic/safe snapshot swap. — unchanged `LoadProject` command path.

## Audio Graph

- [x] Audio input node. — `AudioInput` kind defined; capture stays on the
      recording path (not yet a mix-graph node instance).
- [x] Audio clip node. — `AudioClip` kind defined; Pass-1 clip render unchanged
      (per-clip graph nodes deferred).
- [x] MIDI clip node. — `MidiClip` kind defined; MIDI scheduling unchanged.
- [x] Instrument node. — `Instrument` track mixer kind.
- [x] Insert plugin node. — per-insert nodes in `plan_runtime_audio_graph`.
- [x] Track mixer node. — default mixer kind for audio/midi tracks.
- [x] Send node. — per-send nodes on source tracks.
- [x] Return track node. — `ReturnTrack` mixer kind.
- [x] Bus track node. — `BusTrack` mixer kind.
- [x] Group track node. — `GroupTrack` mixer kind + Pass-2 routing.
- [x] Master node. — `Master` kind + `master_index` in `RuntimeAudioGraph`.
- [x] Output node. — `master-output` sink node after master strip.
- [x] Meter node. — per-track meter nodes in the plan.
- [x] Topological sort. — `pass2_routing_indices` from Kahn toposort.
- [x] Cycle detection. — DFS in `plan_runtime_audio_graph`; fails `load_project`.
- [x] Invalid route UI feedback. — `get_debug_info` exposes rejected route
      count + summaries; `load_project` returns `InvalidRoutingGraph` on cycles.
- [x] Stereo-first graph. — plan mirrors existing stereo Pass-1/Pass-2 render.
- [ ] Hardware output target later.

## Audio Clips and Regions

Status notes (engine = `SphereDirectAudioEngine`):

- [x] WAV clip playback. — `render_project_block_interleaved` + `audio_source`
      (in-memory decode + mmap streaming for large WAV).
- [x] Source handle model. — `RuntimeClip.source: Arc<ClipAudioSource>`,
      resolved/decoded on the control thread, shared by handle into the runtime.
- [x] Clip start beat. — `start_beat` → `start_sample` at build time.
- [x] Clip duration beats. — `duration_beats` → `duration_samples`.
- [x] Source offset. — `offset_seconds` applied to source read position.
- [x] Clip gain. — `gain` applied per sample in both render paths.
- [x] Fade-in placeholder. — `fade_in_samples` resolved from snapshot, applied
      as a linear ramp via `clip_fade_gain`. Curve shaping beyond linear is TODO.
- [x] Fade-out placeholder. — `fade_out_samples`, linear ramp via the same fn.
- [x] Clip mute. — new `EngineClipSnapshot.muted` → `RuntimeClip.muted`; muted
      clips are skipped in both render paths (distinct from track mute).
- [~] Clip missing media state. — clips with empty path / decode failure are
  skipped (logged) and render silence; the control/UI layer owns the clip's
  missing state. A distinct engine-surfaced missing/failed count is still TODO.
- [x] Clip scheduling by beat. — beat→sample resolution at build time.
- [ ] Clip scheduling with tempo map later. — still constant-tempo
      (`samples_per_beat` scalar); tempo map is Phase T.
- [x] Non-destructive editing. — gain / offset / speed / fades / mute never
      mutate the decoded source; all live in the snapshot/runtime overlay.
- [x] Project save/load. — engine rebuilds runtime from `EngineProjectSnapshot`
      (`load_project`); the new `muted` field is `#[serde(default)]` so older
      snapshots still load. Save side lives in the project/UI layer.

## Disk Streaming and Decode

Status notes (engine = `SphereDirectAudioEngine`; verified by reading
`audio_file.rs` / `audio_source.rs` / `engine.rs`). Decode + the explicit
prefetch/ring-buffer streaming architecture (Phase F) are NOT yet built — see
the deferred items below; they are not faked here.

- [~] Media import worker. — decode runs off the audio thread in
  `RuntimeProject::build` (control thread, `load_project`); a dedicated
  worker-pool is the caller's (Electron / GPUI background executor)
  responsibility, as documented on `AudioEngine::generate_peaks`. No separate
  in-crate worker thread yet.
- [~] Metadata probe worker. — `probe_audio_file` exists and runs off-callback;
  threading it onto a worker is caller-side.
- [x] WAV import. — inline RIFF parser `load_wav` (+ symphonia fallback).
- [x] AIFF import. — symphonia (`aiff` / `aif`).
- [x] FLAC import. — symphonia.
- [x] MP3 decode worker/cache. — symphonia decode off-callback; small files
      cached in `EngineInner.audio_cache` keyed by path; oversized files now stream
      through the Phase F decoder thread instead of erroring.
- [x] OGG later. — already decodes via symphonia (`ogg` / `oga`) for files
      under `MAX_IN_MEMORY_DECODE_BYTES`; same large-file caveat as MP3/FLAC.
- [ ] CAF later. — not wired (would need the symphonia CAF/ALAC path); later.
- [x] Disk prefetch worker. — `streaming_source.rs` spawns a
      `daux-stream-decoder` thread per large compressed clip that decodes ahead of
      the playhead and reseeks on transport seek. (Thread-per-clip is fine for the
      rare large-compressed case; a shared worker pool is a later optimisation.)
- [x] Ring buffer per active source/clip. — `StreamingRing`: a bounded,
      preallocated stereo ring (`AtomicU32` per sample → no data-race UB) filled by
      the worker and read lock-free by the callback. Large WAV continues to stream
      via `MappedWavSource` (mmap, spec §7 "WAV may stream directly").
- [x] Underrun detection. — out-of-window reads bump a per-ring + process-wide
      counter (`total_disk_underruns`), surfaced via `getDebugInfo().diskUnderruns`.
- [x] Silence fallback on underrun/missing media. — missing/empty/out-of-range
      sources return silence (`sample_source_stereo`); clips with no resolvable
      media are skipped at build and render silent.
- [~] No file IO in callback. — no explicit open/read/seek in the callback
  (decode + probe happen on the control thread). Caveat: `MappedWavSource`
  demand-pages mapped WAV bytes, so a page fault can block on disk inside the
  callback; Phase F prefetch/lock will close this.
- [~] Project media folder policy. — recording writes to
  `<projectRoot>/Media/Audio` (`recording.rs`); import-folder + cache-dir layout
  (plan §14) largely lives in the project/UI layer, not this crate.
- [x] Cache cleanup policy. — reference-based eviction: `audio_cache.retain`
      drops sources no longer referenced by any clip on each `load_project`. A
      disk decode-cache cleanup policy is part of the Phase F decode-cache work.

### Phase F — streaming status

Implemented in `streaming_source.rs` (`StreamingSource` + `StreamingRing`),
wired via `ClipAudioSource::Streaming` and enabled for compressed files larger
than `MAX_IN_MEMORY_DECODE_BYTES` (previously hard-errored):

1. ✅ Bounded preallocated ring buffer per streaming clip.
2. ✅ Background decoder thread prefetching ahead of the playhead, with
   accurate-seek reposition on transport seek.
3. ✅ Underrun counter + silence fallback, surfaced via `getDebugInfo`.

Decode-to-WAV-cache became unnecessary — compressed files stream directly. The
chosen cache dir (`<projectRoot>/Cache/Audio`) is reserved for a future
peak/analysis cache rather than a PCM decode cache.

Remaining follow-ups (separate slices, not blocking):

- Shared decoder-thread pool instead of one thread per streaming clip.
- Lower the streaming threshold so mid-size compressed files also stream
  (currently they still decode fully in memory).
- Prefetch/lock mapped-WAV pages so `MappedWavSource` cannot page-fault on the
  audio thread (the remaining "No file IO in callback" caveat).
- Real-device stress test (3-hour file, repeated seeks) — ring logic is
  unit-tested, but end-to-end glitch behaviour is not yet measured on hardware.

## Waveform/Peak Cache

Status notes — already implemented in the timeline UI layer
(`crates/SphereUIComponents/src/components/timeline/`), which consumes
`DirectAudio::`. Verified by reading `audio_import.rs`,
`waveform_cache.rs`, `waveform_canvas.rs`.

- [x] Peak worker. — `audio_import::run_import_pipeline` runs probe + peak
      generation on `cx.background_executor().spawn`, never the render thread; UI
      refresh throttled to ≤10 Hz.
- [x] Chunked peak files. — in-memory chunked storage (`CHUNK_PEAKS = 4096`,
      keyed by `(spp, chunk_index)`) plus a persistent on-disk cache
      (`<cache_dir>/futureboard/Peaks/<key>.peaks.json`).
- [x] Multiple LODs. — `LOD_LEVELS` 256…65536 (mirrors `DirectAudio::`);
      `pick_best_samples_per_peak` selects the LOD for the zoom level.
- [x] Cache key by file hash + modified time. — `stable_cache_key` = crc32 over
      path + file length + mtime + target sample rate + `WAVEFORM_ALGORITHM_VERSION`;
      a source-file edit changes the key (invalidation) and the version bump
      invalidates the whole disk cache.
- [x] Pending state. — `WaveformDisplayStatus::Pending` / `AudioImportState`.
- [x] Partial state. — `WaveformDisplayStatus::Partial { chunks_ready,
chunks_total }`, driven by progressive chunk install.
- [x] Ready state. — `WaveformDisplayStatus::Ready { meta }`.
- [x] Missing/failed state. — `WaveformDisplayStatus::Error` /
      `AudioImportState::Failed` (e.g. metadata-read or decode failure).
- [x] Visible-only waveform drawing. — `waveform_canvas` clamps to
      `visible_start`/`visible_end` (viewport) and emits one bar per visible pixel
      column.
- [x] No peak generation in render. — render reads cached peaks via
      `aggregate_peak_range_in_entry`; the module is documented "render path is
      read-only" and all generation is on the background executor.
- [ ] Large file waveform stress test. — manual QA item (also tracked under
      Performance and QA). Not yet exercised on hardware; the streaming WAV peak
      scan (`generate_wav_peaks_streaming`) is built for it but unmeasured.

## Mixer

Engine core verified in `engine.rs`/`runtime.rs`/`render.rs`; meter display in
the UI (`audio_transport.rs` poll, `vu_meter.rs`, `mixer_panel.rs`).

- [x] Track volume. — `SetTrackVolume` → `apply_fader_and_sum` /
      `apply_track_chain_at_beat`.
- [x] Track pan. — `SetTrackPan` → `pan_gains`.
- [x] Track mute. — `SetTrackMute` + `effective_track_muted` (solo/automation aware).
- [x] Track solo. — `SetTrackSolo` + `has_solo` gating.
- [x] Track arm. — `EngineTrackSnapshot.armed` + recording config
      (`JsRecordingTrackConfig`); arm has no effect on playback render.
- [~] Track input monitor. — `InputMonitorMode` (Off/Auto/Input) wired; software
  monitor mixes live input to master during **active recording** only. Full
  playback-time monitor through the track chain is still TODO.
- [x] Master fader. — atomic `master_volume`, applied with soft-limit at output.
- [x] Pan law. — defined + consistent (linear/balance, unity center), used by
      both the fader path and pan automation via `pan_gains`. The spec's suggested
      equal-power law is an available alternative (mix-affecting; not switched
      unilaterally — would drop centered tracks ≈3 dB).
- [x] Per-track buffers. — preallocated `RuntimeTrack.block_l/block_r`.
- [x] Send buffers. — preallocated `recv_l/recv_r`.
- [x] Bus accumulation buffers. — routing tracks process their `recv_*` as input
      (Pass 2 in `render_project_block_interleaved`).
- [x] Master buffer. — summed output → master scratch → master inserts → output.
- [x] Peak meters. — per-track + master peak/RMS via atomics; UI polls.
- [x] Meter decay. — UI `smooth_meter_value` (asymmetric attack/release);
      master peak additionally smoothed in the callback (`smooth_peak`).
- [x] Peak hold. — **added**: `meter_peak_hold_l/r` on track + master, updated by
      `update_meter_hold` (instant attack, slow release), drawn as a bright tick in
      `vu_meter::meter_surface`.
- [x] Clip indicator. — **added**: `meter_clip` latches when the raw (pre-clamp)
      engine peak reaches 0 dBFS, auto-clears once the held peak falls back; drawn as
      a red cap atop the meter (`update_meter_clip` + `paint_clip_cap`).
- [x] Meter update throttling. — poll throttled to `PowerMode::meter_update_hz`
      (15/30/60 Hz) in `apply_engine_meters`.
- [x] No full-app repaint from meters. — poll notifies only the timeline entity;
      meters are single GPU-composited `canvas` quads, not a div tree per segment.

## Routing

Verified/implemented in `engine.rs` (`render_project_block_interleaved` Pass 1/2,
`route_main_output`, `accumulate_sends`).

- [x] Track output to master. — default when `output_track_id` is empty/master.
- [x] Track output to bus. — **fixed**: the primary block path now honours
      `output_track_id` via `route_main_output` (post-fader signal summed into the
      target routing track's receive buffer). Previously only the per-sample
      fallback handled it; the block path always went to master.
- [x] Bus to master. — Pass 2 processes routing tracks → master output.
- [x] Bus inserts. — routing tracks run their insert chain in `process_track_block`.
- [x] Return track. — return is a routing track that receives sends.
- [x] Send to return. — `accumulate_sends`.
- [x] Send gain. — `RuntimeSend.level`.
- [x] Post-fader sends. — `accumulate_sends(.., pre_fader = false)` after fader.
- [x] Pre-fader sends later. — already done: `pre_fader` tap before fader.
- [ ] Group track later. — group tracks route in Pass-2 via `RuntimeAudioGraph`;
      UI/group-folder semantics may still evolve.
- [x] Reject track -> bus -> same track cycle. — sends/output only accept
      routing-type targets; a routing target can't loop back to a source track.
- [x] Reject return self-send. — `accumulate_sends`/`route_main_output` reject
      `target == src_index`.
- [x] Reject master back-routing. — master is only ever summed into; it never
      sends, and `is_master_output` short-circuits output routing.
- [x] Reject bus cycle. — `plan_runtime_audio_graph` DFS cycle detection rejects
      cyclic bus/return/group graphs at load time; Pass-2 uses topological order.

## Plugin Hosting

- [ ] VST3 scan.
- [ ] VST3 descriptor cache.
- [ ] VST3 effect instantiate.
- [ ] VST3 instrument instantiate.
- [ ] Plugin insert load.
- [ ] Plugin insert bypass.
- [ ] Plugin insert remove.
- [ ] Plugin process audio.
- [ ] Plugin latency query.
- [ ] Plugin parameter discovery.
- [ ] Stable parameter IDs.
- [ ] Normalized parameter values.
- [ ] Parameter display text.
- [ ] Save plugin state.
- [ ] Load plugin state.
- [ ] Failed plugin fallback state.
- [ ] Bad plugin does not crash main app.
- [ ] CLAP plan.
- [ ] AU plan.
- [ ] LV2 plan.

## Plugin Host Architecture

- [ ] Pure `SpherePluginHost` core API.
- [ ] Thin N-API wrapper for Electron only.
- [ ] Native GPUI links pure core API.
- [ ] No N-API symbols in native binary.
- [ ] Separate plugin scanner process.
- [ ] Scanner timeout.
- [ ] Scanner crash blacklist.
- [ ] Plugin index cache.
- [ ] Plugin editor processing separated from audio processing.

## Plugin Editor

- [ ] GPUI `PluginView` shell.
- [ ] Native child HWND on Windows.
- [ ] Native child NSView on macOS.
- [ ] Linux plugin editor host plan.
- [ ] VST3 `IPlugView` attach.
- [ ] Resize handling.
- [ ] Remove/detach handling.
- [ ] Close/reopen editor.
- [ ] Fallback error UI.
- [ ] Editor focus handling.
- [ ] Editor does not block audio.

## MIDI and Instruments

- [ ] MIDI event runtime model.
- [ ] MIDI clip note on/off scheduling.
- [ ] MIDI CC scheduling.
- [ ] Pitch bend scheduling.
- [ ] Channel pressure scheduling.
- [ ] Block-accurate scheduling documented.
- [ ] Sample-accurate scheduling planned.
- [ ] MIDI panic/all notes off.
- [ ] Instrument track runtime.
- [ ] MIDI to VST3 instrument.
- [ ] Instrument audio output into track chain.
- [ ] MIDI input devices.
- [ ] Arm MIDI track.
- [ ] Monitor MIDI input.
- [ ] MIDI recording.

## Automation Integration

- [ ] Track volume read automation.
- [ ] Track pan read automation.
- [ ] Send gain read automation.
- [ ] Insert bypass automation semantics.
- [ ] Plugin parameter read automation.
- [ ] Master volume automation.
- [ ] Master plugin parameter automation.
- [ ] Tempo automation model.
- [ ] Time signature placeholder.
- [ ] Precompiled runtime automation lanes.
- [ ] No automation allocation in callback.
- [ ] Smoothing for continuous params.
- [ ] Discrete target hold behavior.

## Transport and Clock

- [x] Play. — `StartTransport` / `play()`.
- [x] Stop. — `StopTransport` + UI stop (pause transport, keep position).
- [x] Pause. — same as stop-transport; audio stream stays open.
- [x] Record. — native transport record toggles DAUx `start_recording` /
      `stop_recording`; creates clips + waveforms on stop.
- [x] Loop. — `SetLoop` command + block-boundary wrap in callback; UI syncs
      loop region beats → seconds.
- [x] Seek. — `Seek { position_seconds }` at block boundary.
- [x] Rewind/forward. — bar nudge commands in transport chrome.
- [x] Go start/end. — `transport:go-to-start` / `transport:go-to-end`.
- [x] Follow playhead. — timeline `follow_playhead` + auto-scroll.
- [x] Auto-scroll UI-only. — does not reload engine.
- [x] Metronome. — generated clicks in callback; `setMetronomeEnabled`.
- [ ] Count-in. — later.
- [ ] Pre-roll. — later.
- [x] Sample position. — `position_samples` atomic.
- [x] Beat position. — `RuntimeTransportSnapshot.position_beats` via tempo map.
- [x] Seconds position. — `position_seconds` in stats/debug.
- [x] Static tempo. — `SetBpm` + project snapshot BPM.
- [x] Tempo map API. — `TempoMap` step-hold segments + conversion fns
      (full tempo-map playback still deferred).
- [x] Time signature. — `SetTimeSignature` for metronome accent + snapshot.
- [x] Seek applies at block boundary. — command drain runs before render.

## Recording

- [x] Arm audio track. — track header / mixer `R` button + `EngineTrackSnapshot.armed`.
- [x] Select input device/channel. — Settings `device_in` resolved to DAUx input
      device id; per-track mono/stereo or `AudioDeviceChannel` maps to
      `input_channels`.
- [x] Monitor off/auto/input. — `InputMonitorMode` cycles Off → Auto → Input;
      software monitor mix onto master during active recording when enabled.
- [x] Record WAV. — `recording.rs` cpal input → disk writer → float32 WAV in
      `<projectRoot>/Media/Audio`.
- [x] Finalize file safely. — temp file in `.rec/<session>/`, header patched,
      atomic rename on stop.
- [x] Create audio clip after recording. — native UI `commit_recording_results`
      inserts clip at `start_beat` with measured duration.
- [x] Generate waveform after recording. — reuses `spawn_timeline_audio_import_jobs`.
- [ ] Record MIDI clip. — **deferred**; no MIDI input capture in engine yet.
- [ ] MIDI overdub later.
- [ ] Punch in/out later.
- [ ] Manual recording offset.
- [ ] Loopback latency measurement later.

## Latency

Phase V (reporting) and Phase W (playback PDC + recording offset) are implemented
in `latency_graph.rs` + render-path delay lines.

- [~] Device input latency. — cpal does not expose driver-reported input
  latency; the buffer figure is the available proxy. Driver-reported value TODO.
- [~] Device output latency. — same: estimated from buffer size, not the
  driver's reported output latency.
- [x] Buffer latency. — `buffer_frames`/`buffer_ms` in `getLatencyInfo` and
      `getDauxStatus().estimatedLatencyMs`.
- [x] Plugin latency query. — `Vst3RuntimeProcessor::get_latency_samples` (C++
      `getLatencySamples`), summed per track for enabled native-plugin inserts.
- [x] Track latency display. — per-track `plugin_samples`/`plugin_ms` plus
      `path_samples` / `pdc_delay_samples` from `RuntimeLatencyGraph`.
- [x] Master latency display. — `master_samples`/`master_ms`.
- [x] Latency graph. — `plan_runtime_latency_graph` propagates send/return/bus
      path latencies; `max_path_samples` in `getLatencyInfo`.
- [x] Playback delay compensation. — per-track ring-buffer delay after fader,
      before sends/main route (`apply_pdc_delay_block`); disable with `FUTUREBOARD_PDC=0`.
- [x] Send/return latency handling. — return `output_latency` includes max
      send feed latency; PDC aligns source/return paths to master.
- [x] Recording offset compensation. — `stop_recording` shifts `start_beat` by
      round-trip buffer + monitored path latency (manual offset still TODO).
- [x] Automation alignment. — automation evaluates at transport beat on the
      undelayed fader (correct UX); audible output is PDC-aligned.
- [x] Meter alignment. — track meters accumulate post-PDC `block_*` samples.

## Offline Render / Export

- [ ] Master WAV export.
- [ ] Selected range export.
- [ ] FLAC export.
- [ ] AIFF export.
- [ ] MP3 export later.
- [ ] Bit depth selection.
- [ ] Sample-rate conversion.
- [ ] Dithering.
- [ ] Offline plugin mode if supported.
- [ ] Progress reporting.
- [ ] Cancel support.
- [ ] Stem export later.
- [ ] Bus stem export later.

## Settings

- [ ] Audio backend setting.
- [ ] Input device setting.
- [ ] Output device setting.
- [ ] Sample-rate setting.
- [ ] Buffer-size setting.
- [ ] Monitor mode setting.
- [ ] Recording format.
- [ ] Recording bit depth.
- [x] Latency compensation setting. — Settings → Playback → Plugin Delay
      Compensation; persisted in `playback.latency_compensation`, synced to DAUx PDC.
- [ ] Performance renderer setting.
- [ ] Waveform quality setting.
- [ ] Meter update rate setting.
- [ ] Disk streaming cache size.
- [ ] Worker thread count.
- [ ] Low-end GPU mode.
- [ ] Plugin scan paths.
- [ ] Plugin formats enabled.
- [ ] Scan on startup.
- [ ] Clear plugin cache.

## Diagnostics and Recovery

- [ ] Audio status UI.
- [ ] Device error UI.
- [ ] Recovering state UI.
- [ ] Device lost recovery.
- [ ] XRun/dropout counter.
- [ ] CPU load.
- [ ] Graph node count.
- [ ] Plugin count.
- [ ] Disk stream status.
- [ ] Peak cache status.
- [ ] Safe mode.
- [ ] Plugin blacklist.
- [ ] Crash recovery plan.
- [ ] Debug env flags documented.

## Performance and QA

- [ ] 32 tracks usable.
- [ ] 100 tracks empty stress.
- [ ] 1000 clips stress.
- [ ] 3-hour audio file stress.
- [ ] 20 plugins stress.
- [ ] Bad plugin scan stress.
- [ ] Repeated plugin editor open/close.
- [ ] Resize during playback.
- [ ] Auto-scroll during playback.
- [ ] No callback allocations verified.
- [ ] Command queue overflow handling.
- [ ] Meter throttling verified.
- [ ] Waveform generation backgrounded.
- [ ] Plugin scan backgrounded.
