use gpui::{
    div, px, AppContext, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, Styled, UniformListScrollHandle, Window, WindowHandle,
};

use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Instant};

use crate::components;
use crate::components::add_track_dialog::{AddTrackKind, AddTrackWindow};
use crate::components::file_browser::FileBrowserState;
use crate::components::midi_editor_window::MidiEditorWindow;
use crate::components::plugin_editor_window::PluginEditorWindow;
use crate::components::plugin_manager::PluginManagerWindow;
use crate::components::plugin_picker::{
    compute_filter_result, ensure_default_highlight, plugin_picker_overlay,
    CatalogStatus as PluginCatalogStatus, PickerFilter, PluginPickerCallbacks, PluginPickerPrefs,
    PluginPickerState, PluginSearchIndex,
};
use crate::components::project_switcher::ProjectSwitcherState;
use crate::components::project_wizard::ProjectWizardWindow;
use crate::components::settings_dialog::SettingsWindow;
use crate::components::text_input::{
    text_input_context_entries, TextInputCallbacks, TextInputState,
};
use crate::components::timeline::timeline::TimelineContextTarget;
use crate::components::timeline::timeline_state::ClipType;
use crate::components::MixerWindow;
use crate::components::{BottomPanelResizeDrag, BottomPanelState};
use crate::overlay::{project_title_anchor, titlebar_label_anchor};
use crate::paths::FutureboardPaths;
use crate::project::recent::RecentProjectsStore;
use crate::settings::{GlobalSettingsModel, SettingsModel, SettingsSchema};
use crate::theme::{self, Colors};
use sphere_plugin_host::load_au_cache_state;

mod audio_transport;
mod browser_ops;
mod engine_snapshot;
mod frame_diagnostics;
mod helpers;
mod input_ops;
mod mixer_ops;
mod plugin_ops;
mod project_ops;
mod studio_render;
mod studio_state;
mod track_clip_ops;
mod transport_ops;
mod window_ops;

use engine_snapshot::volume_norm_to_linear;
use frame_diagnostics::FrameDiagnostics;
use helpers::{
    find_clip_summary, is_supported_audio_ext, is_text_input_key, key_debug, normalize_command_id,
    reveal_path, should_handle_global_transport_shortcut, transport_command_from_id, FocusContext,
};
pub use studio_state::{ContextTarget, MenuBarUiState, OpenPopover, StudioPanelVisibility};
use studio_state::{TextContextMenu, TextMenuTarget, TransportCommand};

/// Flip to `true` to seed the studio with demo tracks/clips at startup.
/// Production builds must keep this `false` — the real app starts empty.
const USE_DEMO_PROJECT: bool = false;

/// Notify a satellite window's root view without calling `Entity::update` (which
/// can re-enter the main studio entity and trip GPUI's lease checks).
pub(crate) fn notify_window_root<T: gpui::Render>(app: &mut gpui::App, handle: &WindowHandle<T>) {
    if let Ok(entity) = handle.entity(app) {
        app.notify(entity.entity_id());
    }
}

