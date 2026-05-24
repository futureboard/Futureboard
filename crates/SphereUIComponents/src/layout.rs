use gpui::{
    div, px, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, ScrollHandle, Styled, Timer, Window,
};

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::components;
use crate::components::file_browser::{read_directory, FileBrowserState};
use crate::components::mixer_panel::MixerCallbacks;
use crate::components::timeline::timeline_state::{
    self, ClipType, TimelineState, TrackState, TrackType,
};
use crate::components::timeline::waveform_cache;
use crate::components::{BottomPanelResizeDrag, BottomPanelState};
use crate::theme::{self, Colors};

use DAUx::types::{
    EngineClipAudioProcess, EngineClipSnapshot, EngineProjectSnapshot, EngineRoutingSnapshot,
    EngineTrackSnapshot,
};

/// Flip to `true` to seed the studio with demo tracks/clips at startup.
/// Production builds must keep this `false` — the real app starts empty.
const USE_DEMO_PROJECT: bool = false;

/// Top-menu open state. `open_menu_id` is the manifest menu id currently
/// showing its dropdown; `anchor_x` is the click x position used to align
/// the dropdown panel underneath the clicked label.
#[derive(Debug, Clone, Default)]
pub struct MenuBarUiState {
    pub open_menu_id: Option<String>,
    pub anchor_x: f32,
    /// Nested submenu ids open underneath the root dropdown. `path[0]` is
    /// the submenu open in the root panel, `path[1]` in *that* submenu's
    /// panel, etc.
    pub submenu_path: Vec<String>,
}

pub struct StudioLayout {
    active_bottom_tab: components::BottomTab,
    bottom_panel_state: BottomPanelState,
    timeline: Entity<components::timeline::Timeline>,
    file_browser: FileBrowserState,
    /// Stable scroll handle for the browser tree. Lives on the layout
    /// (not in `FileBrowserState`) so the state stays free of gpui types
    /// and so the handle survives across renders.
    browser_scroll: ScrollHandle,
    menu_bar: MenuBarUiState,
    audio_engine: Option<DAUx::AudioEngine>,
    audio_running: bool,
    audio_last_error: Option<String>,
    audio_stats: Option<DAUx::EngineStats>,
    last_audio_project_signature: Option<String>,
    last_engine_playhead_beat: f32,
    last_engine_sync: Instant,
    /// Owns keyboard focus for the studio surface. Without a focused
    /// element GPUI never dispatches key events to `capture_key_down`,
    /// so we focus this handle on first render — that is what makes
    /// Spacebar, Enter, L, K, R, Home reach `shortcut_command`.
    focus_handle: FocusHandle,
    /// Menu/key command IDs we've already logged as unsupported. Keeps
    /// the unified dispatcher quiet after the first miss per command.
    logged_unsupported_commands: HashSet<String>,
}

#[derive(Debug, Clone, Copy)]
enum TransportCommand {
    PlayPause,
    Stop,
    ReturnToStart,
    ToggleLoop,
    ToggleMetronome,
    Record,
}

impl StudioLayout {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let audio_engine = match DAUx::AudioEngine::new(DAUx::AudioEngine::default_config()) {
            Ok(engine) => {
                eprintln!(
                    "[audio] sphere-direct-audio-engine v{} ready (backend={:?}, sr={}, buf={})",
                    engine.version(),
                    engine.config().backend,
                    engine.config().sample_rate,
                    engine.config().buffer_size
                );
                let devices = engine.list_output_devices();
                eprintln!("[audio] {} output device(s) discovered", devices.len());
                for d in devices.iter().take(8) {
                    eprintln!(
                        "[audio]   - {} ({} ch @ {} Hz){}",
                        d.name,
                        d.channels,
                        d.default_sample_rate,
                        if d.is_default { "  [default]" } else { "" }
                    );
                }
                Some(engine)
            }
            Err(error) => {
                eprintln!("[audio] failed to initialize engine: {error}");
                None
            }
        };

