use gpui::{
    div, px, AppContext, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, Styled, UniformListScrollHandle, Window,
};

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::components;
use crate::components::add_track_dialog::{
    add_track_dialog, AddTrackDialogCallbacks, AddTrackDialogState, AddTrackKind,
};
use crate::components::context_menu::ContextMenuEntry;
use crate::components::file_browser::{read_directory, FileBrowserState};
use crate::components::mixer_panel::MixerCallbacks;
use crate::components::project_switcher::ProjectSwitcherState;
use crate::components::project_wizard::{
    ProjectTemplate, ProjectWizardCallbacks, ProjectWizardResult, ProjectWizardState,
};
use crate::components::text_input::{TextInputAction, TextInputState};
use crate::components::timeline::timeline::TimelineContextTarget;
use crate::components::timeline::timeline_state::{
    self, ClipType, CreateTrackOptions, TimelineState, TrackState, TrackType,
};
use crate::components::timeline::waveform_cache;
use crate::components::{BottomPanelResizeDrag, BottomPanelState};
use crate::project::{
    apply_to_timeline, io::save_project, io::load_project, now_secs,
    recent::RecentProjectsStore, FutureboardProject,
};
use crate::theme::{self, Colors};

use DAUx::types::{
    EngineClipAudioProcess, EngineClipSnapshot, EngineProjectSnapshot, EngineRoutingSnapshot,
    EngineTrackSnapshot,
};

/// Flip to `true` to seed the studio with demo tracks/clips at startup.
/// Production builds must keep this `false` — the real app starts empty.
const USE_DEMO_PROJECT: bool = false;

// Frame pacing details live in tasks/native/frame-pacing.md.

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

#[derive(Debug, Clone)]
pub enum OpenPopover {
    Context {
        target: ContextTarget,
        x: f32,
        y: f32,
    },
}

#[derive(Debug, Clone)]
pub enum ContextTarget {
    TimelineEmpty,
    Track(String),
    Clip(String),
    Browser(Option<PathBuf>),
    Mixer(String),
}

pub struct StudioLayout {
    active_bottom_tab: components::BottomTab,
    bottom_panel_state: BottomPanelState,
    timeline: Entity<components::timeline::Timeline>,
    file_browser: FileBrowserState,
    /// Stable scroll handle for the browser tree. Lives on the layout
    /// (not in `FileBrowserState`) so the state stays free of gpui types
    /// and so the handle survives across renders.
    browser_scroll: UniformListScrollHandle,
    menu_bar: MenuBarUiState,
    project_switcher: ProjectSwitcherState,
    add_track_dialog: AddTrackDialogState,
    open_popover: Option<OpenPopover>,
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
    /// Repaint-rate diagnostics. Ticks once per `Render`, smoothed
    /// EMA frame time, exposed in the status bar.
    frame_diag: FrameDiagnostics,
    /// Current horizontal scroll offset for the mixer channel strip area.
    /// Updated by the mixer scroll-wheel handler and clamped each frame.
    mixer_scroll_x: f32,

    // ── Project file system ───────────────────────────────────────────────────
    /// Absolute path to the currently open `.fbproj` file, if any.
    project_path: Option<PathBuf>,
    /// Root folder of the current project (contains Media/, Cache/, etc.).
    project_folder: Option<PathBuf>,
    /// Persistent recent-projects list backed by `~/.config/Futureboard/recent.json`.
    recent_projects: RecentProjectsStore,
    /// State for the New Project wizard overlay.
    project_wizard: ProjectWizardState,
    /// Text inputs for the project wizard (project name and BPM).
    wizard_name_input: TextInputState,
    wizard_bpm_input: TextInputState,
}

/// Rolling UI repaint diagnostics.
///
/// Counts how often `Render` runs and how far apart those calls are
/// — i.e. effective UI frame cadence, not unconditional display
/// refresh. When the app is idle (nothing dirty), `Render` is not
/// called and the readout stops updating; the `idle_after` check
/// in `hud` decays the displayed FPS to 0.
struct FrameDiagnostics {
    last_frame: Option<Instant>,
    last_log: Instant,
    frame_count: u64,
    /// Exponentially-smoothed frame-to-frame interval, in ms.
    frame_time_ema_ms: f32,
    fps: f32,
    /// Most recent raw frame interval, in ms.
    frame_ms: f32,
    log_to_stderr: bool,
}

