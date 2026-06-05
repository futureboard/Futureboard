# Audio Recording System — Implementation Checklist

Companion to [audio-recording-system-roadmap.md](audio-recording-system-roadmap.md)
and [phase-a-audit.md](phase-a-audit.md). Boxes pre-checked from the Phase A audit
reflect what is **already implemented** in the current codebase; unchecked boxes are
the real remaining work.

Legend: `[x]` done · `[~]` partial · `[ ]` not started.

---

## Phase A — Audit (read-only)

- [x] Inspect audio backend (`SphereDirectAudioEngine`).
- [x] Inspect Preferences > Audio + Recording tabs.
- [x] Inspect Track Inspector routing.
- [x] Inspect transport record button wiring.
- [x] Inspect track arm/monitor state + routing model + save/load.
- [x] Inspect engine input capture support.
- [x] Document gaps → `phase-a-audit.md`.

---

## Phase B — Device Channel Enumeration  ✅

- [x] Channel count → option builder approach (kept `JsAudioDeviceInfo.channels`; no struct churn / electron-binding break).
- [x] Input/output channels exposed from `device/mod.rs` (`list_input_devices`/`list_output_devices`).
- [x] `build_input_channel_options` / `build_output_channel_options` helper (`crates/SphereUIComponents/src/audio_routing.rs`).
  - 1ch → `Input 1`
  - 2ch → `Input 1`, `Input 2`, `Input 1+2 (Stereo)`
  - >2ch → mono channels, then stereo pairs, then `All Inputs`
- [x] `FUTUREBOARD_AUDIO_DEVICE_DEBUG=1` logs devices/channels (`device/mod.rs`; UI helper `audio_device_debug_enabled`).
- [x] No crash on 0 channels (builder returns empty) / unnamed devices (enumeration skips).
- [x] Unit tests: 6/6 pass (`audio_routing::tests`).

## Phase C — Preferences Audio Channel UI  ✅

- [x] Audio tab shows backend / input device / output device / sample rate / buffer.
- [x] Input Channels card — selected device + channel routes (Phase B builder), reactive to schema.
- [x] Output Channels card — same for the selected output device.
- [x] Reuse shared `settings_section_card` / `settings_section_title` / `settings_daw_row` style; no hardcoded colors.
- [x] Graceful empty states ("No device selected." / "No channels reported by this device.").
- [ ] Optional input test meter (later).

> Channel counts threaded via `(String, u32)` lists from `open_settings_dialog`
> → `open_settings_window` → `SettingsWindow` → `build_settings_content`
> → `audio_channel_section`. Snapshot at window-open (device-change refresh on
> reopen); legacy `settings_dialog()` path passes empty lists.

## Phase D — Track Input/Output Routing Model

- [x] `TrackInputRouting { None, AllInputs, AudioDeviceChannel{device_id,channel}, MidiDevice }`.
- [x] `TrackOutputRouting` (Main/Bus/None; hardware later).
- [x] Routing save/load (`project/format.rs` encode/decode).
- [x] Stereo-pair-capable input model (`AudioDeviceChannels { device_id, channels }`) with save/load support.
- [x] Recording runtime consumes explicit multi-channel routes via existing `JsRecordingTrackConfig.input_channels`.

## Phase E — Inspector Routing UI  ✅ (input selector; output/compat deferred)

- [x] Format ComboBox (Mono/Stereo) present.
- [x] **Replaced placeholder** `audio_input_options()` with `build_input_routing_options` driven by the selected input device's channel count (Phase B builder).
- [x] **Removed** always-`None` `parse_audio_input_option`; combo now maps each option label → real `TrackInputRouting` (`AudioDeviceChannel{device_id,channel}` per mono channel, `AllInputs` for multi-channel).
- [x] Selector highlights the current routing; degrades to `["None"]` when no engine/device.
- [x] Device channels enumerated only while the input combo is open (`selected_input_device_channels`, no per-frame cost).
- [x] Explicit stereo-pair options (Input 1+2, Input 3+4, etc.) map to `AudioDeviceChannels`.
- [x] Phase E remainder: output selector lists Main, None, and real Bus/Return track targets.
- [x] Phase E remainder: saved unavailable input/output routes appear as `Missing - ...` entries and are not silently reset.
- [x] Phase E remainder: input routes are format-compatible (Mono tracks show mono routes; Stereo tracks show stereo-pair routes).
- [x] Phase E remainder: hardware output routes are listed from the selected output device's mono hardware channels.

---

## Phase F — Track Arm / Monitor State

- [x] Arm flag (`track.armed`) + monitor mode (`input_monitor`).
- [x] Routing persists in project format.
- [x] Header / mixer / inspector arm+monitor controls share `TimelineState`; Inspector/Mixer mark dirty and sync only when the toggle actually changes.
- [x] Arm + monitor state is persisted in project format (`record_arm`, `input_monitor`) rather than session-only.

## Phase G — Transport Record Button