        let timeline = cx.new(|_| {
            if USE_DEMO_PROJECT {
                components::timeline::Timeline::with_demo_content()
            } else {
                components::timeline::Timeline::new()
            }
        });
        if let Some(engine) = audio_engine.clone() {
            let seek_engine = engine.clone();
            let param_engine = engine.clone();
            let _ = timeline.update(cx, |timeline, _cx| {
                timeline.set_native_audio_callbacks(
                    Some(Arc::new(move |beats, bpm| {
                        let seconds = beats.max(0.0) as f64 * 60.0 / bpm.max(1.0) as f64;
                        if let Err(error) = seek_engine.seek(seconds) {
                            eprintln!("[audio] seek failed: {error}");
                        }
                    })),
                    Some(Arc::new(move |track_id, param_id, value| {
                        let engine_value = match param_id.as_str() {
                            "volume" => volume_norm_to_linear(value) as f64,
                            "mute" | "solo" => {
                                if value >= 0.5 {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            _ => value as f64,
                        };
                        if let Err(error) =
                            param_engine.update_track_param(&track_id, &param_id, engine_value)
                        {
                            if !matches!(error, DAUx::SphereAudioError::EngineNotOpen) {
                                eprintln!(
                                    "[audio] track param update failed: track={} param={} error={}",
                                    track_id, param_id, error
                                );
                            }
                        }
                    })),
                );
            });
        }

        Self::spawn_audio_poll(cx);

        Self {
            active_bottom_tab: components::BottomTab::Mixer,
            bottom_panel_state: BottomPanelState::default(),
            timeline,
            file_browser: FileBrowserState::default(),
            browser_scroll: ScrollHandle::new(),
            menu_bar: MenuBarUiState::default(),
            audio_engine,
            audio_running: false,
            audio_last_error: None,
            audio_stats: None,
            last_audio_project_signature: None,
            last_engine_playhead_beat: 0.0,
            last_engine_sync: Instant::now(),
            focus_handle: cx.focus_handle(),
            logged_unsupported_commands: HashSet::new(),
        }
    }
}

impl StudioLayout {
    fn spawn_audio_poll(cx: &mut Context<Self>) {
        // 16 ms ≈ 60 Hz — matches a typical display refresh and is fine for
        // VU + transport-time animation. The engine produces position
        // snapshots at audio-block boundaries (~5-10 ms at 256-sample
        // buffers), but the UI never needs to repaint faster than the
        // display, so we cap polling at the refresh interval and let
        // `interpolated_playhead_beat` smooth between engine snapshots.
        cx.spawn(async move |this, cx| loop {
            Timer::after(Duration::from_millis(16)).await;
            let _ = this.update(cx, |this, cx| {
                if this.poll_native_audio(cx) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn poll_native_audio(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(engine) = self.audio_engine.as_ref() else {
            return false;
        };
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
                if timeline.state.transport.playhead_beats != next {
                    timeline.state.transport.playhead_beats = next;
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

    fn interpolated_playhead_beat(&self, bpm: f32) -> f32 {
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

    fn apply_engine_meters(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
        let meters = engine.meters();
        let _ = self.timeline.update(cx, move |timeline, cx| {
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
                }
            }
            changed |= smooth_meter_value(
                &mut timeline.state.master.meter_level_l,
                meters.master_peak_l.clamp(0.0, 1.0) as f32,
            );
            changed |= smooth_meter_value(
                &mut timeline.state.master.meter_level_r,
                meters.master_peak_r.clamp(0.0, 1.0) as f32,
            );
            if changed {
                cx.notify();
            }
        });
    }

    fn sync_audio_project(&mut self, cx: &mut Context<Self>, force: bool) -> bool {
        let Some(engine) = self.audio_engine.as_ref() else {
            self.audio_last_error = Some("audio engine unavailable".to_string());
            return false;
        };

        let sample_rate = self.current_audio_sample_rate();
        let snapshot = {
            let timeline = self.timeline.read(cx);
            build_engine_project_snapshot(&timeline.state, sample_rate)
        };
        let signature = serde_json::to_string(&snapshot).unwrap_or_default();
        if !force && self.last_audio_project_signature.as_deref() == Some(signature.as_str()) {
            return true;
        }

        match engine.load_project(snapshot) {
            Ok(()) => {
                self.last_audio_project_signature = Some(signature);
                self.audio_last_error = None;
                true
            }
            Err(error) => {
                self.audio_last_error = Some(error.to_string());
                eprintln!("[audio] load_project failed: {error}");
                false
            }
        }
    }

    fn current_audio_sample_rate(&self) -> u32 {
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

    fn start_native_playback(&mut self, cx: &mut Context<Self>) {
        if self.audio_engine.is_none() {
            self.audio_last_error = Some("audio engine unavailable".to_string());
            return;
        }

        // Open the audio device FIRST. The DirectAudioEngine renders clip
        // ranges in `runtime.sample_rate` units; if we load the project at
        // the default 44.1 kHz fallback before the cpal/WASAPI stream picks
        // its real rate (e.g. 48 kHz), every clip's start_sample and
        // duration_samples land in the wrong reference frame and playback
        // either drifts in pitch or stops short. Opening the stream up
        // front lets us hand `sync_audio_project` the real hardware rate.
        if !self.audio_running {
            let Some(engine) = self.audio_engine.as_mut() else {
                return;
            };
            if let Err(error) = engine.start() {
                self.audio_running = false;
                self.audio_last_error = Some(error.to_string());
                eprintln!("[audio] start failed: {error}");
                return;
            }
            self.audio_running = true;
            // Refresh stats so current_audio_sample_rate() reads the rate
            // cpal actually negotiated, not the pre-open fallback.
            if let Some(engine) = self.audio_engine.as_ref() {
                self.audio_stats = Some(engine.stats());
            }
        }

        // Now push the project snapshot. Force a resync so the runtime gets
        // (re)built at the real sample rate even when the project signature
        // is unchanged from the cached one stored before the stream opened.
        if !self.sync_audio_project(cx, true) {
            return;
        }

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

    fn stop_native_playback(&mut self, cx: &mut Context<Self>) {
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

    fn seek_native_playhead(&mut self, cx: &mut Context<Self>, beat: f32) {
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

    /// Single entry point for menu items, keyboard shortcuts, and chrome
    /// buttons. `command_id` matches the Electron/shared menu manifest
    /// IDs (e.g. `transport:play-pause`). Unknown IDs are logged once
    /// and then ignored — this is the contract that lets future menu
    /// entries appear in the chrome without crashing the dispatcher.
    fn dispatch_command_id(&mut self, command_id: &str, cx: &mut Context<Self>) {
        if let Some(command) = transport_command_from_id(command_id) {
            self.dispatch_transport_command(command, cx);
            return;
        }
        match command_id {
            "noop" => {}

            // ── View / zoom ──────────────────────────────────────────────
            "view:zoom-in" => self.zoom_timeline_by(cx, 1.25),
            "view:zoom-out" => self.zoom_timeline_by(cx, 0.8),
            "view:reset-zoom" => self.reset_timeline_zoom(cx),

            // ── Transport extras (shared menu IDs) ───────────────────────
            "transport:go-to-end" => {
                let end = self.project_end_beat(cx);
                self.seek_native_playhead(cx, end);
            }
            "transport:rewind" => self.nudge_playhead_bars(cx, -1.0),
            "transport:fast-forward" => self.nudge_playhead_bars(cx, 1.0),

            other => {
                if self
                    .logged_unsupported_commands
                    .insert(other.to_string())
                {
                    eprintln!("[command] unsupported in native: {}", other);
                }
            }
        }
    }

    fn zoom_timeline_by(&self, cx: &mut Context<Self>, factor: f32) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.zoom_by(factor, 0.0);
            cx.notify();
        });
    }

    fn reset_timeline_zoom(&self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            let current = timeline.state.viewport.pixels_per_second.max(0.0001);
            // 150 px/s matches the Web UI default zoom (see timeline_state.rs:460).
            let factor = 150.0 / current;
            timeline.state.zoom_by(factor, 0.0);
            cx.notify();
        });
    }

    fn project_end_beat(&self, cx: &mut Context<Self>) -> f32 {
        let timeline = self.timeline.read(cx);
        timeline
            .state
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| clip.start_beat + clip.duration_beats)
            .fold(0.0_f32, f32::max)
    }

    fn nudge_playhead_bars(&mut self, cx: &mut Context<Self>, bars: f32) {
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

    fn dispatch_transport_command(&mut self, command: TransportCommand, cx: &mut Context<Self>) {
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
            }
            TransportCommand::ToggleMetronome => {
                let _ = self.timeline.update(cx, |timeline, cx| {
                    timeline.state.transport.metronome_enabled =
                        !timeline.state.transport.metronome_enabled;
                    cx.notify();
                });
            }
            TransportCommand::Record => {
                eprintln!("[transport] record is disabled in native Stage 2.1");
            }
        }
    }

    fn transport_chrome_state(&self, cx: &mut Context<Self>) -> components::TransportChromeState {
        let (
            position_label,
            bpm_label,
            time_signature_label,
            recording,
            loop_enabled,
            metronome_enabled,
        ) = {
            let timeline = self.timeline.read(cx);
            (
                timeline
                    .state
                    .format_bar_beat(timeline.state.transport.playhead_beats),
                format!("{:.0}", timeline.state.bpm),
                format!(
                    "{}/{}",
                    timeline.state.time_signature_num, timeline.state.time_signature_den
                ),
                timeline.state.transport.recording,
                timeline.state.transport.loop_enabled,
                timeline.state.transport.metronome_enabled,
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
        let _on_record = make_command_handler("transport:record");

        components::TransportChromeState {
            playing,
            recording,
            loop_enabled,
            metronome_enabled,
            position_label,
            bpm_label,
            time_signature_label,
            on_return_to_start,
            on_play_toggle,
            on_stop,
            on_loop_toggle,
            on_metronome_toggle,
        }
    }

    fn status_text(&self) -> (String, String) {
        let left = match (&self.audio_last_error, &self.audio_stats) {
            (Some(error), _) => format!("Audio: {error}"),
            (_, Some(stats)) if stats.transport_playing => "Playing".to_string(),
            (_, Some(stats)) if stats.running => "Audio ready".to_string(),
            _ => "Ready".to_string(),
        };
        let right = self
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
        (left, right)
    }

    /// Map a keystroke to a shared menu command ID. Keys mirror the
    /// `transport:*` IDs from `packages/shared/generated/native-menu.json`
    /// so the keyboard and menu paths fan into the same dispatcher.
    /// Text-input guarding is N/A here because GPUI delivers key events
    /// only when nothing focusable consumes them; if/when text inputs
    /// land in the studio surface, gate this on `event.bubble_phase`.
    fn shortcut_command_id(event: &KeyDownEvent) -> Option<&'static str> {
        if event.is_held {
            return None;
        }
        let key = event.keystroke.key.as_str();
        let mods = event.keystroke.modifiers;
        if mods.control || mods.alt || mods.platform || mods.function {
            return None;
        }
        match key {
            "space" => Some("transport:play-pause"),
            "enter" | "numpad_enter" => Some("transport:stop"),
            "l" | "L" => Some("transport:toggle-loop"),
            "k" | "K" => Some("transport:toggle-metronome"),
            "r" | "R" => Some("transport:record"),
            "home" => Some("transport:go-to-start"),
            _ => None,
        }
    }

    /// Run a single-level directory scan on the GPUI background executor,
    /// then push the result back into `file_browser.index` on the UI
    /// thread. Never blocks render — this is the only place `read_dir`
    /// is allowed to happen at runtime.
    fn spawn_directory_load(cx: &mut Context<Self>, path: PathBuf) {
        let started = std::time::Instant::now();
        let path_for_log = path.clone();
        eprintln!("[indexer] load requested: {}", path_for_log.display());
        cx.spawn(async move |this, cx| {
            let scan_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { read_directory(&scan_path) })
                .await;
            let elapsed = started.elapsed();
            let _ = this.update(cx, move |this, cx| {
                match result {
                    (entries, None) => {
                        eprintln!(
                            "[indexer] load completed: {} ({} entries, {} ms)",
                            path.display(),
                            entries.len(),
                            elapsed.as_millis()
                        );
                        this.file_browser.apply_loaded(path, entries);
                    }
                    (_, Some(error)) => {
                        eprintln!(
                            "[indexer] load failed: {} -> {} ({} ms)",
                            path.display(),
                            error,
                            elapsed.as_millis()
                        );
                        this.file_browser.apply_error(path, error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn spawn_timeline_audio_import_jobs(
        cx: &mut Context<Self>,
        timeline: Entity<components::timeline::Timeline>,
        path: PathBuf,
        path_key: String,
    ) {
        cx.spawn(async move |_this, cx| {
            let meta_path = path.clone();
            let metadata = cx
                .background_executor()
                .spawn(async move { DAUx::probe_audio_file(&meta_path) })
                .await;

            match metadata {
                Ok(info) => {
                    let format = info.format.as_str().to_string();
                    let meta_path_key = path_key.clone();
                    let _ = timeline.update(cx, move |timeline, cx| {
                        timeline.state.update_audio_clip_metadata(
                            &meta_path_key,
                            &format,
                            info.sample_rate,
                            info.channels,
                            info.total_frames,
                            info.duration_seconds,
                        );
                        cx.notify();
                    });
                }
                Err(error) => {
                    eprintln!(
                        "[audio-import] WARNING using fallback duration because metadata failed: path={} error={}",
                        path_key, error
                    );
                }
            }

            let decode_path = path.clone();
            let preview = cx
                .background_executor()
                .spawn(async move { waveform_cache::decode_and_cache_file(&decode_path) })
                .await;
            match preview {
                Some(preview) => {
                    let _ = timeline.update(cx, move |timeline, cx| {
                        if let Some(source_duration) =
                            timeline.state.audio_source_duration_seconds(&path_key)
                        {
                            let delta = (preview.duration_seconds - source_duration).abs();
                            if delta > 0.01 {
                                eprintln!(
                                    "[waveform] WARNING preview duration differs from DirectAudioEngine metadata: path={} preview_duration_seconds={:.6} metadata_duration_seconds={:.6}",
                                    path_key, preview.duration_seconds, source_duration
                                );
                            }
                        }
                        cx.notify();
                    });
                }
                None => {
                    // decode_and_cache_file returned None — either the
                    // extension was rejected (keep
                    // `is_supported_audio_ext` and waveform_cache's
                    // match arm aligned) or the file body itself failed
                    // to decode. Either way the clip will render with
                    // the placeholder waveform; the realtime engine
                    // also won't be able to play it.
                    eprintln!(
                        "[waveform] decode produced no preview: path={} — clip will use placeholder waveform and likely fail playback",
                        path_key
                    );
                }
            }
        })
        .detach();
    }

    /// Build the callback bundle used by the mixer. Every mutation lands in
    /// the same `TimelineState` instance owned by the Timeline entity, so the
    /// TrackHeader and Mixer always read identical values.
    fn build_mixer_callbacks(&self) -> MixerCallbacks {
        let audio_engine = self.audio_engine.clone();
        let timeline_select = self.timeline.clone();
        let on_select_track: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |id: &String, _w, cx| {
            let id = id.clone();
            timeline_select.update(cx, |t, cx| {
                t.state.select_track(&id);
                cx.notify();
            });
        });

        let timeline_vol = self.timeline.clone();
        let on_volume_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
            let id = id.clone();
            let v = *v;
            timeline_vol.update(cx, |t, cx| {
                t.state.set_track_volume(&id, v);
                cx.notify();
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "volume", volume_norm_to_linear(v) as f64);
            }
        });

        let audio_engine = self.audio_engine.clone();
        let timeline_pan = self.timeline.clone();
        let on_pan_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
            let id = id.clone();
            let v = *v;
            timeline_pan.update(cx, |t, cx| {
                t.state.set_track_pan(&id, v);
                cx.notify();
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "pan", v as f64);
            }
        });

        let audio_engine = self.audio_engine.clone();
        let timeline_mute = self.timeline.clone();
        let on_toggle_mute: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                let mut muted = false;
                timeline_mute.update(cx, |t, cx| {
                    t.state.toggle_track_mute(&id);
                    muted = t
                        .state
                        .find_track(&id)
                        .map(|track| track.muted)
                        .unwrap_or(false);
                    cx.notify();
                });
                if let Some(engine) = audio_engine.as_ref() {
                    let _ = engine.update_track_param(&id, "mute", if muted { 1.0 } else { 0.0 });
                }
            });

        let audio_engine = self.audio_engine.clone();
        let timeline_solo = self.timeline.clone();
        let on_toggle_solo: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                let mut solo = false;
                timeline_solo.update(cx, |t, cx| {
                    t.state.toggle_track_solo(&id);
                    solo = t
                        .state
                        .find_track(&id)
                        .map(|track| track.solo)
                        .unwrap_or(false);
                    cx.notify();
                });
                if let Some(engine) = audio_engine.as_ref() {
                    let _ = engine.update_track_param(&id, "solo", if solo { 1.0 } else { 0.0 });
                }
            });

        let timeline_arm = self.timeline.clone();
        let on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_arm.update(cx, |t, cx| {
                    t.state.toggle_track_arm(&id);
                    cx.notify();
                });
            });

        let timeline_input = self.timeline.clone();
        let on_toggle_input: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |id: &String, _w, cx| {
            let id = id.clone();
            timeline_input.update(cx, |t, cx| {
                t.state.toggle_track_input_monitor(&id);
                cx.notify();
            });
        });

