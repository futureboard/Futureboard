# Futureboard Studio — Audio Recording System Roadmap / Implementation Plan

## Purpose

Design and implement a real Audio Recording system for Futureboard Studio.

Current problems:

- Audio Preferences shows backend/input/output devices, but does not expose real input/output channel lists.
- Track Inspector routing has Input/Output selectors, but channel/device data is not fully wired.
- Recording controls exist visually, but the transport recording function is not fully active.
- Track arm / monitor / input routing / recording file creation / clip creation need to become real.
- Recording must integrate with DAUx/SphereDirectAudioEngine, project media folders, timeline clips, waveform cache, and save/load.

Scope:

- Audio device channel enumeration
- Input/output routing
- Track record arm / monitor modes
- Transport record button and recording state machine
- Recording file writer
- Recording clip creation
- Project asset folder integration
- Waveform generation after record
- Recording latency compensation
- Punch in/out later
- Loop recording/takes later
- MIDI recording later, but this document focuses on AUDIO recording first

> **Important:** Do not implement all phases at once. Use this as a master plan and split into safe patches.

---

## 0. Core Goal

Futureboard must support basic real-world audio recording:

1. User selects input device/channel.
2. User arms an audio track.
3. User enables monitoring if needed.
4. User presses Record in the transport.
5. Audio is written to a WAV file in the project folder.
6. A new audio clip appears on the timeline after recording.
7. Recorded audio plays back.
8. Waveform cache is generated in the background.
9. Save/load keeps the recorded media linked using project-relative paths.

Minimum usable flow:

- One stereo input
- One armed audio track
- Record to WAV
- Clip appears after stop
- Playback works

---

## 1. Current Known Issues

Observed UI state:

- Preferences > Audio lists:
  - Backend
  - Input Device
  - Output Device
  - Driver Status
  - Sample Rate
  - Buffer Size
- Track Inspector routing lists:
  - Format
  - Input
  - Output
- But input/output channels are not properly discovered or shown.
- Recording state is not fully wired into transport/engine.
- Track record arm exists in UI, but may not drive recording runtime.

Problems to solve:

- Device enumeration returns devices but not channel routes.
- No proper channel selector such as:
  - Input 1
  - Input 2
  - Input 1+2 Stereo
  - Output 1+2
- Track input selector is generic/placeholder.
- Recording command likely not creating a recorder node/file writer.
- Transport Record button not connected to full recording lifecycle.
- Recording clip is not created from recorded file.
- Asset copy/project folder integration is incomplete.

---

## 2. Definitions

- **Audio Device:** A physical or virtual audio input/output device.
- **Device Channel:** A mono hardware channel exposed by a device.
- **Channel Pair:** Two mono channels used as stereo input/output.
- **Input Route:** A selected device/channel configuration assigned to a track.
- **Output Route:** Where a track sends audio: Main, Bus, Hardware output later.
- **Record Armed Track:** A track that will record input when transport recording starts.
- **Input Monitoring:** Whether the live input signal is heard through the track.
- **Recording Take:** A single recorded file/clip created during one record pass.
- **Recorder:** Runtime component that receives audio from input route and writes it to disk.

---

## 3. Data Model

Add/confirm audio device model:

```rust
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub direction: AudioDeviceDirection,
    pub is_default: bool,
    pub sample_rates: Vec<u32>,
    pub default_sample_rate: Option<u32>,
    pub max_input_channels: u16,
    pub max_output_channels: u16,
    pub input_channels: Vec<AudioChannelInfo>,
    pub output_channels: Vec<AudioChannelInfo>,
}
```

```rust
pub enum AudioDeviceDirection {
    Input,
    Output,
    Duplex,
}
```

```rust
pub struct AudioChannelInfo {
    pub index: u16,
    pub name: String,
    pub kind: AudioChannelKind,
}
```

```rust
pub enum AudioChannelKind {
    Mono,
    StereoLeft,
    StereoRight,
    Unknown,
}
```

Track routing:

```rust
pub struct TrackInputRoute {
    pub device_id: Option<String>,
    pub channels: AudioChannelSelection,
}
```

```rust
pub enum AudioChannelSelection {
    None,
    Mono { channel: u16 },
    Stereo { left: u16, right: u16 },
    All,
}
```

