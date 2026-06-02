# Futureboard Audio System Roadmap

Planning status: design-only pass. Do not implement code from this document without a separate implementation task.

This document is the production-oriented roadmap for Futureboard Studio's native audio side: device management, realtime engine, project/runtime snapshots, audio graph, mixer, routing, disk streaming, waveform cache, plugin hosting, MIDI/instrument playback, automation integration, recording, latency compensation, offline rendering, diagnostics, performance, and release readiness.

## 1. Core Direction

Futureboard must be a real DAW audio engine, not UI-driven audio.

The target architecture is:

```txt
GPUI / Project Commands
  -> Audio Engine Controller
    -> Immutable Runtime Snapshots
      -> Realtime-Safe Audio Graph
        -> Mixer / Routing / Plugins / Automation / Disk Streams
          -> Audio Device Callback
```

Audio principles:

- UI must never drive audio timing.
- UI edits create commands and project-state mutations.
- Runtime snapshots are built off the audio callback thread.
- Audio callback consumes prepared runtime data only.
- No allocation, filesystem access, plugin scan, UI call, blocking lock, string formatting, or project graph rebuild in the callback.
- Failed graph rebuilds must leave the previous valid graph running.

## 2. System Scope

The audio system covers:

- Audio backend/device system.
- Realtime callback design.
- Audio engine controller and command queue.
- Runtime project snapshots.
- Audio graph and routing graph validation.
- Audio tracks, MIDI tracks, instrument tracks, buses, returns, groups, and master.
- Audio clips, regions, fades, gain, source offsets, and future stretch/pitch modes.
- Disk streaming and media decode.
- Waveform/peak cache.
- Mixer core and metering.
- Inserts and plugin hosting.
- Sends/returns/buses/groups/master.
- MIDI and instrument routing.
- Automation read integration.
- Tempo map integration.
- Recording and monitoring.
- Latency reporting and compensation.
- Offline render/export.
- Diagnostics, recovery, safe mode.
- Performance and regression testing.

## 3. Architecture Overview

```txt
Futureboard Studio
├── UI Layer
│   ├── GPUI app shell
│   ├── timeline
│   ├── mixer
│   ├── plugin view shell
│   ├── preferences
│   └── command dispatcher
│
├── Project State
│   ├── tracks
│   ├── clips
│   ├── routing
│   ├── plugins
│   ├── automation
│   ├── tempo map
│   └── save/load
│
├── Audio Engine Controller
│   ├── audio device backend
│   ├── command queue
│   ├── runtime snapshot builder
│   ├── transport bridge
│   └── diagnostics
│
├── Realtime Audio Engine
│   ├── callback
│   ├── graph processor
│   ├── mixer
│   ├── disk streaming readers
│   ├── plugin processors
│   ├── automation evaluator
│   └── meters
│
├── Plugin Host
│   ├── VST3 host
│   ├── CLAP host
│   ├── AU host later
│   ├── plugin editor bridge
│   └── plugin scanner process
│
└── Background Workers
    ├── audio import/decode
    ├── waveform peak generation
    ├── media analysis
    ├── plugin scan
    ├── offline render
    └── cache cleanup
```

## 4. Threads and Responsibilities

UI thread:

- GPUI rendering.
- User input.
- Project editing.
- Mixer UI.
- Plugin editor shell.
- Command creation.
- Must not perform heavy decode, plugin scan, waveform generation, or blocking device operations.

Audio callback thread:

- Process audio blocks.
- Read active runtime snapshot.
- Process graph.
- Emit meters to realtime-safe buffer.
- Apply graph swap at block boundary.
- Must not allocate, block, log per block, read project state, or open files.

Audio engine control thread:

- Receives UI commands.
- Applies control-side engine changes.
- Builds runtime snapshots.
- Validates routing graph.
- Starts/stops/restarts devices.
- Handles device changes and errors.

Disk streaming pool:

- Prefetch audio file chunks.
- Decode compressed formats into cache.
- Fill ring buffers.
- Recover from underruns.

Waveform/analysis workers:

