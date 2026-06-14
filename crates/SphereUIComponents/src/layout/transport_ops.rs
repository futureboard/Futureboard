use gpui::{Context, Window};

use std::sync::Arc;

use crate::components;
use crate::components::text_input::TextInputState;

use super::{StudioLayout, TransportCommand};

/// Inline BPM + time-signature numeric editors opened from the transport bar
/// (the small pop-up fields on the BPM / time-sig displays). Second
/// `StudioLayout` decomposition slice — accessed across the transport ops
/// modules. Holds focus-handle-backed text inputs, so it is built via
/// `new(cx)` rather than `Default`.
pub(crate) struct TempoEditState {
    /// Inline numeric BPM editor field.
    pub bpm_input: TextInputState,
    /// Whether the inline BPM editor is open.
    pub bpm_editing: bool,
    /// Inline time-signature numerator field.
    pub ts_num_input: TextInputState,
    /// Inline time-signature denominator field.
    pub ts_den_input: TextInputState,
    /// Whether the inline time-signature editor is open.
    pub ts_editing: bool,
    /// `Some(id)` edits one time-sig marker; `None` edits the project default.
    pub ts_edit_point_id: Option<String>,
    /// True while the numerator field holds focus (false → denominator).
    pub ts_edit_focus_num: bool,
}

impl TempoEditState {
    pub(super) fn new(cx: &mut Context<StudioLayout>) -> Self {
        Self {
            bpm_input: TextInputState::new("transport-bpm-input", cx.focus_handle()),
            bpm_editing: false,
            ts_num_input: TextInputState::new("transport-ts-num-input", cx.focus_handle()),
            ts_den_input: TextInputState::new("transport-ts-den-input", cx.focus_handle()),
            ts_editing: false,
            ts_edit_point_id: None,
            ts_edit_focus_num: true,
        }
    }
}

