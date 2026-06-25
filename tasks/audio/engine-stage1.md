# Futureboard Studio — Audio Engine + DSP Full System Roadmap

## Purpose

Design and implement the Futureboard Studio **Audio Engine + DSP system** as a full production DAW foundation.

This roadmap is split into **4 major stages**.

This file covers **Stage 1 first** in full detail, while also defining the high-level direction for Stages 2–4.

Futureboard audio must become:

```txt
Realtime-safe audio device layer
→ deterministic transport clock
→ immutable runtime project snapshot
→ audio graph / mixer / routing
→ DSP node chain
→ plugin host integration
→ recording / render / export
→ high-performance native DAW engine
```

This is a **system roadmap**, not a single implementation patch.

Do not implement all stages at once.

---

# 4-Stage Audio Engine + DSP Plan

## Stage 1 — Core Audio Engine Foundation

Stage 1 builds the reliable realtime foundation:

- Audio device backend abstraction
- Stream lifecycle
- Realtime callback rules
- Transport clock
- Runtime project snapshot
- Basic audio graph
- Track/master mixing
- Audio clip playback foundation
- Gain/pan/mute/solo
- Metering
- Command queue
- Engine diagnostics
- Safe UI/engine boundary
- No plugin DSP yet unless already isolated
- No heavy DSP chain yet
- No recording yet except scaffolding

## Stage 2 — DAW Routing + Plugin Runtime

Stage 2 adds real DAW graph power:

- Insert chains
- Bus / return / send / group routing
- Plugin processor hosting
- Plugin latency reporting
- Parameter bridge
- Basic automation runtime
- Instrument track audio path
- MIDI-to-plugin event routing
- Realtime-safe graph updates
- Plugin bypass/remove/reorder
- Runtime graph validation

## Stage 3 — Recording + Offline Render + Advanced Runtime

Stage 3 makes the engine production workflow capable:

- Audio recording
- Input monitoring
- Recording writer thread
- Recorded clip creation
- Waveform after record
- Offline bounce/export
- Stem export
- Freeze/bounce track
- Latency compensation / PDC
- Loop/punch recording
- Render queues
- Media asset management integration

## Stage 4 — DSP Suite + Pro Engine

Stage 4 builds Futureboard-native DSP power:

- Stock EQ
- Compressor
- Gate/expander
- Limiter
- Saturation
- Reverb/delay
- Analyzer
- Channel strip
- Sampler/synth foundations
- Oversampling
- SIMD optimization
- DSP graph profiling
- High-quality resampling
- Time-stretch/warp DSP
- Loudness analysis
- Future DAUx plugin protocol integration

---

# Stage 1 — Core Audio Engine Foundation

## Stage 1 Goal

Stage 1 should make Futureboard audio **stable, deterministic, and realtime-safe**.

The engine should be able to:

1. Discover audio devices.
2. Open an output stream.
3. Start/stop audio without freezing the UI.
4. Maintain a transport clock.
5. Load an immutable runtime project snapshot.
6. Process an empty project as silence.
7. Process basic audio clips if available.
8. Mix tracks into master.
9. Apply volume/pan/mute/solo.
10. Send meter data to UI safely.
11. Handle graph/project updates without realtime violations.
12. Provide debug diagnostics.
13. Avoid GPUI nested-update and UI/audio deadlocks.
14. Keep future plugin/recording/routing work possible.

---

## Stage 1 Hard Rules

- Do not block the UI thread when starting/stopping audio.
- Do not wait synchronously for audio callback acknowledgment.
- Do not call `LoadProject` on every play/stop/seek/edit.
- Do not allocate in the realtime callback.
- Do not lock blocking mutexes in the realtime callback.
- Do not perform filesystem I/O in the realtime callback.
- Do not scan plugins in the realtime callback.
- Do not decode MP3/WAV in the realtime callback.
- Do not call UI/GPUI from the audio callback.
- Do not log per audio block unless debug ring-buffered/throttled.
- Do not use Node/Electron in the realtime audio path.
- Do not route audio through JSON/IPC.
- Do not panic across audio backend callbacks.
- Empty project must play silence safely.
- Bad graph update must not kill currently running audio.
- Audio engine must remain usable without plugin host.
- Plugin processing is Stage 2 unless already safely integrated.

