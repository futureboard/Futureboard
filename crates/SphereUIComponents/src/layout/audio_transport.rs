use gpui::{App, Context};

use std::time::{Duration, Instant};

use crate::components;

use super::engine_snapshot::{build_engine_project_snapshot, log_engine_sync_snapshot};
use super::helpers::{smooth_meter_value, update_meter_clip, update_meter_hold};
use super::transport_freeze_debug::{self, PlayWatchdog};
use super::{ContextTarget, OpenPopover, StudioLayout};

impl StudioLayout {
    pub(super) fn dispatch_midi_preview_command(
        &mut self,
        command: components::piano_roll::UiMidiPreviewCommand,
        cx: &App,
    ) {
        let ui_start = Instant::now();
        let track_id = match &command {
            components::piano_roll::UiMidiPreviewCommand::NoteOn { track_id, .. }
            | components::piano_roll::UiMidiPreviewCommand::NoteOff { track_id, .. }
            | components::piano_roll::UiMidiPreviewCommand::AllNotesOff { track_id }
            | components::piano_roll::UiMidiPreviewCommand::MidiPanic { track_id } => {
                track_id.clone()
            }
        };
        let bridge_instance = self.resolve_track_instrument_plugin(&track_id, cx);
        // Per-note hot path: never block the GPUI thread on the bridge runtime
        // mutex. A background save / plugin-state capture (`request_plugin_states`,
        // ~1.5s) or a plugin load can hold it, and pressing notes or muting while
        // it was held used to freeze the UI. Probe with `try_lock`; if it is busy
        // but this track has a bridged instrument, route through the lock-free
        // engine command bus anyway (the correct path once a bridge plugin exists).
        let sink_ready = match self.plugin_bridge_runtime.as_ref().map(|rt| rt.try_lock()) {
            Some(Ok(bridge)) => bridge
                .loaded_instance_ids()
                .into_iter()
                .any(|id| bridge.audio_sink_for(&id).is_some()),
            Some(Err(_)) => bridge_instance.is_some(),
            None => false,
        };

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
                        "[midi-preview-ui] note_on track={track_id} pitch={pitch} velocity={velocity} source=piano_key"
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
                        "[midi-preview-ui] note_off track={track_id} pitch={pitch} source=piano_key"
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
            if crate::forensic_trace::preview_perf_trace_enabled() {
                eprintln!(
                    "[preview-perf] ui_handler_ms={:.3}",
                    ui_start.elapsed().as_secs_f64() * 1000.0
                );
            }
            return;
        }