Track output routing:

```rust
pub enum TrackOutputRoute {
    Main,
    Bus { track_id: String },
    Hardware { device_id: String, channels: AudioChannelSelection },
    None,
}
```

Track recording state:

```rust
pub struct TrackRecordingState {
    pub armed: bool,
    pub monitor_mode: MonitorMode,
    pub input_route: TrackInputRoute,
    pub record_format: RecordFormat,
}
```

```rust
pub enum MonitorMode {
    Off,
    Auto,
    In,
}
```

Recording session:

```rust
pub struct ActiveRecordingSession {
    pub id: String,
    pub started_at_beat: f64,
    pub started_at_sample: u64,
    pub sample_rate: u32,
    pub tracks: Vec<ActiveTrackRecording>,
}
```

```rust
pub struct ActiveTrackRecording {
    pub track_id: String,
    pub input_route: TrackInputRoute,
    pub file_path: PathBuf,
    pub writer_id: String,
    pub channels: u16,
    pub frames_written: u64,
}
```

Recorded clip:

```rust
pub struct RecordedAudioClip {
    pub track_id: String,
    pub clip_id: String,
    pub source_path: PathBuf,
    pub start_beat: f64,
    pub duration_beats: f64,
    pub source_offset_frames: u64,
    pub recorded_frames: u64,
}
```

---

## 4. Preferences > Audio Channel Enumeration

Goal: Preferences must show real input/output devices and their channels.

Current UI:

- Backend ComboBox
- Input Device ComboBox
- Output Device ComboBox
- Driver Status
- Sample Rate
- Buffer Size

Add:

- Input Channels list
- Output Channels list
- Channel preview/status
- Optional test input meter later

Recommended UI:

```txt
Audio Engine
- Backend              [WASAPI Shared]
- Input Device         [Built-in Microphone]
- Output Device        [Headphone (Realtek)]
- Driver Status        Ready

Input Channels
[ Built-in Microphone ]
  - Mono 1
  - Stereo 1/2 if available
  - All Inputs if supported

Output Channels
[ Headphone (Realtek) ]
  - Main 1/2
  - Output 1
  - Output 2

Sample Rate & Buffer
- Sample Rate          [48000 Hz]
- Buffer Size          [256]
- Output Buffer        5.33 ms
```

Rules:

- If device has 1 input channel, expose: Input 1 (Mono)
- If device has 2 input channels, expose: Input 1 (Mono), Input 2 (Mono), Input 1+2 (Stereo)
- If device has more: Input 1, Input 2, Input 1+2, Input 3+4, etc.

Need helper:

```rust
pub fn build_input_channel_options(device: &AudioDeviceInfo) -> Vec<AudioRouteOption>
pub fn build_output_channel_options(device: &AudioDeviceInfo) -> Vec<AudioRouteOption>
```

```rust
pub struct AudioRouteOption {
    pub id: String,
    pub label: String,
    pub selection: AudioChannelSelection,
}
```

Acceptance:

- Preferences displays actual channel options.
- Track Inspector can reuse the same channel option builder.
- No placeholder-only "System Input" unless no data exists.
- No crash if backend cannot report names.

---

## 5. Track Inspector Routing

Goal: Track Inspector must allow selecting actual recording input and output routes.

For Audio Track:

- Format: Mono / Stereo
- Input: None, Default Input, Device Name — Input 1, Device Name — Input 2, Device Name — Input 1+2 Stereo
- Output: Main, Bus tracks, None, Hardware output later

For Instrument Track:

- MIDI Input
- MIDI Channel
- Audio Output: Main, Bus, None

For Bus/Return:

- Output: Main, Bus, None
- No record input unless explicitly supported later.

Rules:

- Track format affects compatible input options.
- Mono track can use mono input.
- Stereo track can use stereo pair.
- If stereo track receives mono input, allow mono-to-stereo duplicate only if explicitly supported.
- If selected input becomes unavailable, show Missing / Unavailable.
- Do not silently reset user routing.

UI: Use shared SettingsRow/ComboBox style. No button grids. No hardcoded colors.

Acceptance:

- Audio track input selector shows real channels.
- Output selector shows Main/buses.
- Track format and input compatibility are sane.
- Routing persists.

---

