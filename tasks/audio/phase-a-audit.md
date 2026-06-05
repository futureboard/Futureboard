# Phase A — Audit: Current Audio Recording Surface

Audit for [audio-recording-system-roadmap.md](audio-recording-system-roadmap.md).
Read-only pass. **Headline finding: the recording runtime is already substantially
implemented** — much further along than the roadmap's "Current Known Issues"
section assumed. The real remaining gap is *device channel selection in the UI*,
not the recording engine.

## Files inspected

Engine (`crates/SphereDirectAudioEngine/src`):
- `device/mod.rs` — input/output device enumeration
- `recording.rs` — full recording session (input stream, writer thread, WAV)
- `types.rs` — `JsAudioDeviceInfo`, `JsStartRecordingConfig`, `JsRecordingResult`

UI (`crates/SphereUIComponents/src`):
- `layout/recording_ops.rs` — start/stop/toggle recording, clip commit
- `layout/transport_ops.rs` — `TransportCommand::Record → toggle_native_recording`
- `components/panel.rs` — Inspector routing combos (`audio_input_options`, …)
- `components/settings_dialog.rs` — Preferences (Audio + Recording tabs)
- `project/mod.rs`, `project/format.rs` — routing model + save/load
- `components/timeline/timeline_state.rs` — `TrackInputRouting`, arm/monitor

## What already EXISTS (do NOT rebuild)

| Roadmap phase | Status | Where |
|---|---|---|
| K — Engine input capture (cpal input stream, duplex) | ✅ done | `recording.rs::build_f32_input_stream` |
| J — WAV float32 writer thread + bounded channel + finalize | ✅ done | `recording.rs::disk_writer_thread`, `write_wav_placeholder`/`finalize_wav` |
| L — Recording runtime session (start/stop/results) | ✅ done | `recording.rs::{start_recording,stop_recording}` |
| G — Transport Record button → recording | ✅ basic | `transport_ops.rs:103` `TransportCommand::Record` |
| H — Recording state + start playback on record | ✅ basic | `recording_ops.rs::start_native_recording` (`transport.recording`) |
| I — Project folder + take naming + collision | ✅ done | `recording.rs` (`Media/Audio/.rec/<session>`, `unique_wav_path`, counter) |
| M — Clip creation after stop | ✅ done | `recording_ops.rs::commit_recording_results` → `insert_recorded_clip` |
| N — Waveform after record | ✅ done | `commit_recording_results` → `spawn_timeline_audio_import_jobs` |
| O — Input monitoring (Off/In) | ✅ basic | `recording.rs::apply_recording_monitor_mix`, `input_monitor.is_active` |
| D/F — Track arm + monitor + input route model + save/load | ✅ partial | `TrackInputRouting{None,AllInputs,AudioDeviceChannel{device_id,channel},MidiDevice}`, `format.rs` encode/decode |
| 12/S — Unsaved-workspace guard | ✅ basic | `start_native_recording` ("save the project to a folder before recording") |
| Q — Recording prefs page | ✅ partial | `settings_dialog.rs` Recording tab: path, audio format, metronome count-in |

The audio callback already avoids file I/O/locks: it does `try_send(data.to_vec())`
into a bounded crossbeam channel drained by the writer thread. Good.

## The REAL gaps (prioritized)

### 1. Inspector input/output selectors are placeholders — **blocking** (Phase E)
`components/panel.rs`:
```rust
fn audio_input_options() -> Vec<String> {
    // TODO(device-enumeration): ...
    vec!["None".to_string()]
}
fn parse_audio_input_option(label: &str) -> TrackInputRouting {
    match label { "None" => TrackInputRouting::None, _ => TrackInputRouting::None }
}
```
So the **runtime fully supports `AudioDeviceChannel { device_id, channel }`, but the
UI never lets the user pick one** — every audio track's input parses to `None`.
This is the single most impactful gap: recording "works" but you can't choose an
input in the Inspector. Output options are likewise hardcoded to `["Main","None"]`
(no buses/hardware).

> Add-track dialog has its own hardcoded placeholders too
> (`"System Input (Stereo)", "Input 1", "Input 2", "None"`) that aren't real.

### 2. Device channel enumeration is count-only (Phase B)
`types.rs::JsAudioDeviceInfo` exposes `channels: u32` (a *count*), not a per-channel
list. `device/mod.rs` reports `default_input_config().channels()` only. There is no
`build_input_channel_options` / channel-pair derivation, so the UI has no real data
to build "Input 1 / Input 2 / Input 1+2 Stereo" from.

### 3. Preferences > Audio has no channel lists (Phase C)
Settings Audio tab shows backend/in/out device + sample rate + buffer, but no
Input Channels / Output Channels lists.

### 4. Channel-selection model is single-index, not a stereo pair (Phase D)
`TrackInputRouting::AudioDeviceChannel { device_id, channel: u32 }` holds one
channel. `recording_ops.rs::recording_input_channels` falls back to `[0,1]` for a
stereo-format track regardless of the routed device/pair. The roadmap's
`AudioChannelSelection { Mono | Stereo{left,right} | All }` is not yet modeled, so
true stereo-pair routing (e.g. Input 3+4) can't be expressed.

### 5. No recording latency/offset (Phase R)
No `recording_offset` setting; recorded clip start uses raw transport beat
(`recording_start_beat`). No input-latency compensation.

### 6. Recording prefs incomplete vs roadmap §23 (Phase Q)
Present: path, audio format, metronome count-in. Missing: save-before-recording
toggle, default monitor mode, recording offset, generate-waveform toggle (waveform
is currently always generated).

## Smaller observations
- Monitor modes: `input_monitor` exists with `is_active(armed)`; only Off/In-style
  mixing is implemented (Auto aliases via `is_active`). Matches roadmap "Off/In first".
- `resolve_input_device_id` matches by name/id with a string fallback — fine, but
  depends on device names being stable across enumerations.
- Recording is committed via `schedule_audio_project_sync` + `engine_*_dirty` flags
  (deferred), so it already avoids the nested-update panic class. Good — keep this
  pattern for any new recording commands (see [[gpui-nested-entity-update-panic]]).

## Corrected recommended first slice

The roadmap's "slice 1" is exactly right and is the **easy, low-risk** next step
(data + UI only, no realtime/audio-callback changes):

1. **Phase B** — extend `JsAudioDeviceInfo` with a channel list (or add a channel
   count → option builder) and expose it from `device/mod.rs`.
2. **Phase C** — show Input/Output Channels in Preferences > Audio.
3. **Phase E** — replace `audio_input_options`/`parse_audio_input_option`
   placeholders with real device-channel options, wired to
   `TrackInputRouting::AudioDeviceChannel`. This alone unlocks the
   already-working recording runtime.

Defer Phase D's full `AudioChannelSelection` stereo-pair refactor until after the
single-channel selector works end-to-end (keeps the patch small and buildable).

## Acceptance for Phase A
- [x] Audited engine backend, recording runtime, transport, arm/monitor, routing model, prefs.
- [x] Documented what exists vs. the roadmap (most of K/J/L/G/H/I/M/N/O already done).
- [x] Identified the blocking gap (Inspector input selector placeholder + count-only device info).
- [x] Produced a corrected, minimal first implementation slice (B → C → E).
