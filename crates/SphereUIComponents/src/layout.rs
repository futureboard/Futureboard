use gpui::{
    div, px, AppContext, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, Styled, UniformListScrollHandle, Window, WindowHandle,
};

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::components;
use crate::components::add_track_dialog::{
    open_add_track_window, AddTrackDialogState, AddTrackKind, AddTrackWindow,
};
use crate::components::plugin_manager::{open_plugin_manager_window, PluginManagerWindow};
use crate::components::context_menu::ContextMenuEntry;
use crate::components::file_browser::{read_directory, FileBrowserState};
use crate::components::mixer_panel::MixerCallbacks;
use crate::components::project_switcher::ProjectSwitcherState;
use crate::components::project_wizard::{
    open_project_wizard_window, ProjectCreateCallback, ProjectWizardResult, ProjectWizardWindow,
};
use crate::components::{external_mixer_debug, open_mixer_window, MixerSnapshot, MixerWindow};
use crate::components::settings_dialog::{
    OnSettingUpdate, SettingsWindow, open_settings_window,
};
use crate::settings::{SettingsModel, SettingsSchema, GlobalSettingsModel};
use crate::components::text_input::{
    text_input_context_entries, TextInputAction, TextInputCallbacks, TextInputState,
};
use crate::components::timeline::timeline::TimelineContextTarget;
use crate::components::timeline::timeline_state::{
    self, ClipType, CreateTrackOptions, TimelineState, TrackState, TrackType,
};
use crate::components::{BottomPanelResizeDrag, BottomPanelState};
use crate::paths::FutureboardPaths;
use crate::project::{
    apply_to_timeline, io::load_project, io::save_project, now_secs, recent::RecentProjectsStore,
    FutureboardProject,
};
use crate::overlay::{project_title_anchor, titlebar_label_anchor, OverlayAnchor};
use crate::theme::{self, Colors};

use DAUx::types::{
    EngineClipAudioProcess, EngineClipSnapshot, EngineProjectSnapshot, EngineRoutingSnapshot,
    EngineTrackSnapshot,
};

/// Flip to `true` to seed the studio with demo tracks/clips at startup.
/// Production builds must keep this `false` — the real app starts empty.
const USE_DEMO_PROJECT: bool = false;

/// Notify a satellite window's root view without calling `Entity::update` (which
/// can re-enter the main studio entity and trip GPUI's lease checks).
pub(crate) fn notify_window_root<T: gpui::Render>(
    app: &mut gpui::App,
    handle: &WindowHandle<T>,
) {
    if let Ok(entity) = handle.entity(app) {
        app.notify(entity.entity_id());
    }
}

// Frame pacing details live in tasks/native/frame-pacing.md.

/// Top-menu open state. `open_menu_id` is the manifest menu id currently
/// showing its dropdown; `anchor` is the label rect used to position the panel.
#[derive(Debug, Clone, Default)]
pub struct MenuBarUiState {
    pub open_menu_id: Option<String>,
    pub anchor: OverlayAnchor,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextMenuTarget {
    ProjectSwitcherSearch,
    BrowserSearch,
}

#[derive(Debug, Clone, Copy)]
struct TextContextMenu {
    target: TextMenuTarget,
    x: f32,
    y: f32,
}

#[derive(Debug, Clone)]
pub enum ContextTarget {
    TimelineEmpty,
    Track(String),
    Clip(String),
    Browser(Option<PathBuf>),
    Mixer(String),
}

/// Which docked studio panels are visible in the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StudioPanelVisibility {
    pub browser: bool,
    pub inspector: bool,
    pub mixer_docked: bool,
}

impl Default for StudioPanelVisibility {
    fn default() -> Self {
        Self {
            browser: true,
            inspector: true,
            mixer_docked: true,
        }
    }
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
    project_switcher_search_input: TextInputState,
    browser_search_input: TextInputState,
    add_track_window: Option<WindowHandle<AddTrackWindow>>,
    plugin_manager_window: Option<WindowHandle<PluginManagerWindow>>,
    /// External settings window handle; None when closed.
    settings_window: Option<WindowHandle<SettingsWindow>>,
    /// Detached mixer window for multi-monitor layouts.
    mixer_window: Option<WindowHandle<MixerWindow>>,
    /// Open external mixer after the current studio update completes.
    pending_mixer_external_open: Option<Bounds<gpui::Pixels>>,
    panels: StudioPanelVisibility,
    settings: gpui::Entity<SettingsModel>,

    text_context_menu: Option<TextContextMenu>,
    open_popover: Option<OpenPopover>,
    audio_engine: Option<DAUx::AudioEngine>,
    audio_running: bool,
    audio_last_error: Option<String>,
    audio_stats: Option<DAUx::EngineStats>,
    last_audio_project_signature: Option<String>,
    engine_project_dirty: bool,
    engine_media_dirty: bool,
    /// True while a background `load_project` (file decode) is running.
    audio_sync_in_flight: bool,
    /// Queued when media/project changes during an in-flight sync.
    audio_sync_pending: bool,
    /// Start transport once the current background sync completes.
    pending_play_after_sync: bool,
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
    /// Centralized filesystem paths for the entire application.
    paths: FutureboardPaths,
    /// Absolute path to the currently open `.fbproj` file, if any.
    project_path: Option<PathBuf>,
    /// Root folder of the current project (contains Media/, Cache/, etc.).
    project_folder: Option<PathBuf>,
    /// Persistent recent-projects list backed by `<AppData>/Futureboard Studio/recent.json`.
    recent_projects: RecentProjectsStore,
    /// External borderless New Project utility window, if it is currently alive.
    project_wizard_window: Option<WindowHandle<ProjectWizardWindow>>,
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
        // ── Centralized path resolution ───────────────────────────────────
        let paths = FutureboardPaths::resolve();
        if let Err(e) = paths.ensure_user_dirs() {
            eprintln!("[paths] failed to create user directories: {e}");
        }

        let settings = SettingsModel::load_or_create(cx);
        cx.set_global(GlobalSettingsModel(settings.clone()));

        let schema = settings.read(cx).current.clone();
        let backend = match schema.hardware.audio.driver_type.as_str() {
            "WASAPI Exclusive" => DAUx::AudioBackend::WasapiExclusive,
            _ => DAUx::AudioBackend::Auto,
        };
        let audio_config = DAUx::EngineConfig {
            sample_rate: schema.general.project_defaults.sample_rate,
            buffer_size: schema.general.project_defaults.buffer_size,
            channels: 2,
            backend,
        };