## 6. Transport Recording Function

Goal: Transport bar Record button must start/stop recording.

Transport states: Stopped, Playing, Recording, Paused later.

Record button behavior:

- If stopped: Record button arms recording mode. Press Play while record enabled starts recording. Or pressing Record can immediately start recording depending DAW behavior.
- Recommended simple behavior:
  - Press Record toggles recording armed mode.
  - Press Play starts playback/recording if record armed.
  - Press Record while playing starts recording immediately if tracks armed.
  - Press Stop ends recording and finalizes files.

Simpler first implementation:

- Press Record: if not recording, start transport recording immediately; if recording, stop recording.
- Press Stop: stop recording if active; finalize files.

Record prerequisites:

- At least one audio track armed.
- Armed tracks must have valid input route.
- Audio input device must be available.
- Project must have a folder or temporary recording directory.

If no armed track: Show status "No armed audio tracks." Do not start recording.

If no valid input: Show status "No input device/channel selected." Do not start recording.

If unsaved workspace:

- Record to temporary session folder, then copy into project folder on Save.
- Or ask to save project first.
- Recommended for first version: Ask user to save project before audio recording. Later support temporary unsaved recording.

Transport commands:

```rust
TransportCommand::StartRecording
TransportCommand::StopRecording
TransportCommand::ToggleRecording
```

Engine commands:

```rust
AudioCommand::BeginRecording { session: RecordingSessionPlan }
AudioCommand::EndRecording
AudioCommand::AbortRecording
```

Acceptance:

- Record button changes active state.
- Pressing Record with armed track starts recording.
- Stop finalizes files and creates clips.
- Errors show in status bar/dialog, not panic.

---

## 7. Recording State Machine

Recording flow:

```txt
Idle → PrepareRecording → Recording → Finalizing → Complete → Idle
```

Error path: `PrepareRecording → Failed(reason) → Idle`

Cancel path: `Recording → Abort → Cleanup partial files → Idle`

State details:

- **Idle:** no active recording
- **PrepareRecording:** validate armed tracks, validate input routes, create file paths, create writers, build session plan, send BeginRecording to engine
- **Recording:** audio callback/control path writes samples to record buffers/writers; UI shows recording time; transport button active; track headers show recording indicator
- **Finalizing:** stop accepting samples, close WAV writers, compute duration, create clips, enqueue waveform generation, mark project dirty
- **Complete:** clips created, status message

Important:

- Do not write to disk directly in audio callback if it can block.
- Use record ring buffers or writer thread.
- Audio callback pushes input samples to lock-free/bounded buffer.
- Recording writer thread writes WAV file.

Acceptance:

- Recording state is visible.
- Recording finalization is deterministic.
- Partial files are handled.

---

## 8. Recording File Writer

Initial format:

- WAV
- 24-bit or 32-bit float
- sample rate from engine
- channels from track input/format

Recommended initial:

- 32-bit float WAV
- no clipping from internal float engine
- later add 24-bit PCM option

Writer architecture:

Audio callback:

- receives input buffer
- copies needed channels into preallocated recording ring buffer
- never blocks on disk
- if ring full: increment drop counter, optionally stop recording with error, never block callback

Writer thread:

- drains ring buffer
- writes to WAV
- finalizes header on stop

Data structures:

```rust
pub struct RecordingWriterHandle {
    pub id: String,
    pub ring: RecordingRingBuffer,
    pub command_tx: RecordingWriterCommandSender,
}
```

```rust
pub enum RecordingWriterCommand {
    Start { path, sample_rate, channels },
    WriteBlock { ... } // if not using direct ring
    Stop,
    Abort,
}
```

Need careful ownership:

- writer thread owns file handle
- engine owns producer side
- controller owns session metadata

Acceptance:

- No file I/O in audio callback.
- Recorded WAV is valid.
- Stop flushes and closes file.
- Abort deletes or marks partial file.

---

## 9. Input Monitoring

Monitor modes: Off, Auto, In.

- **Off:** do not route live input to output.
- **In:** always route input through track chain to output.
- **Auto:** monitor input when track armed and transport stopped or recording. During playback maybe monitor only if recording.

Initial implementation:

- Off and In first.
- Auto can alias to In while armed if simpler.

