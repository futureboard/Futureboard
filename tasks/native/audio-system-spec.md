# Futureboard Audio Engine Technical Specification

Planning status: specification draft. This is not implementation code.

## 1. Non-Negotiable Runtime Contract

The audio callback is a realtime processor. It may only consume prepared data and write bounded realtime-safe outputs.

The callback must not:

- Allocate heap memory.
- Block on mutexes or unbounded channels.
- Open/read/seek files directly.
- Decode compressed audio.
- Scan/load plugins.
- Attach/open/close plugin editors.
- Read GPUI or project state.
- Format strings or log per block.
- Rebuild the graph.
- Call N-API, Electron, or UI APIs.

The callback may:

- Read immutable runtime snapshots.
- Process preallocated buffers.
- Process stable plugin instances.
- Evaluate precompiled automation.
- Consume bounded realtime-safe commands.
- Write meters to a ring buffer.
- Swap prepared graph snapshots at block boundary.

## 2. Command and Snapshot Boundary

UI/project edits must cross into audio through commands or rebuilt snapshots.

Command categories:

- Transport: Play, Stop, Pause, SeekBeat, SetLoop, SetTempo.
- Device: StartDevice, StopDevice, RestartDevice, SetBackend, SetBufferSize, SetSampleRate, SetInput, SetOutput.
- Project/graph: LoadProjectSnapshot, AddTrack, RemoveTrack, AddClip, RemoveClip, MoveClip, UpdateClip, SetTrackRouting.
- Mixer: SetTrackVolume, SetTrackPan, SetTrackMute, SetTrackSolo, SetSendGain, SetMasterVolume.
- Plugin: LoadPlugin, UnloadPlugin, SetPluginParameter, BypassInsert.
- Automation: AddAutomationLane, RemoveAutomationLane, AddAutomationPoint, MoveAutomationPoint, SetAutomationReadMode.

Rules:

- Small commands for simple parameter changes.
- Full snapshot only for project load or structural rebuild.
- Batch commands for multi-edit.
- Never use full `LoadProject` for every tiny change.
- Plugin editor open/close is UI/control-side, not audio callback-side.

## 3. Runtime Data Requirements

Runtime data must be:

- Immutable once published.
- Validated before publish.
- Free of project/UI string lookup in callback paths.
- Sorted for efficient traversal.
- Backed by stable numeric handles where possible.
- Prepared with all required buffers/scratch state outside the callback.

Runtime snapshot publication must support:

- Current graph remains active if new graph validation fails.
- New graph becomes active at a block boundary.
- Old graph is dropped only when callback can no longer observe it.
- Diagnostics report graph build failure without stopping audio.

## 4. Audio Device Specification

Device lifecycle:

```txt
Closed -> Opening -> Ready -> Running
Running -> Recovering -> Running
Running -> Error
Running -> DeviceLost -> Recovering | Closed
```

Device configuration:

- Backend.
- Input device.
- Output device.
- Sample rate.
- Buffer size.
- Channel layout.
- Exclusive mode.

Configuration changes:

- If backend requires restart, UI must show restart-required state.
- Restart must be controlled by engine controller.
- Callback must never see partially-applied sample-rate/block-size changes.

## 5. Audio Graph Specification

Graph validation:

- All target IDs resolve.
- Routing graph has no cycles.
- Master does not route back to tracks/buses.
- Sends do not target their own return path.
- Channel layout is supported.
- Plugin IO layouts are compatible or explicitly adapted.

Graph processing:

- Topological order.
- Clear accumulation buffers before use.
- Process source nodes.
- Process inserts.
- Apply fader/pan.
- Accumulate sends/returns/buses.
- Process master.
- Write output.
- Emit meters.

Initial constraints:

- Stereo tracks and master.
- Post-fader sends first.
- No feedback routing.
- No hardware output routing beyond main output.

## 6. Mixer Math

Gain:

- UI fader may use dB scale.
- Runtime uses linear gain.
- `-inf` maps to zero.
- Smoothing required for audible continuous gain changes.

Pan:

- Define pan law before implementation.
- Initial suggested law: equal-power stereo pan.
- Pan automation must use same law as mixer fader.

Mute/solo:

- Mute is discrete.
- Solo resolution should happen before runtime graph or in a prepared track active mask.
- Solo changes should not require full graph rebuild if avoidable.

Meters:

- Peak first.
- RMS/LUFS later.
- Meter writes to ring buffer.
- UI polls/throttles meter updates.

## 7. Media and Streaming Specification

Source handles:

- Runtime clip references a source handle, not a path string.
- Source handle resolves to streaming/cache reader prepared off callback.
- Missing media returns silence and diagnostic state.

Streaming:

- Disk workers prefetch.
- Callback reads from ring/prepared buffers.
- Underrun produces silence and increments counter.
- Worker catches up where possible.

Decode:

- WAV may stream directly.
- Compressed formats decode in worker/cache path.
- MP3/FLAC decode never happens in callback.

Waveforms:

- Peak cache generated in worker.
- Multiple LODs.
- Cache invalidated by file hash/modified time.
- Render uses cached peaks only.