        let audio_engine = self.audio_engine.clone();
        let timeline_master = self.timeline.clone();
        let on_master_volume_change: std::sync::Arc<
            dyn Fn(&f32, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |v: &f32, _w, cx| {
            let v = *v;
            timeline_master.update(cx, |t, cx| {
                t.state.set_master_volume(v);
                cx.notify();
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(
                    "__master__",
                    "volume",
                    volume_norm_to_linear(v) as f64,
                );
            }
        });

        MixerCallbacks {
            on_select_track,
            on_volume_change,
            on_pan_change,
            on_toggle_mute,
            on_toggle_solo,
            on_toggle_arm,
            on_toggle_input,
            on_master_volume_change,
        }
    }
}

impl Render for StudioLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_tab_click = cx.listener(|this, tab: &components::BottomTab, _window, cx| {
            this.active_bottom_tab = *tab;
            cx.notify();
        });

        let on_resize_start = cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
            let bs = &mut this.bottom_panel_state;
            bs.is_resizing = true;
            bs.resize_start_y = f32::from(event.position.y);
            bs.resize_start_height = bs.height_px;
            let window_h: f32 = window.bounds().size.height.into();
            bs.max_height_px = (window_h * 0.70).max(bs.min_height_px + 40.0);
            cx.notify();
        });

        let on_resize_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<BottomPanelResizeDrag>, _window, cx| {
                let bs = &mut this.bottom_panel_state;
                let cur_y: f32 = event.event.position.y.into();
                let delta = bs.resize_start_y - cur_y;
                let new_h =
                    (bs.resize_start_height + delta).clamp(bs.min_height_px, bs.max_height_px);
                if (new_h - bs.height_px).abs() > 0.5 {
                    bs.height_px = new_h;
                    cx.notify();
                }
            },
        );

        // Pull the live track list and current selection out of the Timeline so
        // the Mixer and Inspector render against the same data the TrackHeader
        // sees. Cloning the Vec is cheap relative to a full render.
        let (tracks, master, selected_track_id, selected_clip_id) = {
            let t = self.timeline.read(cx);
            (
                t.state.tracks.clone(),
                t.state.master.clone(),
                t.state.selection.selected_track_id.clone(),
                t.state.selection.selected_clip_ids.first().cloned(),
            )
        };

        let panel_state = self.bottom_panel_state;
        let mixer_callbacks = self.build_mixer_callbacks();

        // ── File browser callbacks ──────────────────────────────────────
        let on_browser_toggle: std::sync::Arc<
            dyn Fn(&(String, Option<PathBuf>), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(id, path): &(String, Option<PathBuf>), _w, cx| {
                let id = id.clone();
                let path = path.clone();
                let _ = this.update(cx, |this, cx| {
                    let expanded = this.file_browser.toggle_node(&id, path.as_deref());
                    if expanded {
                        // Drain any newly-expanded paths whose contents
                        // haven't been indexed yet and kick off a
                        // background load for each.
                        let pending = this.file_browser.paths_needing_load();
                        for p in pending {
                            this.file_browser.mark_loading(p.clone());
                            Self::spawn_directory_load(cx, p);
                        }
                    }
                    cx.notify();
                });
            })
        };
        let on_browser_select: std::sync::Arc<
            dyn Fn(&PathBuf, &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |path: &PathBuf, _w, cx| {
                let path = path.clone();
                this.update(cx, |this, cx| {
                    this.file_browser.select(path);
                    cx.notify();
                });
            })
        };
        // Double-click on an audio file imports it onto the timeline using the
        // existing waveform-cache + import_audio_at path.
        let on_browser_activate: std::sync::Arc<
            dyn Fn(&PathBuf, &mut Window, &mut gpui::App) + 'static,
        > = {
            let timeline = self.timeline.clone();
            let layout = cx.entity().clone();
            std::sync::Arc::new(move |path: &PathBuf, _w, cx| {
                // Filter on extension before mutating timeline state so
                // double-clicking a non-audio file (e.g. .txt, .png) does
                // not create a phantom clip with the 8-bar fallback
                // duration that never resolves to real metadata.
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if !is_supported_audio_ext(&ext) {
                    eprintln!(
                        "[import] ignoring non-audio activation: ext='{}' path={}",
                        ext,
                        path.display()
                    );
                    return;
                }

                let path = path.clone();
                let path_for_decode = path.clone();
                let timeline_for_decode = timeline.clone();
                timeline.update(cx, |t, cx| {
                    let path_key = path.to_string_lossy().to_string();
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "Imported Audio".to_string());
                    t.state
                        .import_audio_to_selected_or_new_track(path_key, name);
                    cx.notify();
                });
                let path_key = path_for_decode.to_string_lossy().to_string();
                let _ = layout.update(cx, move |_layout, cx| {
                    Self::spawn_timeline_audio_import_jobs(
                        cx,
                        timeline_for_decode,
                        path_for_decode,
                        path_key,
                    );
                });
            })
        };

        let file_browser = self.file_browser.clone();
        let browser_scroll = self.browser_scroll.clone();

        // ── Top-menu callbacks ─────────────────────────────────────────────
        let on_open_menu: std::sync::Arc<
            dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(id, anchor_x): &(String, f32), _w, cx| {
                let id = id.clone();
                let anchor_x = *anchor_x;
                this.update(cx, |this, cx| {
                    if this.menu_bar.open_menu_id.as_deref() == Some(id.as_str()) {
                        this.menu_bar.open_menu_id = None;
                    } else {
                        this.menu_bar.open_menu_id = Some(id);
                        this.menu_bar.anchor_x = anchor_x;
                    }
                    this.menu_bar.submenu_path.clear();
                    cx.notify();
                });
            })
        };
        let on_close_menu: std::sync::Arc<dyn Fn(&(), &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |_: &(), _w, cx| {
                this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    cx.notify();
                });
            })
        };
        let on_toggle_submenu: std::sync::Arc<
            dyn Fn(&(usize, String), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(depth, id): &(usize, String), _w, cx| {
                let depth = *depth;
                let id = id.clone();
                this.update(cx, |this, cx| {
                    // Truncate the path to this depth, then toggle: if the
                    // requested id is already open at this depth, close it;
                    // otherwise open it (closing anything deeper).
                    let already_open = this.menu_bar.submenu_path.get(depth) == Some(&id);
                    this.menu_bar.submenu_path.truncate(depth);
                    if !already_open {
                        this.menu_bar.submenu_path.push(id);
                    }
                    cx.notify();
                });
            })
        };
        let on_menu_command: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |command: &String, _w, cx| {
                let command = command.clone();
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id(&command, cx);
                    cx.notify();
                });
            })
        };

        let open_menu_id = self.menu_bar.open_menu_id.clone();
        let menu_anchor_x = self.menu_bar.anchor_x;
        let submenu_path = self.menu_bar.submenu_path.clone();
        let viewport_width: f32 = window.bounds().size.width.into();
        let viewport_height: f32 = window.bounds().size.height.into();

        let dropdown_overlay = open_menu_id.as_ref().and_then(|id| {
            let manifest = crate::menu::MenuManifest::load();
            manifest.menus.iter().find(|m| &m.id == id).map(|menu| {
                components::menu_dropdown::menu_dropdown(
                    menu,
                    menu_anchor_x,
                    viewport_width,
                    viewport_height,
                    &submenu_path,
                    on_toggle_submenu.clone(),
                    on_menu_command.clone(),
                    on_close_menu.clone(),
                )
            })
        });
        let transport_chrome = self.transport_chrome_state(cx);
        let (status_left, status_right) = self.status_text();
        let shortcut_target = cx.entity().clone();

        // Take initial keyboard focus so transport shortcuts (Space, Enter, L,
        // K, R, Home) reach `capture_key_down` below. GPUI only delivers key
        // events to focused elements; without this the root div never sees
        // keystrokes even though `shortcut_command` is wired.
        if window.focused(cx).is_none() {
            self.focus_handle.focus(window);
        }
        let focus_holder = self.focus_handle.clone();

        div()
            // NOTE: `track_focus` deliberately lives on the tiny invisible
            // `focus_holder` child below, NOT on this root. Putting it on
            // the root makes GPUI insert a full-window Normal hitbox
            // (see `should_insert_hitbox` — `tracked_focus_handle.is_some()`
            // triggers it). That hitbox is benign for click dispatch, but
            // on Windows it lands above the chrome's
            // `WindowControlArea::Drag` hitbox in the `mouse_hit_test.ids`
            // vector — which the NCHITTEST callback iterates in
            // window-control-vector order, not z-order — and the OS sees
            // a non-caption hit, refusing to start the window move.
            // Hoisting focus onto a 0×0 child preserves shortcut
            // delivery without adding the full-window hitbox.
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .bg(Colors::surface_base())
            .font_family(theme::FONT_FAMILY)
            .capture_key_down(move |event, _window, cx| {
                if let Some(command_id) = Self::shortcut_command_id(event) {
                    let _ = shortcut_target.update(cx, |this, cx| {
                        this.dispatch_command_id(command_id, cx);
                        cx.notify();
                    });
                }
            })
            // Invisible focus anchor. 0×0 means no visible footprint and
            // an effectively unreachable hitbox; `track_focus` only needs
            // it to register the focus handle. The root's
            // `capture_key_down` still fires for any key while this
            // descendant is focused (capture phase: root → focused).
            .child(
                div()
                    .w(px(0.0))
                    .h(px(0.0))
                    .track_focus(&focus_holder),
            )
            .child(components::app_chrome(
                window,
                open_menu_id.as_deref(),
                on_open_menu,
                transport_chrome,
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(components::sidebar(
                        &file_browser,
                        browser_scroll,
                        on_browser_toggle,
                        on_browser_select,
                        on_browser_activate,
                    ))
                    .child(self.timeline.clone())
                    .child(crate::components::panel::inspector_panel(
                        &tracks,
                        selected_track_id.as_deref(),
                        selected_clip_id.as_deref(),
                        find_clip_summary(&tracks, selected_clip_id.as_deref()),
                    )),
            )
            .child(components::bottom_panel(
                self.active_bottom_tab,
                panel_state,
                &tracks,
                &master,
                selected_track_id.as_deref(),
                mixer_callbacks,
                on_tab_click,
                on_resize_start,
                on_resize_move,
            ))
            .child(components::status_bar(status_left, status_right))
            // Dropdown overlay — rendered last so it sits above every other
            // panel. The dropdown's own backdrop captures click-outside.
            .children(dropdown_overlay)
    }
}