Signal flow while monitoring: `input route → track inserts if enabled → volume/pan → sends → output`

Avoid direct feedback:

- if input/output are same device/speakers, user may get feedback.
- show warning later.
- do not attempt automatic feedback suppression first.

Acceptance:

- Armed track with Monitor In shows input meter.
- Monitor Off prevents input from output.
- Monitoring does not require recording active.

---

## 10. Track Arm / Record UI

UI controls:

- Track Header: R button, I/Input monitor button
- Mixer strip: R button, I button
- Inspector: State M/S/R/I, Input route, Format, Monitor mode
- Transport: Record button, recording timer/status

Rules:

- R arms track.
- I toggles monitor mode or cycles Off/Auto/In.
- Multiple tracks can be armed.
- Record button disabled or warns if no armed track.
- Track meter should show input level when armed/monitoring.

Recording indicators:

- Transport record button red/active.
- Timeline record region maybe later.
- Track header recording glow/indicator.
- Status bar: `Recording 00:00:12 — Audio Track 1`

Acceptance:

- Track arm state persists or is session-only depending chosen behavior.
- Record button clearly indicates active recording.
- User knows what is being recorded.

---

## 11. Clip Creation After Recording

On stop recording:

1. Finalize WAV file.
2. Calculate recorded duration frames.
3. Convert duration to beats using tempo map/static BPM.
4. Create audio clip on the armed track.
5. Clip start = recording start beat.
6. Clip duration = recorded duration beats.
7. Source path = project-relative path.
8. Mark project dirty.
9. Queue waveform generation.
10. Select created clip optionally.

Clip naming:

- Track name + take number
- Example: `Audio Track 1_001.wav`, `Audio Track 1 Take 001`

File naming:

```txt
Project/Assets/Audio/Recordings/
  Audio Track 1_2026-06-04_13-42-10.wav
or:
  Audio Track 1 Take 001.wav
```

Recommended folder: `Assets/Audio/Recordings/`

Collision handling:

- if name exists, increment take number
- never overwrite silently

Acceptance:

- Recorded clip appears in timeline after stop.
- Clip aligns to record start.
- Clip has correct duration.
- Waveform pending then ready.

---

## 12. Project Folder / Unsaved Workspace

Recording needs a file path.

Saved project: record directly to `ProjectFolder/Assets/Audio/Recordings/`

Unsaved workspace options:

- A. Ask to save project before recording.
- B. Record to temp folder, move/copy on Save.
- C. Record to global recording cache.

Recommended first version: **A. Ask to save project before recording.**

Behavior:

- User presses Record in unsaved workspace.
- Dialog: "Save project before recording audio?" — Save Project / Cancel
- Save Project opens Save As flow.
- After save, start recording if still requested.

Later:

- support temp recording folder: `AppData/Futureboard/TempRecordings/<session-id>/`
- on Save, copy into project folder
- on discard, clean temp files

Acceptance:

- Recording never fails due to missing project folder without explanation.
- No recorded file is lost silently.

---

## 13. Waveform Generation After Record

After recording finalizes:

- enqueue peak generation job
- show waveform pending on clip
- timeline renders placeholder/preview
- when peaks ready, update clip waveform

Do not:

- generate waveform synchronously on UI thread
- generate waveform in audio callback
- block stop/finalize for full peak analysis if file is large

Minimum:

- create clip immediately with "waveform pending"
- background worker generates peaks

Acceptance:

- Newly recorded clip appears immediately.
- Waveform appears shortly after.
- UI remains responsive.

---

## 14. Latency and Recording Offset

Initial recording may be offset due to: input latency, output latency, buffer size, backend latency, plugin latency, monitoring path.

Need settings:

- Recording Offset Samples
- Recording Offset ms
- Automatic latency compensation later

Initial:

- record start sample aligned to engine transport sample
- use known device input latency if backend exposes it
- expose manual offset in Preferences > Recording

Settings:

```txt
Recording
- Recording format: WAV Float32
- Input latency compensation: Auto/Manual
- Manual offset: 0 samples / 0 ms
- Count-in
- Pre-roll later
- Create takes/layers later
```

Acceptance:

- Manual recording offset field exists or is documented.
- Recorded clip start is reasonably aligned.
- Full loopback calibration later.