impl FrameDiagnostics {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            last_frame: None,
            last_log: now,
            frame_count: 0,
            frame_time_ema_ms: 16.7,
            fps: 60.0,
            frame_ms: 16.7,
            log_to_stderr: std::env::var_os("FUTUREBOARD_FRAME_DIAG").is_some(),
        }
    }

    fn tick(&mut self, reason: &str) {
        let now = Instant::now();
        if let Some(prev) = self.last_frame {
            let dt = now.duration_since(prev).as_secs_f32() * 1000.0;
            // Drop absurd intervals: first frame after a long idle,
            // or a debugger pause. Anything > 1 s is not a repaint
            // cadence sample.
            if dt > 0.0 && dt < 1000.0 {
                let alpha = 0.12;
                self.frame_time_ema_ms = self.frame_time_ema_ms * (1.0 - alpha) + dt * alpha;
                self.frame_ms = dt;
                self.fps = if self.frame_time_ema_ms > 0.0 {
                    1000.0 / self.frame_time_ema_ms
                } else {
                    0.0
                };
            }
        }
        self.last_frame = Some(now);
        self.frame_count = self.frame_count.saturating_add(1);

        if self.log_to_stderr && now.duration_since(self.last_log) >= Duration::from_secs(1) {
            eprintln!(
                "[frame] {:.1} fps  {:.2} ms (last {:.2} ms)  reason={}  frames={}",
                self.fps, self.frame_time_ema_ms, self.frame_ms, reason, self.frame_count
            );
            self.last_log = now;
        }
    }

    fn hud(&self) -> String {
        format!("{:.0} fps  {:.1} ms", self.fps, self.frame_time_ema_ms)
    }
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
            browser_scroll: UniformListScrollHandle::new(),
            menu_bar: MenuBarUiState::default(),
            project_switcher: ProjectSwitcherState::default(),
            add_track_dialog: AddTrackDialogState::closed(),
            open_popover: None,
            audio_engine,
            audio_running: false,
            audio_last_error: None,
            audio_stats: None,
            last_audio_project_signature: None,
            last_engine_playhead_beat: 0.0,
            last_engine_sync: Instant::now(),
            focus_handle: cx.focus_handle(),
            logged_unsupported_commands: HashSet::new(),
            frame_diag: FrameDiagnostics::new(),
            mixer_scroll_x: 0.0,
            project_path: None,
            project_folder: None,
            recent_projects: RecentProjectsStore::load(),
            project_wizard: ProjectWizardState::closed(),
            wizard_name_input: TextInputState::new("wizard-name", cx.focus_handle())
                .with_placeholder("Project name"),
            wizard_bpm_input: TextInputState::new("wizard-bpm", cx.focus_handle()),
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
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| loop {
            executor.timer(Duration::from_millis(16)).await;
            let _ = this.update(cx, |this, cx| {
                if this.poll_native_audio(cx) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn poll_native_audio(&mut self, cx: &mut Context<Self>) -> bool {
        let _s = crate::perf::PerfScope::enter("poll_native_audio");
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

    fn sync_metronome_controls(&mut self, cx: &mut Context<Self>) {
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
        let normalized = normalize_command_id(command_id);
        let command_id = normalized.as_str();
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

            // ── Project / track / edit commands available in native shell ─
            "project:new" | "project:new-from-template" => self.open_project_wizard(cx),
            "project:open" => self.cmd_open_project(cx),
            "project:save" => self.cmd_save_project(cx),
            "project:save-as" => self.cmd_save_project_as(cx),
            "project:save-copy" => self.cmd_save_project_copy(cx),
            "project:open-recent" => self.cmd_open_recent_project(cx),
            "project:recent-clear" => {
                self.recent_projects.clear();
                self.sync_recent_to_switcher();
            }
            "project:reveal-folder" => self.cmd_reveal_project_folder(cx),
            "project:switch-current" => {}

            // ── Dev stress-test commands (not in release menus) ──────────────
            "dev:tracks-32" => self.stress_add_tracks(32, cx),
            "dev:tracks-64" => self.stress_add_tracks(64, cx),
            "dev:tracks-128" => self.stress_add_tracks(128, cx),
            "dev:tracks-500" => self.stress_add_tracks(500, cx),

            "track:add" | "project:add-track" => self.open_add_track_dialog(cx),
            "track:add-audio" => self.open_add_track_dialog_with_kind(AddTrackKind::Audio, cx),
            "track:add-midi" => self.open_add_track_dialog_with_kind(AddTrackKind::Midi, cx),
            "track:add-instrument" => {
                self.open_add_track_dialog_with_kind(AddTrackKind::Instrument, cx)
            }
            "track:add-plugin" => self.open_add_track_dialog_with_kind(AddTrackKind::Plugin, cx),
            "track:add-bus" => self.open_add_track_dialog_with_kind(AddTrackKind::Bus, cx),
            "track:add-return" => self.open_add_track_dialog_with_kind(AddTrackKind::Return, cx),
            "track:add-group" => self.open_add_track_dialog_with_kind(AddTrackKind::Group, cx),
            "track:add-master" => self.open_add_track_dialog_with_kind(AddTrackKind::Master, cx),
            "track:delete" => self.delete_selected_track(cx),
            "track:mute" => self.toggle_selected_track_mute(cx),
            "track:solo" => self.toggle_selected_track_solo(cx),
            "track:arm" => self.toggle_selected_track_arm(cx),
            "mixer:reset-volume" => self.reset_selected_track_volume(cx),
            "mixer:reset-pan" => self.reset_selected_track_pan(cx),
            "edit:delete" | "clip:delete" => self.delete_selected_clip_or_track(cx),
            "edit:duplicate" | "clip:duplicate" => self.duplicate_selected_clip(cx),

            // ── Transport extras (shared menu IDs) ───────────────────────
            "transport:go-to-end" => {
                let end = self.project_end_beat(cx);
                self.seek_native_playhead(cx, end);
            }
            "transport:rewind" => self.nudge_playhead_bars(cx, -1.0),
            "transport:fast-forward" => self.nudge_playhead_bars(cx, 1.0),

            other => {
                if self.logged_unsupported_commands.insert(other.to_string()) {
                    eprintln!("[command] unsupported in native: {}", other);
                }
            }
        }
    }

    fn reset_project(&mut self, cx: &mut Context<Self>) {
        self.project_path = None;
        self.project_folder = None;
        self.project_switcher = ProjectSwitcherState::default();
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state = TimelineState::default();
            cx.notify();
        });
    }

    // ── Project wizard ────────────────────────────────────────────────────────

    fn open_project_wizard(&mut self, _cx: &mut Context<Self>) {
        self.project_wizard = ProjectWizardState::open();
        self.wizard_name_input.set_value("Untitled Project");
        self.wizard_name_input.select_all();
        self.wizard_bpm_input.set_value(
            format!("{:.0}", self.project_wizard.bpm()).as_str(),
        );
    }

    fn close_project_wizard(&mut self) {
        self.project_wizard = ProjectWizardState::closed();
    }

    fn on_project_created(&mut self, result: &ProjectWizardResult, cx: &mut Context<Self>) {
        self.close_project_wizard();

        let folder = match crate::project::io::create_project_folder(&result.location, &result.name) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("[project] failed to create folder: {e}");
                return;
            }
        };
        let project_file = folder.join(format!(
            "{}.{}",
            crate::project::io::sanitize_project_name(&result.name),
            crate::project::io::PROJECT_FILE_EXT
        ));

        // Reset timeline to match wizard settings
        let _ = self.timeline.update(cx, |timeline, _cx| {
            timeline.state = TimelineState::default();
            timeline.state.bpm = result.bpm as f32;
            timeline.state.time_signature_num = result.time_sig_num;
            timeline.state.time_signature_den = result.time_sig_den;
        });

        // Create tracks from template
        let audio_count = result.template.audio_tracks();
        let midi_count = result.template.midi_tracks();
        if audio_count > 0 || midi_count > 0 {
            let _ = self.timeline.update(cx, |timeline, _cx| {
                for i in 0..audio_count {
                    let color = timeline.state.track_color_for_index(i as usize);
                    timeline.state.create_track(CreateTrackOptions {
                        track_type: TrackType::Audio,
                        name: format!("Audio {}", i + 1),
                        color,
                        volume: timeline_state::volume::db_to_norm(0.0),
                        pan: 0.0,
                        armed: false,
                        input_monitor: false,
                    });
                }
                for i in 0..midi_count {
                    let color = timeline.state.track_color_for_index((audio_count + i) as usize);
                    timeline.state.create_track(CreateTrackOptions {
                        track_type: TrackType::Midi,
                        name: format!("MIDI {}", i + 1),
                        color,
                        volume: timeline_state::volume::db_to_norm(0.0),
                        pan: 0.0,
                        armed: false,
                        input_monitor: false,
                    });
                }
            });
        }

        // Save initial project file
        let tl_state = self.timeline.read(cx).state.clone();
        let mut project = FutureboardProject::from(&tl_state);
        project.name = result.name.clone();
        project.settings.sample_rate = result.sample_rate;

        if let Err(e) = save_project(&mut project, &project_file) {
            eprintln!("[project] initial save failed: {e}");
        }

        self.project_path = Some(project_file.clone());
        self.project_folder = Some(folder);
        self.project_switcher.current_project.name = result.name.clone();
        self.project_switcher.current_project.path = Some(project_file.clone());
        self.project_switcher.current_project.is_dirty = false;
        self.project_switcher.current_project.subtitle = "Saved".to_string();

        self.recent_projects.push(&result.name, project_file, now_secs());
        self.sync_recent_to_switcher();
        cx.notify();
    }

    // ── Save / load ───────────────────────────────────────────────────────────

    fn mark_dirty(&mut self) {
        self.project_switcher.current_project.is_dirty = true;
        self.project_switcher.current_project.subtitle = "Unsaved changes".to_string();
    }

    fn cmd_save_project(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.project_path.clone() {
            self.do_save_project(&path, cx);
        } else {
            self.cmd_save_project_as(cx);
        }
    }

    fn cmd_save_project_as(&mut self, cx: &mut Context<Self>) {
        let default_dir = self
            .project_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(crate::project::io::default_projects_dir);
        let name = self.project_switcher.current_project.name.clone();
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Save Project As")
                .set_directory(&default_dir)
                .set_file_name(&format!(
                    "{}.{}",
                    crate::project::io::sanitize_project_name(&name),
                    crate::project::io::PROJECT_FILE_EXT
                ))
                .add_filter("Futureboard Project", &[crate::project::io::PROJECT_FILE_EXT])
                .save_file()
                .await;
            if let Some(handle) = result {
                let path = handle.path().to_path_buf();
                let _ = entity.update(cx, |this, cx| {
                    this.do_save_project(&path, cx);
                    this.project_path = Some(path);
                });
            }
        })
        .detach();
    }

    fn cmd_save_project_copy(&mut self, cx: &mut Context<Self>) {
        let default_dir = self
            .project_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(crate::project::io::default_projects_dir);
        let name = self.project_switcher.current_project.name.clone();
        let entity = cx.entity().clone();
        let tl_state = self.timeline.read(cx).state.clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Save Copy")
                .set_directory(&default_dir)
                .set_file_name(&format!(
                    "{} Copy.{}",
                    crate::project::io::sanitize_project_name(&name),
                    crate::project::io::PROJECT_FILE_EXT
                ))
                .add_filter("Futureboard Project", &[crate::project::io::PROJECT_FILE_EXT])
                .save_file()
                .await;
            if let Some(handle) = result {
                let path = handle.path().to_path_buf();
                let mut project = FutureboardProject::from(&tl_state);
                let _ = entity.update(cx, |_this, _cx| {
                    if let Err(e) = save_project(&mut project, &path) {
                        eprintln!("[project] save copy failed: {e}");
                    }
                });
            }
        })
        .detach();
    }

    fn do_save_project(&mut self, path: &PathBuf, cx: &mut Context<Self>) {
        let tl_state = self.timeline.read(cx).state.clone();
        let mut project = FutureboardProject::from(&tl_state);
        project.name = self.project_switcher.current_project.name.clone();
        match save_project(&mut project, path) {
            Ok(()) => {
                self.project_switcher.current_project.is_dirty = false;
                self.project_switcher.current_project.subtitle = "Saved".to_string();
                self.project_switcher.current_project.path = Some(path.clone());
                self.recent_projects.push(
                    &project.name,
                    path.clone(),
                    now_secs(),
                );
                self.sync_recent_to_switcher();
            }
            Err(e) => {
                eprintln!("[project] save failed: {e}");
                self.project_switcher.current_project.subtitle = format!("Save failed: {e}");
            }
        }
    }

    fn cmd_open_project(&mut self, cx: &mut Context<Self>) {
        let default_dir = self
            .project_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(crate::project::io::default_projects_dir);
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Open Project")
                .set_directory(&default_dir)
                .add_filter("Futureboard Project", &[crate::project::io::PROJECT_FILE_EXT])
                .pick_file()
                .await;
            if let Some(handle) = result {
                let path = handle.path().to_path_buf();
                let _ = entity.update(cx, |this, cx| {
                    this.load_project_from_path(path, cx);
                });
            }
        })
        .detach();
    }

    fn load_project_from_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match load_project(&path) {
            Ok(project) => {
                let _ = self.timeline.update(cx, |timeline, _cx| {
                    apply_to_timeline(&project, &mut timeline.state);
                });
                self.project_path = Some(path.clone());
                self.project_folder = path.parent().map(|p| p.to_path_buf());
                self.project_switcher.current_project.name = project.name.clone();
                self.project_switcher.current_project.path = Some(path.clone());
                self.project_switcher.current_project.is_dirty = false;
                self.project_switcher.current_project.subtitle = "Opened".to_string();
                self.recent_projects.push(&project.name, path, now_secs());
                self.sync_recent_to_switcher();
                cx.notify();
            }
            Err(e) => {
                eprintln!("[project] load failed: {e}");
            }
        }
    }

    fn cmd_open_recent_project(&mut self, cx: &mut Context<Self>) {
        self.recent_projects.refresh_missing();
        let idx = self.project_switcher.selected_index;
        if idx == 0 {
            return;
        }
        let path = self
            .recent_projects
            .entries()
            .get(idx.saturating_sub(1))
            .map(|e| e.path.clone());
        if let Some(path) = path {
            self.load_project_from_path(path, cx);
        }
    }

    fn cmd_reveal_project_folder(&self, _cx: &mut Context<Self>) {
        #[cfg(target_os = "windows")]
        if let Some(folder) = &self.project_folder {
            let _ = std::process::Command::new("explorer").arg(folder).spawn();
        }
        #[cfg(target_os = "macos")]
        if let Some(folder) = &self.project_folder {
            let _ = std::process::Command::new("open").arg(folder).spawn();
        }
        #[cfg(target_os = "linux")]
        if let Some(folder) = &self.project_folder {
            let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
        }
    }

    fn sync_recent_to_switcher(&mut self) {
        self.recent_projects.refresh_missing();
        self.project_switcher.recent_projects = self
            .recent_projects
            .entries()
            .iter()
            .map(|e| crate::components::project_switcher::ProjectSummary {
                name: e.name.clone(),
                path: Some(e.path.clone()),
                is_current: self.project_path.as_ref() == Some(&e.path),
                is_dirty: false,
                subtitle: if e.missing {
                    "Missing".to_string()
                } else {
                    String::new()
                },
            })
            .collect();
    }

    /// Dev-only: bulk-create `count` tracks for scalability stress testing.
    /// Tracks cycle through Audio/MIDI/Instrument types. Does not add clips.
    #[cfg(debug_assertions)]
    fn stress_add_tracks(&mut self, count: usize, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, _cx| {
            for _ in 0..count {
                let idx = timeline.state.tracks.len();
                let track_type = match idx % 3 {
                    0 => TrackType::Audio,
                    1 => TrackType::Midi,
                    _ => TrackType::Instrument,
                };
                let color = timeline.state.track_color_for_index(idx);
                timeline.state.create_track(timeline_state::CreateTrackOptions {
                    track_type,
                    name: format!("Track {}", idx + 1),
                    color,
                    volume: timeline_state::volume::db_to_norm(0.0),
                    pan: 0.0,
                    armed: false,
                    input_monitor: false,
                });
            }
        });
        cx.notify();
    }

    #[cfg(not(debug_assertions))]
    fn stress_add_tracks(&mut self, _count: usize, _cx: &mut Context<Self>) {}

    fn open_add_track_dialog(&mut self, cx: &mut Context<Self>) {
        self.open_add_track_dialog_with_kind(AddTrackKind::Audio, cx);
    }

    fn open_add_track_dialog_with_kind(&mut self, kind: AddTrackKind, cx: &mut Context<Self>) {
        let mut track_count = 0;
        let mut has_master_track = false;
        let _ = self.timeline.update(cx, |timeline, _cx| {
            track_count = timeline.state.tracks.len();
            has_master_track = timeline
                .state
                .tracks
                .iter()
                .any(|track| track.track_type == TrackType::Master);
        });
        self.open_add_track_dialog_with_context(kind, track_count, has_master_track, cx);
    }

    fn open_add_track_dialog_with_context(
        &mut self,
        kind: AddTrackKind,
        track_count: usize,
        has_master_track: bool,
        cx: &mut Context<Self>,
    ) {
        let mut dialog = AddTrackDialogState::open_for(track_count, has_master_track);
        dialog.selected_kind = kind;
        dialog.track_name = format!("{} {}", kind.label(), dialog.next_number);
        self.add_track_dialog = dialog;
        self.open_popover = None;
        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        cx.notify();
    }

    fn close_add_track_dialog(&mut self, cx: &mut Context<Self>) {
        self.add_track_dialog.is_open = false;
        cx.notify();
    }

    fn select_add_track_kind(&mut self, kind: AddTrackKind, cx: &mut Context<Self>) {
        if kind.native_track_type().is_none() {
            return;
        }
        self.add_track_dialog.selected_kind = kind;
        self.add_track_dialog.track_name =
            format!("{} {}", kind.label(), self.add_track_dialog.next_number);
        self.add_track_dialog.channel_count = if kind == AddTrackKind::Midi { 0 } else { 2 };
        self.add_track_dialog.arm_track = false;
        self.add_track_dialog.monitor_mode = "off";
        cx.notify();
    }

    fn confirm_add_track_dialog(&mut self, cx: &mut Context<Self>) {
        if !self.add_track_dialog.is_open || !self.add_track_dialog.is_valid() {
            return;
        }
        let dialog = self.add_track_dialog.clone();
        let Some(track_type) = dialog.selected_kind.native_track_type() else {
            return;
        };
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            let count = dialog.count.clamp(1, 32) as usize;
            let base_name = cleaned_track_name(&dialog.track_name, dialog.selected_kind);
            let mut selected_track_id = None;
            for i in 0..count {
                let name = if count == 1 {
                    base_name.clone()
                } else {
                    format!("{} {}", numbered_name_stem(&base_name), dialog.next_number + i)
                };
                let id = timeline.state.create_track(CreateTrackOptions {
                    track_type,
                    name,
                    color: timeline
                        .state
                        .track_color_for_index(dialog.color_index.saturating_add(i)),
                    volume: timeline_state::volume::db_to_norm(0.0),
                    pan: 0.0,
                    armed: dialog.selected_kind == AddTrackKind::Audio && dialog.arm_track,
                    input_monitor: dialog.selected_kind == AddTrackKind::Audio
                        && dialog.monitor_mode != "off",
                });
                selected_track_id = Some(id);
            }
            if let Some(id) = selected_track_id {
                timeline.state.select_track(&id);
            }
            cx.notify();
        });
        self.add_track_dialog.is_open = false;
        cx.notify();
    }

    fn delete_selected_track(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.delete_track(&id);
                cx.notify();
            }
        });
    }

    fn delete_selected_clip_or_track(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_clip_ids.first().cloned() {
                timeline.state.delete_clip(&id);
            } else if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.delete_track(&id);
            }
            cx.notify();
        });
    }

    fn duplicate_selected_clip(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_clip_ids.first().cloned() {
                timeline.state.duplicate_clip(&id);
                cx.notify();
            }
        });
    }

    fn toggle_selected_track_mute(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_mute(&id);
                cx.notify();
            }
        });
    }

    fn toggle_selected_track_solo(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_solo(&id);
                cx.notify();
            }
        });
    }

    fn toggle_selected_track_arm(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_arm(&id);
                cx.notify();
            }
        });
    }

    fn reset_selected_track_volume(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline
                    .state
                    .set_track_volume(&id, timeline_state::volume::db_to_norm(0.0));
                cx.notify();
            }
        });
    }

    fn reset_selected_track_pan(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.set_track_pan(&id, 0.0);
                cx.notify();
            }
        });
    }

    fn project_switcher_visible_count(&self) -> usize {
        1 + self
            .project_switcher
            .recent_projects
            .iter()
            .filter(|project| !project.is_current)
            .filter(|project| {
                let query = self.project_switcher.query.trim().to_lowercase();
                if query.is_empty() {
                    return true;
                }
                let path = project
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                project.name.to_lowercase().contains(&query) || path.contains(&query)
            })
            .count()
    }

    fn handle_project_switcher_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.project_switcher.is_open {
            return false;
        }
        if event.is_held {
            return true;
        }
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => {
                self.project_switcher.is_open = false;
                true
            }
            "backspace" => {
                self.project_switcher.query.pop();
                self.project_switcher.selected_index = 0;
                true
            }
            "arrow_down" | "down" => {
                let max = self.project_switcher_visible_count().saturating_sub(1);
                self.project_switcher.selected_index =
                    (self.project_switcher.selected_index + 1).min(max);
                true
            }
            "arrow_up" | "up" => {
                self.project_switcher.selected_index =
                    self.project_switcher.selected_index.saturating_sub(1);
                true
            }
            "enter" | "numpad_enter" => {
                if self.project_switcher.selected_index > 0 {
                    self.dispatch_command_id("project:open-recent", cx);
                    self.project_switcher.is_open = false;
                }
                true
            }
            _ => {
                let no_mods = {
                    let mods = event.keystroke.modifiers;
                    !mods.control && !mods.alt && !mods.platform && !mods.function
                };
                if no_mods && key.chars().count() == 1 {
                    self.project_switcher.query.push_str(key);
                    self.project_switcher.selected_index = 0;
                    true
                } else {
                    false
                }
            }
        }
    }

    fn handle_add_track_dialog_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.add_track_dialog.is_open {
            return false;
        }
        let key = event.keystroke.key.as_str();
        let no_mods = {
            let mods = event.keystroke.modifiers;
            !mods.control && !mods.alt && !mods.platform && !mods.function
        };
        match key {
            "escape" => self.close_add_track_dialog(cx),
            "enter" | "numpad_enter" => self.confirm_add_track_dialog(cx),
            "backspace" if no_mods => {
                self.add_track_dialog.track_name.pop();
                cx.notify();
            }
            "delete" if no_mods => {
                self.add_track_dialog.track_name.clear();
                cx.notify();
            }
            "arrow_up" | "up" if no_mods => {
                self.add_track_dialog.count = (self.add_track_dialog.count + 1).min(32);
                cx.notify();
            }
            "arrow_down" | "down" if no_mods => {
                self.add_track_dialog.count = self.add_track_dialog.count.saturating_sub(1).max(1);
                cx.notify();
            }
            _ if no_mods && key == "space" => {
                self.add_track_dialog.track_name.push(' ');
                cx.notify();
            }
            _ if no_mods && key.chars().count() == 1 => {
                self.add_track_dialog.track_name.push_str(key);
                cx.notify();
            }
            _ => {}
        }
        true
    }

    fn context_entries(
        &self,
        target: &ContextTarget,
        cx: &mut Context<Self>,
    ) -> Vec<ContextMenuEntry> {
        match target {
            ContextTarget::TimelineEmpty => vec![
                ContextMenuEntry::item("Add Audio Track", "track:add-audio"),
                ContextMenuEntry::item("Add MIDI Track", "track:add-midi"),
                ContextMenuEntry::Separator,
                ContextMenuEntry::item("Paste", "edit:paste").with_shortcut("Ctrl+V"),
                ContextMenuEntry::Separator,
                ContextMenuEntry::item("Zoom In", "view:zoom-in"),
                ContextMenuEntry::item("Zoom Out", "view:zoom-out"),
            ],
            ContextTarget::Clip(clip_id) => {
                let exists = self.timeline.read(cx).state.find_clip(clip_id).is_some();
                vec![
                    ContextMenuEntry::disabled_item("Rename", "clip:rename"),
                    ContextMenuEntry::item("Duplicate", "clip:duplicate").with_shortcut("Ctrl+D"),
                    ContextMenuEntry::danger_item("Delete", "clip:delete"),
                    ContextMenuEntry::Separator,
                    ContextMenuEntry::item("Split at Playhead", "clip:split-at-playhead"),
                    ContextMenuEntry::disabled_item(
                        if exists {
                            "Reveal in Browser"
                        } else {
                            "Clip unavailable"
                        },
                        "browser:reveal",
                    ),
                ]
            }
            ContextTarget::Track(track_id) => {
                let track = self.timeline.read(cx).state.find_track(track_id).cloned();
                let (muted, solo, armed) = track
                    .as_ref()
                    .map(|t| (t.muted, t.solo, t.armed))
                    .unwrap_or((false, false, false));
                vec![
                    ContextMenuEntry::disabled_item("Rename Track", "track:rename"),
                    ContextMenuEntry::disabled_item("Duplicate Track", "track:duplicate"),
                    ContextMenuEntry::danger_item("Delete Track", "track:delete"),
                    ContextMenuEntry::Separator,
                    ContextMenuEntry::checked_item("Mute", "track:mute", muted),
                    ContextMenuEntry::checked_item("Solo", "track:solo", solo),
                    ContextMenuEntry::checked_item("Arm", "track:arm", armed),
                ]
            }
            ContextTarget::Browser(path) => vec![
                ContextMenuEntry::item("Import to Timeline", "browser:import"),
                ContextMenuEntry::disabled_item(
                    if path.is_some() {
                        "Reveal in Explorer/Finder"
                    } else {
                        "No file selected"
                    },
                    "browser:reveal",
                ),
                ContextMenuEntry::Separator,
                ContextMenuEntry::item("Refresh", "browser:refresh"),
            ],
            ContextTarget::Mixer(_) => vec![
                ContextMenuEntry::item("Reset Volume", "mixer:reset-volume"),
                ContextMenuEntry::item("Reset Pan", "mixer:reset-pan"),
                ContextMenuEntry::Separator,
                ContextMenuEntry::item("Mute", "track:mute"),
                ContextMenuEntry::item("Solo", "track:solo"),
            ],
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

    fn frame_reason(&self) -> &'static str {
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

        // Ctrl/Cmd shortcuts (no alt, no function)
        if (mods.control || mods.platform) && !mods.alt && !mods.function {
            return match key {
                "s" | "S" if mods.shift => Some("project:save-as"),
                "s" | "S" => Some("project:save"),
                "o" | "O" => Some("project:open"),
                "n" | "N" => Some("project:new"),
                _ => None,
            };
        }

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
    fn build_mixer_callbacks(&self, owner: Entity<Self>) -> MixerCallbacks {
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
        let on_context_menu: std::sync::Arc<
            dyn Fn(&(String, f32, f32), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner;
            std::sync::Arc::new(move |(track_id, x, y): &(String, f32, f32), _w, cx| {
                let track_id = track_id.clone();
                let x = *x;
                let y = *y;
                let _ = this.update(cx, |this, cx| {
                    let _ = this.timeline.update(cx, |timeline, cx| {
                        timeline.state.select_track(&track_id);
                        cx.notify();
                    });
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.project_switcher.is_open = false;
                    this.open_popover = Some(OpenPopover::Context {
                        target: ContextTarget::Mixer(track_id),
                        x,
                        y,
                    });
                    cx.notify();
                });
            })
        };

        MixerCallbacks {
            on_select_track,
            on_volume_change,
            on_pan_change,
            on_toggle_mute,
            on_toggle_solo,
            on_toggle_arm,
            on_toggle_input,
            on_master_volume_change,
            on_context_menu: Some(on_context_menu),
        }
    }
}

impl Render for StudioLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _root_scope = crate::perf::PerfScope::enter("StudioLayout");
        // Frame pacing tick. See FrameDiagnostics docs — only counts
        // real repaints, not display refreshes.
        let reason = self.frame_reason();
        let reason_static: &'static str = match reason {
            "transport" => "transport",
            "panel-resize" => "panel-resize",
            "menu" => "menu",
            _ => "idle/interaction",
        };
        self.frame_diag.tick(reason);
        crate::perf::tick_root_frame(reason_static);

        let on_tab_click = cx.listener(|this, tab: &components::BottomTab, _window, cx| {
            this.active_bottom_tab = *tab;
            cx.notify();
        });

        // Mixer scroll — updated by the mixer scroll-wheel handler.
        let mixer_scroll_x = self.mixer_scroll_x;
        // Approximate the scrollable channel area width: full window minus the
        // master strip (STRIP_WIDTH) plus gutter (1px) and a small margin.
        let window_w: f32 = window.bounds().size.width.into();
        let mixer_viewport_width = (window_w - 90.0).max(100.0);
        let on_mixer_scroll: std::sync::Arc<
            dyn Fn(f32, &mut gpui::Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |new_x: f32, _w, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.mixer_scroll_x = new_x;
                    cx.notify();
                });
            })
        };

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
        let mixer_callbacks = self.build_mixer_callbacks(cx.entity().clone());

        crate::perf::count("tracks", tracks.len() as u64);

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
        let on_browser_context: std::sync::Arc<
            dyn Fn(&(Option<PathBuf>, f32, f32), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(path, x, y): &(Option<PathBuf>, f32, f32), _w, cx| {
                let path = path.clone();
                let x = *x;
                let y = *y;
                let _ = this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.project_switcher.is_open = false;
                    this.open_popover = Some(OpenPopover::Context {
                        target: ContextTarget::Browser(path),
                        x,
                        y,
                    });
                    cx.notify();
                });
            })
        };

        let file_browser = self.file_browser.clone();
        let browser_scroll = self.browser_scroll.clone();

        let on_timeline_context: components::timeline::timeline::TimelineContextMenuCb = {
            let this = cx.entity().clone();
            std::sync::Arc::new(
                move |(target, x, y): &(TimelineContextTarget, f32, f32), _w, cx| {
                    let target = target.clone();
                    let x = *x;
                    let y = *y;
                    let _ = this.update(cx, |this, cx| {
                        let context_target = match target {
                            TimelineContextTarget::TimelineEmpty => ContextTarget::TimelineEmpty,
                            TimelineContextTarget::TrackHeader(id) => {
                                let _ = this.timeline.update(cx, |timeline, cx| {
                                    timeline.state.select_track(&id);
                                    cx.notify();
                                });
                                ContextTarget::Track(id)
                            }
                            TimelineContextTarget::Clip(id) => {
                                let _ = this.timeline.update(cx, |timeline, cx| {
                                    timeline.state.select_clip(&id);
                                    cx.notify();
                                });
                                ContextTarget::Clip(id)
                            }
                        };
                        this.menu_bar.open_menu_id = None;
                        this.menu_bar.submenu_path.clear();
                        this.project_switcher.is_open = false;
                        this.open_popover = Some(OpenPopover::Context {
                            target: context_target,
                            x,
                            y,
                        });
                        cx.notify();
                    });
                },
            )
        };
        let _ = self.timeline.update(cx, |timeline, _cx| {
            timeline.set_context_menu_callback(Some(on_timeline_context));
        });
        let on_add_track: components::timeline::timeline::TimelineAddTrackCb = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |request, _w, cx| {
                let request = *request;
                let _ = this.update(cx, |this, cx| {
                    this.open_add_track_dialog_with_context(
                        AddTrackKind::Audio,
                        request.track_count,
                        request.has_master_track,
                        cx,
                    );
                });
            })
        };
        let _ = self.timeline.update(cx, |timeline, _cx| {
            timeline.set_add_track_callback(Some(on_add_track));
        });

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
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
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
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    cx.notify();
                });
            })
        };
        let on_project_open: std::sync::Arc<dyn Fn(&f32, &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |anchor_x: &f32, _w, cx| {
                let anchor_x = *anchor_x;
                let _ = this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.open_popover = None;
                    this.project_switcher.is_open = !this.project_switcher.is_open;
                    this.project_switcher.anchor_x = anchor_x;
                    if this.project_switcher.is_open {
                        this.project_switcher.query.clear();
                        this.project_switcher.selected_index = 0;
                    }
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
        let on_close_popover: std::sync::Arc<dyn Fn(&(), &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |_: &(), _w, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    cx.notify();
                });
            })
        };
        let on_popover_command: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |command: &String, _w, cx| {
                let command = command.clone();
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id(&command, cx);
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    cx.notify();
                });
            })
        };
        let popover_overlay = if self.project_switcher.is_open {
            Some(
                components::project_switcher::project_switcher_popover(
                    &self.project_switcher,
                    viewport_width,
                    viewport_height,
                    on_popover_command.clone(),
                    on_close_popover.clone(),
                )
                .into_any_element(),
            )
        } else {
            match self.open_popover.clone() {
                Some(OpenPopover::Context { target, x, y }) => Some(
                    components::context_menu::context_menu_overlay(
                        self.context_entries(&target, cx),
                        x,
                        y,
                        viewport_width,
                        viewport_height,
                        on_popover_command.clone(),
                        on_close_popover.clone(),
                    )
                    .into_any_element(),
                ),
                None => None,
            }
        };
        let add_track_overlay = if self.add_track_dialog.is_open {
            let target = cx.entity().clone();
            let callbacks = AddTrackDialogCallbacks {
                on_close: Arc::new({
                    let target = target.clone();
                    move |_: &(), _w, cx| {
                        let _ = target.update(cx, |this, cx| this.close_add_track_dialog(cx));
                    }
                }),
                on_confirm: Arc::new({
                    let target = target.clone();
                    move |_: &(), _w, cx| {
                        let _ = target.update(cx, |this, cx| this.confirm_add_track_dialog(cx));
                    }
                }),
                on_select_kind: Arc::new({
                    let target = target.clone();
                    move |kind: &AddTrackKind, _w, cx| {
                        let kind = *kind;
                        let _ =
                            target.update(cx, |this, cx| this.select_add_track_kind(kind, cx));
                    }
                }),
                on_count_delta: Arc::new({
                    let target = target.clone();
                    move |delta: &i32, _w, cx| {
                        let delta = *delta;
                        let _ = target.update(cx, |this, cx| {
                            let current = this.add_track_dialog.count as i32;
                            this.add_track_dialog.count = (current + delta).clamp(1, 32) as u32;
                            cx.notify();
                        });
                    }
                }),
                on_channel_count: Arc::new({
                    let target = target.clone();
                    move |channels: &u32, _w, cx| {
                        let channels = *channels;
                        let _ = target.update(cx, |this, cx| {
                            this.add_track_dialog.channel_count = channels.clamp(1, 2);
                            cx.notify();
                        });
                    }
                }),
                on_color_index: Arc::new({
                    let target = target.clone();
                    move |index: &u32, _w, cx| {
                        let index = *index as usize;
                        let _ = target.update(cx, |this, cx| {
                            this.add_track_dialog.color_index = index;
                            cx.notify();
                        });
                    }
                }),
                on_arm: Arc::new({
                    let target = target.clone();
                    move |armed: &bool, _w, cx| {
                        let armed = *armed;
                        let _ = target.update(cx, |this, cx| {
                            this.add_track_dialog.arm_track = armed;
                            cx.notify();
                        });
                    }
                }),
                on_monitor: Arc::new({
                    let target = target.clone();
                    move |mode: &String, _w, cx| {
                        let mode = match mode.as_str() {
                            "auto" => "auto",
                            "in" => "in",
                            _ => "off",
                        };
                        let _ = target.update(cx, |this, cx| {
                            this.add_track_dialog.monitor_mode = mode;
                            cx.notify();
                        });
                    }
                }),
            };
            Some(add_track_dialog(&self.add_track_dialog, callbacks).into_any_element())
        } else {
            None
        };

        let wizard_overlay = if self.project_wizard.is_open {
            let name_focused = self.wizard_name_input.focus_handle.is_focused(window);
            let bpm_focused = self.wizard_bpm_input.focus_handle.is_focused(window);
            let name_input = self.wizard_name_input.clone();
            let bpm_input = self.wizard_bpm_input.clone();
            let target = cx.entity().clone();
            let callbacks = ProjectWizardCallbacks {
                on_close: Arc::new({
                    let target = target.clone();
                    move |_, _, cx| {
                        let _ = target.update(cx, |this, _cx| this.close_project_wizard());
                    }
                }),
                on_create: Arc::new({
                    let target = target.clone();
                    move |result: &ProjectWizardResult, _, cx| {
                        let result = result.clone();
                        let _ = target.update(cx, |this, cx| {
                            this.on_project_created(&result, cx);
                        });
                    }
                }),
                on_template: Arc::new({
                    let target = target.clone();
                    move |tmpl: &ProjectTemplate, _, cx| {
                        let tmpl = *tmpl;
                        let _ = target.update(cx, |this, cx| {
                            this.project_wizard.apply_template(tmpl);
                            cx.notify();
                        });
                    }
                }),
                on_bpm_step: Arc::new({
                    let target = target.clone();
                    move |delta: &i32, _, cx| {
                        let delta = *delta;
                        let _ = target.update(cx, |this, cx| {
                            let current = this.project_wizard.bpm() as f32;
                            let new_bpm = (current + delta as f32).clamp(20.0, 999.0);
                            let text = format!("{:.0}", new_bpm);
                            this.project_wizard.bpm_text = text.clone();
                            this.wizard_bpm_input.set_value(text);
                            cx.notify();
                        });
                    }
                }),
                on_time_sig_num: Arc::new({
                    let target = target.clone();
                    move |n: &u32, _, cx| {
                        let n = *n;
                        let _ = target.update(cx, |this, cx| {
                            this.project_wizard.time_sig_num = n;
                            cx.notify();
                        });
                    }
                }),
                on_time_sig_den: Arc::new({
                    let target = target.clone();
                    move |d: &u32, _, cx| {
                        let d = *d;
                        let _ = target.update(cx, |this, cx| {
                            this.project_wizard.time_sig_den = d;
                            cx.notify();
                        });
                    }
                }),
                on_sample_rate: Arc::new({
                    let target = target.clone();
                    move |sr: &u32, _, cx| {
                        let sr = *sr;
                        let _ = target.update(cx, |this, cx| {
                            this.project_wizard.sample_rate = sr;
                            cx.notify();
                        });
                    }
                }),
                on_browse_location: Arc::new({
                    let target = target.clone();
                    move |_, _window, cx| {
                        let current = target.read(cx).project_wizard.location.clone();
                        let fut = rfd::AsyncFileDialog::new()
                            .set_title("Choose Project Location")
                            .set_directory(&current)
                            .pick_folder();
                        let target2 = target.clone();
                        cx.spawn(async move |cx| {
                            if let Some(handle) = fut.await {
                                let path = handle.path().to_path_buf();
                                let _ = target2.update(cx, |this, cx| {
                                    this.project_wizard.location = path;
                                    cx.notify();
                                });
                            }
                        })
                        .detach();
                    }
                }),
            };
            Some(
                components::project_wizard(
                    &self.project_wizard,
                    &name_input,
                    name_focused,
                    &bpm_input,
                    bpm_focused,
                    callbacks,
                )
                .into_any_element(),
            )
        } else {
            None
        };

        let transport_chrome = self.transport_chrome_state(cx);
        let project_chrome = components::ProjectChromeState {
            name: self.project_switcher.current_project.name.clone(),
            is_dirty: self.project_switcher.current_project.is_dirty,
            on_open_project_menu: on_project_open,
        };
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
            .capture_key_down(move |event, window, cx| {
                // ── Wizard text input routing ─────────────────────────────
                // When the wizard is open and a text field has focus, route
                // keys to it before any global shortcut handling so that
                // spacebar doesn't start playback, letters don't trigger
                // commands, etc.
                // Check focus BEFORE the mutable update borrow (requires &Window).
                let (wizard_open, name_focused, bpm_focused) = {
                    let layout = shortcut_target.read(cx);
                    let open = layout.project_wizard.is_open;
                    let nf = layout.wizard_name_input.focus_handle.is_focused(window);
                    let bf = layout.wizard_bpm_input.focus_handle.is_focused(window);
                    (open, nf, bf)
                };
                let wizard_consumed = if wizard_open && (name_focused || bpm_focused) {
                    shortcut_target.update(cx, |this, cx| {
                        let input = if name_focused {
                            &mut this.wizard_name_input
                        } else {
                            &mut this.wizard_bpm_input
                        };
                        let action = input.handle_key(event);
                        // Keep wizard state in sync so result()/is_valid() see current text.
                        this.project_wizard.name = this.wizard_name_input.value.clone();
                        this.project_wizard.bpm_text = this.wizard_bpm_input.value.clone();
                        match action {
                            TextInputAction::Cancel => {
                                this.close_project_wizard();
                            }
                            TextInputAction::Submit | TextInputAction::Consumed => {}
                            TextInputAction::Pass => {}
                        }
                        cx.notify();
                        action != TextInputAction::Pass
                    })
                } else {
                    false
                };
                if wizard_consumed {
                    return;
                }

                let handled = shortcut_target.update(cx, |this, cx| {
                    let handled = this.handle_add_track_dialog_key(event, cx)
                        || this.handle_project_switcher_key(event, cx);
                    if handled {
                        cx.notify();
                    }
                    handled
                });
                if handled {
                    return;
                }
                if event.keystroke.key.as_str() == "escape" {
                    let _ = shortcut_target.update(cx, |this, cx| {
                        let _ = this.timeline.update(cx, |timeline, cx| {
                            if timeline.state.dragging_track_id.is_some() {
                                timeline.state.clear_track_drag();
                                cx.notify();
                            }
                        });
                        this.menu_bar.open_menu_id = None;
                        this.menu_bar.submenu_path.clear();
                        this.open_popover = None;
                        this.project_switcher.is_open = false;
                        this.project_wizard.is_open = false;
                        cx.notify();
                    });
                    return;
                }
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
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&focus_holder))
            .child({
                let _s = crate::perf::PerfScope::enter("AppChrome");
                components::app_chrome(
                    window,
                    open_menu_id.as_deref(),
                    on_open_menu,
                    project_chrome,
                    transport_chrome,
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child({
                        let _s = crate::perf::PerfScope::enter("Sidebar");
                        components::sidebar(
                            &file_browser,
                            browser_scroll,
                            on_browser_toggle,
                            on_browser_select,
                            on_browser_activate,
                            on_browser_context,
                        )
                    })
                    .child(self.timeline.clone())
                    .child({
                        let _s = crate::perf::PerfScope::enter("Inspector");
                        crate::components::panel::inspector_panel(
                            &tracks,
                            selected_track_id.as_deref(),
                            selected_clip_id.as_deref(),
                            find_clip_summary(&tracks, selected_clip_id.as_deref()),
                        )
                    }),
            )
            .child({
                let _s = crate::perf::PerfScope::enter("BottomPanel");
                components::bottom_panel(
                    self.active_bottom_tab,
                    panel_state,
                    &tracks,
                    &master,
                    selected_track_id.as_deref(),
                    mixer_callbacks,
                    mixer_scroll_x,
                    mixer_viewport_width,
                    on_mixer_scroll,
                    on_tab_click,
                    on_resize_start,
                    on_resize_move,
                )
            })
            .child({
                let _s = crate::perf::PerfScope::enter("StatusBar");
                components::status_bar(status_left, status_right)
            })
            // Dropdown overlay — rendered last so it sits above every other
            // panel. The dropdown's own backdrop captures click-outside.
            .children(dropdown_overlay)
            .children(popover_overlay)
            .children(add_track_overlay)
            .children(wizard_overlay)
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
    matches!(
        ext,
        "wav" | "wave" | "mp3" | "flac" | "ogg" | "oga" | "aiff" | "aif"
    )
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

fn normalize_command_id(command_id: &str) -> String {
    command_id.trim().replace('.', ":").replace('_', "-")
}

fn cleaned_track_name(name: &str, kind: AddTrackKind) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        kind.label().to_string()
    } else {
        trimmed.to_string()
    }
}

fn numbered_name_stem(name: &str) -> String {
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
    if stem.is_empty() {
        "Track".to_string()
    } else {
        stem.to_string()
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