        let audio_engine = match DAUx::AudioEngine::new(audio_config) {
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
                let mut engine = engine;
                match engine.start() {
                    Ok(()) => {
                        let stats = engine.stats();
                        eprintln!(
                            "[audio] stream warmed: backend={} sr={} buf={}",
                            stats.backend_name, stats.sample_rate, stats.buffer_size
                        );
                    }
                    Err(error) => {
                        eprintln!("[audio] warm-up failed; will retry on first Play: {error}");
                    }
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
        let metronome_enabled = schema.recording.metronome.enabled;
        let _ = timeline.update(cx, |t, _cx| {
            t.state.transport.metronome_enabled = metronome_enabled;
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
        {
            let target = cx.entity().clone();
            let _ = timeline.update(cx, |timeline, _cx| {
                timeline.set_project_changed_callback(Some(Arc::new(move |cx| {
                    let _ = target.update(cx, |this, _cx| {
                        this.mark_dirty();
                    });
                })));
            });
        }
        {
            let target = cx.entity().clone();
            let _ = timeline.update(cx, |timeline, _cx| {
                timeline.set_media_changed_callback(Some(Arc::new(move |cx| {
                    // Only mark dirty here — never read/sync Timeline from this
                    // callback. It runs inside Timeline::update (e.g. file drop)
                    // and sync_audio_project reads Timeline, which panics.
                    let _ = target.update(cx, |this, _cx| {
                        this.mark_engine_media_dirty();
                    });
                })));
            });
        }
        let initial_audio_stats = audio_engine.as_ref().map(|engine| engine.stats());
        let initial_audio_running = initial_audio_stats
            .as_ref()
            .map(|stats| stats.running)
            .unwrap_or(false);

        Self::spawn_audio_poll(cx);

        let studio_entity = cx.entity();
        crate::platform_chrome::register_studio_menu_dispatcher(studio_entity, cx);

        // settings and paths are loaded and registered at the top of this function

        Self {
            active_bottom_tab: components::BottomTab::Mixer,
            bottom_panel_state: BottomPanelState::default(),
            timeline,
            file_browser: FileBrowserState::default(),
            browser_scroll: UniformListScrollHandle::new(),
            menu_bar: MenuBarUiState::default(),
            project_switcher: ProjectSwitcherState::default(),
            project_switcher_search_input: TextInputState::new(
                "project-switcher-search-input",
                cx.focus_handle(),
            )
            .with_placeholder("Search projects..."),
            browser_search_input: TextInputState::new(
                "browser-search-input",
                cx.focus_handle(),
            )
            .with_placeholder("Search..."),
            add_track_window: None,
            plugin_manager_window: None,
            settings_window: None,
            mixer_window: None,
            pending_mixer_external_open: None,
            panels: StudioPanelVisibility::default(),
            settings,

            text_context_menu: None,
            open_popover: None,
            audio_engine,
            audio_running: initial_audio_running,
            audio_last_error: None,
            audio_stats: initial_audio_stats,
            last_audio_project_signature: None,
            engine_project_dirty: true,
            engine_media_dirty: true,
            audio_sync_in_flight: false,
            audio_sync_pending: false,
            pending_play_after_sync: false,
            last_engine_playhead_beat: 0.0,
            last_engine_sync: Instant::now(),
            focus_handle: cx.focus_handle(),
            logged_unsupported_commands: HashSet::new(),
            frame_diag: FrameDiagnostics::new(),
            mixer_scroll_x: 0.0,
            paths,
            project_path: None,
            project_folder: None,
            recent_projects: RecentProjectsStore::load(),
            project_wizard_window: None,
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

    fn poll_native_audio(&mut self, cx: &mut Context<Self>) -> bool {
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

    /// Update smoothed meter levels in timeline state. Does not call
    /// `cx.notify` — repaints are driven by the audio poll when transport
    /// is active, or by user interaction when idle.
    fn apply_engine_meters(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.audio_engine.as_ref() else {
            return;
        };
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

    fn complete_audio_project_sync(
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

    fn mark_engine_project_dirty(&mut self) {
        self.engine_project_dirty = true;
    }

    pub(crate) fn mark_engine_media_dirty(&mut self) {
        self.engine_project_dirty = true;
        self.engine_media_dirty = true;
    }

    fn ensure_audio_stream_warm(&mut self) -> bool {
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
    pub(crate) fn dispatch_command_id(&mut self, command_id: &str, cx: &mut Context<Self>) {
        self.dispatch_command_id_from_bounds(command_id, None, cx);
    }

    fn dispatch_command_id_from_bounds(
        &mut self,
        command_id: &str,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let normalized = normalize_command_id(command_id);
        let command_id = normalized.as_str();
        if let Some(command) = transport_command_from_id(command_id) {
            self.dispatch_transport_command(command, cx);
            return;
        }
        match command_id {
            "noop" => {}

            "browser:import" => {
                let path = match &self.open_popover {
                    Some(OpenPopover::Context { target: ContextTarget::Browser(path), .. }) => path.clone(),
                    _ => None,
                };
                if let Some(path) = path {
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    if is_supported_audio_ext(&ext) {
                        let timeline = self.timeline.clone();
                        let layout = cx.entity().clone();
                        let path_for_decode = path.clone();
                        let timeline_for_decode = timeline.clone();
                        timeline.update(cx, |t, cx| {
                            let path_key = path.to_string_lossy().to_string();
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "Imported Audio".to_string());
                            t.state.import_audio_to_selected_or_new_track(path_key, name);
                            cx.notify();
                        });
                        let _ = layout.update(cx, |this, cx| {
                            this.mark_dirty();
                            this.mark_engine_media_dirty();
                            this.schedule_audio_project_sync(cx, false, "browser_import");
                        });
                        let path_key = path_for_decode.to_string_lossy().to_string();
                        let owner = layout.clone();
                        let _ = layout.update(cx, move |_layout, cx| {
                            Self::spawn_timeline_audio_import_jobs(
                                cx,
                                owner,
                                timeline_for_decode,
                                path_for_decode,
                                path_key,
                            );
                        });
                    }
                }
            }
            "browser:reveal" => {
                let path = match &self.open_popover {
                    Some(OpenPopover::Context { target: ContextTarget::Browser(path), .. }) => path.clone(),
                    _ => None,
                };
                if let Some(path) = path {
                    reveal_path(&path);
                }
            }
            "browser:refresh" => {
                let path = match &self.open_popover {
                    Some(OpenPopover::Context { target: ContextTarget::Browser(path), .. }) => path.clone(),
                    _ => None,
                };
                if let Some(path) = path {
                    self.file_browser.mark_loading(path.clone());
                    Self::spawn_directory_load(cx, path);
                } else {
                    let pending = self.file_browser.expanded_paths.clone();
                    for p in pending {
                        self.file_browser.mark_loading(p.clone());
                        Self::spawn_directory_load(cx, p);
                    }
                }
            }
            "browser:copy-path" => {
                let path = match &self.open_popover {
                    Some(OpenPopover::Context { target: ContextTarget::Browser(path), .. }) => path.clone(),
                    _ => None,
                };
                if let Some(path) = path {
                    let path_str = path.to_string_lossy().to_string();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path_str));
                }
            }
            "browser:open" => {
                let path = match &self.open_popover {
                    Some(OpenPopover::Context { target: ContextTarget::Browser(path), .. }) => path.clone(),
                    _ => None,
                };
                if let Some(path) = path {
                    let id = path.to_string_lossy().to_string();
                    let expanded = self.file_browser.toggle_node(&id, Some(&path));
                    if expanded {
                        let pending = self.file_browser.paths_needing_load();
                        for p in pending {
                            self.file_browser.mark_loading(p.clone());
                            Self::spawn_directory_load(cx, p);
                        }
                    }
                }
            }
            "browser:new-folder" => {
                eprintln!("[browser] TODO: new folder action");
            }
            "browser:rename" => {
                eprintln!("[browser] TODO: rename action");
            }

            // ── View / zoom ──────────────────────────────────────────────
            "view:zoom-in" => self.zoom_timeline_by(cx, 1.25),
            "view:zoom-out" => self.zoom_timeline_by(cx, 0.8),
            "view:reset-zoom" => self.reset_timeline_zoom(cx),

            // ── Project / track / edit commands available in native shell ─
            "project:new" | "project:new-from-template" => {
                self.open_project_wizard(owner_bounds, cx)
            }
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

            "app:preferences" | "edit:preferences" | "project:settings" => {
                self.open_settings_dialog(owner_bounds, cx);
            }

            "panel:toggle-browser" | "window.show_browser" => self.toggle_browser_panel(cx),
            "panel:toggle-inspector" | "view:toggle-inspector" | "window.show_inspector" => {
                self.toggle_inspector_panel(cx)
            }
            "panel:toggle-mixer" | "view:toggle-mixer" | "window.show_mixer" => {
                self.toggle_mixer_panel(cx)
            }
            "panel:mixer-float" | "floatingwindow:mixer" => {
                self.open_mixer_external_window(owner_bounds, cx);
            }

            "track:add" | "project:add-track" => {
                self.open_add_track_external_window(AddTrackKind::Audio, owner_bounds, cx)
            }
            "track:add-audio" => {
                self.open_add_track_external_window(AddTrackKind::Audio, owner_bounds, cx)
            }
            "track:add-midi" => {
                self.open_add_track_external_window(AddTrackKind::Midi, owner_bounds, cx)
            }
            "track:add-instrument" => {
                self.open_add_track_external_window(AddTrackKind::Instrument, owner_bounds, cx)
            }
            "track:add-plugin" => {
                self.open_add_track_external_window(AddTrackKind::Plugin, owner_bounds, cx)
            }
            "track:add-bus" => self.open_add_track_external_window(AddTrackKind::Bus, owner_bounds, cx),
            "track:add-return" => {
                self.open_add_track_external_window(AddTrackKind::Return, owner_bounds, cx)
            }
            "track:add-group" => {
                self.open_add_track_external_window(AddTrackKind::Group, owner_bounds, cx)
            }
            "track:add-master" => {
                self.open_add_track_external_window(AddTrackKind::Master, owner_bounds, cx)
            }
            "plugins:manager" => self.open_plugin_manager_external_window(owner_bounds, cx),
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
        self.file_browser.set_project_folder(None);
        self.project_switcher = ProjectSwitcherState::default();
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state = TimelineState::default();
            cx.notify();
        });
    }

    // ── Project wizard ────────────────────────────────────────────────────────

    fn open_project_wizard(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.project_wizard_window.clone() {
            if handle
                .update(cx, |_wizard, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.project_wizard_window = None;
        }

        let owner = cx.entity().clone();
        let on_create: ProjectCreateCallback = Arc::new(move |result, cx| {
            owner
                .update(cx, |this, cx| this.on_project_created(&result, cx))
                .map_err(|error| format!("Unable to update the main studio window: {error}"))
        });
        let bounds = owner_bounds.unwrap_or_else(|| Bounds {
            origin: gpui::Point::default(),
            size: gpui::size(px(1400.0), px(900.0)),
        });

        match open_project_wizard_window(bounds, on_create, cx) {
            Ok(handle) => self.project_wizard_window = Some(handle),
            Err(error) => eprintln!("[project] failed to open project wizard window: {error}"),
        }
    }

    fn on_project_created(
        &mut self,
        result: &ProjectWizardResult,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let safe_name = crate::project::io::sanitize_project_name(&result.name);
        let target_folder = result.location.join(&safe_name);
        if target_folder.exists() {
            return Err("A project with this name already exists at that location.".to_string());
        }
        let folder = match crate::project::io::create_project_folder(&result.location, &result.name)
        {
            Ok(f) => f,
            Err(e) => {
                return Err(format!("Failed to create project folder: {e}"));
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
                    let color = timeline
                        .state
                        .track_color_for_index((audio_count + i) as usize);
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

        save_project(&mut project, &project_file)
            .map_err(|e| format!("Failed to save initial project file: {e}"))?;

        self.project_path = Some(project_file.clone());
        self.project_folder = Some(folder.clone());
        self.file_browser.set_project_folder(Some(folder));
        self.project_switcher.current_project.name = result.name.clone();
        self.project_switcher.current_project.path = Some(project_file.clone());
        self.project_switcher.current_project.is_dirty = false;
        self.project_switcher.current_project.subtitle = "Saved".to_string();

        self.recent_projects
            .push(&result.name, project_file, now_secs());
        self.sync_recent_to_switcher();
        cx.notify();
        Ok(())
    }

    // ── Save / load ───────────────────────────────────────────────────────────

    fn mark_dirty(&mut self) {
        self.project_switcher.current_project.is_dirty = true;
        self.project_switcher.current_project.subtitle = "Unsaved changes".to_string();
        self.mark_engine_project_dirty();
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
                .add_filter(
                    "Futureboard Project",
                    &[crate::project::io::PROJECT_FILE_EXT],
                )
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
                .add_filter(
                    "Futureboard Project",
                    &[crate::project::io::PROJECT_FILE_EXT],
                )
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
                self.recent_projects
                    .push(&project.name, path.clone(), now_secs());
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
                .add_filter(
                    "Futureboard Project",
                    &[crate::project::io::PROJECT_FILE_EXT],
                )
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
                self.file_browser.set_project_folder(self.project_folder.clone());
                self.project_switcher.current_project.name = project.name.clone();
                self.project_switcher.current_project.path = Some(path.clone());
                self.project_switcher.current_project.is_dirty = false;
                self.project_switcher.current_project.subtitle = "Opened".to_string();
                self.recent_projects.push(&project.name, path, now_secs());
                self.sync_recent_to_switcher();
                self.mark_engine_media_dirty();
                self.schedule_audio_project_sync(cx, true, "project_loaded");
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
                timeline
                    .state
                    .create_track(timeline_state::CreateTrackOptions {
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

    fn open_add_track_external_window(
        &mut self,
        kind: AddTrackKind,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
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

        self.open_add_track_external_window_with_context(
            kind,
            track_count,
            has_master_track,
            owner_bounds,
            cx,
        );
    }

    /// Opens/activates the Add Track external window without reading/updating the Timeline.
    ///
    /// This is critical for callbacks originating from Timeline events: Timeline may already be
    /// mid-update, and calling `self.timeline.update(...)` would panic (GPUI re-entrancy guard).
    fn open_add_track_external_window_with_context(
        &mut self,
        kind: AddTrackKind,
        track_count: usize,
        has_master_track: bool,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {

        // If window is already open, activate and refresh its context.
        if let Some(handle) = self.add_track_window.clone() {
            if handle
                .update(cx, |win, window, _cx| {
                    win.set_context(kind, track_count, has_master_track);
                    window.activate_window();
                })
                .is_ok()
            {
                return;
            }
            self.add_track_window = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.open_popover = None;
        self.text_context_menu = None;

        let owner_bounds = owner_bounds.unwrap_or_else(|| Bounds {
            origin: gpui::Point::default(),
            size: gpui::size(px(1400.0), px(900.0)),
        });

        let layout = cx.entity().clone();
        let on_confirm_request: Arc<
            dyn Fn(AddTrackDialogState, String, &mut gpui::App) + 'static,
        > =
            Arc::new(move |dialog, _name, cx| {
                let Some(track_type) = dialog.selected_kind.native_track_type() else {
                    return;
                };
                let _ = layout.update(cx, |this, cx| {
                    this.mark_dirty();
                    let _ = this.timeline.update(cx, |timeline, cx| {
                        let count = dialog.count.clamp(1, 32) as usize;
                        let base_name = cleaned_track_name(&dialog.track_name, dialog.selected_kind);
                        let mut selected_track_id = None;
                        for i in 0..count {
                            let name = if count == 1 {
                                base_name.clone()
                            } else {
                                format!(
                                    "{} {}",
                                    numbered_name_stem(&base_name),
                                    dialog.next_number + i
                                )
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
                    cx.notify();
                });
            });

        match open_add_track_window(
            owner_bounds,
            kind,
            track_count,
            has_master_track,
            on_confirm_request,
            cx,
        ) {
            Ok(handle) => self.add_track_window = Some(handle),
            Err(err) => eprintln!("[add-track] failed to open window: {err}"),
        }
    }

    fn open_plugin_manager_external_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.plugin_manager_window.clone() {
            if handle
                .update(cx, |_pm, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.plugin_manager_window = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.open_popover = None;
        self.text_context_menu = None;

        let owner_bounds = owner_bounds.unwrap_or_else(|| Bounds {
            origin: gpui::Point::default(),
            size: gpui::size(px(1400.0), px(900.0)),
        });

        match open_plugin_manager_window(owner_bounds, cx) {
            Ok(handle) => self.plugin_manager_window = Some(handle),
            Err(err) => eprintln!("[plugin-manager] failed to open window: {err}"),
        }
    }

    fn open_settings_dialog(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        // If window is already open, activate it
        if let Some(handle) = self.settings_window.clone() {
            if handle
                .update(cx, |_settings, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.settings_window = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.open_popover = None;
        self.project_switcher.is_open = false;
        self.text_context_menu = None;

        let owner_bounds = owner_bounds.unwrap_or_else(|| Bounds {
            origin: gpui::Point::default(),
            size: gpui::size(px(1400.0), px(900.0)),
        });
        let settings = self.settings.clone();
        let owner = cx.entity().clone();

        let mut available_inputs = if let Some(ref engine) = self.audio_engine {
            engine.list_input_devices().into_iter().map(|d| d.name).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let schema = self.settings.read(cx).current.clone();
        if !available_inputs.contains(&schema.hardware.audio.device_in)
            && !schema.hardware.audio.device_in.is_empty()
        {
            available_inputs.push(schema.hardware.audio.device_in.clone());
        }
        if available_inputs.is_empty() {
            available_inputs.push("Built-in Microphone".to_string());
        }

        let mut available_outputs = if let Some(ref engine) = self.audio_engine {
            engine.list_output_devices().into_iter().map(|d| d.name).collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if !available_outputs.contains(&schema.hardware.audio.device_out)
            && !schema.hardware.audio.device_out.is_empty()
        {
            available_outputs.push(schema.hardware.audio.device_out.clone());
        }
        if available_outputs.is_empty() {
            available_outputs.push("Speakers (Realtek)".to_string());
        }

        let available_backends = vec![
            "WASAPI Exclusive".to_string(),
            "WASAPI Shared".to_string(),
            "ASIO".to_string(),
        ];

        let on_update: OnSettingUpdate = Arc::new(move |updater, cx| {
            let updater = updater.clone();
            let _ = owner.update(cx, |this, cx| {
                let _ = this.settings.update(cx, |settings, cx| {
                    settings.update_setting(move |s| updater(s), cx);
                });
                this.sync_settings_to_systems(cx);
                cx.notify();
            });
        });

        match open_settings_window(
            owner_bounds,
            settings,
            available_inputs,
            available_outputs,
            available_backends,
            on_update,
            cx,
        ) {
            Ok(handle) => self.settings_window = Some(handle),
            Err(err) => eprintln!("[settings] failed to open settings window: {err}"),
        }
    }

    fn close_settings_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window.take() {
            let _ = handle.update(cx, |_settings, window, _cx| window.remove_window());
        }
        self.text_context_menu = None;
        cx.notify();
    }

    pub(crate) fn notify_mixer_window(&mut self, cx: &mut Context<Self>) {
        self.push_mixer_snapshot_to_window(cx);
    }

    pub(crate) fn build_mixer_snapshot(&self, cx: &gpui::App) -> MixerSnapshot {
        let timeline = self.timeline.read(cx);
        MixerSnapshot {
            tracks: timeline.state.tracks.clone(),
            master: timeline.state.master.clone(),
            selected_track_id: timeline.state.selection.selected_track_id.clone(),
            mixer_scroll_x: self.mixer_scroll_x,
        }
    }

    pub(crate) fn mixer_view_state(
        &self,
        cx: &gpui::App,
    ) -> (
        Vec<TrackState>,
        timeline_state::MasterBusState,
        Option<String>,
        f32,
    ) {
        let snapshot = self.build_mixer_snapshot(cx);
        (
            snapshot.tracks,
            snapshot.master,
            snapshot.selected_track_id,
            snapshot.mixer_scroll_x,
        )
    }

    pub(crate) fn push_mixer_snapshot_to_window(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.mixer_window.clone() else {
            return;
        };
        let snapshot = self.build_mixer_snapshot(cx);
        let _ = handle.update(cx, |mixer, _window, cx| {
            mixer.set_snapshot(snapshot);
            cx.notify();
        });
    }

    pub(crate) fn set_mixer_scroll_x(&mut self, scroll_x: f32, _cx: &mut Context<Self>) -> bool {
        if (self.mixer_scroll_x - scroll_x).abs() > 0.25 {
            self.mixer_scroll_x = scroll_x;
            true
        } else {
            false
        }
    }

    pub(crate) fn mixer_window_handle(&self) -> Option<WindowHandle<MixerWindow>> {
        self.mixer_window.clone()
    }

    fn prune_mixer_window(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.mixer_window.clone() else {
            return;
        };
        if handle
            .update(cx, |_mixer, _window, _cx| ())
            .is_err()
        {
            self.mixer_window = None;
        }
    }

    fn mixer_panel_chrome_visible(&self) -> bool {
        self.panels.mixer_docked || self.mixer_window.is_some()
    }

    pub(crate) fn toggle_browser_panel(&mut self, cx: &mut Context<Self>) {
        self.panels.browser = !self.panels.browser;
        cx.notify();
    }

    pub(crate) fn toggle_inspector_panel(&mut self, cx: &mut Context<Self>) {
        self.panels.inspector = !self.panels.inspector;
        cx.notify();
    }

    pub(crate) fn toggle_mixer_panel(&mut self, cx: &mut Context<Self>) {
        if self.mixer_window.is_some() {
            self.close_mixer_window(cx);
            self.panels.mixer_docked = true;
        } else {
            self.panels.mixer_docked = !self.panels.mixer_docked;
        }
        cx.notify();
    }

    pub(crate) fn open_mixer_external_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        external_mixer_debug("external mixer open requested");
        self.pending_mixer_external_open = Some(owner_bounds.unwrap_or_else(|| Bounds {
            origin: gpui::Point::default(),
            size: gpui::size(px(1400.0), px(900.0)),
        }));
        self.schedule_pending_mixer_external_open(cx);
        cx.notify();
    }

    fn schedule_pending_mixer_external_open(&mut self, cx: &mut Context<Self>) {
        if self.pending_mixer_external_open.is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(0))
                .await;
            let _ = this.update(cx, |layout, cx| layout.flush_pending_mixer_external_open(cx));
        })
        .detach();
    }

    fn flush_pending_mixer_external_open(&mut self, cx: &mut Context<Self>) {
        let Some(owner_bounds) = self.pending_mixer_external_open.take() else {
            return;
        };

        self.prune_mixer_window(cx);
        if let Some(handle) = self.mixer_window.clone() {
            if handle
                .update(cx, |_mixer, window, _cx| window.activate_window())
                .is_ok()
            {
                self.panels.mixer_docked = false;
                self.push_mixer_snapshot_to_window(cx);
                cx.notify();
                return;
            }
            self.mixer_window = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.open_popover = None;
        self.panels.mixer_docked = false;

        let snapshot = self.build_mixer_snapshot(cx);
        let callbacks = self.build_mixer_callbacks(cx.entity().clone());
        let owner = cx.entity().clone();
        let on_close: std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + Send + Sync> =
            std::sync::Arc::new(move |_window, cx| {
                let _ = owner.update(cx, |layout, cx| layout.close_mixer_window(cx));
            });
        let scroll_owner = cx.entity().clone();
        let on_mixer_scroll: std::sync::Arc<dyn Fn(f32, &mut Window, &mut gpui::App) + Send + Sync> =
            std::sync::Arc::new(move |new_x: f32, _w, cx| {
                let _ = scroll_owner.update(cx, |layout, cx| {
                    if layout.set_mixer_scroll_x(new_x, cx) {
                        layout.push_mixer_snapshot_to_window(cx);
                    }
                });
            });

        match open_mixer_window(
            owner_bounds,
            snapshot,
            callbacks,
            on_close,
            on_mixer_scroll,
            cx,
        ) {
            Ok(handle) => {
                self.mixer_window = Some(handle);
                cx.notify();
            }
            Err(err) => {
                eprintln!("[mixer] failed to open external mixer window: {err}");
                self.panels.mixer_docked = true;
                cx.notify();
            }
        }
    }

    pub(crate) fn close_mixer_window(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.mixer_window.take() {
            let _ = handle.update(cx, |_mixer, window, _cx| window.remove_window());
        }
        cx.notify();
    }

    fn panel_chrome_state(&self, cx: &mut Context<Self>) -> components::PanelChromeState {
        let make_handler = |command_id: &'static str| {
            let this = cx.entity().clone();
            Arc::new(move |_: &(), _window: &mut Window, cx: &mut gpui::App| {
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id(command_id, cx);
                    cx.notify();
                });
            })
        };
        components::PanelChromeState {
            browser_visible: self.panels.browser,
            inspector_visible: self.panels.inspector,
            mixer_visible: self.mixer_panel_chrome_visible(),
            on_toggle_browser: make_handler("panel:toggle-browser"),
            on_toggle_mixer: make_handler("panel:toggle-mixer"),
            on_toggle_inspector: make_handler("panel:toggle-inspector"),
        }
    }

    fn sync_settings_to_systems(&mut self, cx: &mut Context<Self>) {
        let schema = self.settings.read(cx).current.clone();

        // 1. Sync metronome enabled state
        let _ = self.timeline.update(cx, |timeline, _cx| {
            timeline.state.transport.metronome_enabled = schema.recording.metronome.enabled;
        });
        self.sync_metronome_controls(cx);

        // 2. Sync audio engine settings
        self.sync_audio_engine_settings(cx);
    }

    fn sync_audio_engine_settings(&mut self, cx: &mut Context<Self>) {
        let schema = self.settings.read(cx).current.clone();
        
        let mut rebuild = false;
        if let Some(ref engine) = self.audio_engine {
            let config = engine.config();
            let desired_backend = match schema.hardware.audio.driver_type.as_str() {
                "WASAPI Exclusive" => DAUx::AudioBackend::WasapiExclusive,
                _ => DAUx::AudioBackend::Auto,
            };
            if config.backend != desired_backend
                || config.sample_rate != schema.general.project_defaults.sample_rate
                || config.buffer_size != schema.general.project_defaults.buffer_size
            {
                rebuild = true;
            }
        } else {
            rebuild = true;
        }

        if rebuild {
            eprintln!("[audio] settings changed, rebuilding audio engine stream...");
            
            // Stop and release active engine
            if let Some(mut engine) = self.audio_engine.take() {
                let _ = engine.stop();
            }

            // Construct new config
            let backend = match schema.hardware.audio.driver_type.as_str() {
                "WASAPI Exclusive" => DAUx::AudioBackend::WasapiExclusive,
                _ => DAUx::AudioBackend::Auto,
            };
            let config = DAUx::EngineConfig {
                sample_rate: schema.general.project_defaults.sample_rate,
                buffer_size: schema.general.project_defaults.buffer_size,
                channels: 2,
                backend,
            };

            // Build new engine
            match DAUx::AudioEngine::new(config) {
                Ok(mut engine) => {
                    match engine.start() {
                        Ok(()) => {
                            let stats = engine.stats();
                            eprintln!(
                                "[audio] settings sync: stream rebuilt and started. backend={} sr={} buf={}",
                                stats.backend_name, stats.sample_rate, stats.buffer_size
                            );
                            
                            // Re-bind timeline callbacks
                            let seek_engine = engine.clone();
                            let param_engine = engine.clone();
                            let _ = self.timeline.update(cx, |timeline, _cx| {
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

                            self.audio_engine = Some(engine);
                            self.audio_running = true;
                            self.audio_last_error = None;
                        }
                        Err(error) => {
                            eprintln!("[audio] settings sync: warm-up failed: {error}");
                            self.audio_last_error = Some(error.to_string());
                        }
                    }
                }
                Err(error) => {
                    eprintln!("[audio] settings sync: failed to initialize engine: {error}");
                    self.audio_last_error = Some(error.to_string());
                }
            }
        }
    }

    // Add Track is now an external window that owns its own state.

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
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_mute(&id);
                cx.notify();
            }
        });
    }

    fn toggle_selected_track_solo(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_solo(&id);
                cx.notify();
            }
        });
    }

    fn toggle_selected_track_arm(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_arm(&id);
                cx.notify();
            }
        });
    }

    fn reset_selected_track_volume(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
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
        self.mark_dirty();
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
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.project_switcher.is_open {
            return false;
        }
        if event.is_held {
            return true;
        }
        let key = event.keystroke.key.as_str();
        if self.text_context_menu.take().is_some() && key == "escape" {
            cx.notify();
            return true;
        }

        let search_focused = self.project_switcher_search_input.is_focused(window);
        match key {
            "escape" => {
                self.project_switcher.is_open = false;
                self.text_context_menu = None;
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
                if search_focused || is_text_input_key(event) {
                    let action = self
                        .project_switcher_search_input
                        .handle_key_with_clipboard(event, Some(cx));
                    self.sync_text_input_target(TextMenuTarget::ProjectSwitcherSearch);
                    return !matches!(action, TextInputAction::Pass);
                }
                false
            }
        }
    }

    fn handle_browser_key(
        &mut self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let search_focused = self.browser_search_input.is_focused(window);
        if !search_focused {
            return false;
        }
        if event.is_held {
            return true;
        }
        let key = event.keystroke.key.as_str();
        if self.text_context_menu.take().is_some() && key == "escape" {
            cx.notify();
            return true;
        }

        match key {
            "arrow_down" | "down" => {
                self.file_browser.select_next();
                cx.notify();
                true
            }
            "arrow_up" | "up" => {
                self.file_browser.select_previous();
                cx.notify();
                true
            }
            "arrow_left" | "left" => {
                self.file_browser.collapse_selected_or_parent();
                let pending = self.file_browser.paths_needing_load();
                for p in pending {
                    self.file_browser.mark_loading(p.clone());
                    Self::spawn_directory_load(cx, p);
                }
                cx.notify();
                true
            }
            "arrow_right" | "right" => {
                self.file_browser.expand_selected();
                let pending = self.file_browser.paths_needing_load();
                for p in pending {
                    self.file_browser.mark_loading(p.clone());
                    Self::spawn_directory_load(cx, p);
                }
                cx.notify();
                true
            }
            "enter" | "numpad_enter" => {
                if let Some(selected_path) = self.file_browser.selected.clone() {
                    if selected_path.is_dir() {
                        let id = selected_path.to_string_lossy().to_string();
                        let expanded = self.file_browser.toggle_node(&id, Some(&selected_path));
                        if expanded {
                            let pending = self.file_browser.paths_needing_load();
                            for p in pending {
                                self.file_browser.mark_loading(p.clone());
                                Self::spawn_directory_load(cx, p);
                            }
                        }
                    } else {
                        let ext = selected_path
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_ascii_lowercase())
                            .unwrap_or_default();
                        if is_supported_audio_ext(&ext) {
                            let timeline = self.timeline.clone();
                            let layout = cx.entity().clone();
                            let path = selected_path.clone();
                            let path_for_decode = path.clone();
                            let timeline_for_decode = timeline.clone();
                            timeline.update(cx, |t, cx| {
                                let path_key = path.to_string_lossy().to_string();
                                let name = path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| "Imported Audio".to_string());
                                t.state.import_audio_to_selected_or_new_track(path_key, name);
                                cx.notify();
                            });
                            let _ = layout.update(cx, |this, cx| {
                                this.mark_dirty();
                                this.mark_engine_media_dirty();
                                this.schedule_audio_project_sync(cx, false, "browser_import");
                            });
                            let path_key = path_for_decode.to_string_lossy().to_string();
                            let owner = layout.clone();
                            let _ = layout.update(cx, move |_layout, cx| {
                                Self::spawn_timeline_audio_import_jobs(
                                    cx,
                                    owner,
                                    timeline_for_decode,
                                    path_for_decode,
                                    path_key,
                                );
                            });
                        }
                    }
                }
                true
            }
            _ => {
                if search_focused || is_text_input_key(event) {
                    let action = self
                        .browser_search_input
                        .handle_key_with_clipboard(event, Some(cx));
                    self.sync_text_input_target(TextMenuTarget::BrowserSearch);
                    return !matches!(action, TextInputAction::Pass);
                }
                false
            }
        }
    }

    fn handle_settings_dialog_key(
        &mut self,
        _event: &KeyDownEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> bool {
        // Settings is now an external window that handles its own keyboard events.
        false
    }

    fn handle_add_track_dialog_key(
        &mut self,
        _event: &KeyDownEvent,
        _window: &Window,
        _cx: &mut Context<Self>,
    ) -> bool {
        // Add Track is now an external window that handles its own keyboard events.
        false
    }

    fn text_input_mut(&mut self, target: TextMenuTarget) -> &mut TextInputState {
        match target {
            TextMenuTarget::ProjectSwitcherSearch => &mut self.project_switcher_search_input,
            TextMenuTarget::BrowserSearch => &mut self.browser_search_input,
        }
    }

    fn text_input(&self, target: TextMenuTarget) -> &TextInputState {
        match target {
            TextMenuTarget::ProjectSwitcherSearch => &self.project_switcher_search_input,
            TextMenuTarget::BrowserSearch => &self.browser_search_input,
        }
    }

    fn sync_text_input_target(&mut self, target: TextMenuTarget) {
        match target {
            TextMenuTarget::ProjectSwitcherSearch => {
                self.project_switcher.query = self.project_switcher_search_input.value.clone();
                self.project_switcher.selected_index = 0;
            }
            TextMenuTarget::BrowserSearch => {
                self.file_browser.set_filter(&self.browser_search_input.value);
            }
        }
    }

    fn text_input_has_focus(&self, window: &Window) -> bool {
        self.project_switcher_search_input.is_focused(window)
            || self.browser_search_input.is_focused(window)
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
            ContextTarget::Browser(path_opt) => {
                let mut entries = Vec::new();
                if let Some(path) = path_opt {
                    if path.is_dir() {
                        let is_drive = path.parent().is_none();
                        if is_drive {
                            entries.push(ContextMenuEntry::item("Open Folder", "browser:reveal"));
                            entries.push(ContextMenuEntry::item("Refresh", "browser:refresh"));
                        } else {
                            entries.push(ContextMenuEntry::item("Open", "browser:open"));
                            entries.push(ContextMenuEntry::item("Reveal in Explorer/Finder", "browser:reveal"));
                            entries.push(ContextMenuEntry::item("Refresh", "browser:refresh"));
                            entries.push(ContextMenuEntry::disabled_item("New Folder", "browser:new-folder"));
                            entries.push(ContextMenuEntry::disabled_item("Rename", "browser:rename"));
                            entries.push(ContextMenuEntry::item("Copy Path", "browser:copy-path"));
                        }
                    } else {
                        let ext = path
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_ascii_lowercase())
                            .unwrap_or_default();
                        
                        if is_supported_audio_ext(&ext) {
                            entries.push(ContextMenuEntry::item("Import to Timeline", "browser:import"));
                            entries.push(ContextMenuEntry::item("Reveal in Explorer/Finder", "browser:reveal"));
                            entries.push(ContextMenuEntry::item("Copy Path", "browser:copy-path"));
                            entries.push(ContextMenuEntry::disabled_item("Rename", "browser:rename"));
                        } else if ext == "fbproj" {
                            entries.push(ContextMenuEntry::item("Open Project", "project:open"));
                            entries.push(ContextMenuEntry::item("Reveal in Explorer/Finder", "browser:reveal"));
                            entries.push(ContextMenuEntry::item("Copy Path", "browser:copy-path"));
                        } else {
                            entries.push(ContextMenuEntry::item("Reveal in Explorer/Finder", "browser:reveal"));
                            entries.push(ContextMenuEntry::item("Copy Path", "browser:copy-path"));
                        }
                    }
                } else {
                    entries.push(ContextMenuEntry::disabled_item("No file selected", "noop"));
                }
                entries
            }
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
        owner: Entity<Self>,
        timeline: Entity<components::timeline::Timeline>,
        path: PathBuf,
        _path_key: String,
    ) {
        components::timeline::audio_import::spawn_timeline_import_from_layout(
            path,
            timeline,
            owner,
            cx,
        );
    }

    /// Build the callback bundle used by the mixer. Every mutation lands in
    /// the same `TimelineState` instance owned by the Timeline entity, so the
    /// TrackHeader and Mixer always read identical values.
    pub(crate) fn build_mixer_callbacks(&self, owner: Entity<Self>) -> MixerCallbacks {
        let audio_engine = self.audio_engine.clone();
        let timeline_select = self.timeline.clone();
        let owner_select = owner.clone();
        let on_select_track: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |id: &String, _w, cx| {
            let id = id.clone();
            external_mixer_debug(&format!("mixer command dispatched select_track id={id}"));
            timeline_select.update(cx, |t, cx| {
                t.state.select_track(&id);
                cx.notify();
            });
            let _ = owner_select.update(cx, |layout, cx| {
                layout.push_mixer_snapshot_to_window(cx);
            });
        });

        let timeline_vol = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_volume_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
            let id = id.clone();
            let v = *v;
            external_mixer_debug(&format!("mixer command dispatched set_volume id={id} v={v:.3}"));
            timeline_vol.update(cx, |t, cx| {
                t.state.set_track_volume(&id, v);
                cx.notify();
            });
            let _ = owner_dirty.update(cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "volume", volume_norm_to_linear(v) as f64);
            }
        });

        let audio_engine = self.audio_engine.clone();
        let timeline_pan = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_pan_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
            let id = id.clone();
            let v = *v;
            external_mixer_debug(&format!("mixer command dispatched set_pan id={id} v={v:.3}"));
            timeline_pan.update(cx, |t, cx| {
                t.state.set_track_pan(&id, v);
                cx.notify();
            });
            let _ = owner_dirty.update(cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "pan", v as f64);
            }
        });

        let audio_engine = self.audio_engine.clone();
        let timeline_mute = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_toggle_mute: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                let mut muted = false;
                external_mixer_debug(&format!("mixer command dispatched toggle_mute id={id}"));
                timeline_mute.update(cx, |t, cx| {
                    t.state.toggle_track_mute(&id);
                    muted = t
                        .state
                        .find_track(&id)
                        .map(|track| track.muted)
                        .unwrap_or(false);
                    cx.notify();
                });
                let _ = owner_dirty.update(cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
                if let Some(engine) = audio_engine.as_ref() {
                    let _ = engine.update_track_param(&id, "mute", if muted { 1.0 } else { 0.0 });
                }
            });

        let audio_engine = self.audio_engine.clone();
        let timeline_solo = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_toggle_solo: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                let mut solo = false;
                external_mixer_debug(&format!("mixer command dispatched toggle_solo id={id}"));
                timeline_solo.update(cx, |t, cx| {
                    t.state.toggle_track_solo(&id);
                    solo = t
                        .state
                        .find_track(&id)
                        .map(|track| track.solo)
                        .unwrap_or(false);
                    cx.notify();
                });
                let _ = owner_dirty.update(cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
                if let Some(engine) = audio_engine.as_ref() {
                    let _ = engine.update_track_param(&id, "solo", if solo { 1.0 } else { 0.0 });
                }
            });

        let timeline_arm = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                external_mixer_debug(&format!("mixer command dispatched toggle_arm id={id}"));
                timeline_arm.update(cx, |t, cx| {
                    t.state.toggle_track_arm(&id);
                    cx.notify();
                });
                let _ = owner_dirty.update(cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
            });

        let timeline_input = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_toggle_input: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |id: &String, _w, cx| {
            let id = id.clone();
            external_mixer_debug(&format!("mixer command dispatched toggle_input id={id}"));
            timeline_input.update(cx, |t, cx| {
                t.state.toggle_track_input_monitor(&id);
                cx.notify();
            });
            let _ = owner_dirty.update(cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
        });

        let audio_engine = self.audio_engine.clone();
        let timeline_master = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_master_volume_change: std::sync::Arc<
            dyn Fn(&f32, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |v: &f32, _w, cx| {
            let v = *v;
            external_mixer_debug(&format!("mixer command dispatched master_volume v={v:.3}"));
            timeline_master.update(cx, |t, cx| {
                t.state.set_master_volume(v);
                cx.notify();
            });
            let _ = owner_dirty.update(cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
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
                    if this.set_mixer_scroll_x(new_x, cx) {
                        this.push_mixer_snapshot_to_window(cx);
                        cx.notify();
                    }
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
        let on_resize_end = cx.listener(|this, _event: &gpui::MouseUpEvent, _window, cx| {
            if this.bottom_panel_state.is_resizing {
                this.bottom_panel_state.is_resizing = false;
                cx.notify();
            }
        });

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
        let on_browser_search_context: std::sync::Arc<
            dyn Fn(&(f32, f32), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |(x, y): &(f32, f32), _w, cx| {
                let x = *x;
                let y = *y;
                let _ = this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.project_switcher.is_open = false;
                    this.text_context_menu = Some(TextContextMenu {
                        target: TextMenuTarget::BrowserSearch,
                        x,
                        y,
                    });
                    cx.notify();
                });
            })
        };

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
                let _ = layout.update(cx, |this, cx| {
                    this.mark_dirty();
                    this.mark_engine_media_dirty();
                    this.schedule_audio_project_sync(cx, false, "timeline_audio_import");
                });
                let path_key = path_for_decode.to_string_lossy().to_string();
                let owner = layout.clone();
                let _ = layout.update(cx, move |_layout, cx| {
                    Self::spawn_timeline_audio_import_jobs(
                        cx,
                        owner,
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
                    // Timeline requests originate while Timeline may already be mid-update.
                    // Use the request context to avoid a nested `timeline.update(...)`.
                    this.open_add_track_external_window_with_context(
                        AddTrackKind::Audio,
                        request.track_count,
                        request.has_master_track,
                        None,
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
                        this.menu_bar.anchor = titlebar_label_anchor(anchor_x);
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
            std::sync::Arc::new(move |command: &String, w, cx| {
                let command = command.clone();
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id_from_bounds(&command, Some(w.bounds()), cx);
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    cx.notify();
                });
            })
        };
        let on_project_open: std::sync::Arc<dyn Fn(&f32, &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |anchor_x: &f32, w, cx| {
                let anchor_x = *anchor_x;
                let _ = this.update(cx, |this, cx| {
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.open_popover = None;
                    this.text_context_menu = None;
                    this.project_switcher.is_open = !this.project_switcher.is_open;
                    this.project_switcher.anchor = project_title_anchor(anchor_x);
                    if this.project_switcher.is_open {
                        this.project_switcher.query.clear();
                        this.project_switcher_search_input.set_value("");
                        this.project_switcher_search_input.focus_handle.focus(w);
                        this.project_switcher.selected_index = 0;
                    }
                    cx.notify();
                });
            })
        };

        let open_menu_id = self.menu_bar.open_menu_id.clone();
        let menu_anchor = self.menu_bar.anchor;
        let submenu_path = self.menu_bar.submenu_path.clone();
        let viewport_width: f32 = window.bounds().size.width.into();
        let viewport_height: f32 = window.bounds().size.height.into();

        let chrome_policy = crate::platform_chrome::PlatformChromePolicy::current();
        let dropdown_overlay = if chrome_policy.show_in_window_menubar {
            open_menu_id.as_ref().and_then(|id| {
                if id == components::menu_bar::MENU_PICKER_ID {
                    Some(
                        components::menu_bar::menu_picker_dropdown(
                            menu_anchor,
                            viewport_width,
                            viewport_height,
                            on_open_menu.clone(),
                            on_close_menu.clone(),
                        )
                        .into_any_element(),
                    )
                } else {
                    let manifest = crate::menu::MenuManifest::load();
                    manifest.menus.iter().find(|m| &m.id == id).map(|menu| {
                        components::menu_dropdown::menu_dropdown(
                            menu,
                            menu_anchor,
                            viewport_width,
                            viewport_height,
                            &submenu_path,
                            on_toggle_submenu.clone(),
                            on_menu_command.clone(),
                            on_close_menu.clone(),
                        )
                        .into_any_element()
                    })
                }
            })
        } else {
            None
        };
        let on_close_popover: std::sync::Arc<dyn Fn(&(), &mut Window, &mut gpui::App) + 'static> = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |_: &(), _w, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    this.text_context_menu = None;
                    cx.notify();
                });
            })
        };
        let on_popover_command: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = cx.entity().clone();
            std::sync::Arc::new(move |command: &String, w, cx| {
                let command = command.clone();
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id_from_bounds(&command, Some(w.bounds()), cx);
                    this.open_popover = None;
                    this.project_switcher.is_open = false;
                    cx.notify();
                });
            })
        };
        let popover_overlay = if self.project_switcher.is_open {
            let search_context_callbacks = TextInputCallbacks {
                on_context_menu: Some(Arc::new({
                    let this = cx.entity().clone();
                    move |(x, y): &(f32, f32), _w, cx| {
                        let x = *x;
                        let y = *y;
                        let _ = this.update(cx, |this, cx| {
                            this.text_context_menu = Some(TextContextMenu {
                                target: TextMenuTarget::ProjectSwitcherSearch,
                                x,
                                y,
                            });
                            cx.notify();
                        });
                    }
                })),
            };
            Some(
                components::project_switcher::project_switcher_popover(
                    &self.project_switcher,
                    &self.project_switcher_search_input,
                    self.project_switcher_search_input.is_focused(window),
                    search_context_callbacks,
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
        // Settings is now an external window — no overlay needed.
        let settings_overlay: Option<gpui::AnyElement> = None;
        let text_context_overlay = self.text_context_menu.map(|menu| {
            let clipboard_has_text = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .is_some_and(|text| !text.is_empty());
            let entries =
                text_input_context_entries(self.text_input(menu.target), clipboard_has_text);
            let command_target = cx.entity().clone();
            let close_target = cx.entity().clone();
            components::context_menu::context_menu_overlay(
                entries,
                menu.x,
                menu.y,
                viewport_width,
                viewport_height,
                Arc::new(move |command: &String, _window, cx| {
                    let command = command.clone();
                    let _ = command_target.update(cx, |this, cx| {
                        if let Some(menu) = this.text_context_menu {
                            let input = this.text_input_mut(menu.target);
                            let _ = input.apply_context_command(&command, cx);
                            this.sync_text_input_target(menu.target);
                        }
                        this.text_context_menu = None;
                        cx.notify();
                    });
                }),
                Arc::new(move |_: &(), _window, cx| {
                    let _ = close_target.update(cx, |this, cx| {
                        this.text_context_menu = None;
                        cx.notify();
                    });
                }),
            )
        });
        // Add Track moved to an external window.

        self.prune_mixer_window(cx);

        let transport_chrome = self.transport_chrome_state(cx);
        let panel_chrome = self.panel_chrome_state(cx);
        let show_browser = self.panels.browser;
        let show_inspector = self.panels.inspector;
        let show_mixer_docked = self.panels.mixer_docked;
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
                let handled = shortcut_target.update(cx, |this, cx| {
                    let handled = this.handle_settings_dialog_key(event, window, cx)
                        || this.handle_add_track_dialog_key(event, window, cx)
                        || this.handle_project_switcher_key(event, window, cx)
                        || this.handle_browser_key(event, window, cx);
                    if handled {
                        cx.notify();
                    }
                    handled
                });
                if handled {
                    return;
                }
                if shortcut_target.read(cx).text_input_has_focus(window) && is_text_input_key(event)
                {
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
                        this.text_context_menu = None;
                        this.project_switcher.is_open = false;
                        cx.notify();
                    });
                    return;
                }
                if let Some(command_id) = Self::shortcut_command_id(event) {
                    let _ = shortcut_target.update(cx, |this, cx| {
                        this.dispatch_command_id_from_bounds(command_id, Some(window.bounds()), cx);
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
                    panel_chrome,
                )
            })
            .child({
                let mut main_row = div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0();
                if show_browser {
                    main_row = main_row.child({
                        let _s = crate::perf::PerfScope::enter("Sidebar");
                        components::sidebar(
                            &file_browser,
                            browser_scroll,
                            &self.browser_search_input,
                            self.browser_search_input.is_focused(window),
                            on_browser_search_context,
                            on_browser_toggle,
                            on_browser_select,
                            on_browser_activate,
                            on_browser_context,
                        )
                    });
                }
                main_row = main_row.child(self.timeline.clone());
                if show_inspector {
                    main_row = main_row.child({
                        let _s = crate::perf::PerfScope::enter("Inspector");
                        crate::components::panel::inspector_panel(
                            &tracks,
                            selected_track_id.as_deref(),
                            selected_clip_id.as_deref(),
                            find_clip_summary(&tracks, selected_clip_id.as_deref()),
                        )
                    });
                }
                main_row
            })
            .children(if show_mixer_docked {
                let _s = crate::perf::PerfScope::enter("BottomPanel");
                Some(
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
                        on_resize_end,
                    )
                    .into_any_element(),
                )
            } else {
                None
            })
            .child({
                let _s = crate::perf::PerfScope::enter("StatusBar");
                components::status_bar(status_left, status_right)
            })
            // Dropdown overlay — rendered last so it sits above every other
            // panel. The dropdown's own backdrop captures click-outside.
            .children(dropdown_overlay)
            .children(popover_overlay)
            // Add Track moved to external window.
            .children(settings_overlay)
            .children(text_context_overlay)
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
        volume: volume_norm_to_linear(state.master.volume),
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