## 8. Plugin Specification

Host:

- Pure host core for native.
- N-API wrapper only for Electron.
- Scanner out-of-process.
- Processing path does not depend on N-API.

Plugin instance lifecycle:

```txt
Descriptor -> Load Requested -> Created -> Initialized -> Active
Active -> Suspended
Active -> Failed
Active -> Unloaded
```

Processing:

- Plugin process called from audio graph.
- Plugin buffers preallocated or owned by graph processor.
- Plugin latency reported to latency graph.
- Plugin crash handling depends on process isolation maturity; scanner must be isolated first.

Parameters:

- Stable parameter ID.
- Normalized value.
- Display text/unit from plugin/controller where available.
- UI changes use control path.
- Automation changes use runtime-safe parameter path.
- Smoothing policy must be explicit.

Editor:

- Native child view hosted by GPUI shell.
- Editor attach/resize/detach on correct platform thread.
- Editor failure does not affect audio processing.

## 9. Automation Specification

Automation read mode:

- Runtime evaluator receives precompiled lanes.
- Values evaluated by beat.
- Continuous params smoothed.
- Discrete params use hold semantics.
- Missing/unresolved targets are skipped and diagnosed.

Tempo automation:

- Separate tempo map runtime.
- Static tempo remains valid until full tempo map playback lands.
- Beat/time conversion must be tested before tempo automation affects callback scheduling.

Plugin automation:

- Target resolves to plugin instance handle + parameter handle.
- Missing plugin/parameter preserves lane but skips dispatch.
- Automation values are normalized.

## 10. Recording Specification

Audio recording:

- Recording writer runs outside callback.
- Callback writes input/audio data into bounded buffer or recording path designed for realtime safety.
- File finalized on stop.
- Clip created only after valid file exists.
- Waveform generation starts after recording finalizes.

MIDI recording:

- MIDI input events timestamped.
- Events converted to clip-local beats.
- Panic/all-notes-off on stop.
- Overdub and quantize-on-record later.

Monitoring:

- Off, Auto, Input.
- Software monitoring passes through track chain.
- Direct monitoring later if backend supports it.

## 11. Latency Specification

Latency graph must account for:

- Device input latency.
- Device output latency.
- Buffer size.
- Plugin latency.
- Lookahead.
- Bus/return routing.
- Recording offset.

Initial deliverables:

- Query plugin latency.
- Display track/master latency.
- Manual recording offset.

Full PDC:

- Max graph latency.
- Delay shorter paths.
- Send/return aware compensation.
- Automation and meter alignment.
- Recording placement compensation.

## 12. Offline Render Specification

Offline render:

- Does not use realtime device callback.
- May allocate.
- May process faster than realtime.
- Uses same graph semantics as playback.
- Supports progress/cancel.
- Uses plugin offline mode where available.

Initial target:

- Master WAV export.
- Selected range export.

Later:

- FLAC/AIFF/MP3.
- Stems.
- Dithering.
- Sample-rate conversion quality options.

## 13. Error Recovery Specification

Recoverable errors:

- Device lost.
- Graph build failed.
- Missing media.
- Plugin load failed.
- Plugin editor failed.
- Disk stream underrun.
- Scanner crash.

Recovery behavior:

- Keep last valid graph running when possible.
- Surface clear UI status.
- Avoid crash from bad plugin scanner.
- Provide safe mode.
- Allow reset audio engine.
- Preserve project data for unresolved plugins/media.

## 14. Diagnostics Specification

Counters:

- XRuns/dropouts.
- Callback CPU load.
- Graph node count.
- Active plugin count.
- Disk underruns.
- Missing media count.
- Meter queue drops.
- Command queue overflows.

Debug flags:

- `FUTUREBOARD_AUDIO_DEBUG`.
- `FUTUREBOARD_AUDIO_CALLBACK_DEBUG`.
- `FUTUREBOARD_ROUTING_DEBUG`.
- `FUTUREBOARD_PLUGIN_DEBUG`.
- `FUTUREBOARD_PLUGIN_VIEW_DEBUG`.
- `FUTUREBOARD_PLUGIN_SCAN_DEBUG`.
- `FUTUREBOARD_WAVEFORM_DEBUG`.
- `FUTUREBOARD_DISK_STREAM_DEBUG`.
- `FUTUREBOARD_AUTOMATION_DEBUG`.
- `FUTUREBOARD_TRANSPORT_DEBUG`.

Callback debug must be ring-buffered and throttled.

## 15. Acceptance Criteria

The spec is satisfied when:

- Audio callback passes no-allocation/no-lock review.
- Device start/stop/restart is stable.
- Runtime snapshot is the only callback project data source.
- WAV clip playback works.
- Long file streaming works.
- Mixer volume/pan/mute/solo/master work.
- Routing graph rejects cycles.
- VST3 insert processes audio.
- Plugin editor opens in GPUI shell.
- MIDI clip can play an instrument plugin.
- Automation Read mode affects mixer/plugin params.
- Audio recording creates a valid WAV clip.
- Master WAV export works.
- Device loss and bad plugin scan do not kill the app.
- 32-track project remains usable.