pub struct StudioLayout {
    active_bottom_tab: components::BottomTab,
    bottom_panel_state: BottomPanelState,
    timeline: Entity<components::timeline::Timeline>,
    /// Piano-roll editor for MIDI clips in the bottom panel router.
    piano_roll: Entity<components::piano_roll::PianoRoll>,
    /// Audio clip editor for the bottom panel router.
    audio_editor: Entity<components::AudioEditorHost>,
    /// Routes bottom Editor tab between audio / MIDI / empty state.
    clip_editor_panel: Entity<components::ClipEditorPanel>,
    /// Second piano-roll instance for the floating MIDI editor (same timeline).
    piano_roll_floating: Entity<components::piano_roll::PianoRoll>,
    /// Global floating MIDI editor window (one instance; switches clip on open).
    midi_editor_window: Option<WindowHandle<MidiEditorWindow>>,
    pending_midi_editor_open: Option<Bounds<gpui::Pixels>>,
    file_browser: FileBrowserState,
    /// Stable scroll handle for the browser tree. Lives on the layout
    /// (not in `FileBrowserState`) so the state stays free of gpui types
    /// and so the handle survives across renders.
    browser_scroll: UniformListScrollHandle,
    menu_bar: MenuBarUiState,
    project_switcher: ProjectSwitcherState,
    project_switcher_search_input: TextInputState,
    browser_search_input: TextInputState,
    /// Phase 2b insert plugin picker overlay state.
    plugin_picker: PluginPickerState,
    plugin_picker_search_input: TextInputState,
    plugin_picker_prefs: PluginPickerPrefs,
    plugin_search_index: Option<PluginSearchIndex>,
    plugin_picker_au_error: Option<String>,
    add_track_window: Option<WindowHandle<AddTrackWindow>>,
    plugin_manager_window: Option<WindowHandle<PluginManagerWindow>>,
    /// Cached plugin registry scan result. `None` until the first
    /// `+ Add Insert` click triggers a sync scan (or the Plugin Manager
    /// dialog populates it). Phase 2a uses the first insert-capable
    /// entry; Phase 2b adds a real picker overlay.
    available_plugins: Option<Vec<sphere_plugin_host::RegistryPlugin>>,
    /// `true` if the cached preset directory exists on disk. Drives the
    /// "No plugin index found" message in the picker.
    plugin_cache_present: bool,
    /// Picker catalog state — drives the skeleton / error UI in the overlay.
    /// `Loading` while the background SQLite read is in flight; `Ready` once
    /// `available_plugins` has been populated.
    plugin_catalog_status: PluginCatalogStatus,
    /// Open native plugin editor windows (Phase 4). Keyed by
    /// `(track_id, insert_id)` → the GPUI-hosted editor window handle. GPUI
    /// owns the borderless shell; the C++ backend embeds the VST3 IPlugView in
    /// a native child region. Dropping the window entity detaches the view.
    open_plugin_editors:
        std::collections::HashMap<(String, String), WindowHandle<PluginEditorWindow>>,
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
    /// Last time we pushed engine meter levels into timeline state. Used to
    /// throttle meter updates per the active `PowerMode` so low-end GPUs
    /// don't repaint 60 Hz for sub-perceptual meter wiggles.
    last_meter_apply: Instant,
    /// Active BPM drag id (matches `BpmDragSample::drag_id`). Resets when a
    /// new drag begins. Drives delta-accumulated BPM editing.
    bpm_drag_active_id: Option<u64>,
    /// Previous cursor Y from the last BPM drag sample. Each new sample
    /// applies `cur_y - prev_y`, so dragging is unbounded by window
    /// height — FL Studio–style behavior.
    bpm_drag_prev_y: f32,
    /// Accumulated BPM offset (signed) for the active drag.
    bpm_drag_accum: f32,
    /// Last time we sent `engine.set_bpm` during a live BPM drag. Throttles
    /// audio-engine tempo commits to ~30 Hz; the UI state still updates
    /// every event, but we don't flood the engine with sub-perceptual
    /// tempo writes during fast vertical drags.
    last_engine_bpm_commit: Option<Instant>,
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

impl StudioLayout {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // ── Centralized path resolution ───────────────────────────────────
        let paths = FutureboardPaths::resolve();
        if let Err(e) = paths.ensure_user_dirs() {
            eprintln!("[paths] failed to create user directories: {e}");
        }

        let settings = SettingsModel::load_or_create(cx);
        cx.set_global(GlobalSettingsModel(settings.clone()));
        crate::boot::log("settings loaded");

        let schema = settings.read(cx).current.clone();

