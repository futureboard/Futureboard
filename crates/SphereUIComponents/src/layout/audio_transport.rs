use gpui::Context;

use std::time::{Duration, Instant};

use crate::components;

use super::engine_snapshot::{build_engine_project_snapshot, log_engine_sync_snapshot};
use super::helpers::smooth_meter_value;
use super::StudioLayout;

impl StudioLayout {
    pub(super) fn spawn_audio_poll(cx: &mut Context<Self>) {
        // 16 ms ≈ 60 Hz — matches a typical display refresh and is fine for
        // VU + transport-time animation. The engine produces position
        // snapshots at audio-block boundaries (~5-10 ms at 256-sample
        // buffers), but the UI never needs to repaint faster than the
        // display, so we cap polling at the refresh interval and let
        // `interpolated_playhead_beat` smooth between engine snapshots.
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| loop {
            executor.timer(Duration::from_millis(16)).await;
            let Ok((changed, mixer_handle)) = this.update(cx, |this, cx| {
                let changed = this.poll_native_audio(cx);
                (changed, this.mixer_window.clone())
            }) else {
                continue;
            };
            if changed {
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
        let _s = crate::perf::PerfScope::enter("poll_native_audio");
        if self.audio_engine.is_none() {
            return false;
        }

        if self.engine_project_dirty || self.engine_media_dirty {
            self.schedule_audio_project_sync(cx, false, "engine_dirty_poll");
        }

        let engine = self.audio_engine.as_ref().expect("checked above");
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

        let bpm = {
            let timeline = self.timeline.read(cx);
            timeline.state.bpm
        };
        let engine_beat = (stats.position_seconds * bpm.max(1.0) as f64 / 60.0) as f32;
        self.last_engine_playhead_beat = engine_beat.max(0.0);
        self.last_engine_sync = Instant::now();
        self.apply_engine_meters(cx);

        if stats.transport_playing {
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
        state_changed || was_playing
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
    pub(super) fn apply_engine_meters(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        // Throttle meter polling to `PowerMode::meter_update_hz`. The audio
        // poll fires at 60 Hz; on low-end GPUs that's too many meter writes
        // and the resulting notify cascade is what drove FPS drops.
        let power = crate::perf::power_mode();
        let min_interval = Duration::from_secs_f32(1.0 / power.meter_update_hz().max(1.0));
        if self.last_meter_apply.elapsed() < min_interval {
            return;
        }
        self.last_meter_apply = Instant::now();
        let meters = engine.meters();
        let _ = self.timeline.update(cx, |timeline, _cx| {
            for track_meter in meters.tracks {
                if let Some(track) = timeline
                    .state
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == track_meter.track_id)
                {
                    let next_l = track_meter.peak_l.clamp(0.0, 1.0) as f32;
                    let next_r = track_meter.peak_r.clamp(0.0, 1.0) as f32;
                    let _ = smooth_meter_value(&mut track.meter_level_l, next_l);
                    let _ = smooth_meter_value(&mut track.meter_level_r, next_r);
                }
            }
            let _ = smooth_meter_value(
                &mut timeline.state.master.meter_level_l,
                meters.master_peak_l.clamp(0.0, 1.0) as f32,
            );
            let _ = smooth_meter_value(
                &mut timeline.state.master.meter_level_r,
                meters.master_peak_r.clamp(0.0, 1.0) as f32,
            );
        });
    }

    /// Queue a background engine sync. `load_project` decodes media on the
    /// caller thread — never invoke it from the UI poll loop or render path.
    pub(crate) fn schedule_audio_project_sync(
        &mut self,
        cx: &mut Context<Self>,
        force: bool,
        reason: &'static str,
    ) {
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
        let snapshot = {
            let timeline = self.timeline.read(cx);
            build_engine_project_snapshot(&timeline.state, sample_rate)
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
            let result = cx
                .background_executor()
                .spawn(async move { engine.load_project(snapshot) })
                .await;
            let _ = owner.update(cx, |this, cx| {
                this.complete_audio_project_sync(cx, result, signature);
            });
            let studio_id = owner.entity_id();
            let _ = cx.update(|app| app.notify(studio_id));
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
        if self
            .audio_stats
            .as_ref()
            .map(|stats| stats.running)
            .unwrap_or(self.audio_running)
        {
            return true;
        }

        let Some(engine) = self.audio_engine.as_mut() else {
            self.audio_last_error = Some("audio engine unavailable".to_string());
            return false;
        };
        match engine.start() {
            Ok(()) => {
                self.audio_stats = Some(engine.stats());
                self.audio_running = true;
                self.audio_last_error = None;
                true
            }
            Err(error) => {
                self.audio_running = false;
                self.audio_last_error = Some(error.to_string());
                eprintln!("[audio] stream warm-up failed: {error}");
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
        eprintln!("[transport] Play requested");
        if self.audio_engine.is_none() {
            self.audio_last_error = Some("audio engine unavailable".to_string());
            return;
        }

        if !self.ensure_audio_stream_warm() {
            return;
        }

        if self.audio_sync_in_flight {
            self.pending_play_after_sync = true;
            return;
        }
        if self.engine_project_dirty || self.engine_media_dirty {
            self.pending_play_after_sync = true;
            self.schedule_audio_project_sync(cx, false, "transport_play_pending_sync");
            return;
        }
        self.sync_metronome_controls(cx);

        let (playhead_beats, bpm) = {
            let timeline = self.timeline.read(cx);
            (timeline.state.transport.playhead_beats, timeline.state.bpm)
        };
        let seconds = playhead_beats.max(0.0) as f64 * 60.0 / bpm.max(1.0) as f64;
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        if let Err(error) = engine.seek(seconds) {
            self.audio_last_error = Some(error.to_string());
            eprintln!("[audio] seek before play failed: {error}");
            return;
        }

        // Surface what actually made it into the realtime runtime. Silent
        // playback almost always shows `loaded_clips=0` or `ready_clips=0`
        // here — typically a missing/unreadable media path.
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

        if let Err(error) = engine.play() {
            self.audio_last_error = Some(error.to_string());
            eprintln!("[audio] play failed: {error}");
            return;
        }

        self.audio_stats = Some(engine.stats());
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.transport.playing = true;
            cx.notify();
        });
        self.poll_native_audio(cx);
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
            cx.notify();
        });
    }
}