---

## Stage 1 Architecture

```txt
Futureboard UI / GPUI
    ↓ commands
AudioEngineController
    ↓ non-blocking command queue
RealtimeAudioEngine
    ↓ process callback
AudioGraphRuntimeSnapshot
    ↓
Audio Device Backend
```

Stage 1 components:

```txt
crates/SphereDirectAudioEngine/
├─ device/
│  ├─ mod.rs
│  ├─ wasapi.rs
│  ├─ coreaudio.rs
│  ├─ pipewire.rs
│  ├─ jack.rs
│  └─ null.rs
├─ engine/
│  ├─ mod.rs
│  ├─ controller.rs
│  ├─ callback.rs
│  ├─ command.rs
│  ├─ transport.rs
│  ├─ snapshot.rs
│  ├─ graph.rs
│  ├─ mixer.rs
│  ├─ meters.rs
│  └─ diagnostics.rs
├─ dsp/
│  ├─ gain.rs
│  ├─ pan.rs
│  ├─ meter.rs
│  └─ silence.rs
├─ media/
│  ├─ source.rs
│  ├─ clip.rs
│  └─ reader.rs
└─ lib.rs
```

The exact repo may differ. Inspect current structure first.

---

## Part A — Stage 1 Audit

Before changing code, create:

```txt
tasks/native/audio-engine-stage-1-audit.md
```

Search:

```bash
rg -n "SphereDirectAudioEngine|DAUx|AudioEngine|AudioCommand|LoadProject|Transport|Play|Stop|WASAPI|CoreAudio|PipeWire|JACK|cpal|callback|stream|meter|RuntimeProject|RuntimeTrack|RuntimeClip|graph|mixer|buffer|panic|Mutex|RwLock|try_send|recv|block_on" crates apps
```

Document:

- Current engine crate/module structure
- Current device backend path
- Current stream lifecycle
- Current play/stop path
- Current command queue
- Current project snapshot format
- Current runtime graph format
- Current audio callback rules
- Current meter path
- Current transport clock
- Current thread model
- Current failure modes
- Known UI/audio deadlock risks
- Files likely to change
- Build commands to validate

### Acceptance

- Audit file exists.
- It lists exact current audio flow from UI to callback.
- It identifies the first safe patch.

---

## Part B — Audio Device Backend Abstraction

## Goal

Make device discovery and stream lifecycle consistent across platforms.

### Supported Backends

Windows:

- WASAPI Shared
- WASAPI Exclusive later
- DAUx future

macOS:

- CoreAudio

Linux:

- PipeWire
- JACK
- ALSA fallback

Testing:

- Null backend
- Dummy sine/silence backend

### Device Info Model

```rust
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub backend: AudioBackendKind,
    pub direction: AudioDeviceDirection,
    pub is_default: bool,
    pub max_input_channels: u16,
    pub max_output_channels: u16,
    pub supported_sample_rates: Vec<u32>,
    pub default_sample_rate: Option<u32>,
    pub supported_buffer_sizes: Vec<u32>,
}
```

```rust
pub enum AudioBackendKind {
    Auto,
    Wasapi,
    CoreAudio,
    PipeWire,
    Jack,
    Alsa,
    Null,
}
```

```rust
pub enum AudioDeviceDirection {
    Input,
    Output,
    Duplex,
}
```

### Stream Config

```rust
pub struct AudioStreamConfig {
    pub backend: AudioBackendKind,
    pub input_device_id: Option<String>,
    pub output_device_id: Option<String>,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub input_channels: u16,
    pub output_channels: u16,
}
```

### Stream State

```rust
pub enum AudioStreamState {
    Closed,
    Opening,
    Warmed,
    Running,
    Stopping,
    Error(String),
}
```

### Rules