- Generate peak cache.
- Analyze loudness later.
- Detect transients later.
- Run expensive media analysis away from UI/audio callback.

Plugin scanner process:

- Scans plugins safely.
- Timeouts slow plugins.
- Blacklists repeated crashes.
- Writes plugin index cache.
- Must not crash the main app.

Plugin UI/platform thread:

- Attach/resize/detach native plugin editor views.
- Obey HWND/NSView/platform main-thread rules.
- Never be part of audio processing.

## 5. Audio Backend Support

Backend policy:

- Windows: WASAPI Shared, WASAPI Exclusive
- macOS: CoreAudio.
- Linux: PipeWire, JACK, ALSA fallback.

Settings:

- Backend.
- Input device.
- Output device.
- Sample rate.
- Buffer size.
- Channel layout.
- Exclusive mode.
- Auto reconnect.
- Safe mode.
- Drift compensation later.

Device commands:

- EnumerateDevices.
- SetBackend.
- SetInputDevice.
- SetOutputDevice.
- SetSampleRate.
- SetBufferSize.
- RestartAudioDevice.
- TestOutput.
- TestInput.
- ResetAudioEngine.

Device states:

- Closed.
- Opening.
- Ready.
- Running.
- Error.
- Recovering.
- DeviceLost.

Acceptance:

- Device can start/stop safely.
- Device enumeration never freezes UI.
- Sample-rate/buffer changes use controlled restart or reconfiguration.
- Callback receives stable sample rate and block size.

## 6. Project Audio Model

Project:

- ID/name.
- Sample rate.
- BPM/tempo map.
- Time signature.
- Tracks.
- Master track.
- Markers.
- Media pool.
- Plugin references.
- Routing.
- Automation.
- Project folder.

Track kinds:

- Audio.
- Instrument.
- MIDI.
- Bus.
- Return.
- Group.
- Master.

Track fields:

- ID/name/kind/color.
- Clips.
- Inserts.
- Sends.
- Routing.
- Volume/pan/mute/solo.
- Armed/monitoring.
- Automation lanes.

Clip kinds:

- Audio clip.
- MIDI clip.
- Automation clip later.
- Pattern clip later.

Audio clip:

- ID/name.
- Source path/media handle.
- Start beat.
- Duration beats.
- Source offset.
- Gain.
- Fade in/out.
- Muted/selected.
- Stretch mode later.
- Reverse later.
- Pitch shift later.

Media pool:

- Source path.
- Project media path.
- Sample rate.
- Channels.
- Duration.
- File hash.
- Peak cache status.
- Analysis status.
- Missing status.

## 7. Runtime Snapshot Model

Project state is editable. Runtime state is immutable and realtime-ready.

```rust
pub struct RuntimeProjectSnapshot {
    pub sample_rate: f64,
    pub block_size: u32,
    pub transport: RuntimeTransportSnapshot,
    pub tempo_map: RuntimeTempoMapSnapshot,
    pub tracks: Vec<RuntimeTrackSnapshot>,
    pub routing_graph: RuntimeRoutingGraph,
    pub automation: RuntimeAutomationSnapshot,
    pub media: RuntimeMediaSnapshot,
    pub latency: RuntimeLatencyGraph,
}

pub struct RuntimeTrackSnapshot {
    pub track_id: RuntimeTrackId,
    pub kind: RuntimeTrackKind,
    pub clips: Vec<RuntimeClipSnapshot>,
    pub inserts: Vec<RuntimeInsertSnapshot>,
    pub sends: Vec<RuntimeSendSnapshot>,
    pub output: RuntimeOutputTarget,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub latency_samples: u32,
}

pub struct RuntimeClipSnapshot {
    pub clip_id: RuntimeClipId,
    pub source: RuntimeSourceHandle,
    pub start_beat: f64,
    pub duration_beats: f64,
    pub source_offset_seconds: f64,
    pub gain: f32,
    pub fade_in: RuntimeFade,
    pub fade_out: RuntimeFade,
}
```

Graph update flow:

1. UI creates command.
2. Control thread mutates project state or receives changed project state.
3. Snapshot builder validates model and routing.
4. Snapshot builder prepares runtime handles, sorted events, graph order, buffers, and latency data.
5. Snapshot is atomically swapped at a block boundary.
6. If validation fails, old snapshot keeps running and UI receives an error.

Acceptance:

- Audio callback never reads mutable UI project state.
- Runtime graph swaps safely.
- Failed graph builds do not kill current playback.

## 8. Audio Graph and Routing

Graph node types:

- AudioInputNode.
- AudioClipNode.
- MidiClipNode.
- InstrumentNode.
- InsertPluginNode.
- TrackMixerNode.
- SendNode.
- ReturnTrackNode.
- BusTrackNode.
- GroupTrackNode.
- MasterNode.
- OutputNode.
- MeterNode.

Audio track flow:

```txt
Audio clips / input
  -> track pre-gain
  -> inserts
  -> volume/pan
  -> sends
  -> output target
  -> bus/group/master
  -> master inserts
  -> master fader
  -> output
```

Return flow:

```txt
send receive
  -> return inserts
  -> return fader
  -> output target
  -> master
```

Bus flow:

```txt
track outputs summed
  -> bus inserts
  -> bus fader
  -> output target
  -> master
```

Routing rules:

- Output target can be master, bus, group, hardware later.
- Sends can target returns or buses.
- Detect cycles before building runtime graph.
- Topologically sort nodes.
- Reject invalid routes with UI feedback.

Initial routing limits:

- Stereo only first.
- Post-fader sends first.
- Pre-fader sends later.
- No feedback routing.

Cycle examples to reject:

- Track A -> Bus B -> Track A.
- Return A sends to itself.
- Master routes back to track.
- Bus A -> Bus B -> Bus A.

## 9. Mixer System

Mixer strip:

- Track name/type/color.
- Inserts.
- Sends.
- Pan.
- Volume fader.
- Meter.
- Mute/solo/record/input monitor.
- Output routing.
- Latency indicator later.

Runtime mixer:

- Per-track buffer.
- Post-insert buffer.
- Send buffer.
- Bus accumulation buffer.
- Master buffer.

Mixer commands:

- SetTrackVolume.
- SetTrackPan.
- SetTrackMute.
- SetTrackSolo.
- SetTrackArm.
- SetTrackMonitor.
- SetTrackOutput.
- AddSend.
- RemoveSend.
- SetSendGain.
- SetSendTarget.
- SetSendPrePost.
- AddBusTrack.
- AddReturnTrack.
- AddGroupTrack.
- AddInsert.
- RemoveInsert.
- SetInsertBypass.
- MoveInsert.

Metering:

- Peak meter first.
- RMS later.
- LUFS later.
- Meter decay.
- Peak hold.
- Clip indicator.
- Meter update throttled to 15/30/60 Hz.
- No UI update per audio block.

## 10. Plugin System

Plugin format order:

- VST3 first.
- CLAP next.
- AU on macOS.
- LV2 later for Linux.

Plugin scanner:

- Separate process.
- Scans default/custom paths.
- Writes plugin index.
- Bad plugins do not crash main app.
- Timeout slow plugins.
- Blacklist repeated crashes.
- Shallow scan first, deep validation later.

Plugin index:

- ID, format, name, vendor, version, path.
- Category/class ID/bundle ID.
- Instrument/effect classification.
- Audio/MIDI input/output layout.
- Last scanned and file modified time.
- Status/crash count.
- Metadata JSON for non-realtime use only.

Host architecture:

- `SpherePluginHost` pure core API.
- N-API wrapper for Electron only.
- Native GPUI links pure host API.
- No N-API symbols in native binary.
- Plugin editor hosted separately from processing.

Insert loading flow:

1. User selects plugin.
2. UI sends `LoadPluginInsert`.
3. Control thread validates descriptor.
4. Host creates plugin instance.
5. Runtime graph receives insert node.
6. Audio callback processes plugin.
7. UI shows Ready/Failed.

