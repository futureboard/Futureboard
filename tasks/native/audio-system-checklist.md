# Futureboard Audio System Checklist

Planning status: checklist only. No implementation code is implied.

## Realtime Safety

- [ ] Audio callback never allocates.
- [ ] Audio callback never blocks on locks.
- [ ] Audio callback never opens files.
- [ ] Audio callback never scans plugins.
- [ ] Audio callback never calls UI.
- [ ] Audio callback never logs per block.
- [ ] Audio callback never rebuilds project graph.
- [ ] Audio callback reads immutable runtime snapshot only.
- [ ] Graph swaps happen at safe block boundaries.
- [ ] Failed graph build preserves current running graph.
- [ ] Debug output is ring-buffered/throttled.

## Device Backend

- [ ] Enumerate audio backends.
- [ ] Enumerate input devices.
- [ ] Enumerate output devices.
- [ ] WASAPI Shared support.
- [ ] WASAPI Exclusive support.
- [ ] CoreAudio support plan.
- [ ] PipeWire support plan.
- [ ] JACK support plan.
- [ ] ALSA fallback plan.
- [ ] Set sample rate.
- [ ] Set buffer size.
- [ ] Restart device safely.
- [ ] Test output.
- [ ] Test input.
- [ ] Recover from device loss.
- [ ] Persist device settings.
- [ ] Mark restart-required settings.

## Runtime Snapshot

- [ ] Define runtime project snapshot.
- [ ] Define runtime transport snapshot.
- [ ] Define runtime tempo map snapshot.
- [ ] Define runtime track snapshot.
- [ ] Define runtime clip snapshot.
- [ ] Define runtime routing graph.
- [ ] Define runtime automation snapshot.
- [ ] Define runtime media/source handles.
- [ ] Resolve project string IDs to runtime handles.
- [ ] Sort clips/events for playback.
- [ ] Validate routing before swap.
- [ ] Build snapshots off audio callback.
- [ ] Atomic/safe snapshot swap.

## Audio Graph

- [ ] Audio input node.
- [ ] Audio clip node.
- [ ] MIDI clip node.
- [ ] Instrument node.
- [ ] Insert plugin node.
- [ ] Track mixer node.
- [ ] Send node.
- [ ] Return track node.
- [ ] Bus track node.
- [ ] Group track node.
- [ ] Master node.
- [ ] Output node.
- [ ] Meter node.
- [ ] Topological sort.
- [ ] Cycle detection.
- [ ] Invalid route UI feedback.
- [ ] Stereo-first graph.
- [ ] Hardware output target later.

## Audio Clips and Regions

- [ ] WAV clip playback.
- [ ] Source handle model.
- [ ] Clip start beat.
- [ ] Clip duration beats.
- [ ] Source offset.
- [ ] Clip gain.
- [ ] Fade-in placeholder.
- [ ] Fade-out placeholder.
- [ ] Clip mute.
- [ ] Clip missing media state.
- [ ] Clip scheduling by beat.
- [ ] Clip scheduling with tempo map later.
- [ ] Non-destructive editing.
- [ ] Project save/load.

## Disk Streaming and Decode

- [ ] Media import worker.
- [ ] Metadata probe worker.
- [ ] WAV import.
- [ ] AIFF import.
- [ ] FLAC import.
- [ ] MP3 decode worker/cache.
- [ ] OGG later.
- [ ] CAF later.
- [ ] Disk prefetch worker.
- [ ] Ring buffer per active source/clip.
- [ ] Underrun detection.
- [ ] Silence fallback on underrun/missing media.
- [ ] No file IO in callback.
- [ ] Project media folder policy.
- [ ] Cache cleanup policy.

## Waveform/Peak Cache

- [ ] Peak worker.
- [ ] Chunked peak files.
- [ ] Multiple LODs.
- [ ] Cache key by file hash + modified time.
- [ ] Pending state.
- [ ] Partial state.
- [ ] Ready state.
- [ ] Missing/failed state.
- [ ] Visible-only waveform drawing.
- [ ] No peak generation in render.
- [ ] Large file waveform stress test.

## Mixer

- [ ] Track volume.
- [ ] Track pan.
- [ ] Track mute.
- [ ] Track solo.
- [ ] Track arm.
- [ ] Track input monitor.
- [ ] Master fader.
- [ ] Pan law.
- [ ] Per-track buffers.
- [ ] Send buffers.
- [ ] Bus accumulation buffers.
- [ ] Master buffer.
- [ ] Peak meters.
- [ ] Meter decay.
- [ ] Peak hold.
- [ ] Clip indicator.
- [ ] Meter update throttling.
- [ ] No full-app repaint from meters.

## Routing

- [ ] Track output to master.
- [ ] Track output to bus.
- [ ] Bus to master.
- [ ] Bus inserts.
- [ ] Return track.
- [ ] Send to return.
- [ ] Send gain.
- [ ] Post-fader sends.
- [ ] Pre-fader sends later.
- [ ] Group track later.
- [ ] Reject track -> bus -> same track cycle.
- [ ] Reject return self-send.
- [ ] Reject master back-routing.
- [ ] Reject bus cycle.

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

- [ ] Play.
- [ ] Stop.
- [ ] Pause.
- [ ] Record.
- [ ] Loop.
- [ ] Seek.
- [ ] Rewind/forward.
- [ ] Go start/end.
- [ ] Follow playhead.
- [ ] Auto-scroll UI-only.
- [ ] Metronome.
- [ ] Count-in.
- [ ] Pre-roll.
- [ ] Sample position.
- [ ] Beat position.
- [ ] Seconds position.
- [ ] Static tempo.
- [ ] Tempo map API.
- [ ] Time signature.
- [ ] Seek applies at block boundary.

## Recording

- [ ] Arm audio track.
- [ ] Select input device/channel.
- [ ] Monitor off/auto/input.
- [ ] Record WAV.
- [ ] Finalize file safely.
- [ ] Create audio clip after recording.
- [ ] Generate waveform after recording.
- [ ] Record MIDI clip.
- [ ] MIDI overdub later.
- [ ] Punch in/out later.
- [ ] Manual recording offset.
- [ ] Loopback latency measurement later.

## Latency

- [ ] Device input latency.
- [ ] Device output latency.
- [ ] Buffer latency.
- [ ] Plugin latency query.
- [ ] Track latency display.
- [ ] Master latency display.
- [ ] Latency graph.
- [ ] Playback delay compensation.
- [ ] Send/return latency handling.
- [ ] Recording offset compensation.
- [ ] Automation alignment.
- [ ] Meter alignment.

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
- [ ] Latency compensation setting.
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