- Device enumeration must not block UI.
- Opening stream should happen on audio/control thread.
- UI must receive status asynchronously.
- If selected device disappears, engine enters recoverable error state.
- Device config must be validated before stream starts.
- Use Null backend fallback for tests.

### Acceptance

- Engine can enumerate output devices.
- Engine can enumerate input devices if backend supports it.
- Engine can open output stream.
- Engine can close stream cleanly.
- Empty stream outputs silence.
- Device status is visible in logs/status.

---

## Part C — Realtime Callback Contract

## Goal

Define and enforce callback behavior.

### Callback Responsibilities

Audio callback can:

- read current runtime snapshot pointer
- process graph
- write output buffer
- read input buffer if available
- update transport sample counter
- push meter values to realtime-safe meter queue
- drain lightweight realtime-safe commands if designed

Audio callback must not:

- allocate
- lock blocking mutex
- read/write files
- call UI/GPUI
- format strings
- scan plugins
- decode audio
- create/destroy graph nodes
- wait on channels
- call async runtime/block_on

### Runtime Context

```rust
pub struct AudioCallbackContext {
    pub sample_rate: u32,
    pub block_size: usize,
    pub sample_position: SampleFrame,
    pub transport_state: RuntimeTransportState,
    pub runtime_project: Arc<RuntimeProjectSnapshot>,
    pub scratch: AudioScratchBuffers,
}
```

### Safety Strategy

- Preallocate scratch buffers.
- Use immutable snapshot pointer.
- Use atomic transport state.
- Use bounded lock-free queues.
- Use per-block stack-local references only.
- Avoid heap growth after stream start.

### Acceptance

- Callback can run empty project.
- No panics in callback.
- No per-block logging in normal mode.
- Clear comments describe realtime restrictions.

---

## Part D — Engine Controller and Command Queue

## Goal

UI should control engine through non-blocking commands.

### Command Types

```rust
pub enum AudioEngineCommand {
    LoadProjectSnapshot(Arc<RuntimeProjectSnapshot>),
    Play,
    Stop,
    Pause,
    Seek { beat: f64 },
    SetTempo { bpm: f64 },
    SetLoop { enabled: bool, start_beat: f64, end_beat: f64 },
    SetTrackVolume { track_id: String, gain: f32 },
    SetTrackPan { track_id: String, pan: f32 },
    SetTrackMute { track_id: String, muted: bool },
    SetTrackSolo { track_id: String, solo: bool },
    Shutdown,
}
```

### Command Rules

- UI uses `try_send` or non-blocking send.
- Play/stop/seek must return immediately to UI.
- If command queue is full, log status and fail gracefully.
- Structural graph changes should be snapshot swaps.
- Small param changes may be lightweight commands.
- No UI code should hold `StudioLayout` lock while sending engine command.

### Command Outcome

```rust
pub enum EngineCommandResult {
    Accepted,
    DroppedQueueFull,
    Rejected(String),
}
```

### Acceptance

- Pressing Play never freezes UI.
- Stop never freezes UI.
- Seek never freezes UI.
- No blocking `recv()` in UI play path.
- Commands are logged with debug flag.

---

## Part E — Transport Clock

## Goal

Create deterministic playhead timing.

### Transport State

```rust
pub enum TransportMode {
    Stopped,
    Playing,
    Paused,
    Recording,
}
```

```rust
pub struct RuntimeTransportState {
    pub mode: TransportMode,
    pub sample_position: SampleFrame,
    pub beat_position: f64,
    pub bpm: f64,
    pub loop_enabled: bool,
    pub loop_start_beat: f64,
    pub loop_end_beat: f64,
}
```

### Rules

- Audio callback owns sample-position advancement while stream running.
- UI reads transport snapshot asynchronously.
- UI does not drive audio timing.
- Seek updates sample/beat position at safe boundary.
- Play starts from current transport position.
- Stop resets or pauses according to chosen DAW behavior.
- Empty project still advances transport.

### Stage 1 Tempo

- Static BPM only.
- Tempo map is Stage 2 timeline.
- Transport supports `SetTempo(bpm)` static.