        // Apply saved Renderer choice — Settings is "* Restart required",
        // so this only takes effect at process start. The env var
        // `FUTUREBOARD_WGPU_TIMELINE=1` still wins as a dev override.
        {
            use crate::components::timeline::render::{
                set_preferred_backend, set_preferred_gpu_device_id, TimelineRendererBackend,
            };
            let chosen = match schema.performance.render_mode {
                crate::settings::RenderMode::CpuRender => TimelineRendererBackend::GpuiPaint,
                #[cfg(feature = "gpu-renderer")]
                crate::settings::RenderMode::GpuAcceleration => TimelineRendererBackend::Wgpu,
                #[cfg(not(feature = "gpu-renderer"))]
                crate::settings::RenderMode::GpuAcceleration => TimelineRendererBackend::GpuiPaint,
            };
            set_preferred_backend(chosen);
            // Saved GPU device id (empty string == Auto).
            let device_id = match &schema.performance.gpu_device {
                crate::settings::GpuDevicePreference::Auto => "",
                crate::settings::GpuDevicePreference::DeviceId(id) => id.as_str(),
            };
            set_preferred_gpu_device_id(device_id);
            if std::env::var_os("FUTUREBOARD_GPU_RENDERER_DEBUG").is_some() {
                eprintln!(
                    "[gpu-renderer] startup: render_mode={:?} gpu_device={:?}",
                    schema.performance.render_mode, schema.performance.gpu_device
                );
            }
        }

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
        crate::boot::log("audio engine handle ready");

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

