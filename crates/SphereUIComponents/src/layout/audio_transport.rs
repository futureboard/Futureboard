use gpui::{Context, App};

use std::time::{Duration, Instant};

use crate::components;

use super::engine_snapshot::{build_engine_project_snapshot, log_engine_sync_snapshot};
use super::helpers::{smooth_meter_value, update_meter_clip, update_meter_hold};
use super::transport_freeze_debug::{self, PlayWatchdog};
use super::StudioLayout;

impl StudioLayout {
    pub(super) fn dispatch_midi_preview_command(
        &mut self,
        command: components::piano_roll::UiMidiPreviewCommand,
        cx: &App,
    ) {
        let track_id = match &command {
            components::piano_roll::UiMidiPreviewCommand::NoteOn { track_id, .. }
            | components::piano_roll::UiMidiPreviewCommand::NoteOff { track_id, .. }
            | components::piano_roll::UiMidiPreviewCommand::AllNotesOff { track_id }
            | components::piano_roll::UiMidiPreviewCommand::MidiPanic { track_id } => {
                track_id.clone()
            }
        };
        let bridge_instance = self.resolve_track_instrument_plugin(&track_id, cx);
        let sink_ready = self
            .plugin_bridge_runtime
            .as_ref()
            .and_then(|runtime| runtime.lock().ok())
            .and_then(|bridge| bridge.audio_sink().map(|_| ()))
            .is_some();

        if sink_ready {
            // Shared-memory bridge is live — always route through the main DAW
            // engine (track → mixer → master), even if timeline runtime_backend
            // has not caught up yet.
            let Some(engine) = self.audio_engine.as_ref() else {
                eprintln!("[midi-preview-ui] engine_command_bus_connected=false");
                return;
            };
            let instance_id = bridge_instance
                .clone()
                .unwrap_or_else(|| "bridge".to_string());
            let result = match &command {
                components::piano_roll::UiMidiPreviewCommand::NoteOn {
                    channel,
                    pitch,
                    velocity,
                    ..
                } => {
                    eprintln!(
                        "[midi-preview-ui] source=piano_key type=note_on track={track_id} instance={instance_id} pitch={pitch} velocity={velocity} sink_ready=true"
                    );
                    engine.plugin_preview_note_on(
                        track_id.clone(),
                        instance_id.clone(),
                        *channel,
                        *pitch,
                        *velocity,
                    )
                }
                components::piano_roll::UiMidiPreviewCommand::NoteOff {
                    channel, pitch, ..
                } => {
                    eprintln!(
                        "[midi-preview-ui] source=piano_key type=note_off track={track_id} instance={instance_id} pitch={pitch} sink_ready=true"
                    );
                    engine.plugin_preview_note_off(
                        track_id.clone(),
                        instance_id.clone(),
                        *channel,
                        *pitch,
                    )
                }
                components::piano_roll::UiMidiPreviewCommand::AllNotesOff { .. } => {
                    engine.plugin_preview_all_notes_off(track_id.clone(), instance_id.clone())
                }
                components::piano_roll::UiMidiPreviewCommand::MidiPanic { .. } => {
                    engine.plugin_preview_all_notes_off(track_id.clone(), instance_id.clone())
                }
            };
            if let Err(error) = result {
                eprintln!("[EngineMidiPreview] bridge dispatch failed: {error}");
            }
            return;
        }

        if let Some(instance_id) = bridge_instance {
            if let Some(runtime) = self.plugin_bridge_runtime.as_ref() {
                if let Ok(mut bridge) = runtime.lock() {
                    let result = match command {
                        components::piano_roll::UiMidiPreviewCommand::NoteOn {
                            channel,
                            pitch,
                            velocity,
                            ..
                        } => {
                            eprintln!(
                                "[midi-preview-ui] note_on source=piano_roll pitch={pitch} velocity={velocity} instance={instance_id} (IPC fallback — DSP bridge pending)"
                            );
                            bridge.preview_note_on(instance_id.clone(), channel, pitch, velocity)
                        }
                        components::piano_roll::UiMidiPreviewCommand::NoteOff {
                            channel, pitch, ..
                        } => bridge.preview_note_off(instance_id.clone(), channel, pitch),
                        components::piano_roll::UiMidiPreviewCommand::AllNotesOff { .. } => {
                            bridge.preview_all_notes_off(instance_id.clone())
                        }
                        components::piano_roll::UiMidiPreviewCommand::MidiPanic { .. } => {
                            bridge.midi_panic(instance_id.clone())
                        }
                    };
                    if let Err(error) = result {
                        eprintln!("[plugin-bridge] midi preview dispatch failed: {error}");
                    }
                    return;
                }
            }
        }

        let Some(engine) = self.audio_engine.as_ref() else {
            eprintln!("[PopoutMidiEditor] engine_command_bus_connected=false");
            return;
        };
        eprintln!("[PopoutMidiEditor] engine_command_bus_connected=true");
        let result = match command {
            components::piano_roll::UiMidiPreviewCommand::NoteOn {
                channel,
                pitch,
                velocity,
                ..
            } => {
                eprintln!(
                    "[PopoutMidiEditor] active_track_id={} dispatch PreviewNoteOn -> engine",
                    track_id
                );
                engine.midi_preview_note_on(track_id, channel, pitch, velocity)
            }
            components::piano_roll::UiMidiPreviewCommand::NoteOff {
                channel, pitch, ..
            } => {
                eprintln!(
                    "[PopoutMidiEditor] active_track_id={} dispatch PreviewNoteOff -> engine",
                    track_id
                );
                engine.midi_preview_note_off(track_id, channel, pitch)
            }
            components::piano_roll::UiMidiPreviewCommand::AllNotesOff { .. } => {
                eprintln!(
                    "[PopoutMidiEditor] active_track_id={} dispatch PreviewAllNotesOff -> engine",
                    track_id
                );
                engine.midi_preview_all_notes_off(track_id)
            }
            components::piano_roll::UiMidiPreviewCommand::MidiPanic { .. } => {
                engine.midi_preview_all_notes_off(track_id)
            }
        };
        if let Err(error) = result {
            eprintln!("[EngineMidiPreview] dispatch failed: {error}");
        }
    }