---

## 15. Punch In / Punch Out Later

Not required first, but design should not block it.

Future:

- punch-in/out range
- recording starts only inside range
- pre-roll before punch
- clip split/merge
- take lanes

State: `punch_enabled`, `punch_start_beat`, `punch_end_beat`

Acceptance later: Recording only creates clip for punch region.

---

## 16. Loop Recording / Takes Later

Future:

- loop recording creates takes
- take lanes
- comping
- replace/overdub modes
- record mode: Takes, Overwrite, Merge

Do not implement now unless foundation exists.

---

## 17. MIDI Recording Later

This document focuses on audio recording, but transport recording should be compatible with MIDI recording later.

MIDI record future:

- armed MIDI/instrument tracks record MIDI events
- note on/off, CC, pitch bend
- quantize on record
- overdub, merge/replace

Transport recording should support: audio armed tracks, MIDI armed tracks, both at same time later.

---

## 18. Audio Engine Integration

Engine must support input buffers.

Current output-only risk:

- App discovers output device but may not open input stream or duplex stream.
- Recording requires input capture.

Backend requirements:

- **Windows WASAPI:** shared mode duplex or separate input/output streams; handle default input device; channel mapping; clock drift later
- **CoreAudio:** input/output device streams; channel layout
- **Linux:** PipeWire/JACK better for duplex; ALSA fallback

Engine API:

```rust
pub fn enumerate_audio_devices() -> Vec<AudioDeviceInfo>
pub fn set_input_device(device_id: Option<String>)
pub fn set_output_device(device_id: Option<String>)
pub fn start_stream(config: AudioStreamConfig)
pub fn begin_recording(plan: RecordingSessionPlan)
pub fn end_recording()
```

Audio callback needs: input buffer if available, output buffer, sample frame count, timestamp if available.

If backend currently output-only:

- Phase 1: add input device enumeration, add input channel list
- Phase 2: add duplex callback/input stream
- Phase 3: record to file

Acceptance:

- Engine exposes input channel count.
- Recording callback receives input samples.
- Output playback still works.

---

## 19. Channel Mapping

For each armed track:

- input route selects device and channels
- engine maps input buffer channels to track recording buffer

Examples:

- Mono track: Input 1 → mono file; Input 2 → mono file
- Stereo track: Input 1+2 → stereo file; Input 1 mono to stereo duplicate optional later

Mapping:

```rust
pub struct InputChannelMap {
    pub source_device_id: String,
    pub source_channels: Vec<u16>,
    pub destination_channels: Vec<u16>,
}
```

Rules:

- validate channel index exists
- if selected channel unavailable, block recording with error
- no panic on invalid route

Acceptance:

- Stereo input records stereo file.
- Mono input records mono file.
- Invalid input route shows error.

---

## 20. Recording Commands and Outcomes

Commands from UI:

```rust
StudioCommand::ToggleRecord
StudioCommand::StartRecording
StudioCommand::StopRecording
StudioCommand::ArmTrack(track_id, bool)
StudioCommand::SetTrackMonitor(track_id, MonitorMode)
StudioCommand::SetTrackInputRoute(track_id, TrackInputRoute)
```

Engine commands:

```rust
AudioCommand::BeginRecording(RecordingSessionPlan)
AudioCommand::EndRecording
AudioCommand::AbortRecording
```

Command outcome:

```rust
pub struct RecordingCommandOutcome {
    pub changed: bool,
    pub project_dirty: bool,
    pub status: Option<String>,
    pub created_clips: Vec<String>,
}
```

Avoid nested updates:

- Recording command should return outcome.
- StudioLayout applies dirty after child updates.
- Do not update StudioLayout from inside Timeline/Engine callback synchronously.

> See [[gpui-nested-entity-update-panic]] memory and `tasks/native/edit-shortcuts-and-nested-update-fix.md` — child→parent dirty marks must be deferred (`cx.defer`) or return an outcome the parent applies.

Acceptance:

- Recording commands do not cause GPUI double lease panic.
- Status messages are shown safely.

---

## 21. Error Handling

Common errors: no armed tracks, no input device selected, input device disconnected, channel unavailable, project unsaved, cannot create recording folder, permission denied, disk full, writer thread failed, ring buffer overflow, sample rate mismatch, backend does not support input, recording already active.