        let piano_roll = {
            let timeline = timeline.clone();
            cx.new(|cx| components::piano_roll::PianoRoll::new(timeline, cx))
        };
        let audio_editor = {
            let timeline = timeline.clone();
            cx.new(|cx| components::AudioEditorHost::new(timeline, cx))
        };
        let clip_editor_panel = cx.new(|_| {
            components::ClipEditorPanel::new(
                timeline.clone(),
                piano_roll.clone(),
                audio_editor.clone(),
            )
        });
        let piano_roll_floating = {
            let timeline = timeline.clone();
            cx.new(|cx| {
                let mut pr = components::piano_roll::PianoRoll::new(timeline, cx);
                pr.midi_editor_sink = true;
                pr
            })
        };
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
        {
            let target = cx.entity().clone();
            let _ = timeline.update(cx, |timeline, _cx| {
                timeline.set_open_editor_callback(Some(Arc::new(move |_window, cx| {
                    let _ = target.update(cx, |this, cx| {
                        this.active_bottom_tab = components::BottomTab::Editor;
                        this.panels.mixer_docked = true;
                        cx.notify();
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
        {
            let pop_owner = studio_entity.clone();
            let _ = piano_roll.update(cx, |pr, _cx| {
                pr.set_pop_out_handler(Some(Arc::new(move |_window, cx| {
                    let _ = pop_owner.update(cx, |layout, cx| {
                        layout.open_midi_editor_external_window(None, cx);
                    });
                })));
            });
        }
        crate::platform_chrome::register_studio_menu_dispatcher(studio_entity, cx);

        // Close native plugin editors before GPUI/thread-local teardown on exit.
        let _ = cx.on_app_quit(|layout, cx| {
            layout.shutdown_plugin_editors(cx);
            async {}
        });

        // settings and paths are loaded and registered at the top of this function

        Self {
            active_bottom_tab: components::BottomTab::Mixer,
            bottom_panel_state: BottomPanelState::default(),
            timeline,
            piano_roll,
            audio_editor,
            clip_editor_panel,
            piano_roll_floating,
            midi_editor_window: None,
            pending_midi_editor_open: None,
            file_browser: FileBrowserState::default(),
            browser_scroll: UniformListScrollHandle::new(),
            menu_bar: MenuBarUiState::default(),
            project_switcher: ProjectSwitcherState::default(),
            project_switcher_search_input: TextInputState::new(
                "project-switcher-search-input",
                cx.focus_handle(),
            )
            .with_placeholder("Search projects..."),
            browser_search_input: TextInputState::new("browser-search-input", cx.focus_handle())
                .with_placeholder("Search..."),
            plugin_picker: PluginPickerState::closed(),
            plugin_picker_search_input: TextInputState::new(
                "plugin-picker-search-input",
                cx.focus_handle(),
            )
            .with_placeholder("Search plugins by name, vendor, category, or format…"),
            plugin_picker_prefs: PluginPickerPrefs::load(),
            plugin_search_index: None,
            plugin_picker_au_error: load_au_cache_state().last_error,
            add_track_window: None,
            plugin_manager_window: None,
            available_plugins: None,
            plugin_cache_present: false,
            plugin_catalog_status: PluginCatalogStatus::Loading,
            open_plugin_editors: std::collections::HashMap::new(),
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
            last_meter_apply: Instant::now(),
            bpm_drag_active_id: None,
            bpm_drag_prev_y: 0.0,
            bpm_drag_accum: 0.0,
            last_engine_bpm_commit: None,
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
                    Some(OpenPopover::Context {
                        target: ContextTarget::Browser(path),
                        ..
                    }) => path.clone(),
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
                            t.state
                                .import_audio_to_selected_or_new_track(path_key, name);
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
                    Some(OpenPopover::Context {
                        target: ContextTarget::Browser(path),
                        ..
                    }) => path.clone(),
                    _ => None,
                };
                if let Some(path) = path {
                    reveal_path(&path);
                }
            }
            "browser:refresh" => {
                let path = match &self.open_popover {
                    Some(OpenPopover::Context {
                        target: ContextTarget::Browser(path),
                        ..
                    }) => path.clone(),
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
                    Some(OpenPopover::Context {
                        target: ContextTarget::Browser(path),
                        ..
                    }) => path.clone(),
                    _ => None,
                };
                if let Some(path) = path {
                    let path_str = path.to_string_lossy().to_string();
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(path_str));
                }
            }
            "browser:open" => {
                let path = match &self.open_popover {
                    Some(OpenPopover::Context {
                        target: ContextTarget::Browser(path),
                        ..
                    }) => path.clone(),
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
            "track:add-bus" => {
                self.open_add_track_external_window(AddTrackKind::Bus, owner_bounds, cx)
            }
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
            "edit:undo" => {
                let _ = self.timeline.update(cx, |timeline, cx| {
                    timeline.undo_edit(cx);
                });
                self.mark_dirty();
            }
            "edit:redo" => {
                let _ = self.timeline.update(cx, |timeline, cx| {
                    timeline.redo_edit(cx);
                });
                self.mark_dirty();
            }
            "edit:duplicate" | "clip:duplicate" => self.duplicate_selected_clip(cx),

            "editor:open-bottom" => self.open_midi_editor_bottom_panel(cx),
            "midi:open-editor" | "editor:open-midi-window" => {
                self.open_midi_editor_external_window(owner_bounds, cx)
            }
            "midi:select-all" | "midi:delete-selected" | "midi:quantize" | "midi:fit-notes" => {
                self.dispatch_midi_editor_menu_command(command_id, cx)
            }

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

    fn selected_midi_clip_id(&self, cx: &Context<Self>) -> Option<String> {
        let tl = self.timeline.read(cx);
        let clip_id = tl.state.selection.selected_clip_ids.first()?.clone();
        tl.state
            .find_clip(&clip_id)
            .filter(|(_, c)| matches!(c.clip_type, ClipType::Midi { .. }))
            .map(|_| clip_id)
    }

    fn select_midi_clip(&mut self, clip_id: &str, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |tl, cx| {
            tl.state.select_clip(clip_id);
            cx.notify();
        });
    }

    pub(crate) fn open_editor_bottom_panel(&mut self, cx: &mut Context<Self>) {
        self.active_bottom_tab = components::BottomTab::Editor;
        self.panels.mixer_docked = true;
        cx.notify();
    }

    pub(crate) fn open_midi_editor_bottom_panel(&mut self, cx: &mut Context<Self>) {
        self.open_editor_bottom_panel(cx);
    }

    fn dispatch_midi_editor_menu_command(&mut self, command_id: &str, cx: &mut Context<Self>) {
        let roll = if self.midi_editor_window.is_some() {
            self.piano_roll_floating.clone()
        } else {
            self.piano_roll.clone()
        };
        let cmd = command_id.to_string();
        let _ = roll.update(cx, |pr, cx| pr.run_menu_command(&cmd, cx));
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
                "e" | "E" => Some("midi:open-editor"),
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

    fn spawn_timeline_audio_import_jobs(
        cx: &mut Context<Self>,
        owner: Entity<Self>,
        timeline: Entity<components::timeline::Timeline>,
        path: PathBuf,
        _path_key: String,
    ) {
        components::timeline::audio_import::spawn_timeline_import_from_layout(
            path, timeline, owner, cx,
        );
    }
}