### Acceptance

- Play advances transport.
- Stop stops transport.
- Seek changes playhead.
- Tempo drag uses lightweight `SetTempo`, not `LoadProject`.
- UI remains responsive.

---

## Part F — Runtime Project Snapshot

## Goal

Audio callback consumes immutable runtime project data.

### Runtime Project

```rust
pub struct RuntimeProjectSnapshot {
    pub id: String,
    pub sample_rate: u32,
    pub tracks: Vec<RuntimeTrack>,
    pub master: RuntimeMaster,
    pub graph: RuntimeAudioGraph,
}
```

### Runtime Track

```rust
pub struct RuntimeTrack {
    pub id: String,
    pub name: String,
    pub kind: RuntimeTrackKind,
    pub clips: Vec<RuntimeAudioClip>,
    pub volume_gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
}
```

```rust
pub enum RuntimeTrackKind {
    Audio,
    Instrument,
    Midi,
    Bus,
    Return,
    Master,
}
```

### Runtime Clip

```rust
pub struct RuntimeAudioClip {
    pub id: String,
    pub source_id: String,
    pub start_beat: f64,
    pub duration_beats: f64,
    pub source_offset_frames: u64,
    pub gain: f32,
    pub muted: bool,
}
```

### Stage 1 Rules

- Snapshot can contain zero clips.
- Zero clips = silence.
- Missing audio source = silence + status/error outside callback.
- Snapshot build happens outside callback.
- Failed snapshot build does not replace current runtime.

### Acceptance

- Project snapshot can be built from current project model.
- Snapshot can be loaded into engine.
- Empty project snapshot plays silence.
- Snapshot has debug summary logs.

---

## Part G — Basic Audio Graph

## Goal

Process tracks into master.

### Stage 1 Graph

```txt
Track Audio Source / Silence
→ Track Gain/Pan/Mute/Solo
→ Master Sum
→ Master Gain
→ Output
```

No Stage 1 insert plugins required.

No Stage 1 sends/returns required.

### Graph Nodes

```rust
pub enum RuntimeNodeKind {
    TrackSource,
    TrackMixer,
    MasterMixer,
    Output,
}
```

### Processing Order

1. Clear master buffer.
2. For each audible track:
   - render track source into track buffer
   - apply clip gain if applicable
   - apply track gain/pan
   - sum into master
3. Apply master gain.
4. Write to output buffer.
5. Push meters.

### Solo/Mute Rules

- If any track is soloed, only soloed tracks are audible.
- Muted tracks are silent.
- Master mute later.

### Acceptance

- Master outputs silence for empty project.
- Track gain/pan changes affect output if source exists.
- Mute/solo behavior is deterministic.
- No graph rebuild in callback.

---

## Part H — DSP Primitives Stage 1

## Goal

Create small, tested DSP primitives.

### Stage 1 DSP Modules

- Gain
- Pan
- Silence
- Sum/mix
- Peak meter
- RMS meter optional
- dB conversion
- Clamp helpers

### Gain

```rust
pub fn db_to_gain(db: f32) -> f32;
pub fn gain_to_db(gain: f32) -> f32;
pub fn apply_gain(buffer: &mut [f32], gain: f32);
```

### Pan

Use equal-power pan or simple linear pan.

Recommended:

```rust
pub enum PanLaw {
    EqualPower,
    Linear,
}
```

```rust
pub fn pan_gains(pan: f32, law: PanLaw) -> (f32, f32);
```

### Meter

```rust
pub struct PeakMeter {
    pub peak_l: f32,
    pub peak_r: f32,
}
```

Rules:

- No heap allocation.
- Unit tests for gain/pan/meter.
- Avoid denormals where relevant later.

### Acceptance

- Gain conversion tested.
- Pan tested.
- Meter tested.
- DSP primitives reused by graph.

---

## Part I — Audio Clip Playback Foundation

## Goal

Support basic clip playback if audio sources are already decoded/available.

Stage 1 can support one of:

1. Predecoded PCM buffer source.
2. Simple WAV reader source.
3. Placeholder source that returns silence until disk streaming is ready.

Recommended Stage 1:

- Start with predecoded PCM / cached clip data if already exists.
- Avoid full disk streaming in Stage 1 unless already scaffolded.
- Add source abstraction so Stage 2/3 can swap disk streaming.

### Audio Source

```rust
pub trait AudioSource: Send + Sync {
    fn id(&self) -> &str;
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> u16;
    fn total_frames(&self) -> u64;
    fn read_frames(
        &self,
        source_frame: u64,
        frames: usize,
        output: &mut [&mut [f32]],
    ) -> usize;
}
```

### Rules

- Source reads must be realtime-safe if called in callback.
- If source is not realtime-safe, pre-render/read into cache before playback.
- Missing/slow source outputs silence.
- Do not decode compressed audio in callback.

### Acceptance

- Runtime clip can render from available PCM source.
- Missing source does not panic.
- Clip start/end scheduling works at block level.
- Full disk streaming deferred.

---

## Part J — Metering

## Goal

Send meters to UI safely.

### Meter Data

```rust
pub struct TrackMeterFrame {
    pub track_id: String,
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms_l: Option<f32>,
    pub rms_r: Option<f32>,
}
```

### Rules

- Meter computation in callback is allowed if lightweight.
- Sending meters must use realtime-safe bounded queue/ring buffer.
- UI drains meters at 15/30/60 Hz.
- Meters must not trigger full project rerender.
- No string allocations in callback for meter IDs if avoidable.
  - Prefer numeric/compact runtime track handles.

### Acceptance

- Master meter works.
- Track meters work if tracks exist.
- UI update rate is throttled.
- Meter queue overflow is safe.

---

## Part K — Engine Diagnostics

## Goal

Add useful debug without breaking realtime safety.

### Debug Flags

```txt
FUTUREBOARD_AUDIO_DEBUG=1
FUTUREBOARD_AUDIO_DEVICE_DEBUG=1
FUTUREBOARD_AUDIO_COMMAND_DEBUG=1
FUTUREBOARD_AUDIO_CALLBACK_DEBUG=1
FUTUREBOARD_AUDIO_GRAPH_DEBUG=1
FUTUREBOARD_TRANSPORT_DEBUG=1
FUTUREBOARD_METER_DEBUG=1
```

### Logs

Allowed outside callback:

- device enumeration
- stream open/close
- command accepted/rejected
- snapshot summary
- graph node count
- play/stop/seek
- underrun/dropout summary

Callback logs:

- only first N blocks if debug enabled
- or ring-buffered counters
- never format every block in release path

### Diagnostics UI Later

- backend
- device
- sample rate
- buffer size
- callback running
- xrun count
- graph nodes
- CPU estimate
- dropped meter frames

### Acceptance

- Debug logs show audio lifecycle.
- Normal mode is quiet.
- Callback does not spam logs.

---

## Part L — UI / Engine Boundary

## Goal

Prevent UI/audio deadlocks and nested GPUI update panics.

### Rules

- UI creates commands.
- Engine receives commands asynchronously.
- Engine status returns via event queue.
- UI drains status events safely in StudioLayout update cycle.
- Child components do not update parent directly.
- Timeline/Mixer changes return `CommandOutcome`.
- StudioLayout applies dirty state after child update.

### Bad Pattern

```txt
StudioLayout.update
→ Timeline.update
→ Timeline calls StudioLayout.update
→ GPUI double lease panic
```

### Good Pattern

```txt
StudioLayout.update
→ Timeline.update returns CommandOutcome
→ StudioLayout applies dirty/status after child update returns
```

### Acceptance

- Play/stop does not freeze.
- Delete/edit commands do not double-update StudioLayout.
- Engine callback never touches GPUI.

---

## Part M — Shutdown Safety

## Goal

Audio engine shuts down cleanly.

### Shutdown Order