Error strategy:

- show status bar message for small errors
- show dialog for blocking errors
- never panic
- cleanup partial files if needed
- preserve project state

Examples:

- "No armed tracks. Arm an audio track before recording."
- "Input device is unavailable."
- "Save the project before recording audio."
- "Could not create recording file: permission denied."
- "Recording stopped: disk writer could not keep up."

Acceptance:

- Every recording failure has user-readable message.
- No silent failure.

---

## 22. Diagnostics and Debug Flags

Add:

`FUTUREBOARD_RECORDING_DEBUG=1` — record button pressed, armed tracks, validation results, session start/end, file path, frames written, clip created

`FUTUREBOARD_AUDIO_DEVICE_DEBUG=1` — devices, channels, input/output config, backend stream setup

`FUTUREBOARD_RECORD_WRITER_DEBUG=1` — writer start, blocks written, ring buffer status, finalize result

`FUTUREBOARD_INPUT_MONITOR_DEBUG=1` — monitor routes, input levels

Log examples:

```txt
[recording] start requested armed_tracks=1 project_saved=true
[recording] track=track-1 input=Realtek Input 1+2 file=Assets/Audio/Recordings/Audio Track 1 Take 001.wav
[recording] writer started sr=48000 ch=2 format=float32
[recording] stop frames=192000 duration=4.000s
[recording] clip created id=clip-123 start=1.1 length=2.0bt
[recording] waveform job queued
```

---

## 23. Preferences > Recording Page

Add/complete Recording settings page:

```txt
Recording
- Record Format: WAV Float32 / WAV 24-bit later
- Default Record Folder: Assets/Audio/Recordings
- Save project before recording: On
- Count-in: Off / 1 bar / 2 bars later
- Pre-roll: Off later
- Monitoring Mode Default: Off / Auto / In
- Recording Offset: 0 samples / ms
- Create Takes: Off later
- Auto-name takes: On
- Generate waveform after recording: On
```

Acceptance:

- Recording page exists and uses shared Settings components.
- Settings persist.
- Unsupported options disabled with clear "Soon" or hidden.

---

## 24. UI Checklist

Transport:

- [ ] Record button exists
- [ ] Record active state visible
- [ ] Record disabled/warning state
- [ ] Recording timer/status

Track Header:

- [ ] R arm button works
- [ ] I monitor button works
- [ ] Input meter later
- [ ] Armed state visible

Mixer:

- [ ] R button works
- [ ] I button works
- [ ] Input signal meter later

Inspector:

- [ ] Format ComboBox
- [ ] Input Device/Channel ComboBox
- [ ] Output ComboBox
- [ ] Arm/Monitor state

Preferences:

- [ ] Audio device channels
- [ ] Recording settings
- [ ] Latency/offset settings

Timeline:

- [ ] Recording region preview while recording
- [ ] Clip appears on stop
- [ ] Waveform pending state
- [ ] Recorded clip selected after stop optional

---

## 25. Implementation Phases A-Z