- [x] `TransportCommand::Record → toggle_native_recording`.
- [x] Validation: no project folder / no armed audio tracks → status + abort.
- [x] Record button dispatches `transport:record` and shows active recording state without disabled opacity.
- [x] "No input device/channel selected" validation; armed audio tracks with `None`, missing devices, invalid channels, or mixed input devices abort before engine start.

## Phase H — Recording State Machine

- [x] `transport.recording` flag; start playback on record; stop finalizes.
- [x] Commit avoids nested update (deferred sync) — see [[gpui-nested-entity-update-panic]].
- [x] Explicit Idle/Prepare/Recording/Finalizing/Error UI states (`RecordingUiState`) drive status text.
- [x] Start/stop/finalize errors surface through `audio_last_error` + `RecordingUiState::Failed`; partial-file cleanup remains engine-owned.

## Phase I — Project Recording Folder

- [x] `Media/Audio/` + `.rec/<session>` temp dir.
- [x] Take filename + collision counter (`unique_wav_path`).
- [x] Saved-project requirement enforced.

## Phase J — WAV Writer Thread

- [x] Dedicated disk-writer thread; bounded channel; no I/O in audio callback.
- [x] Float32 WAV; header placeholder + finalize; temp→final rename.
- [x] Ring-buffer overflow → atomic dropped-block counter; stop/finalize surfaces user-visible error and prevents silent bad clips.
- [ ] 24-bit PCM option (later).

## Phase K — Engine Input Capture

- [x] cpal input stream (`build_f32_input_stream`).
- [x] Input channel count from device default config.
- [~] Explicit input/output channel mapping (currently default-config driven).

## Phase L — Recording Runtime Session

- [x] `start_recording` / `stop_recording`; per-track writers; results returned.
- [~] Frames-written counters surfaced to UI (exist internally).
- [ ] `AbortRecording` path.

## Phase M — Clip Creation After Stop

- [x] `insert_recorded_clip` creates clip at start beat, duration from frames/bpm.
- [x] Relative path stored; project marked dirty.
- [~] Auto-select recorded clip (optional).

## Phase N — Waveform After Record

- [x] Enqueues waveform via `spawn_timeline_audio_import_jobs`.
- [~] "Waveform pending" placeholder state on the new clip (verify).

## Phase O — Input Monitoring

- [x] Monitor mix Off/In (`apply_recording_monitor_mix`, `input_monitor.is_active`).
- [ ] Explicit Auto mode semantics.
- [ ] Feedback warning when in/out share a device (later).
- [ ] Input level meter on armed track (later).

## Phase P — Recording Error UX

- [~] Errors set `audio_last_error` + eprintln.
- [ ] Blocking errors shown as dialog; small errors in status bar.
- [ ] Cover: device disconnected, channel unavailable, permission denied, disk full, writer overflow.

## Phase Q — Recording Preferences Page

- [x] Recording tab: path, audio format, metronome count-in.
- [ ] Save-before-recording toggle.
- [ ] Default monitor mode.
- [ ] Recording offset (samples/ms).
- [ ] Generate-waveform-after-record toggle.

## Phase R — Latency Offset

- [ ] Manual recording offset setting.
- [ ] Apply offset to recorded clip start.
- [ ] Display backend input latency if exposed.

## Phase S — Unsaved Workspace Recording

- [x] Basic guard: require saved project folder before recording.
- [ ] Temp recording folder + copy-into-project on save (later).
- [ ] Cleanup temp files on discard.

## Phase T — Punch In/Out Scaffold

- [ ] `punch_enabled` / `punch_start_beat` / `punch_end_beat` data model.
- [ ] UI hidden/disabled.

## Phase U — Loop Takes Scaffold

- [ ] Take naming / take-lane plan (no full impl).

## Phase V — MIDI Recording Compatibility

- [ ] Transport record path tolerates future armed MIDI tracks (no conflict).

## Phase W — Stress / Performance

- [ ] Long recording (5+ min) stays responsive.
- [ ] Ring-buffer overflow handled gracefully.
- [ ] Slow-disk path doesn't block audio.

## Phase X — Cross-platform

- [ ] Windows WASAPI input verified.
- [ ] macOS CoreAudio input.
- [ ] Linux PipeWire/JACK/ALSA input.

## Phase Y — Documentation

- [ ] User docs.
- [ ] Developer docs.
- [ ] Known limitations.

## Phase Z — Stabilization

- [ ] Save/load roundtrip with recorded clips.
- [ ] Crash recovery for in-progress recordings.
- [ ] Release checklist.

---

## Build / Validation (run per slice)

- [ ] `cargo check -p sphere_ui_components`
- [ ] `cargo check --manifest-path apps/native/Cargo.toml`
- [ ] `cargo clippy -p sphere_ui_components -- -D warnings`
- [ ] Engine: `cargo check -p sphere-direct-audio-engine`

## Recommended order

1. **Phase B → C → E** (device channels → prefs UI → real Inspector input selector) — unlocks the existing recording runtime; UI/data only, no realtime risk.
2. Phase D stereo-pair model.
3. Phase R latency offset + Phase Q prefs completion.
4. Phase P error UX hardening + Phase W stress.