fn build_engine_project_snapshot(state: &TimelineState, sample_rate: u32) -> EngineProjectSnapshot {
    let mut tracks: Vec<EngineTrackSnapshot> = state
        .tracks
        .iter()
        .map(|track| EngineTrackSnapshot {
            id: track.id.clone(),
            track_type: track_type_name(track.track_type).to_string(),
            volume: volume_norm_to_linear(track.volume),
            pan: track.pan.clamp(-1.0, 1.0),
            muted: track.muted,
            solo: track.solo,
            armed: track.armed,
            preview_mode: "stereo".to_string(),
            output_track_id: None,
            inserts: Vec::new(),
            sends: Vec::new(),
        })
        .collect();

    tracks.push(EngineTrackSnapshot {
        id: "master".to_string(),
        track_type: "master".to_string(),
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        armed: false,
        preview_mode: "stereo".to_string(),
        output_track_id: None,
        inserts: Vec::new(),
        sends: Vec::new(),
    });

    let clips = state
        .tracks
        .iter()
        .flat_map(|track| {
            track.clips.iter().filter_map(move |clip| {
                if clip.muted {
                    return None;
                }
                let ClipType::Audio {
                    file_id,
                    source_path: Some(source_path),
                } = &clip.clip_type
                else {
                    return None;
                };
                if source_path.trim().is_empty() {
                    return None;
                }

                Some(EngineClipSnapshot {
                    id: clip.id.clone(),
                    track_id: track.id.clone(),
                    asset_id: file_id.clone(),
                    media_path: Some(source_path.clone()),
                    start_beat: clip.start_beat.max(0.0) as f64,
                    duration_beats: clip.duration_beats.max(0.0) as f64,
                    offset_seconds: state.beats_to_seconds(clip.offset_beats.max(0.0)) as f64,
                    gain: clip.gain.clamp(0.0, 4.0),
                    fades: None,
                    audio_process: Some(EngineClipAudioProcess {
                        speed_ratio: 1.0,
                        pitch_semitones: 0.0,
                        preserve_pitch: false,
                        mode: "none".to_string(),
                        quality: "balanced".to_string(),
                    }),
                })
            })
        })
        .collect();

    EngineProjectSnapshot {
        project_id: "futureboard-native".to_string(),
        project_root: None,
        bpm: state.bpm.max(1.0) as f64,
        time_signature: [state.time_signature_num, state.time_signature_den],
        sample_rate: sample_rate.max(1),
        tracks,
        clips,
        routing: EngineRoutingSnapshot {
            master_output_device: None,
            sample_rate: sample_rate.max(1),
            buffer_size: 256,
        },
    }
}