- **Phase A — Audit Current Audio Recording Surface:** inspect audio backend, Preferences > Audio, Track Inspector routing, transport record button, track arm/monitor state, engine input support; document gaps.
- **Phase B — Device Channel Enumeration:** extend AudioDeviceInfo, expose input/output channel counts, build channel options, debug logs.
- **Phase C — Preferences Audio Channel UI:** show input/output channels, shared ComboBox/List UI, no placeholder-only routing.
- **Phase D — Track Input/Output Routing Model:** TrackInputRoute, TrackOutputRoute, AudioChannelSelection, save/load routing.
- **Phase E — Inspector Routing UI:** real input channel ComboBox, real output route ComboBox, format compatibility.
- **Phase F — Track Arm / Monitor State:** arm button, monitor mode, inspector/mixer/header sync, persistence/session decision.
- **Phase G — Transport Record Button:** ToggleRecording command, active state, validation no armed/no input/project unsaved.
- **Phase H — Recording State Machine:** Idle/Prepare/Recording/Finalizing/Error, status messages, no nested updates.
- **Phase I — Project Recording Folder:** Assets/Audio/Recordings, take file naming, collision handling, saved project requirement.
- **Phase J — WAV Writer Thread:** writer thread, float32 WAV, finalize on stop, error handling.
- **Phase K — Engine Input Capture:** duplex/input stream, input buffer exposure, channel mapping.
- **Phase L — Recording Runtime Session:** BeginRecording, EndRecording, route armed tracks to writers, frames written counters.
- **Phase M — Clip Creation After Stop:** create audio clips, start beat/duration, relative paths, mark dirty.
- **Phase N — Waveform After Record:** enqueue peak cache job, waveform pending, update UI when ready.
- **Phase O — Input Monitoring:** monitor Off/In first, route input to output, avoid blocking, meters later.
- **Phase P — Recording Error UX:** dialogs/status, permission/disk/no input, cleanup partial files.
- **Phase Q — Recording Preferences Page:** format, folder, save-before-recording, offset, monitor default.
- **Phase R — Latency Offset:** manual offset, apply to clip start, backend latency display.
- **Phase S — Unsaved Workspace Recording:** temp recording folder, copy into project on save, cleanup temp files.
- **Phase T — Punch In/Out Scaffold:** data model only, UI disabled/hidden.
- **Phase U — Loop Takes Scaffold:** take naming, take lane plan, no full implementation yet.
- **Phase V — MIDI Recording Compatibility:** transport record supports future MIDI tracks, no conflict with audio recording.
- **Phase W — Stress/Performance:** long recording, ring buffer overflow handling, disk slow path, UI responsive.
- **Phase X — Cross-platform Testing:** Windows WASAPI, macOS CoreAudio, Linux PipeWire/JACK/ALSA.
- **Phase Y — Documentation:** user docs, developer docs, known limitations.
- **Phase Z — Stabilization:** bug bash, save/load roundtrip, crash recovery, release checklist.

---

## 26. Recommended First Implementation Slice

Do not start with full recording.

**Recommended slice 1:**

- Phase A: Audit
- Phase B: Device Channel Enumeration
- Phase C: Preferences UI channel list
- Phase D/E: Track input route model + Inspector input selector
- No recording yet

**Recommended slice 2:**

- Phase F: Arm/Monitor state
- Phase G: Transport record validation
- Pressing Record shows correct errors/status

**Recommended slice 3:**

- Phase I/J: Project recording folder + WAV writer thread
- Generate a test recording file from mocked input if needed

**Recommended slice 4:**

- Phase K/L/M: Real input capture → record → clip creation

**Recommended slice 5:**

- Phase N/O/R: waveform, monitoring, latency offset

This keeps patches safe and buildable.

---

## 27. Manual Test Plan

Device channels:

1. Open Preferences > Audio.
2. Select input device.
3. Input channel options appear.
4. Select output device.
5. Output channel options appear.
6. No crash if device has 0 input channels.

Inspector routing:

1. Add Audio Track.
2. Select track.
3. Inspector Input shows real input channels.
4. Select Input 1.
5. Change track format to Stereo.
6. Input options update.

Arm/Record:

1. Arm audio track.
2. Press Record without project saved.
3. Prompt asks to save project.
4. Save project.
5. Press Record.
6. Recording starts.
7. Press Stop.
8. Clip appears.
9. File exists under Assets/Audio/Recordings.
10. Reopen project.
11. Clip resolves.

Monitoring:

1. Arm track.
2. Monitor In.
3. Speak/play input.
4. Meter/input status reacts.
5. Monitor Off stops output.

Failure:

1. Disconnect input device.
2. Press Record.
3. Error shown.
4. No panic.

Performance:

1. Record 5 minutes.
2. UI remains responsive.
3. WAV file valid.
4. Waveform generates after stop.

---

## 28. Final Acceptance Criteria

Audio Recording is usable when:

- Preferences shows real input/output channels.
- Track Inspector can select actual input channels.
- Track arm works.
- Monitor mode works at least Off/In.
- Transport Record starts/stops recording.
- Recording writes valid WAV files.
- Recorded clip appears on timeline.
- Recorded file is stored inside project folder.
- Project save/load restores recorded clips.
- Waveform is generated after recording.
- Errors are user-readable.
- No audio callback blocking.
- No UI freeze.
- No GPUI nested update panic.
- Windows/macOS/Linux backend paths are documented.

_End of document._