impl StudioLayout {
    pub(super) fn zoom_timeline_by(&self, cx: &mut Context<Self>, factor: f32) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.zoom_by(factor, 0.0);
            cx.notify();
        });
    }

    pub(super) fn reset_timeline_zoom(&self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            let current = timeline.state.viewport.pixels_per_second.max(0.0001);
            // 150 px/s matches the Web UI default zoom (see timeline_state.rs:460).
            let factor = 150.0 / current;
            timeline.state.zoom_by(factor, 0.0);
            cx.notify();
        });
    }

    pub(super) fn project_end_beat(&self, cx: &mut Context<Self>) -> f32 {
        let timeline = self.timeline.read(cx);
        timeline
            .state
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| clip.start_beat + clip.duration_beats)
            .fold(0.0_f32, f32::max)
    }

    pub(super) fn nudge_playhead_bars(&mut self, cx: &mut Context<Self>, bars: f32) {
        let (current_beat, num) = {
            let timeline = self.timeline.read(cx);
            (
                timeline.state.transport.playhead_beats,
                timeline.state.time_signature_num as f32,
            )
        };
        let target = (current_beat + bars * num.max(1.0)).max(0.0);
        self.seek_native_playhead(cx, target);
    }

    pub(super) fn dispatch_transport_command(
        &mut self,
        command: TransportCommand,
        cx: &mut Context<Self>,
    ) {
        match command {
            TransportCommand::PlayPause => {
                if self.is_recording_active(cx) {
                    self.log_transport_debug("Spacebar", "stop_recording_and_stop_transport", cx);
                    self.stop_native_recording(cx);
                    return;
                }
                let playing = self
                    .audio_stats
                    .as_ref()
                    .map(|stats| stats.transport_playing)
                    .unwrap_or(false);
                if playing {
                    self.stop_native_playback(cx);
                } else {
                    self.start_native_playback(cx);
                }
            }
            TransportCommand::Stop => {
                if self.is_recording_active(cx) {
                    self.log_transport_debug("Stop", "stop_recording_and_stop_transport", cx);
                    self.stop_native_recording(cx);
                } else {
                    self.stop_native_playback(cx);
                }
            }
            TransportCommand::ReturnToStart => self.seek_native_playhead(cx, 0.0),
            TransportCommand::ToggleLoop => {
                let _ = self.timeline.update(cx, |timeline, cx| {
                    timeline.state.transport.loop_enabled = !timeline.state.transport.loop_enabled;
                    cx.notify();
                });
                self.sync_loop_controls(cx);
            }
            TransportCommand::ToggleMetronome => {
                let enabled = self.timeline.update(cx, |timeline, cx| {
                    timeline.state.transport.metronome_enabled =
                        !timeline.state.transport.metronome_enabled;
                    let enabled = timeline.state.transport.metronome_enabled;
                    cx.notify();
                    enabled
                });
                if let (enabled, Some(engine)) = (enabled, self.audio_engine.as_ref()) {
                    if let Err(error) = engine.set_metronome_enabled(enabled) {
                        if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                            eprintln!("[audio] set metronome failed: {error}");
                        }
                    }
                }
            }
            TransportCommand::ToggleFollowPlayhead => {
                let enabled = self.timeline.update(cx, |timeline, cx| {
                    timeline.state.follow_playhead = !timeline.state.follow_playhead;
                    let enabled = timeline.state.follow_playhead;
                    cx.notify();
                    enabled
                });
                if std::env::var_os("FUTUREBOARD_AUTOSCROLL_DEBUG").is_some() {
                    eprintln!("[autoscroll] toggled follow_playhead -> {}", enabled);
                }
            }
            TransportCommand::Record => {
                if self.is_recording_active(cx) {
                    self.log_transport_debug("Record", "stop_recording_and_stop_transport", cx);
                }
                self.toggle_native_recording(cx)
            }
        }
    }

    pub(super) fn is_recording_active(&self, cx: &mut Context<Self>) -> bool {
        self.timeline.read(cx).state.transport.recording
            || self.recording.preview.is_some()
            || self
                .audio_engine
                .as_ref()
                .map(|engine| engine.recording_status().active)
                .unwrap_or(false)
    }

    pub(super) fn log_transport_debug(&self, event: &str, action: &str, cx: &mut Context<Self>) {
        if std::env::var_os("FUTUREBOARD_TRANSPORT_DEBUG").is_none() {
            return;
        }
        let timeline = self.timeline.read(cx);
        eprintln!(
            "[TransportDebug] event={} before playing={} recording={} active_recording_id={:?} action={}",
            event,
            timeline.state.transport.playing,
            timeline.state.transport.recording,
            self.recording.preview.as_ref().map(|p| p.recording_id),
            action
        );
    }

    pub(super) fn transport_chrome_state(
        &self,
        cx: &mut Context<Self>,
    ) -> components::TransportChromeState {
        let (
            position_label,
            bpm_value,
            bpm_label,
            bpm_has_automation,
            time_signature_label,
            ts_has_markers,
            recording,
            loop_enabled,
            metronome_enabled,
            follow_playhead,
        ) = {
            let timeline = self.timeline.read(cx);
            // The transport always shows the *effective* BPM at the playhead so
            // tempo automation is visible without opening the Tempo Track.
            let bpm = timeline.state.effective_bpm_at_playhead() as f32;
            let bpm_has_automation = timeline.state.tempo_has_automation();
            let bpm_label = if (bpm.fract()).abs() < 0.05 {
                format!("{:.0}", bpm)
            } else {
                format!("{:.1}", bpm)
            };
            (
                timeline
                    .state
                    .format_bar_beat(timeline.state.transport.playhead_beats),
                bpm,
                bpm_label,
                bpm_has_automation,
                {
                    let pt = timeline.state.time_signature_at_playhead();
                    format!("{}/{}", pt.numerator, pt.denominator)
                },
                timeline.state.time_signature_has_markers(),
                timeline.state.transport.recording
                    || self
                        .audio_engine
                        .as_ref()
                        .map(|engine| engine.recording_status().active)
                        .unwrap_or(false),
                timeline.state.transport.loop_enabled,
                timeline.state.transport.metronome_enabled,
                timeline.state.follow_playhead,
            )
        };
        let playing = self
            .audio_stats
            .as_ref()
            .map(|stats| stats.transport_playing)
            .unwrap_or(false);
        let make_command_handler = |command_id: &'static str| {
            let this = cx.entity().clone();
            Arc::new(move |_: &(), _window: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id(command_id, cx);
                    cx.notify();
                });
            })
        };

        let on_return_to_start = make_command_handler("transport:go-to-start");
        let on_play_toggle = make_command_handler("transport:play-pause");
        let on_stop = make_command_handler("transport:stop");
        let on_loop_toggle = make_command_handler("transport:toggle-loop");
        let on_metronome_toggle = make_command_handler("transport:toggle-metronome");
        let on_follow_toggle = make_command_handler("transport:toggle-follow-playhead");
        let on_record = make_command_handler("transport:record");

        let on_set_bpm: components::BpmChangeCb = {
            let this = cx.entity().clone();
            Arc::new(move |bpm: &f32, _window: &mut Window, cx: &mut gpui::App| {
                let bpm = bpm.clamp(components::BPM_MIN, components::BPM_MAX);
                let _ = this.update(cx, |this, cx| {
                    this.set_native_bpm(bpm, cx);
                });
            })
        };

        let on_bpm_drag: components::BpmDragCb = {
            let this = cx.entity().clone();
            Arc::new(
                move |sample: &components::BpmDragSample,
                      _window: &mut Window,
                      cx: &mut gpui::App| {
                    let sample = *sample;
                    let _ = this.update(cx, |this, cx| {
                        this.apply_bpm_drag_sample(sample, cx);
                    });
                },
            )
        };

        let on_bpm_menu: components::BpmMenuCb = {
            let this = cx.entity().clone();
            Arc::new(
                move |pos: &(f32, f32), _window: &mut Window, cx: &mut gpui::App| {
                    let (x, y) = *pos;
                    let _ = this.update(cx, |this, cx| {
                        this.open_tempo_menu(x, y, cx);
                    });
                },
            )
        };

        let on_bpm_edit_start: components::ChromeActionCb = {
            let this = cx.entity().clone();
            Arc::new(move |_: &(), _window: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |this, cx| {
                    this.begin_bpm_edit(cx);
                });
            })
        };

        let on_ts_menu: components::BpmMenuCb = {
            let this = cx.entity().clone();
            Arc::new(
                move |pos: &(f32, f32), _window: &mut Window, cx: &mut gpui::App| {
                    let (x, y) = *pos;
                    let _ = this.update(cx, |this, cx| {
                        this.open_time_signature_menu(x, y, cx);
                    });
                },
            )
        };

        let on_ts_edit_start: components::ChromeActionCb = {
            let this = cx.entity().clone();
            Arc::new(move |_: &(), _window: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |this, cx| {
                    this.begin_ts_edit(None, cx);
                });
            })
        };

        components::TransportChromeState {
            playing,
            recording,
            loop_enabled,
            metronome_enabled,
            follow_playhead,
            position_label,
            bpm: bpm_value,
            bpm_label,
            bpm_has_automation,
            bpm_editing: self.tempo_edit.bpm_editing,
            bpm_input: self.tempo_edit.bpm_input.clone(),
            // The layout's key handler routes keys while editing, so render the
            // caret whenever the editor is open.
            bpm_edit_focused: self.tempo_edit.bpm_editing,
            time_signature_label,
            ts_has_markers,
            ts_editing: self.tempo_edit.ts_editing,
            ts_num_input: self.tempo_edit.ts_num_input.clone(),
            ts_den_input: self.tempo_edit.ts_den_input.clone(),
            ts_edit_focus_num: self.tempo_edit.ts_edit_focus_num,
            on_ts_menu,
            on_ts_edit_start,
            on_return_to_start,
            on_play_toggle,
            on_stop,
            on_record,
            on_loop_toggle,
            on_metronome_toggle,
            on_follow_toggle,
            on_set_bpm,
            on_bpm_drag,
            on_bpm_menu,
            on_bpm_edit_start,
        }
    }

    pub(super) fn status_text(&self) -> (String, String) {
        let left = match (
            self.recording.ui_state.status_text(),
            &self.audio_last_error,
            &self.audio_stats,
        ) {
            (Some(status), _, _) => status,
            (None, Some(error), _) => format!("Audio: {error}"),
            (None, _, Some(stats)) if stats.transport_playing => "Playing".to_string(),
            (None, _, Some(stats)) if stats.running => "Audio ready".to_string(),
            (None, _, _) => "Ready".to_string(),
        };
        let audio = self
            .audio_stats
            .as_ref()
            .map(|stats| {
                format!(
                    "{} Hz  {}  Latency: {:.1} ms",
                    stats.sample_rate.max(1),
                    stats.backend_name,
                    stats.estimated_latency_ms
                )
            })
            .unwrap_or_else(|| "Audio offline".to_string());
        let renderer =
            crate::components::timeline::timeline_surface::active_timeline_renderer_backend();
        // UI repaint cadence. Idle scenes stop updating when nothing is dirty.
        let right = format!(
            "{}  •  UI {}  •  {}",
            audio,
            renderer,
            self.frame_diag.hud()
        );
        (left, right)
    }

    pub(super) fn frame_reason(&self) -> &'static str {
        let playing = self
            .audio_stats
            .as_ref()
            .map(|s| s.transport_playing)
            .unwrap_or(false);
        if playing {
            return "transport";
        }
        if self.bottom_panel_state.is_resizing {
            return "panel-resize";
        }
        if self.open_popover.is_some() || self.menu_bar.open_menu_id.is_some() {
            return "menu";
        }
        "idle/interaction"
    }
}