1. Set `is_shutting_down = true`.
2. Stop transport.
3. Stop audio stream.
4. Close command queue.
5. Stop control thread.
6. Stop meter/status queues.
7. Drop runtime snapshot.
8. Release backend resources.
9. Let GPUI close.

### Rules

- Do not rely only on `Drop`.
- Provide explicit `shutdown()`.
- During shutdown, ignore async UI callbacks safely.
- No TLS access after GPUI teardown.
- No background thread calls `cx.notify()` after shutdown starts.

### Acceptance

- Pressing X exits without TLS panic.
- Audio stream stops before app teardown.
- Shutdown logs are clear.

---

## Part N — Build and Test Commands

Run:

```bash
cargo check -p sphere_directaudioengine
cargo test -p sphere_directaudioengine
cargo check -p sphere_ui_components
cargo check --manifest-path apps/native/Cargo.toml
```

If crate names differ, inspect repo and use closest valid command.

Do not claim validation if not run.

---

# Stage 1 Phases

## Phase 1A — Audit + Realtime Contract

Deliverables:

- `tasks/native/audio-engine-stage-1-audit.md`
- callback hard-rules documented in code
- current play/stop path documented
- command queue risks identified

Acceptance:

- Audit complete.
- First safe patch identified.
- Build green.

---

## Phase 1B — Device Backend Foundation

Deliverables:

- `AudioDeviceInfo`
- `AudioStreamConfig`
- `AudioStreamState`
- backend enum
- output device enumeration
- input device enumeration scaffold
- null backend if useful

Acceptance:

- Device list works.
- Stream config validates.
- Logs show backend/device state.

---

## Phase 1C — Stream Lifecycle

Deliverables:

- open stream
- warm stream
- start/stop stream
- close stream
- recoverable error state
- no UI blocking

Acceptance:

- Stream starts.
- Stream stops.
- Empty output produces silence.
- No UI freeze.

---

## Phase 1D — Command Queue

Deliverables:

- non-blocking command sender
- Play/Stop/Seek/SetTempo commands
- command result/status
- no blocking wait in UI path

Acceptance:

- Press Play returns immediately.
- Press Stop returns immediately.
- Queue full handled safely.

---

## Phase 1E — Transport Clock

Deliverables:

- runtime transport state
- sample position
- beat position
- static BPM
- loop scaffold
- UI transport snapshot

Acceptance:

- Playhead advances.
- Stop works.
- Seek works.
- Tempo set works.

---

## Phase 1F — Runtime Project Snapshot

Deliverables:

- runtime project snapshot struct
- runtime track struct
- runtime clip struct
- master snapshot
- empty project safe path

Acceptance:

- Empty project snapshot loads.
- Snapshot summary logs.
- Failed snapshot does not replace current runtime.

---

## Phase 1G — Basic Graph + Mixer

Deliverables:

- clear buffers
- track-to-master sum
- volume
- pan
- mute
- solo
- master output

Acceptance:

- Empty project silence.
- Track gain/pan works if source exists.
- Mute/solo deterministic.

---

## Phase 1H — DSP Primitives

Deliverables:

- gain module
- pan module
- silence module
- meter module
- dB helpers
- tests

Acceptance:

- Unit tests pass.
- Graph uses primitives.

---

## Phase 1I — Basic Audio Clip Source

Deliverables:

- audio source trait
- PCM/cached source support if available
- missing source silence fallback
- block-level clip scheduling

Acceptance:

- Basic clip can play if source exists.
- Missing clip does not panic.
- No decode in callback.

---

## Phase 1J — Metering

Deliverables:

- master meter
- track meter
- meter queue
- UI throttle expectation
- dropped meter safe handling

Acceptance:

- Meter data reaches UI.
- No full app rerender spam.
- Queue overflow safe.

---

## Phase 1K — Diagnostics + Shutdown

Deliverables:

- debug flags
- engine lifecycle logs
- explicit shutdown
- shutdown guard integration

Acceptance:

- No TLS panic on close.
- Audio stops cleanly.
- Logs diagnose lifecycle.

---

## Phase 1L — QA / Stabilization

Deliverables:

- manual test checklist
- empty project play/stop
- audio device restart
- long idle test
- command stress test
- build/test validation

Acceptance:

- Stage 1 stable.
- No Play freeze.
- No shutdown panic.
- No realtime rule violations found.

---

# Stage 1 Recommended First Patch

Start with **Phase 1A only**.

Do not implement the whole engine.

## First Patch Deliverables

- Create `tasks/native/audio-engine-stage-1-audit.md`
- Document current engine path:
  - UI play button
  - transport command
  - audio command
  - stream callback
  - project snapshot
- Identify blocking waits / mutex risks.
- Identify callback violations.
- Add minimal debug logs if safe.
- Build/check.

Then stop.

---

# Stage 1 Manual Test Checklist

## Device

1. Launch app.
2. Audio devices enumerate.
3. Default output selected.
4. Stream warms.
5. No UI freeze.

## Empty Project

1. Start app with empty project.
2. Press Play.
3. UI remains responsive.
4. Transport advances.
5. Output is silence.
6. Press Stop.
7. Transport stops.

## Command Safety

1. Press Play repeatedly.
2. Press Stop repeatedly.
3. Seek while stopped.
4. Seek while playing.
5. Drag tempo.
6. No freeze.
7. No `LoadProject` spam.

## Snapshot

1. Load empty snapshot.
2. Add one track.
3. Load snapshot.
4. Remove track.
5. Load snapshot.
6. No panic.

## Metering

1. Play empty project.
2. Master meter stays silent.
3. If source exists, meter shows signal.
4. UI does not drop FPS from meters.

## Shutdown

1. Press Play.
2. Press window close.
3. Audio stops.
4. App exits cleanly.
5. No TLS panic.

## Build

```bash
cargo check -p sphere_directaudioengine
cargo test -p sphere_directaudioengine
cargo check -p sphere_ui_components
cargo check --manifest-path apps/native/Cargo.toml
```

---

# Stage 1 Final Acceptance

Stage 1 is complete when:

- Device enumeration works.
- Stream opens/stops safely.
- Empty project plays silence.
- Play/stop/seek do not freeze UI.
- Static BPM transport clock works.
- Runtime project snapshot loads.
- Basic graph/mixer exists.
- Gain/pan/mute/solo primitives work.
- Basic audio clip source abstraction exists.
- Metering reaches UI safely.
- Shutdown is clean.
- Debug diagnostics exist.
- Realtime callback rules are documented and followed.
- Build/check passes.

---

# Stage 2 Preview — DAW Routing + Plugin Runtime

Stage 2 will include:

- Track insert chains
- Bus tracks
- Return tracks
- Send routing
- Group routing
- Plugin processor graph
- VST3/CLAP runtime bridge
- Plugin latency reporting
- Parameter model
- Basic automation runtime
- MIDI-to-instrument plugin path
- Graph validation and cycle rejection
- Realtime-safe graph updates

Do not start Stage 2 until Stage 1 is stable.

---

# Stage 3 Preview — Recording + Offline Render

Stage 3 will include:

- Audio input capture
- Track record arm
- Input monitoring
- WAV writer thread
- Recording clip creation
- Waveform generation after record
- Offline render/export
- Stem export
- Freeze/bounce
- PDC / latency compensation
- Punch/loop recording

Do not start Stage 3 until Stage 1 + core routing are stable.

---

# Stage 4 Preview — DSP Suite + Pro Engine

Stage 4 will include:

- Stock EQ
- Compressor
- Gate
- Limiter
- Saturation
- Delay/Reverb
- Analyzer
- Channel strip
- Sampler/synth foundations
- Oversampling
- SIMD
- Profiling
- High-quality resampling
- Time-stretch/warp DSP
- Future DAUx plugin protocol

Do not start Stage 4 until engine graph/plugin runtime is stable.

---

# One-Line Summary

Stage 1 is:

```txt
Device + Stream + Callback + Command Queue + Transport + Runtime Snapshot + Basic Graph + Mixer + Metering + Shutdown.
```

Build this first.

End of Stage 1.
