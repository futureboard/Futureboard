use gpui::{Context, Window};

use std::sync::Arc;

use crate::components;

use super::{StudioLayout, TransportCommand};
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
            TransportCommand::Stop => self.stop_native_playback(cx),
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
            TransportCommand::Record => self.toggle_native_recording(cx),
        }
    }

    pub(super) fn transport_chrome_state(
        &self,
        cx: &mut Context<Self>,
    ) -> components::TransportChromeState {
        let (
            position_label,
            bpm_value,
            bpm_label,
            time_signature_label,
            recording,
            loop_enabled,
            metronome_enabled,
            follow_playhead,
        ) = {
            let timeline = self.timeline.read(cx);
            let bpm = timeline.state.bpm;
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
                format!(
                    "{}/{}",
                    timeline.state.time_signature_num, timeline.state.time_signature_den
                ),
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
        let _on_record = make_command_handler("transport:record");

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

        components::TransportChromeState {
            playing,
            recording,
            loop_enabled,
            metronome_enabled,
            follow_playhead,
            position_label,
            bpm: bpm_value,
            bpm_label,
            time_signature_label,
            on_return_to_start,
            on_play_toggle,
            on_stop,
            on_loop_toggle,
            on_metronome_toggle,
            on_follow_toggle,
            on_set_bpm,
            on_bpm_drag,
        }
    }

    pub(super) fn status_text(&self) -> (String, String) {
        let left = match (&self.audio_last_error, &self.audio_stats) {
            (Some(error), _) => format!("Audio: {error}"),
            (_, Some(stats)) if stats.transport_playing => "Playing".to_string(),
            (_, Some(stats)) if stats.running => "Audio ready".to_string(),
            _ => "Ready".to_string(),
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
        // UI repaint cadence. Idle scenes stop updating when nothing is dirty.
        let right = format!("{}  •  {}", audio, self.frame_diag.hud());
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