/// Keep in sync with `DAUx::probe_audio_file`,
/// `waveform_cache::decode_file_uncached`, and
/// `file_browser::FileBrowserEntry::is_audio` — any divergence between
/// these lists creates "imports but never plays" or "looks pending
/// forever" bugs.
fn is_supported_audio_ext(ext: &str) -> bool {
    matches!(ext, "wav" | "wave" | "mp3" | "flac" | "ogg" | "oga" | "aiff" | "aif")
}

/// Resolve a shared menu command ID to a transport action.
/// Returns `None` for commands the unified dispatcher should log as
/// unsupported. Keep in lock-step with `apps/web/src/menu/actionRunner.ts`
/// and `packages/shared/generated/native-menu.json`.
fn transport_command_from_id(command_id: &str) -> Option<TransportCommand> {
    match command_id {
        "transport:play-pause" => Some(TransportCommand::PlayPause),
        "transport:stop" => Some(TransportCommand::Stop),
        "transport:go-to-start" => Some(TransportCommand::ReturnToStart),
        "transport:toggle-loop" => Some(TransportCommand::ToggleLoop),
        "transport:toggle-metronome" => Some(TransportCommand::ToggleMetronome),
        "transport:record" => Some(TransportCommand::Record),
        _ => None,
    }
}