Plugin editor:

- GPUI `PluginView` shell.
- Native child HWND/NSView host region.
- VST3 `IPlugView` attach/resize/remove.
- GPUI draws header only.
- Close detaches/removes editor.
- Resize calls plugin view size APIs.
- Error fallback UI if editor cannot open.

Plugin parameters:

- Stable parameter ID.
- Normalized `0.0..=1.0`.
- Display value text/unit.
- Automation target.
- UI/editor changes route to runtime safely.
- Automation changes route to plugin processor/controller.
- Smoothing where needed.

## 11. MIDI and Instrument Audio Integration

Instrument track flow:

```txt
MIDI clips / MIDI input
  -> MIDI event scheduler
  -> instrument plugin
  -> inserts
  -> volume/pan
  -> sends
  -> output
```

MIDI runtime:

- Note on/off.
- CC events.
- Pitch bend.
- Channel pressure.
- Sample accurate later.
- Block accurate first if documented.

MIDI input:

- Device selection.
- Track arm.
- Input monitor.
- Record MIDI.
- Quantize later.
- MIDI panic/all notes off.

Acceptance:

- MIDI clip can play instrument plugin.
- MIDI input can monitor instrument track later.
- Stop sends all-notes-off/panic as needed.

## 12. Automation Integration

Targets:

- Track volume.
- Track pan.
- Send gain.
- Insert bypass.
- Plugin parameters.
- Master volume.
- Master plugin parameters.
- Tempo.
- Time signature later.

Initial support:

- Read mode first.
- Precompiled lane representation.
- Evaluate by beat.
- Smooth volume/pan/plugin params.
- No allocation in callback.

Tempo automation:

- Special system via tempo map.
- Initial static tempo.
- Tempo points and beat/time conversion later.
- Full tempo map affects playback clock, MIDI scheduling, grid/ruler, and eventually audio stretch behavior.

## 13. Transport and Clock

Transport:

- Play, stop, pause, record.
- Loop.
- Seek.
- Rewind/forward.
- Go start/end.
- Follow playhead.
- Auto-scroll.
- Metronome.
- Count-in.
- Pre-roll.

Clock:

- Sample position.
- Beat position.
- Seconds.
- BPM.
- Tempo map.
- Time signature.

Rules:

- Audio callback owns sample clock while running.
- UI reads transport snapshot.
- Seek command applies safely at block boundary.
- Tempo drag sends lightweight tempo command, not full project reload.
- Auto-scroll is UI-only and must not reload audio engine.

## 14. Disk Streaming and Media Decode

Media import flow:

1. User imports file.
2. Worker probes metadata.
3. Compressed sources decode to cache or stream decoder.
4. Waveform worker generates peaks.
5. Project stores media handle.
6. Timeline shows pending/partial/ready waveform status.

Disk streaming:

- Callback reads from prefilled buffers.
- Disk workers prefetch chunks.
- Ring buffer per active clip/source.
- Underrun detection.
- Fallback silence if missing.
- No file IO in callback.

Format order:

- WAV first.
- AIFF.
- FLAC.
- MP3 through worker/cache.
- OGG later.
- CAF on macOS later.

Cache layout:

```txt
Project/Media/Audio
Project/Media/Imports
Project/Cache/Audio
Project/Cache/Peaks
Project/Cache/Waveform
Project/Cache/Analysis
```

Waveform:

- Chunked peaks.
- Multiple LODs.
- Cache by file hash + modified time.
- Visible-only draw.
- Never generate peaks in render.

## 15. Recording System

Audio recording:

- Arm track.
- Select input device/channel.
- Monitor off/auto/input.
- Record WAV initially.
- Write to project media folder.
- Create clip after recording.
- Generate waveform after recording.
- Take lanes later.
- Punch in/out later.

MIDI recording:

- Arm MIDI/instrument track.
- Record note/CC input.
- Create MIDI clip.
- Overdub later.
- Quantize-on-record later.

Monitoring:

- Off.
- Auto.
- Input.
- Direct monitor later if device supports.
- Software monitoring through track chain when enabled.

Recording latency compensation:

- Input latency.
- Output latency.
- Plugin latency.
- Manual offset first.
- Measured loopback later.

## 16. Latency Compensation

Latency sources:

- Input device latency.
- Output device latency.
- Buffer size.
- Plugin latency.
- Lookahead plugins.
- Bus/return routing.
- Recording offset.

Initial:

- Query/display plugin latency.
- Collect insert latency.
- Display track/master latency.
- Manual recording offset.

Full PDC:

- Compute max graph latency.
- Delay shorter paths.
- Handle sends/returns.
- Prevent feedback cycles.
- Align meters/automation.
- Compensate recording placement.

## 17. Offline Render / Export

Export scopes:

- Full mix.
- Selected range.
- Master only.
- Stems later.
- Buses.
- Individual tracks.

Formats:

- WAV first.
- FLAC.
- AIFF.
- MP3 later.
- Bit depth.
- Sample-rate conversion.
- Dithering.

Offline engine:

- Not realtime callback.
- May allocate.
- Can process faster than realtime.
- Plugin offline mode if supported.
- Progress and cancel support.

## 18. Settings and Diagnostics

Audio settings:

- Backend.
- Input/output devices.
- Sample rate.
- Buffer size.
- Monitor mode.
- Recording format/bit depth.
- Latency compensation.
- Driver status.

Performance settings:

- GPU/CPU render.
- Waveform quality.
- Meter update rate.
- Disk streaming cache size.
- Worker thread count.
- Low-end GPU mode.

Plugin settings:

- Scan paths.
- Enabled formats.
- Scan on startup.
- Scanner sandbox mode.
- Failed plugin behavior.
- Rescan and clear cache.

Diagnostics flags:

- `FUTUREBOARD_AUDIO_DEBUG=1`.
- `FUTUREBOARD_AUDIO_CALLBACK_DEBUG=1` must be ring-buffered/throttled.
- `FUTUREBOARD_ROUTING_DEBUG=1`.
- `FUTUREBOARD_PLUGIN_DEBUG=1`.
- `FUTUREBOARD_PLUGIN_VIEW_DEBUG=1`.
- `FUTUREBOARD_PLUGIN_SCAN_DEBUG=1`.
- `FUTUREBOARD_WAVEFORM_DEBUG=1`.
- `FUTUREBOARD_DISK_STREAM_DEBUG=1`.
- `FUTUREBOARD_AUTOMATION_DEBUG=1`.
- `FUTUREBOARD_TRANSPORT_DEBUG=1`.

Diagnostics UI:

- Audio ready/error.
- Backend, device, sample rate, buffer size.
- Latency.
- XRuns/dropouts.
- CPU load.
- Graph nodes.
- Plugin count.
- Disk stream status.
- Peak cache status.

## 19. Performance Requirements

Targets:

- Stable realtime callback under normal load.
- Low idle CPU.
- UI FPS stable while playback runs.
- Waveform generation does not block playback.
- Plugin scan does not block startup.
- 32 tracks usable.
- Old laptop safe mode usable.

Optimization rules:

- No allocations in callback.
- No locks in callback.
- Avoid full graph rebuild for parameter changes.
- Virtualize tracks/clips/mixer strips.
- Chunk waveform data.
- Cap meter updates.
- Throttle UI notifications.
- Batch commands.
- Background decode.

Low-end mode:

- Lower meter rate.
- Lower waveform detail.
- Fewer grid lines.
- Reduced visual effects.
- CPU render fallback.
- Integrated GPU friendly defaults.

## 20. Phase A-Z Roadmap

### Phase A - Audio Architecture Audit

Goals: inspect current `SphereDirectAudioEngine`, Electron/WebAudio flow, plugin host, project snapshot, mixer commands, and current gaps.

Acceptance: gap document exists before engine changes.

### Phase B - Realtime Safety Foundation

Goals: define callback rules, command queue policy, snapshot swap model, and debug counters.