    fn bridge_instrument_instance_id(&self, track_id: &str, cx: &App) -> Option<String> {
        use crate::components::timeline::timeline_state::{
            PluginRuntimeBackend, TrackType,
        };
        let timeline = self.timeline.read(cx);
        let track = timeline.state.find_track(track_id)?;
        let insert = match track.track_type {
            TrackType::Instrument => track.instrument_insert()?,
            TrackType::Midi => track.inserts.first()?,
            _ => return None,
        };
        if insert.runtime_backend != PluginRuntimeBackend::ExternalBridge {
            return None;
        }
        Some(insert.id.clone())
    }

    pub(super) fn resolve_track_instrument_plugin(
        &self,
        track_id: &str,
        cx: &App,
    ) -> Option<String> {
        let timeline = self.timeline.read(cx);
        let track = timeline.state.find_track(track_id)?;
        if let Some(instance_id) = track.instrument_plugin_instance_id.as_ref() {
            return Some(instance_id.clone());
        }
        if let Some(instance_id) = self.bridge_instrument_instance_id(track_id, cx) {
            return Some(instance_id);
        }
        match track.track_type {
            crate::components::timeline::timeline_state::TrackType::Instrument => track
                .instrument_insert()
                .filter(|slot| slot.plugin_id.is_some())
                .map(|slot| slot.id.clone()),
            crate::components::timeline::timeline_state::TrackType::Midi => track
                .inserts
                .first()
                .filter(|slot| slot.plugin_id.is_some())
                .map(|slot| slot.id.clone()),
            _ => None,
        }
        .or_else(|| {
            self.plugin_bridge_runtime.as_ref().and_then(|runtime| {
                runtime
                    .lock()
                    .ok()
                    .and_then(|bridge| bridge.loaded_for_track(track_id))
                    .map(|loaded| loaded.descriptor.insert_id)
            })
        })
    }