fn log_engine_sync_snapshot(snapshot: &EngineProjectSnapshot, dirty: bool, reason: &'static str) {
    let clips_with_path = snapshot
        .clips
        .iter()
        .filter(|clip| {
            clip.media_path
                .as_deref()
                .map(|path| !path.trim().is_empty())
                .unwrap_or(false)
        })
        .count();
    eprintln!(
        "[engine-sync] reason={} tracks={} clips={} clips_with_path={} dirty={}",
        reason,
        snapshot.tracks.len(),
        snapshot.clips.len(),
        clips_with_path,
        dirty
    );
    for clip in &snapshot.clips {
        eprintln!(
            "[engine-sync] clip id={} track={} path={} start={:.3} duration={:.3}",
            clip.id,
            clip.track_id,
            clip.media_path.as_deref().unwrap_or("<none>"),
            clip.start_beat,
            clip.duration_beats
        );
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

fn is_text_input_key(event: &KeyDownEvent) -> bool {
    let key = event.keystroke.key.as_str();
    let mods = event.keystroke.modifiers;
    if (mods.control || mods.platform) && !mods.alt && !mods.function {
        return matches!(key, "a" | "A" | "c" | "C" | "v" | "V" | "x" | "X");
    }
    if mods.control || mods.alt || mods.platform || mods.function {
        return false;
    }
    matches!(
        key,
        "backspace"
            | "delete"
            | "left"
            | "arrow_left"
            | "right"
            | "arrow_right"
            | "home"
            | "end"
            | "space"
    ) || key.chars().count() == 1
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
    let stem = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end();
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

fn reveal_path(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        if path.is_file() {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,\"{}\"", path.display()))
                .spawn();
        } else {
            let _ = std::process::Command::new("explorer")
                .arg(format!("\"{}\"", path.display()))
                .spawn();
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(if path.is_file() { "-R" } else { "" })
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let parent = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        let _ = std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn();
    }
}