Acceptance: all future engine work has a realtime safety contract.

### Phase C - Device Backend Stabilization

Goals: stabilize WASAPI Shared/Exclusive, CoreAudio plan, PipeWire/JACK/ALSA plan, and device status UI.

Acceptance: device start/stop/restart and settings changes are controlled.

### Phase D - Project Runtime Snapshot

Goals: stable runtime project, track, clip, master, routing, and transport snapshot.

Acceptance: callback reads runtime snapshot only.

### Phase E - Audio Clip Playback

Goals: WAV playback, source handles, basic clip scheduling, gain/fade placeholders.

Acceptance: simple audio clip plays reliably.

### Phase F - Disk Streaming

Goals: background reader, ring buffers, underrun handling, long file playback.

Acceptance: long audio file does not freeze app or allocate in callback.

### Phase G - Media Import/Decode

Goals: WAV/AIFF/FLAC/MP3 import, decode workers, project media folder, missing media state.

Acceptance: compressed import happens off UI/audio threads.

### Phase H - Waveform Cache

Goals: peak generation, chunked cache, LODs, pending/partial/ready states.

Acceptance: waveform generation never runs during render.

### Phase I - Mixer Core

Goals: volume/pan/mute/solo, master fader, meters, pan law.

Acceptance: mixer controls affect playback and meters are throttled.

### Phase J - Insert Plugin Loading

Goals: plugin descriptor to insert, VST3 instantiate, runtime insert processing, bypass/remove.

Acceptance: VST3 effect processes audio on a track.

### Phase K - Plugin Host De-NAPI

Goals: pure plugin host core, thin N-API wrapper, native links pure API.

Acceptance: native GPUI path does not depend on N-API symbols.

### Phase L - Plugin Editor Hosting

Goals: GPUI `PluginView`, native child HWND/NSView, VST3 attach/resize/remove, fallback error UI.

Acceptance: plugin editor opens/closes without blocking audio.

### Phase M - Bus Track

Goals: route track output to bus, bus inserts, bus to master.

Acceptance: bus routing works and cycles are rejected.

### Phase N - Return Track / Sends

Goals: send slots, send gain, post-fader sends, return inserts, return to master.

Acceptance: send to return works.

### Phase O - Routing Graph Validation

Goals: cycle detection, topological order, invalid route UI feedback.

Acceptance: invalid graph never reaches callback.

### Phase P - MIDI Runtime Foundation

Goals: MIDI clip event scheduling, note on/off, stop panic, instrument routing placeholder.

Acceptance: MIDI runtime events are deterministic.

### Phase Q - Instrument Tracks

Goals: VST3 instrument insert, MIDI to plugin, plugin audio output to track.

Acceptance: MIDI clip plays instrument plugin.

### Phase R - Automation Read Mode

Goals: track volume, pan, send, plugin parameter automation.

Acceptance: automation read affects runtime safely.

### Phase S - Plugin Parameter Model

Goals: parameter discovery, normalized values, editor events, automation target creation.

Acceptance: plugin parameters are stable automation targets.

### Phase T - Tempo Map Foundation

Goals: static tempo model, tempo point model, beat/time conversion API.

Acceptance: tempo map model exists before tempo playback changes.

### Phase U - Recording

Goals: audio record to WAV, MIDI record, monitoring modes, create clips.

Acceptance: recording finalizes media and clips safely.

### Phase V - Latency Reporting

Goals: device/plugin/track latency display.

Acceptance: latency sources are visible and testable.

### Phase W - Latency Compensation

Goals: graph latency propagation, playback compensation, recording offset compensation.

Acceptance: PDC aligns normal playback paths.

### Phase X - Offline Render

Goals: master WAV export, selected range, stems later.

Acceptance: export renders without realtime device.

### Phase Y - Performance / Stability

Goals: no callback allocation, UI notify throttling, meter caps, low-end mode, stress testing.

Acceptance: stress scenarios pass without dropouts under expected load.

### Phase Z - Release Readiness

Goals: crash recovery, safe mode, plugin blacklist, docs, QA checklist, signed builds.