        if let Some(instance_id) = bridge_instance {
            if let Some(runtime) = self.plugin_bridge_runtime.as_ref() {
                // try_lock, not lock: this IPC fallback runs on the GPUI thread
                // and must not stall it if a background bridge op holds the mutex.
                // On contention we fall through to the engine MIDI-preview path.
                if let Ok(mut bridge) = runtime.try_lock() {
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
                            channel,
                            pitch,
                            ..
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
            components::piano_roll::UiMidiPreviewCommand::NoteOff { channel, pitch, .. } => {
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
        use crate::components::timeline::timeline_state::{PluginRuntimeBackend, TrackType};
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
        if crate::forensic_trace::forensic_trace_enabled() {
            for track_meter in &meters.tracks {
                let peak = track_meter.peak_l.max(track_meter.peak_r);
                if peak > 0.0001 {
                    eprintln!("[Meter] track={} peak={peak:.6}", track_meter.track_id);
                }
            }
            let master_peak = meters.master_peak_l.max(meters.master_peak_r);
            if master_peak > 0.0001 {
                eprintln!("[Meter] master peak={master_peak:.6}");
            }
        }
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

        // Project load can finish plugin restore before the audio engine is
        // warm; re-bind bridge sinks whenever a graph swap completes.
        if self.sync_plugin_bridge_sinks_to_engine(cx, "audio_project_sync_complete") {
            eprintln!("[PluginRestore] graph snapshot swapped generation=post_sync");
        }

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
        {
            let timeline = self.timeline.read(cx);
            crate::forensic_trace::dump_midi_model(&timeline.state);
        }

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
            tempo_points,
            metronome_enabled,
            loop_enabled,
            loop_start_beats,
            loop_end_beats,
            ts_points,
        ) = {
            transport_freeze_debug::log("before timeline.read");
            let timeline = self.timeline.read(cx);
            transport_freeze_debug::log("after timeline.read");
            let tempo_points = timeline
                .state
                .tempo_map
                .points
                .iter()
                .map(|p| DAUx::types::EngineTempoPointSnapshot {
                    beat: p.beat,
                    bpm: p.bpm,
                })
                .collect::<Vec<_>>();
            let ts_points = timeline
                .state
                .time_signature_map
                .points
                .iter()
                .map(
                    |p| DAUx::time_signature_map::RuntimeTimeSignaturePointSnapshot {
                        beat: p.beat,
                        numerator: p.numerator,
                        denominator: p.denominator,
                        grouping: p.effective_grouping(),
                    },
                )
                .collect::<Vec<_>>();
            (
                timeline.state.transport.playhead_beats,
                timeline.state.bpm,
                tempo_points,
                timeline.state.transport.metronome_enabled,
                timeline.state.transport.loop_enabled,
                timeline.state.transport.loop_start_beats,
                timeline.state.transport.loop_end_beats,
                ts_points,
            )
        };

        let Some(engine) = self.audio_engine.as_ref() else {
            transport_freeze_debug::log("abort: engine handle missing");
            return;
        };

        transport_freeze_debug::log("before engine transport sync");
        if let Err(error) = engine.set_tempo_map(bpm as f64, tempo_points) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set tempo map failed: {error}");
            }
        }
        if let Err(error) = engine.set_time_signature_map(ts_points) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set time signature map failed: {error}");
            }
        }
        if let Err(error) = engine.set_metronome_enabled(metronome_enabled) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set metronome failed: {error}");
            }
        }
        let (loop_start_seconds, loop_end_seconds, playhead_seconds) = {
            let state = &self.timeline.read(cx).state;
            let base = state.bpm as f64;
            (
                state
                    .tempo_map
                    .seconds_at_beat(loop_start_beats as f64, base),
                state.tempo_map.seconds_at_beat(loop_end_beats as f64, base),
                state
                    .tempo_map
                    .seconds_at_beat(playhead_beats.max(0.0) as f64, base),
            )
        };
        if let Err(error) = engine.set_loop(loop_enabled, loop_start_seconds, loop_end_seconds) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set loop failed: {error}");
            }
        }
        transport_freeze_debug::log("after engine transport sync");

        transport_freeze_debug::log("before engine.seek");
        if let Err(error) = engine.seek(playhead_seconds) {
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
                playhead_seconds,
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
        let (enabled, bpm) = {
            let timeline = self.timeline.read(cx);
            (
                timeline.state.transport.metronome_enabled,
                timeline.state.bpm as f64,
            )
        };
        if let Err(error) = engine.set_bpm(bpm) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set BPM failed: {error}");
            }
        }
        if let Err(error) = engine.set_metronome_enabled(enabled) {
            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                eprintln!("[audio] set metronome failed: {error}");
            }
        }
        self.sync_time_signature_map_to_engine(cx);
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

    /// Apply a BPM drag sample. The drag is screen-edge independent: on Windows
    /// the OS cursor is warped back to a fixed anchor every move, so vertical
    /// dragging accumulates unbounded relative motion (true DAW-style infinite
    /// scrubbing). Tempo safety: when automation exists the drag edits the
    /// active tempo marker at the playhead instead of the fixed project BPM.
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
            // Capture the cursor anchor + window scale for warp-based dragging.
            self.bpm_drag_anchor = cursor_pos_phys();
            self.bpm_drag_scale = cx
                .windows()
                .first()
                .and_then(|w| w.update(cx, |_, window, _| window.scale_factor()).ok())
                .unwrap_or(1.0)
                .max(0.1);
            // Decide what this drag edits, and capture the start value there.
            let (target_point_id, start_value) = {
                let state = &self.timeline.read(cx).state;
                if state.tempo_has_automation() {
                    let beat = state.transport.playhead_beats as f64;
                    match state.tempo_map.point_id_at_or_before_beat(beat) {
                        Some(id) => {
                            let bpm = state
                                .tempo_map
                                .points
                                .iter()
                                .find(|p| p.id == id)
                                .map(|p| p.bpm as f32)
                                .unwrap_or(state.bpm);
                            (Some(id.to_string()), bpm)
                        }
                        // Before the first marker → edit the fixed base tempo.
                        None => (None, state.bpm),
                    }
                } else {
                    (None, state.bpm)
                }
            };
            self.bpm_drag_target_point_id = target_point_id;
            self.bpm_drag_start_value = start_value;
            if components::bpm_debug_enabled() {
                eprintln!(
                    "[transport-bpm] drag_start id={} start_value={:.2} target_point_id={:?} scale={:.2} anchor={:?}",
                    sample.drag_id,
                    start_value,
                    self.bpm_drag_target_point_id,
                    self.bpm_drag_scale,
                    self.bpm_drag_anchor
                );
            }
            return;
        }

        // Relative vertical movement in logical px. Prefer the warped OS cursor
        // delta (edge-independent); fall back to the window-relative position.
        let dy_logical = match self.warp_drag_delta_y() {
            Some(dy) => dy,
            None => {
                let raw_delta = sample.cur_y - self.bpm_drag_prev_y;
                if raw_delta.abs() < components::BPM_DRAG_DEADZONE_PX {
                    return;
                }
                self.bpm_drag_prev_y = sample.cur_y;
                raw_delta
            }
        };
        if dy_logical == 0.0 {
            return;
        }

        let coarse = sample.control || sample.platform || sample.alt;
        let sensitivity = components::bpm_drag_sensitivity(sample.shift, coarse);
        // Up = positive BPM change. Screen Y grows downward, so negate.
        self.bpm_drag_accum -= dy_logical * sensitivity;

        let raw = self.bpm_drag_start_value + self.bpm_drag_accum;
        let clamped = raw.clamp(components::BPM_MIN, components::BPM_MAX);
        let snapped = if sample.shift {
            (clamped * 10.0).round() / 10.0
        } else if coarse {
            (clamped / 5.0).round() * 5.0
        } else {
            clamped.round()
        };

        let now = Instant::now();
        let engine_due = match self.last_engine_bpm_commit {
            Some(t) => now.duration_since(t) >= Duration::from_millis(33),
            None => true,
        };
        let target_point_id = self.bpm_drag_target_point_id.clone();
        self.apply_bpm_value(snapped, target_point_id.as_deref(), engine_due, cx);
        if engine_due {
            self.last_engine_bpm_commit = Some(now);
        }
    }

    /// Edge-independent vertical drag delta (logical px) using OS cursor warp.
    /// Reads the absolute cursor, computes the delta from the anchor, then
    /// re-pins the cursor to the anchor so it can never reach the screen edge.
    /// Returns `None` on platforms without cursor warp so callers fall back to
    /// window-relative deltas.
    fn warp_drag_delta_y(&mut self) -> Option<f32> {
        let anchor = self.bpm_drag_anchor?;
        let cur = cursor_pos_phys()?;
        let dy_phys = cur.1 - anchor.1;
        if dy_phys == 0 {
            // Echo from our own warp, or no motion.
            return Some(0.0);
        }
        set_cursor_pos_phys(anchor.0, anchor.1);
        Some(dy_phys as f32 / self.bpm_drag_scale)
    }

    /// Cancel the active BPM drag, restoring the value captured at drag start.
    /// Wired to Escape.
    pub(super) fn cancel_bpm_drag(&mut self, cx: &mut Context<Self>) -> bool {
        if self.bpm_drag_active_id.is_none() {
            return false;
        }
        let start = self.bpm_drag_start_value;
        let target_point_id = self.bpm_drag_target_point_id.clone();
        self.apply_bpm_value(start, target_point_id.as_deref(), true, cx);
        self.end_bpm_drag();
        true
    }

    /// Clears transient BPM-drag bookkeeping. The next `on_drag` gets a fresh
    /// `drag_id`, so this is only needed for explicit cancel.
    pub(super) fn end_bpm_drag(&mut self) {
        self.bpm_drag_active_id = None;
        self.bpm_drag_anchor = None;
        self.bpm_drag_accum = 0.0;
    }

    /// Apply a BPM value to the correct target: a tempo marker (mapped mode) or
    /// the fixed project BPM. Centralizes the tempo-safety rule so dragging,
    /// inline edit, and menu commands all behave consistently.
    pub(super) fn apply_bpm_value(
        &mut self,
        bpm: f32,
        target_point_id: Option<&str>,
        commit_to_engine: bool,
        cx: &mut Context<Self>,
    ) {
        let bpm = bpm.clamp(components::BPM_MIN, components::BPM_MAX);
        match target_point_id {
            None => self.set_native_bpm_inner(bpm, commit_to_engine, cx),
            Some(point_id) => {
                let changed = self.timeline.update(cx, |timeline, cx| {
                    let updated = timeline
                        .state
                        .tempo_map
                        .update_point_bpm_by_id(point_id, bpm as f64);
                    if updated {
                        cx.notify();
                    }
                    updated
                });
                if changed {
                    self.mark_dirty();
                    if commit_to_engine {
                        self.sync_tempo_map_to_engine(cx);
                    }
                    cx.notify();
                }
            }
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

    /// Open the compact tempo menu (anchored at screen `x`,`y`) from the
    /// transport BPM display. Reuses the shared context-menu overlay.
    pub(super) fn open_tempo_menu(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        self.open_popover = Some(OpenPopover::Context {
            target: ContextTarget::Tempo,
            x,
            y,
        });
        cx.notify();
    }

    /// Add a tempo marker at the current playhead using the effective BPM there.
    /// This is the primary way to introduce tempo automation from the transport.
    pub(super) fn add_tempo_marker_at_playhead(&mut self, cx: &mut Context<Self>) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            let beat = timeline.state.transport.playhead_beats as f64;
            let bpm = timeline.state.effective_bpm_at_beat(beat);
            timeline.state.tempo_map.add_or_update_point(
                beat,
                bpm,
                crate::components::timeline::timeline_state::TempoCurve::Hold,
            );
            cx.notify();
            true
        });
        if changed {
            self.mark_dirty();
            self.sync_tempo_map_to_engine(cx);
            cx.notify();
        }
    }

    /// Remove all tempo automation, keeping the playhead BPM as a single fixed
    /// marker at beat 0.
    pub(super) fn clear_tempo_automation(&mut self, cx: &mut Context<Self>) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            let bpm = timeline.state.effective_bpm_at_playhead();
            if timeline.state.tempo_map.points.len() == 1
                && timeline
                    .state
                    .tempo_map
                    .points
                    .first()
                    .is_some_and(|p| p.beat <= 1e-6 && (p.bpm - bpm).abs() < 1e-6)
            {
                return false;
            }
            timeline.state.bpm = bpm as f32;
            timeline.state.tempo_map.reset_to_single_point(
                0.0,
                bpm,
                crate::components::timeline::timeline_state::TempoCurve::Hold,
            );
            timeline.state.selected_tempo_point_id = None;
            cx.notify();
            true
        });
        if changed {
            self.mark_dirty();
            self.sync_tempo_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn show_tempo_track(&mut self, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, cx| {
            timeline.state.show_tempo_track_lane();
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn hide_tempo_track(&mut self, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, cx| {
            timeline.state.hide_tempo_track_lane();
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn tempo_track_context_position(&self) -> Option<(f64, f64)> {
        match &self.open_popover {
            Some(OpenPopover::Context {
                target: ContextTarget::TempoTrack { beat, bpm, .. },
                ..
            }) => Some((*beat, *bpm)),
            _ => None,
        }
    }

    pub(super) fn tempo_track_context_point_id(&self) -> Option<String> {
        match &self.open_popover {
            Some(OpenPopover::Context {
                target: ContextTarget::TempoTrack { point_id, .. },
                ..
            }) => point_id.clone(),
            _ => None,
        }
    }

    pub(super) fn add_tempo_point_at_lane(&mut self, beat: f64, bpm: f64, cx: &mut Context<Self>) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.add_tempo_point(beat, bpm) {
                timeline.state.select_tempo_point(&id);
                cx.notify();
                true
            } else {
                false
            }
        });
        if changed {
            self.mark_dirty();
            self.sync_tempo_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn set_fixed_tempo_from_lane(
        &mut self,
        beat: f64,
        bpm: f64,
        cx: &mut Context<Self>,
    ) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            timeline.state.set_fixed_tempo_from_beat(beat, bpm);
            timeline.state.bpm = bpm as f32;
            cx.notify();
            true
        });
        if changed {
            self.mark_dirty();
            self.sync_tempo_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn delete_tempo_point(&mut self, id: &str, cx: &mut Context<Self>) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            if timeline.state.delete_tempo_point(id) {
                cx.notify();
                true
            } else {
                false
            }
        });
        if changed {
            self.mark_dirty();
            self.sync_tempo_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn set_tempo_point_curve(
        &mut self,
        id: &str,
        curve: crate::components::timeline::timeline_state::TempoCurve,
        cx: &mut Context<Self>,
    ) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            if timeline.state.set_tempo_point_curve(id, curve) {
                cx.notify();
                true
            } else {
                false
            }
        });
        if changed {
            self.mark_dirty();
            self.sync_tempo_map_to_engine(cx);
            cx.notify();
        }
    }

    /// Convert fixed-tempo mode into a tempo map by seeding an initial marker at
    /// beat 0 using the current project BPM. No-op if automation already exists.
    pub(super) fn create_tempo_automation(&mut self, cx: &mut Context<Self>) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            if timeline.state.tempo_map.has_automation() {
                return false;
            }
            let bpm = timeline.state.bpm as f64;
            timeline.state.tempo_map.add_or_update_point(
                0.0,
                bpm,
                crate::components::timeline::timeline_state::TempoCurve::Hold,
            );
            cx.notify();
            true
        });
        if changed {
            self.mark_dirty();
            self.sync_tempo_map_to_engine(cx);
            cx.notify();
        }
    }

    /// Insert a tempo marker at an explicit beat using the effective BPM there
    /// (used by the timeline ruler context menu). `create_first` seeds tempo
    /// automation if none exists yet.
    pub(super) fn add_tempo_point_at_beat(
        &mut self,
        beat: f64,
        create_first: bool,
        cx: &mut Context<Self>,
    ) {
        let beat = beat.max(0.0);
        let changed = self.timeline.update(cx, |timeline, cx| {
            use crate::components::timeline::timeline_state::TempoCurve;
            if !timeline.state.tempo_map.has_automation() && create_first {
                // Seed an anchor at beat 0 so the new marker reads as a change
                // from the project's fixed tempo.
                let base = timeline.state.bpm as f64;
                if beat > 1e-6 {
                    timeline
                        .state
                        .tempo_map
                        .add_or_update_point(0.0, base, TempoCurve::Hold);
                }
            }
            let bpm = timeline.state.effective_bpm_at_beat(beat);
            timeline
                .state
                .tempo_map
                .add_or_update_point(beat, bpm, TempoCurve::Hold);
            cx.notify();
            true
        });
        if changed {
            self.mark_dirty();
            self.sync_tempo_map_to_engine(cx);
            cx.notify();
        }
    }

    /// Open the inline numeric BPM editor, seeded with the current effective
    /// BPM at the playhead. Keys are routed by the layout's key handler while
    /// `bpm_editing` is set, so no separate focus grab is needed.
    pub(super) fn begin_bpm_edit(&mut self, cx: &mut Context<Self>) {
        // A drag and an edit are mutually exclusive.
        self.end_bpm_drag();
        let bpm = self.timeline.read(cx).state.effective_bpm_at_playhead();
        let text = if (bpm.fract()).abs() < 0.05 {
            format!("{bpm:.0}")
        } else {
            format!("{bpm:.2}")
        };
        self.bpm_input.set_value(text);
        self.bpm_input.select_all();
        self.bpm_editing = true;
        cx.notify();
    }

    /// Commit the inline BPM editor: parse the field and apply to the active
    /// tempo target (marker at playhead when automation exists, else fixed BPM).
    pub(super) fn commit_bpm_edit(&mut self, cx: &mut Context<Self>) {
        if !self.bpm_editing {
            return;
        }
        let parsed = self.bpm_input.value.trim().parse::<f32>().ok();
        self.bpm_editing = false;
        if let Some(bpm) = parsed {
            let target_point_id = {
                let state = &self.timeline.read(cx).state;
                if state.tempo_has_automation() {
                    let beat = state.transport.playhead_beats as f64;
                    state
                        .tempo_map
                        .point_id_at_or_before_beat(beat)
                        .map(|id| id.to_string())
                } else {
                    None
                }
            };
            self.apply_bpm_value(bpm, target_point_id.as_deref(), true, cx);
        }
        cx.notify();
    }

    /// Cancel the inline BPM editor without applying.
    pub(super) fn cancel_bpm_edit(&mut self, cx: &mut Context<Self>) {
        if !self.bpm_editing {
            return;
        }
        self.bpm_editing = false;
        cx.notify();
    }

    /// Push the project TempoMap to the audio engine so playback, metronome,
    /// and MIDI scheduling use the authoritative hold-mode segments.
    pub(super) fn sync_tempo_map_to_engine(&mut self, cx: &mut Context<Self>) {
        let (default_bpm, points) = {
            let state = &self.timeline.read(cx).state;
            (
                state.bpm as f64,
                state
                    .tempo_map
                    .points
                    .iter()
                    .map(|p| DAUx::types::EngineTempoPointSnapshot {
                        beat: p.beat,
                        bpm: p.bpm,
                    })
                    .collect::<Vec<_>>(),
            )
        };
        if let Some(engine) = self.audio_engine.as_ref() {
            if let Err(error) = engine.set_tempo_map(default_bpm, points) {
                if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                    eprintln!("[audio] sync tempo map failed: {error}");
                }
            }
        }
    }

    pub(super) fn sync_time_signature_map_to_engine(&mut self, cx: &mut Context<Self>) {
        let points = {
            let state = &self.timeline.read(cx).state;
            state
                .time_signature_map
                .points
                .iter()
                .map(
                    |p| DAUx::time_signature_map::RuntimeTimeSignaturePointSnapshot {
                        beat: p.beat,
                        numerator: p.numerator,
                        denominator: p.denominator,
                        grouping: p.effective_grouping(),
                    },
                )
                .collect::<Vec<_>>()
        };
        if let Some(engine) = self.audio_engine.as_ref() {
            if let Err(error) = engine.set_time_signature_map(points) {
                if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                    eprintln!("[audio] sync time signature map failed: {error}");
                }
            }
        }
    }

    pub(super) fn open_time_signature_menu(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        self.open_popover = Some(OpenPopover::Context {
            target: ContextTarget::TimeSignature,
            x,
            y,
        });
        cx.notify();
    }

    pub(super) fn show_time_signature_track(&mut self, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, cx| {
            timeline.state.show_time_signature_track_lane();
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn hide_time_signature_track(&mut self, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, cx| {
            timeline.state.hide_time_signature_track_lane();
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn add_time_signature_marker_at_playhead(&mut self, cx: &mut Context<Self>) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            let beat = timeline.state.transport.playhead_beats as f64;
            let pt = timeline
                .state
                .time_signature_map
                .time_signature_at_beat(beat);
            timeline
                .state
                .add_time_signature_point(beat, pt.numerator, pt.denominator);
            cx.notify();
            true
        });
        if changed {
            self.mark_dirty();
            self.sync_time_signature_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn add_time_signature_point_at_beat(&mut self, beat: f64, cx: &mut Context<Self>) {
        let beat = beat.max(0.0);
        let changed = self.timeline.update(cx, |timeline, cx| {
            let pt = timeline
                .state
                .time_signature_map
                .time_signature_at_beat(beat);
            timeline
                .state
                .add_time_signature_point(beat, pt.numerator, pt.denominator);
            cx.notify();
            true
        });
        if changed {
            self.mark_dirty();
            self.sync_time_signature_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn clear_time_signature_markers(&mut self, cx: &mut Context<Self>) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            let beat = timeline.state.transport.playhead_beats as f64;
            timeline.state.clear_time_signature_markers(beat);
            cx.notify();
            true
        });
        if changed {
            self.mark_dirty();
            self.sync_time_signature_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn delete_time_signature_point(&mut self, id: &str, cx: &mut Context<Self>) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            if timeline.state.delete_time_signature_point(id) {
                cx.notify();
                true
            } else {
                false
            }
        });
        if changed {
            self.mark_dirty();
            self.sync_time_signature_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn move_time_signature_point_to_playhead(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        let changed = self.timeline.update(cx, |timeline, cx| {
            let beat = timeline.state.transport.playhead_beats as f64;
            if timeline.state.move_time_signature_point(id, beat) {
                cx.notify();
                true
            } else {
                false
            }
        });
        if changed {
            self.mark_dirty();
            self.sync_time_signature_map_to_engine(cx);
            cx.notify();
        }
    }

    pub(super) fn ts_track_context_position(&self) -> Option<f64> {
        match &self.open_popover {
            Some(OpenPopover::Context {
                target:
                    ContextTarget::TimeSignatureTrack { beat, .. }
                    | ContextTarget::TimeSignaturePoint { beat, .. },
                ..
            }) => Some(*beat),
            Some(OpenPopover::Context {
                target: ContextTarget::TimelineRuler { beat },
                ..
            }) => Some(*beat),
            _ => None,
        }
    }

    pub(super) fn ts_track_context_point_id(&self) -> Option<String> {
        match &self.open_popover {
            Some(OpenPopover::Context {
                target: ContextTarget::TimeSignatureTrack { point_id, .. },
                ..
            }) => point_id.clone(),
            Some(OpenPopover::Context {
                target: ContextTarget::TimeSignaturePoint { point_id, .. },
                ..
            }) => Some(point_id.clone()),
            _ => None,
        }
    }

    pub(super) fn begin_ts_edit(&mut self, point_id: Option<String>, cx: &mut Context<Self>) {
        let (num, den) = {
            let state = &self.timeline.read(cx).state;
            if let Some(id) = point_id
                .as_deref()
                .or(state.selected_time_signature_point_id.as_deref())
            {
                if let Some(pt) = state.time_signature_map.points.iter().find(|p| p.id == id) {
                    (pt.numerator, pt.denominator)
                } else {
                    let pt = state.time_signature_at_playhead();
                    (pt.numerator, pt.denominator)
                }
            } else {
                let pt = state.time_signature_at_playhead();
                (pt.numerator, pt.denominator)
            }
        };
        self.ts_edit_point_id = point_id.or_else(|| {
            self.timeline
                .read(cx)
                .state
                .selected_time_signature_point_id
                .clone()
        });
        self.ts_num_input.set_value(num.to_string());
        self.ts_den_input.set_value(den.to_string());
        self.ts_num_input.select_all();
        self.ts_editing = true;
        self.ts_edit_focus_num = true;
        cx.notify();
    }

    pub(super) fn commit_ts_edit(&mut self, cx: &mut Context<Self>) {
        if !self.ts_editing {
            return;
        }
        let num = self.ts_num_input.value.trim().parse::<u16>().ok();
        let den = self.ts_den_input.value.trim().parse::<u16>().ok();
        self.ts_editing = false;
        self.ts_edit_focus_num = true;
        let Some(num) = num.filter(|n| (1..=64).contains(n)) else {
            cx.notify();
            return;
        };
        let den = den
            .map(crate::components::timeline::timeline_state::normalize_time_signature_denominator)
            .filter(|d| {
                crate::components::timeline::timeline_state::TS_ALLOWED_DENOMINATORS.contains(d)
            });
        let Some(den) = den else {
            cx.notify();
            return;
        };

        let point_id = self.ts_edit_point_id.clone();
        let changed = self.timeline.update(cx, |timeline, cx| {
            let changed = if let Some(id) = point_id {
                timeline.state.update_time_signature_point(&id, num, den)
            } else {
                let beat = timeline.state.transport.playhead_beats as f64;
                timeline.state.add_time_signature_point(beat, num, den);
                true
            };
            if changed {
                cx.notify();
            }
            changed
        });
        self.ts_edit_point_id = None;
        if changed {
            self.mark_dirty();
            self.sync_time_signature_map_to_engine(cx);
        }
        cx.notify();
    }

    pub(super) fn cancel_ts_edit(&mut self, cx: &mut Context<Self>) {
        if !self.ts_editing {
            return;
        }
        self.ts_editing = false;
        self.ts_edit_point_id = None;
        self.ts_edit_focus_num = true;
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

/// Current OS cursor position in physical screen pixels, if the platform
/// supports querying it. Used to drive screen-edge-independent BPM scrubbing.
#[cfg(target_os = "windows")]
fn cursor_pos_phys() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    Some((p.x, p.y))
}

/// Re-pin the OS cursor to `(x, y)` physical pixels so an active scrub never
/// reaches the screen edge.
#[cfg(target_os = "windows")]
fn set_cursor_pos_phys(x: i32, y: i32) {
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    unsafe {
        let _ = SetCursorPos(x, y);
    }
}

#[cfg(not(target_os = "windows"))]
fn cursor_pos_phys() -> Option<(i32, i32)> {
    None
}

#[cfg(not(target_os = "windows"))]
fn set_cursor_pos_phys(_x: i32, _y: i32) {}