fn track_type_name(track_type: TrackType) -> &'static str {
    match track_type {
        TrackType::Audio => "audio",
        TrackType::Midi => "midi",
        TrackType::Instrument => "instrument",
        TrackType::Master => "master",
    }
}

fn volume_norm_to_linear(norm: f32) -> f32 {
    let norm = norm.clamp(0.0, 1.0);
    if norm <= 0.001 {
        return 0.0;
    }
    let db = timeline_state::volume::norm_to_db(norm);
    if db <= timeline_state::volume::MIN_DB + 0.05 {
        0.0
    } else {
        10.0_f32.powf(db / 20.0).clamp(0.0, 2.0)
    }
}

fn smooth_meter_value(current: &mut f32, target: f32) -> bool {
    let target = target.clamp(0.0, 1.0);
    let rate = if target > *current { 0.72 } else { 0.18 };
    let next = (*current + (target - *current) * rate).clamp(0.0, 1.0);
    let changed = (*current - next).abs() > 0.001;
    *current = if next < 0.002 { 0.0 } else { next };
    changed
}

fn find_clip_summary<'a>(
    tracks: &'a [TrackState],
    clip_id: Option<&str>,
) -> Option<crate::components::panel::SelectedClipSummary<'a>> {
    let id = clip_id?;
    for t in tracks {
        if let Some(c) = t.clips.iter().find(|c| c.id == id) {
            return Some(crate::components::panel::SelectedClipSummary {
                name: &c.name,
                start_beat: c.start_beat,
                duration_beats: c.duration_beats,
                kind: match &c.clip_type {
                    crate::components::timeline::timeline_state::ClipType::Audio { .. } => "Audio",
                    crate::components::timeline::timeline_state::ClipType::Midi { .. } => "MIDI",
                },
                track_name: &t.name,
            });
        }
    }
    None
}