Acceptance: release checklist is complete.

## 21. Recommended Implementation Order

Recommended first slices:

1. Phase A: audit.
2. Phase B: realtime safety rules.
3. Phase D: runtime snapshot stabilization.
4. Phase I: mixer core.
5. Phase J: insert plugin loading.
6. Phase K: de-NAPI plugin host.
7. Phase L: GPUI PluginView native editor.
8. Phase M/N/O: buses, sends, returns, routing validation.
9. Phase F/G/H: disk streaming, media decode, waveform cache.
10. Phase P/Q: MIDI/instrument runtime.
11. Phase R/S: automation/plugin parameters.
12. Phase U/X: recording/export.
13. Phase V/W: latency/PDC.

Do not do all phases at once. Keep builds green and runtime safe after every slice.

## 22. Testing Plan

Unit tests:

- Routing cycle detection.
- Graph topological sort.
- Automation evaluation.
- Tempo map conversion.
- Clip scheduling.
- Pan law.
- Gain conversion.
- Send routing.
- Project save/load roundtrip.

Integration tests:

- Load project with audio clips.
- Load plugin insert.
- Bypass plugin.
- Save/load plugin insert.
- Bus routing.
- Return send.
- Offline render.
- Waveform cache generation.
- Device settings persistence.

Manual tests:

- Play/stop/seek.
- Import WAV/MP3.
- Add 32 tracks.
- Add VST3 plugin.
- Open/close plugin editor.
- Send to return.
- Route track to bus.
- Record audio.
- Record MIDI.
- Export WAV.
- Unplug device.
- Change sample rate.
- Low-end GPU mode.

Stress tests:

- 100 tracks with no clips.
- 32 tracks with audio clips.
- 1000 clips.
- 3-hour audio file.
- Many waveform chunks.
- 20 plugins.
- Bad plugin scan.
- Repeated plugin editor open/close.
- Resize window during playback.
- Auto-scroll during playback.

Realtime tests:

- No callback allocation.
- Underrun detection.
- CPU load.
- Plugin latency.
- Device restart.
- Command queue overflow handling.

## 23. Risks

High:

- Plugin hosting stability.
- Plugin editor parenting.
- Realtime safety.
- Routing graph cycles.
- Disk streaming underruns.
- MP3 decode performance.
- macOS AU scan.
- Linux plugin editor support.
- Latency compensation with sends/returns.
- Tempo automation beat/time mapping.

Medium:

- WGPU/GPUI viewport interop.
- GPU device selection.
- Mixer meter repaint cost.
- Waveform cache invalidation.
- Project format migration.
- MIDI scheduling accuracy.

Low:

- Static mixer UI.
- Basic audio clip playback.
- Basic volume/pan.
- Settings UI.
- Save/load simple fields.

## 24. Complete Audio System Acceptance Criteria

Playback:

- Audio clips play reliably.
- Long files stream without freezing.
- Play/stop/seek stable.
- No obvious dropouts under expected load.

Mixer:

- Tracks route to master.
- Volume/pan/mute/solo work.
- Meters work.
- Master bus works.

Plugins:

- VST3 insert loads.
- Plugin processes audio.
- Plugin editor opens in GPUI shell.
- Bypass/remove works.
- Failed plugins do not crash app.

Routing:

- Bus tracks work.
- Return tracks work.
- Sends work.
- Cycles rejected.

MIDI:

- MIDI clips play instrument plugin.
- Stuck notes handled.
- MIDI editor saves/loads.

Automation:

- Volume/pan read automation works.
- Plugin parameter read automation works.
- Master automation works.

Recording:

- Audio recording creates WAV clip.
- MIDI recording creates MIDI clip.

Export:

- Master WAV export works.

Stability/performance:

- No allocation in callback.
- No UI work in callback.
- Bad plugin does not kill app.
- Device loss handled.
- Project save/load roundtrips.
- 32 tracks usable.
- Old laptop safe mode usable.
- Waveform generation backgrounded.
- Plugin scan backgrounded.