    pub(super) fn spawn_audio_poll(cx: &mut Context<Self>) {
        // 16 ms ≈ 60 Hz — matches a typical display refresh and is fine for
        // VU + transport-time animation. The engine produces position
        // snapshots at audio-block boundaries (~5-10 ms at 256-sample
        // buffers), but the UI never needs to repaint faster than the
        // display, so we cap polling at the refresh interval and let
        // `interpolated_playhead_beat` smooth between engine snapshots.
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| loop {
            if crate::shutdown::ShutdownState::global().is_shutting_down() {
                break;
            }
            executor.timer(Duration::from_millis(16)).await;
            if crate::shutdown::ShutdownState::global().is_shutting_down() {
                break;
            }
            let Ok((changed, mixer_handle)) = this.update(cx, |this, cx| {
                if crate::shutdown::ShutdownState::global().is_shutting_down() {
                    return (false, None);
                }
                let changed = this.poll_native_audio(cx);
                (changed, this.mixer_window.clone())
            }) else {
                continue;
            };
            if changed && !crate::shutdown::ShutdownState::global().is_shutting_down() {
                crate::perf::record_notify("transport");
                let studio_id = this.entity_id();
                let _ = cx.update(|app| app.notify(studio_id));
                if mixer_handle.is_some() {
                    let _ = this.update(cx, |layout, cx| {
                        layout.push_mixer_snapshot_to_window(cx);
                    });
                }
            }
        })
        .detach();
    }

    pub(super) fn poll_native_audio(&mut self, cx: &mut Context<Self>) -> bool {
        if crate::shutdown::ShutdownState::global().is_shutting_down() {
            return false;
        }
        let _s = crate::perf::PerfScope::enter("poll_native_audio");
        if self.audio_engine.is_none() {
            return false;
        }

        if self.engine_project_dirty || self.engine_media_dirty {
            self.schedule_audio_project_sync(cx, false, "engine_dirty_poll");
        }

        // Backstop: close editors whose track/insert was removed by any path
        // (notably the track-header delete button, which mutates the Timeline
        // entity directly and never reaches the StudioLayout delete commands).
        self.reconcile_open_plugin_editors(cx);
        self.poll_plugin_bridge_runtime(cx);
        // Drive native main-owned editor shells: honor OS close + forward resizes.
        self.drive_bridge_editors(cx);

        let engine = self.audio_engine.as_ref().expect("checked above");
        // Throttled raw/bus input-peak trace (gated by FUTUREBOARD_INPUT_DEBUG).
        engine.log_input_debug();
        let stats = engine.stats();
        // State-transition signal — used to notify the root layout even
        // when the transport is paused (e.g. error appears, stream opens).
        let state_changed = self
            .audio_stats
            .as_ref()
            .map(|previous| {
                previous.transport_playing != stats.transport_playing
                    || previous.running != stats.running
                    || previous.last_error != stats.last_error
            })
            .unwrap_or(true);
        self.audio_running = stats.running;
        self.audio_last_error = stats.last_error.clone();

        let engine_beat = stats.position_beats.max(0.0) as f32;
        let sync_changed = (engine_beat - self.last_engine_playhead_beat).abs() > 0.0001
            || self
                .audio_stats
                .as_ref()
                .map(|previous| previous.transport_playing != stats.transport_playing)
                .unwrap_or(true);
        if sync_changed {
            self.last_engine_playhead_beat = engine_beat;
            self.last_engine_sync = Instant::now();
        }
        let meter_changed = self.apply_engine_meters(cx);

        // Realtime recording waveform preview (Part 1) — grow the preview clip
        // and append streamed peaks. Self-contained; notifies the timeline.
        self.update_recording_preview(cx);

        if stats.transport_playing {
            let bpm = {
                let timeline = self.timeline.read(cx);
                timeline.state.bpm
            };
            let interpolated = self.interpolated_playhead_beat(bpm);
            let _ = self.timeline.update(cx, move |timeline, cx| {
                timeline.state.transport.playing = true;
                // No threshold while playing — even sub-pixel beat motion
                // matters for the bar:beat:tick readout in the chrome.
                let next = interpolated.max(0.0);
                let mut dirty = false;
                if timeline.state.transport.playhead_beats != next {
                    timeline.state.transport.playhead_beats = next;
                    dirty = true;
                }
                // Follow Track Volume automation: refresh each track's effective
                // volume so the mixer/track-header/inspector fader track the curve
                // during playback. UI-only (faders read `display_volume`), so this
                // never writes the base value or fires a user-edit command.
                if timeline
                    .state
                    .recompute_effective_volumes(next, "playback_tick")
                {
                    dirty = true;
                }
                // Follow-playhead / auto-scroll. Keeps the playhead visible
                // during playback; user-scroll temporarily disables it via
                // `note_user_scrolled`. Cheap — no rebuild, just viewport
                // scroll_x update.
                if timeline.state.update_auto_scroll_for_playhead(next) {
                    dirty = true;
                }
                if dirty {
                    cx.notify();
                }
            });
        } else {
            let _ = self.timeline.update(cx, |timeline, cx| {
                if timeline.state.transport.playing {
                    timeline.state.transport.playing = false;
                    cx.notify();
                }
            });
        }

        let was_playing = stats.transport_playing;
        self.audio_stats = Some(stats);
        // While playing the root layout must repaint every tick so the
        // transport chrome (bar:beat:tick, status line) tracks the
        // playhead. Otherwise we'd be limited to engine-snapshot cadence
        // and the readout would stutter at ~10-20 Hz.
        state_changed || was_playing || meter_changed
    }

    /// Block-rate automation evaluation scaffolding. Evaluates each track's
    /// Volume/Pan automation lane at the current playhead beat. Engine
    /// application — pushing the value into the realtime parameter queue — is
    /// the next phase; doing it here at 60 Hz unconditionally would both spam
    /// the engine and fight the UI fader, so for now we only trace under
    /// `FUTUREBOARD_AUTOMATION_DEBUG`. The evaluation itself is real (see
    /// [`evaluate_automation`]) so the wiring point is ready for the param
    /// queue without faking any data.
    pub(super) fn evaluate_block_automation(&self, cx: &mut Context<Self>, beat: f32) {
        use crate::components::timeline::timeline_state::{
            automation_debug_enabled, evaluate_automation, AutomationTarget,
        };
        if !automation_debug_enabled() {
            return;
        }
        let timeline = self.timeline.read(cx);
        for track in &timeline.state.tracks {
            for lane in &track.automation_lanes {
                if lane.points.is_empty()
                    || !matches!(
                        lane.target,
                        AutomationTarget::TrackVolume | AutomationTarget::TrackPan
                    )
                {
                    continue;
                }
                let value =
                    evaluate_automation(&lane.points, beat as f64, lane.target.default_value());
                eprintln!(
                    "[automation] evaluate track={} beat={:.3} value={:.3} target={}",
                    track.id,
                    beat,
                    value,
                    lane.target.display_name()
                );
                // TODO(engine): push `value` into the realtime parameter queue
                // for `track.id` (volume/pan) — lock-free, no allocations on the
                // audio-control path — instead of only tracing here.
            }
        }
    }

    pub(super) fn interpolated_playhead_beat(&self, bpm: f32) -> f32 {
        let playing = self
            .audio_stats
            .as_ref()
            .map(|stats| stats.transport_playing)
            .unwrap_or(false);
        if !playing {
            return self.last_engine_playhead_beat;
        }
        self.last_engine_playhead_beat
            + self.last_engine_sync.elapsed().as_secs_f32() * bpm.max(1.0) / 60.0
    }

    /// Update smoothed meter levels in timeline state. Does not call
    /// `cx.notify` — repaints are driven by the audio poll when transport
    /// is active, or by user interaction when idle.
    pub(super) fn apply_engine_meters(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(engine) = self.audio_engine.as_ref() else {
            return false;
        };
        // Throttle meter polling to `PowerMode::meter_update_hz`. The audio
        // poll fires at 60 Hz; on low-end GPUs that's too many meter writes
        // and the resulting notify cascade is what drove FPS drops.
        let power = crate::perf::power_mode();
        let min_interval = Duration::from_secs_f32(1.0 / power.meter_update_hz().max(1.0));
        if self.last_meter_apply.elapsed() < min_interval {
            return false;
        }
        self.last_meter_apply = Instant::now();
        let meters = engine.meters();
        self.timeline.update(cx, |timeline, _cx| {
            let mut changed = false;
            for track_meter in meters.tracks {
                if let Some(track) = timeline
                    .state
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == track_meter.track_id)
                {
                    let next_l = track_meter.peak_l.clamp(0.0, 1.0) as f32;
                    let next_r = track_meter.peak_r.clamp(0.0, 1.0) as f32;
                    changed |= smooth_meter_value(&mut track.meter_level_l, next_l);
                    changed |= smooth_meter_value(&mut track.meter_level_r, next_r);
                    update_meter_hold(&mut track.meter_peak_hold_l, track.meter_level_l);
                    update_meter_hold(&mut track.meter_peak_hold_r, track.meter_level_r);
                    update_meter_clip(
                        &mut track.meter_clip,
                        track_meter.peak_l,
                        track_meter.peak_r,
                        track.meter_peak_hold_l.max(track.meter_peak_hold_r),
                    );
                }
            }
            let master = &mut timeline.state.master;
            changed |= smooth_meter_value(
                &mut master.meter_level_l,
                meters.master_peak_l.clamp(0.0, 1.0) as f32,
            );
            changed |= smooth_meter_value(
                &mut master.meter_level_r,
                meters.master_peak_r.clamp(0.0, 1.0) as f32,
            );
            update_meter_hold(&mut master.meter_peak_hold_l, master.meter_level_l);
            update_meter_hold(&mut master.meter_peak_hold_r, master.meter_level_r);
            update_meter_clip(
                &mut master.meter_clip,
                meters.master_peak_l,
                meters.master_peak_r,
                master.meter_peak_hold_l.max(master.meter_peak_hold_r),
            );
            changed
        })
    }

    /// Queue a background engine sync. `load_project` decodes media on the
    /// caller thread — never invoke it from the UI poll loop or render path.
    pub(crate) fn schedule_audio_project_sync(
        &mut self,
        cx: &mut Context<Self>,
        force: bool,
        reason: &'static str,
    ) {
        if crate::shutdown::ShutdownState::global().is_shutting_down() {
            return;
        }
        let Some(engine) = self.audio_engine.clone() else {
            self.audio_last_error = Some("audio engine unavailable".to_string());
            return;
        };

        if self.audio_sync_in_flight {
            self.audio_sync_pending = true;
            return;
        }

        if !force && !self.engine_project_dirty && !self.engine_media_dirty {
            return;
        }

        let sample_rate = self.current_audio_sample_rate();
        let project_root = self
            .project_folder
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let preferred_input_device = {
            let settings = self.settings.read(cx);
            settings.current.hardware.audio.device_in.clone()
        };
        let preferred_output_device = {
            let settings = self.settings.read(cx);
            settings.current.hardware.audio.device_out.clone()
        };
        eprintln!(
            "[AudioSettings] selected input device = {:?}",
            preferred_input_device
        );
        eprintln!(
            "[AudioSettings] selected output device = {:?}",
            preferred_output_device
        );
        let snapshot = {
            let timeline = self.timeline.read(cx);
            build_engine_project_snapshot(
                &timeline.state,
                sample_rate,
                project_root.as_deref(),
                Some(preferred_input_device.as_str()),
            )
        };
        log_engine_sync_snapshot(
            &snapshot,
            self.engine_project_dirty || self.engine_media_dirty,
            reason,
        );
        let signature = serde_json::to_string(&snapshot).unwrap_or_default();
        if !force && self.last_audio_project_signature.as_deref() == Some(signature.as_str()) {
            self.engine_project_dirty = false;
            self.engine_media_dirty = false;
            return;
        }

        self.audio_sync_in_flight = true;
        let owner = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            let join = std::thread::Builder::new()
                .name("audio-project-load".into())
                .spawn(move || engine.load_project(snapshot));
            let result = match join {
                Ok(handle) => handle.join().unwrap_or_else(|_| {
                    Err(DAUx::SphereAudioError::NativeError(
                        "audio project load thread panicked".to_string(),
                    ))
                }),
                Err(error) => Err(DAUx::SphereAudioError::NativeError(format!(
                    "failed to spawn audio project load thread: {error}"
                ))),
            };
            let _ = owner.update(cx, |this, cx| {
                this.complete_audio_project_sync(cx, result, signature);
            });
            if !crate::shutdown::ShutdownState::global().is_shutting_down() {
                let studio_id = owner.entity_id();
                let _ = cx.update(|app| app.notify(studio_id));
            }
        })
        .detach();
    }

    pub(super) fn complete_audio_project_sync(
        &mut self,
        cx: &mut Context<Self>,
        result: Result<(), DAUx::SphereAudioError>,
        signature: String,
    ) {
        self.audio_sync_in_flight = false;
        match result {
            Ok(()) => {
                self.last_audio_project_signature = Some(signature);
                self.engine_project_dirty = false;
                self.engine_media_dirty = false;
                self.audio_last_error = None;
            }
            Err(error) => {
                self.audio_last_error = Some(error.to_string());
                eprintln!("[audio] load_project failed: {error}");
                // Clear dirty so a failed decode does not retry every poll tick.
                self.engine_project_dirty = false;
                self.engine_media_dirty = false;
            }
        }

        // Phase 2b: read back per-insert instantiation status now that the
        // runtime graph reflects this snapshot. A native-plugin insert that
        // the engine reports as not-ready failed to instantiate — flip its
        // UI slot to `Failed` (no panic; just surfaces the error).
        self.apply_engine_insert_statuses(cx);

        let pending_sync = self.audio_sync_pending;
        self.audio_sync_pending = false;
        if pending_sync {
            self.schedule_audio_project_sync(cx, false, "audio_sync_pending");
            return;
        }

        if self.pending_play_after_sync {
            self.pending_play_after_sync = false;
            self.start_native_playback(cx);
        }
    }

    /// Read structured per-insert status from the engine and reconcile each
    /// UI slot's `load_status` (Phase 2b). Only native-plugin inserts are
    /// reconciled — built-ins are always live, and the stub / unscanned slots
    /// aren't sent to the engine so they keep their optimistic UI status.
    /// Runs on the UI thread right after a sync completes — not in the poll
    /// loop — so the runtime mutex is locked at most once per project change.
    pub(super) fn apply_engine_insert_statuses(&mut self, cx: &mut Context<Self>) {
        use crate::components::timeline::timeline_state::InsertLoadStatus;
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        let statuses = engine.insert_statuses();
        if statuses.is_empty() {
            return;
        }
        let mut changed = false;
        self.timeline.update(cx, |timeline, _cx| {
            for st in &statuses {
                if !st.native {
                    continue;
                }
                let status = if st.ready {
                    InsertLoadStatus::Ready
                } else {
                    InsertLoadStatus::Failed("Plugin failed to instantiate".to_string())
                };
                if timeline
                    .state
                    .set_insert_load_status(&st.track_id, &st.insert_id, status)
                {
                    changed = true;
                }
            }
        });
        if changed {
            cx.notify();
        }
    }

    pub(super) fn mark_engine_project_dirty(&mut self) {
        self.engine_project_dirty = true;
    }

    pub(crate) fn mark_engine_media_dirty(&mut self) {
        self.engine_project_dirty = true;
        self.engine_media_dirty = true;
    }

    pub(super) fn ensure_audio_stream_warm(&mut self) -> bool {
        transport_freeze_debug::log("ensure_audio_stream_warm enter");
        let stream_ready = self
            .audio_stats
            .as_ref()
            .map(|stats| stats.stream_open && stats.running)
            .unwrap_or(false)
            || self.audio_running;
        if stream_ready {
            transport_freeze_debug::log("ensure_audio_stream_warm already warm");
            return true;
        }

        let Some(engine) = self.audio_engine.as_mut() else {
            self.audio_last_error = Some("audio engine unavailable".to_string());
            transport_freeze_debug::log("ensure_audio_stream_warm no engine");
            return false;
        };
        transport_freeze_debug::log("ensure_audio_stream_warm before engine.start");
        // `AudioEngine::start` resumes an open stream without reopening the
        // device or rebuilding/decoding the runtime graph on this thread.
        match engine.start() {
            Ok(()) => {
                transport_freeze_debug::log("ensure_audio_stream_warm after engine.start ok");
                self.audio_stats = Some(engine.stats());
                self.audio_running = true;
                self.audio_last_error = None;
                true
            }
            Err(error) => {
                self.audio_running = false;
                self.audio_last_error = Some(error.to_string());
                eprintln!("[audio] stream warm-up failed: {error}");
                transport_freeze_debug::log("ensure_audio_stream_warm engine.start failed");
                false
            }
        }
    }

    pub(super) fn current_audio_sample_rate(&self) -> u32 {
        self.audio_stats
            .as_ref()
            .map(|stats| stats.sample_rate)
            .filter(|sample_rate| *sample_rate > 0)
            .or_else(|| {
                self.audio_engine
                    .as_ref()
                    .map(|engine| engine.config().sample_rate)
                    .filter(|sample_rate| *sample_rate > 0)
            })
            .unwrap_or(48_000)
    }

    pub(super) fn start_native_playback(&mut self, cx: &mut Context<Self>) {
        transport_freeze_debug::reset_sequence();
        transport_freeze_debug::log("Play requested");
        eprintln!("[transport] Play requested");

        if self.audio_engine.is_none() {
            self.audio_last_error = Some("audio engine unavailable".to_string());
            transport_freeze_debug::log("abort: no audio engine");
            return;
        }

        let already_playing = self
            .audio_stats
            .as_ref()
            .map(|stats| stats.transport_playing)
            .unwrap_or(false);
        if already_playing {
            transport_freeze_debug::log("abort: already playing (idempotent)");
            return;
        }

        transport_freeze_debug::log("before sync/dirty gates");
        if self.audio_sync_in_flight {
            self.pending_play_after_sync = true;
            transport_freeze_debug::log("defer: audio_sync_in_flight");
            return;
        }
        if self.engine_project_dirty || self.engine_media_dirty {
            self.pending_play_after_sync = true;
            transport_freeze_debug::log("defer: schedule_audio_project_sync");
            self.schedule_audio_project_sync(cx, false, "transport_play_pending_sync");
            return;
        }

        transport_freeze_debug::log("before ensure_audio_stream_warm");
        if !self.ensure_audio_stream_warm() {
            transport_freeze_debug::log("abort: stream warm-up failed");
            return;
        }

        transport_freeze_debug::log("after ensure_audio_stream_warm");

        // Read transport state once; do not hold timeline lease across engine I/O.
        let (
            playhead_beats,
            bpm,
            metronome_enabled,
            loop_enabled,
            loop_start_beats,
            loop_end_beats,
            ts_num,
            ts_den,
        ) = {
            transport_freeze_debug::log("before timeline.read");
            let timeline = self.timeline.read(cx);
            transport_freeze_debug::log("after timeline.read");
            (
                timeline.state.transport.playhead_beats,
                timeline.state.bpm,
                timeline.state.transport.metronome_enabled,
                timeline.state.transport.loop_enabled,
                timeline.state.transport.loop_start_beats,
                timeline.state.transport.loop_end_beats,
                timeline.state.time_signature_num,
                timeline.state.time_signature_den,
            )
        };

        let Some(engine) = self.audio_engine.as_ref() else {
            transport_freeze_debug::log("abort: engine handle missing");
            return;
        };

        transport_freeze_debug::log("before engine transport sync");
        if let Err(error) = engine.set_bpm(bpm as f64) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set BPM failed: {error}");
            }
        }
        if let Err(error) = engine.set_time_signature(ts_num, ts_den) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set time signature failed: {error}");
            }
        }
        if let Err(error) = engine.set_metronome_enabled(metronome_enabled) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set metronome failed: {error}");
            }
        }
        let bpm_secs = bpm.max(1.0) as f64;
        let loop_start_seconds = loop_start_beats as f64 * 60.0 / bpm_secs;
        let loop_end_seconds = loop_end_beats as f64 * 60.0 / bpm_secs;
        if let Err(error) = engine.set_loop(loop_enabled, loop_start_seconds, loop_end_seconds) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set loop failed: {error}");
            }
        }
        transport_freeze_debug::log("after engine transport sync");

        let seconds = playhead_beats.max(0.0) as f64 * 60.0 / bpm_secs;
        transport_freeze_debug::log("before engine.seek");
        if let Err(error) = engine.seek(seconds) {
            self.audio_last_error = Some(error.to_string());
            eprintln!("[audio] seek before play failed: {error}");
            transport_freeze_debug::log("abort: seek failed");
            return;
        }
        transport_freeze_debug::log("after engine.seek");

        if std::env::var_os("FUTUREBOARD_PLAYBACK_DEBUG").is_some() {
            let debug = engine.debug_snapshot();
            eprintln!(
                "[playback] starting: sr={} loaded_clips={} ready_clips={} seek_seconds={:.3} bpm={:.1}",
                self.current_audio_sample_rate(),
                debug.loaded_clips,
                debug.ready_clips,
                seconds,
                bpm
            );
            if debug.loaded_clips == 0 {
                eprintln!(
                    "[playback] WARNING: no clips in realtime runtime — verify that imported clips \
                     have a non-empty media path"
                );
            } else if debug.ready_clips == 0 {
                eprintln!(
                    "[playback] WARNING: clips loaded but none decoded — check earlier \
                     '[SphereAudio] clip ... decode FAILED' lines"
                );
            }
        }

        transport_freeze_debug::log("before engine.play");
        if let Err(error) = engine.play() {
            self.audio_last_error = Some(error.to_string());
            eprintln!("[audio] play failed: {error}");
            transport_freeze_debug::log("abort: engine.play failed");
            return;
        }
        transport_freeze_debug::log("after engine.play");

        self.audio_stats = Some(engine.stats());
        transport_freeze_debug::log("before timeline.update playing=true");
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.transport.playing = true;
            cx.notify();
        });
        transport_freeze_debug::log("after timeline.update");
        transport_freeze_debug::log("returning from start_native_playback");

        if let Some(watchdog) = PlayWatchdog::start() {
            let owner = cx.entity().clone();
            cx.spawn(async move |_this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let _ = owner.update(cx, |this, _cx| {
                    let playing = this
                        .audio_stats
                        .as_ref()
                        .map(|stats| stats.transport_playing)
                        .unwrap_or(false);
                    watchdog.check(playing);
                });
            })
            .detach();
        }
    }

    pub(super) fn sync_transport_controls(&mut self, cx: &mut Context<Self>) {
        self.sync_metronome_controls(cx);
        self.sync_loop_controls(cx);
    }

    pub(super) fn sync_metronome_controls(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        let (enabled, bpm, num, den) = {
            let timeline = self.timeline.read(cx);
            (
                timeline.state.transport.metronome_enabled,
                timeline.state.bpm as f64,
                timeline.state.time_signature_num,
                timeline.state.time_signature_den,
            )
        };
        if let Err(error) = engine.set_bpm(bpm) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set BPM failed: {error}");
            }
        }
        if let Err(error) = engine.set_time_signature(num, den) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set time signature failed: {error}");
            }
        }
        if let Err(error) = engine.set_metronome_enabled(enabled) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set metronome failed: {error}");
            }
        }
    }

    pub(super) fn sync_loop_controls(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        let (enabled, start_beats, end_beats, bpm) = {
            let timeline = self.timeline.read(cx);
            let transport = &timeline.state.transport;
            (
                transport.loop_enabled,
                transport.loop_start_beats,
                transport.loop_end_beats,
                timeline.state.bpm,
            )
        };
        let bpm = bpm.max(1.0) as f64;
        let start_seconds = start_beats as f64 * 60.0 / bpm;
        let end_seconds = end_beats as f64 * 60.0 / bpm;
        if let Err(error) = engine.set_loop(enabled, start_seconds, end_seconds) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set loop failed: {error}");
            }
        }
    }

    /// Apply a delta-based BPM drag sample. Accumulates `cur_y - prev_y`
    /// against the captured `start_bpm` so the BPM range is bounded by
    /// modifier sensitivity, not by the window height — i.e. the cursor
    /// hitting the screen edge no longer caps the value (FL Studio style).
    pub(super) fn apply_bpm_drag_sample(
        &mut self,
        sample: components::BpmDragSample,
        cx: &mut Context<Self>,
    ) {
        let new_drag = self.bpm_drag_active_id != Some(sample.drag_id);
        if new_drag {
            self.bpm_drag_active_id = Some(sample.drag_id);
            self.bpm_drag_prev_y = sample.cur_y;
            self.bpm_drag_accum = 0.0;
            self.last_engine_bpm_commit = None;
            if components::bpm_debug_enabled() {
                let maximized = cx.windows().iter().any(|w| {
                    w.update(cx, |_, window, _| window.is_maximized())
                        .unwrap_or(false)
                });
                let scale = cx
                    .windows()
                    .first()
                    .and_then(|w| w.update(cx, |_, window, _| window.scale_factor()).ok())
                    .unwrap_or(1.0);
                eprintln!(
                    "[transport-bpm] drag_start id={} start_bpm={:.2} maximized={} scale_factor={:.2}",
                    sample.drag_id, sample.start_bpm, maximized, scale
                );
            }
            return;
        }
        let raw_delta = sample.cur_y - self.bpm_drag_prev_y;
        // Deadzone: ignore sub-pixel jitter / coalesced events with no real
        // motion. Without this, OS event noise can wobble the accumulator.
        if raw_delta.abs() < components::BPM_DRAG_DEADZONE_PX {
            return;
        }
        self.bpm_drag_prev_y = sample.cur_y;
        let sensitivity =
            components::bpm_drag_sensitivity(sample.shift, sample.control || sample.platform);
        // Up = positive BPM change. Screen Y grows downward, so negate.
        self.bpm_drag_accum -= raw_delta * sensitivity;

        let raw = sample.start_bpm + self.bpm_drag_accum;
        let clamped = raw.clamp(components::BPM_MIN, components::BPM_MAX);
        let snapped = if sample.shift {
            (clamped * 10.0).round() / 10.0
        } else {
            clamped.round()
        };
        if components::bpm_debug_enabled() {
            eprintln!(
                "[transport-bpm] move delta_y={:.2} accum={:.2} sens={:.3} computed={:.2}",
                raw_delta, self.bpm_drag_accum, sensitivity, snapped
            );
        }
        // While dragging, throttle engine tempo commits to ~30 Hz. UI state
        // is updated every event for a smooth readout; the audio engine
        // tempo only changes a few dozen times per second.
        let now = Instant::now();
        let engine_due = match self.last_engine_bpm_commit {
            Some(t) => now.duration_since(t) >= Duration::from_millis(33),
            None => true,
        };
        self.set_native_bpm_inner(snapped, engine_due, cx);
        if engine_due {
            self.last_engine_bpm_commit = Some(now);
        }
    }

    /// Apply a new BPM from the transport BPM drag. Updates the timeline
    /// state and sends a lightweight `set_bpm` to the engine — never reloads
    /// the project. Skips notify when the value would round to the same
    /// stored value to avoid notify-spam during mouse motion.
    pub(super) fn set_native_bpm(&mut self, bpm: f32, cx: &mut Context<Self>) {
        self.set_native_bpm_inner(bpm, true, cx);
    }

    pub(super) fn set_native_bpm_inner(
        &mut self,
        bpm: f32,
        commit_to_engine: bool,
        cx: &mut Context<Self>,
    ) {
        let bpm = bpm.clamp(components::BPM_MIN, components::BPM_MAX);
        let changed = self.timeline.update(cx, |timeline, cx| {
            if (timeline.state.bpm - bpm).abs() > 0.005 {
                timeline.state.bpm = bpm;
                cx.notify();
                true
            } else {
                false
            }
        });
        if !changed {
            return;
        }
        if commit_to_engine {
            if let Some(engine) = self.audio_engine.as_ref() {
                if let Err(error) = engine.set_bpm(bpm as f64) {
                    if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                        eprintln!("[audio] set BPM failed: {error}");
                    }
                }
            }
            if std::env::var_os("FUTUREBOARD_TRANSPORT_DEBUG").is_some() {
                eprintln!("[transport-bpm] commit bpm={:.2}", bpm);
            }
        }
        cx.notify();
    }

    pub(super) fn stop_native_playback(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        if let Err(error) = engine.pause() {
            self.audio_last_error = Some(error.to_string());
            eprintln!("[audio] stop transport failed: {error}");
            return;
        }
        self.audio_stats = Some(engine.stats());
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.transport.playing = false;
            cx.notify();
        });
    }

    pub(super) fn seek_native_playhead(&mut self, cx: &mut Context<Self>, beat: f32) {
        let beat = beat.max(0.0);
        let bpm = {
            let timeline = self.timeline.read(cx);
            timeline.state.bpm
        };
        if let Some(engine) = self.audio_engine.as_ref() {
            let seconds = beat as f64 * 60.0 / bpm.max(1.0) as f64;
            if let Err(error) = engine.seek(seconds) {
                self.audio_last_error = Some(error.to_string());
                eprintln!("[audio] seek failed: {error}");
            }
        }
        self.last_engine_playhead_beat = beat;
        self.last_engine_sync = Instant::now();
        let _ = self.timeline.update(cx, move |timeline, cx| {
            timeline.state.transport.playhead_beats = beat;
            // Preview Track Volume automation at the new playhead so a stopped
            // seek updates the fader/inspector to the value under the cursor.
            timeline.state.recompute_effective_volumes(beat, "seek");
            cx.notify();
        });
    }
}
