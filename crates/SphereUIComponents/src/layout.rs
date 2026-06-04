use gpui::{
    div, px, AppContext, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, Styled, UniformListScrollHandle, Window, WindowHandle,
};

pub use crate::shutdown::ShutdownState;
pub use close_ops::PendingCloseAction;

use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Instant};

use crate::components;
use crate::components::add_track_dialog::{AddTrackKind, AddTrackWindow};
use crate::components::file_browser::FileBrowserState;
use crate::components::message_box_dialog::MessageBoxWindow;
use crate::components::midi_editor_window::MidiEditorWindow;
use crate::components::plugin_editor_window::PluginEditorWindow;
use crate::components::plugin_manager::PluginManagerWindow;
use crate::components::plugin_picker::{
    compute_filter_result, ensure_default_highlight, plugin_picker_overlay,
    CatalogStatus as PluginCatalogStatus, PickerFilter, PluginPickerCallbacks, PluginPickerPrefs,
    PluginPickerState, PluginSearchIndex,
};
use crate::components::project_switcher::ProjectSwitcherState;
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
use crate::settings::{GlobalSettingsModel, SettingsModel};
use crate::theme::{self, Colors};
use sphere_plugin_host::load_au_cache_state;

mod audio_transport;
mod browser_ops;
mod close_ops;
mod engine_snapshot;
mod frame_diagnostics;
mod helpers;
mod input_ops;
mod inspector_ops;
mod mixer_ops;
mod plugin_ops;
mod project_ops;
mod recording_ops;
mod studio_render;
mod studio_state;
mod track_clip_ops;
mod transport_freeze_debug;
mod transport_ops;
mod window_ops;

use engine_snapshot::volume_norm_to_linear;
use frame_diagnostics::FrameDiagnostics;
use helpers::{
    find_clip_summary, is_supported_audio_ext, is_text_input_key, key_debug, normalize_command_id,
    reveal_path, should_handle_global_transport_shortcut, transport_command_from_id, FocusContext,
};
use project_ops::LifecycleAction;
pub use studio_state::{ContextTarget, MenuBarUiState, OpenPopover, StudioPanelVisibility};
use studio_state::{TextContextMenu, TextMenuTarget, TransportCommand};

/// Demo content is opt-in only. The real runtime starts empty and renders
/// project state loaded or created by the user.
fn use_demo_project() -> bool {
    std::env::var_os("FUTUREBOARD_DEMO_PROJECT").is_some_and(|value| value == "1")
}

/// Map the saved Settings renderer choice onto the process-wide timeline
/// renderer preference. Idempotent — the underlying setters use `OnceLock`, so
/// it's safe to call at app launch and again when the studio is built.
fn apply_renderer_preference(schema: &crate::settings::SettingsSchema) {
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

/// Load saved settings and apply the renderer preference early — before the
/// studio window exists — so the GPU renderer can be warmed on the loading
/// screen instead of stalling the first studio frame. Called at app launch.
pub fn apply_saved_renderer_preference(cx: &mut gpui::App) {
    let settings = SettingsModel::load_or_create(cx);
    let schema = settings.read(cx).current.clone();
    apply_renderer_preference(&schema);
}

/// Eagerly initialize the timeline renderer (creating the GPU adapter/device
/// for the WGPU backend) on the current thread. Call on the main UI thread
/// during the loading screen, after [`apply_saved_renderer_preference`].
/// Returns the active backend label for status display.
pub fn warm_up_renderer() -> &'static str {
    crate::components::timeline::timeline_surface::warm_up_timeline_renderer()
}

/// Outcome of an early renderer warm-up: what backend was requested vs. what is
/// actually active, so the Welcome screen can report "GPU ready" vs. a CPU
/// fallback honestly (Part A).
#[derive(Debug, Clone, Copy)]
pub struct RendererWarmup {
    pub backend_label: &'static str,
    /// The user/preference asked for the GPU (WGPU) backend.
    pub gpu_requested: bool,
    /// The GPU backend is actually active (adapter/device created OK).
    pub gpu_active: bool,
}

impl RendererWarmup {
    /// Status text for the Welcome renderer row.
    pub fn status_text(&self) -> &'static str {
        if self.gpu_active {
            "GPU ready"
        } else if self.gpu_requested {
            "CPU fallback"
        } else {
            "CPU render"
        }
    }
}

/// Warm the renderer and report whether the GPU backend was requested and
/// whether it came up. Logs start/end (and any fallback) under
/// `FUTUREBOARD_GPU_RENDERER_DEBUG=1`. Non-fatal: a failed GPU init falls back
/// to CPU paint inside [`warm_up_renderer`].
pub fn warm_up_renderer_status() -> RendererWarmup {
    use crate::components::timeline::render::TimelineRendererBackend;

    let preferred = TimelineRendererBackend::from_env();
    #[cfg(feature = "gpu-renderer")]
    let gpu_requested = matches!(preferred, TimelineRendererBackend::Wgpu);
    #[cfg(not(feature = "gpu-renderer"))]
    let gpu_requested = false;

    let gpu_debug = std::env::var_os("FUTUREBOARD_GPU_RENDERER_DEBUG").is_some();
    if gpu_debug {
        eprintln!(
            "[gpu-renderer] warm-up start (requested backend={})",
            preferred.label()
        );
    }

    let backend_label = warm_up_renderer();

    #[cfg(feature = "gpu-renderer")]
    let gpu_active = backend_label == TimelineRendererBackend::Wgpu.label();
    #[cfg(not(feature = "gpu-renderer"))]
    let gpu_active = false;

    if gpu_debug {
        if gpu_requested && !gpu_active {
            eprintln!("[gpu-renderer] warm-up end: GPU requested but fell back to CPU paint");
        } else {
            eprintln!(
                "[gpu-renderer] warm-up end: backend={backend_label} gpu_active={gpu_active}"
            );
        }
    }

    RendererWarmup {
        backend_label,
        gpu_requested,
        gpu_active,
    }
}

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
    /// Inspector track-name edit field. Hosted here (not in the stateless
    /// inspector render fn) so it owns a real focus handle and routes keys
    /// through the same machinery as the other main-window text fields.
    inspector_name_input: TextInputState,
    /// Track id the `inspector_name_input` is currently editing. When the
    /// selected track changes, render reloads the field from the new track's
    /// name (see `studio_render`). `None` when no track is selected.
    inspector_name_bound: Option<String>,
    inspector_clip_name_input: TextInputState,
    inspector_clip_name_bound: Option<String>,
    /// UI-only selected plugin insert `(track_id, insert_id)` driving the
    /// Plugin Insert inspector target. Pure selection — never marks dirty.
    selected_insert: Option<(String, String)>,
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
    open_inspector_routing_combo: Option<crate::components::panel::InspectorRoutingCombo>,
    inspector_routing_combo_anchor: Option<crate::overlay::OverlayAnchor>,
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
    /// Beat position when the current recording session started.
    recording_start_beat: f32,
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
    /// Handle to this workspace's own window. Set by the app layer right after
    /// the window opens so `close_project` can close it when returning to
    /// Welcome. `None` until wired (e.g. in tests / headless contexts).
    self_window: Option<WindowHandle<StudioLayout>>,
    /// App-level hook that re-opens the Welcome window. Invoked by
    /// `do_close_project`. The app layer owns Welcome window construction, so
    /// the studio crate stays decoupled from native window options.
    on_request_welcome: Option<Arc<dyn Fn(&mut gpui::App) + 'static>>,
    /// Live unsaved-changes guard dialog (Save / Don't Save / Cancel), if one
    /// is currently shown. Tracked so New/Open/Close/Quit don't stack dialogs.
    unsaved_guard_window: Option<WindowHandle<MessageBoxWindow>>,
    /// Close/quit action waiting on the unsaved-changes dialog.
    pending_close_action: Option<close_ops::PendingCloseAction>,
    /// New/Open lifecycle action waiting on the unsaved-changes dialog.
    pending_lifecycle_action: Option<project_ops::LifecycleAction>,
    /// Active keyboard shortcut profile. The default profile is bundled; other
    /// profiles load from `<app dir>/Keymaps/<id>.json`. Drives `shortcut_command_id`.
    active_keymap: crate::keymap::Keymap,
    /// Authoritative project-lifecycle state (Part G). Drives the window title;
    /// the dirty bit is still tracked on `project_switcher.current_project`.
    project_state: crate::app_state::ProjectState,
    /// Last OS window title applied in render, to avoid redundant set calls.
    last_window_title: Option<String>,
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

        // Apply saved Renderer choice — Settings is "* Restart required", so
        // this only takes effect at process start. Idempotent: the same
        // preference is also applied at app launch (before the Welcome window)
        // so the GPU renderer can be warmed on the loading screen. The env var
        // `FUTUREBOARD_WGPU_TIMELINE=1` still wins as a dev override.
        apply_renderer_preference(&schema);

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
                engine.set_pdc_enabled(schema.playback.latency_compensation);
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
            if use_demo_project() {
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

        // Ordered studio teardown before GPUI/thread-local destruction.
        let _ = cx.on_app_quit(|layout, cx| {
            layout.shutdown_studio(cx);
            async {}
        });

        // settings and paths are loaded and registered at the top of this function

        let mut layout = Self {
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
            inspector_name_input: TextInputState::new("inspector-name-input", cx.focus_handle())
                .with_placeholder("Track name"),
            inspector_name_bound: None,
            inspector_clip_name_input: TextInputState::new(
                "inspector-clip-name-input",
                cx.focus_handle(),
            )
            .with_placeholder("Clip name"),
            inspector_clip_name_bound: None,
            selected_insert: None,
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
            open_inspector_routing_combo: None,
            inspector_routing_combo_anchor: None,
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
            recording_start_beat: 0.0,
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
            self_window: None,
            on_request_welcome: None,
            unsaved_guard_window: None,
            pending_close_action: None,
            pending_lifecycle_action: None,
            active_keymap: crate::keymap::Keymap::bundled_default(),
            project_state: crate::app_state::ProjectState::NoProject,
            last_window_title: None,
        };

        if layout.audio_engine.is_some() {
            layout.schedule_audio_project_sync(cx, true, "studio_init");
        }

        layout
    }

    /// Switch the active keyboard shortcut profile. `"default"` restores the
    /// bundled map; any other id loads `<app dir>/Keymaps/<id>.json`. A missing
    /// or invalid profile file leaves the current map untouched. Returns the
    /// active profile id after the call.
    pub fn set_keymap_profile(&mut self, id: &str) -> &str {
        match crate::keymap::Keymap::load_profile(id) {
            Some(map) => self.active_keymap = map,
            None => {
                if crate::keymap::shortcut_debug_enabled() {
                    eprintln!("[shortcut] profile id={id} unavailable — keeping current map");
                }
            }
        }
        self.active_keymap.id.as_str()
    }

    /// Id of the active keyboard shortcut profile (for the preferences UI).
    pub fn active_keymap_id(&self) -> &str {
        &self.active_keymap.id
    }

    /// Wire this layout to its own window handle so `close_project` can close
    /// the workspace window when returning to Welcome.
    pub fn set_self_window(&mut self, handle: WindowHandle<StudioLayout>) {
        self.self_window = Some(handle);
    }

    /// Wire the app-level "return to Welcome" hook used by `close_project`.
    pub fn set_request_welcome_callback(
        &mut self,
        callback: Arc<dyn Fn(&mut gpui::App) + 'static>,
    ) {
        self.on_request_welcome = Some(callback);
    }
}

impl StudioLayout {
    /// Single entry point for menu items, keyboard shortcuts, and chrome
    /// buttons. `command_id` matches the Electron/shared menu manifest
    /// IDs (e.g. `transport:play-pause`). Unknown IDs are logged once
    /// and then ignored — this is the contract that lets future menu
    /// entries appear in the chrome without crashing the dispatcher.
    pub fn dispatch_command_id(&mut self, command_id: &str, cx: &mut Context<Self>) {
        let studio_bounds = self.studio_window_bounds(cx);
        let owner_bounds =
            crate::window_position::resolve_owner_bounds_with_preferred(None, studio_bounds, cx);
        self.dispatch_command_id_from_bounds(command_id, owner_bounds, cx);
    }

    /// Main workspace window bounds — preferred owner for dialogs on Windows.
    pub(super) fn studio_window_bounds(&self, cx: &mut gpui::App) -> Option<Bounds<gpui::Pixels>> {
        self.self_window.as_ref().and_then(|handle| {
            let bounds = handle.update(cx, |_, window, _| window.bounds()).ok()?;
            crate::window_position::is_valid_owner_bounds(bounds).then_some(bounds)
        })
    }

    pub(super) fn dispatch_command_id_from_bounds(
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
            // New Project no longer opens a modal wizard — it drops straight
            // into a fresh, empty, unsaved workspace. All four lifecycle
            // entry points share one unsaved-changes guard (Save / Don't Save /
            // Cancel) before replacing or unloading the current project.
            "project:new" | "project:new-from-template" => {
                self.guard_dirty_then_lifecycle(LifecycleAction::NewProject, owner_bounds, cx)
            }
            "project:close" => self.request_close(
                close_ops::PendingCloseAction::CloseProject,
                owner_bounds,
                cx,
            ),
            // Quit the whole application — distinct from `project:close`, which
            // only unloads the session and returns to Welcome.
            "app:quit" => {
                self.request_close(close_ops::PendingCloseAction::QuitApp, owner_bounds, cx)
            }
            "project:open" => {
                self.guard_dirty_then_lifecycle(LifecycleAction::OpenProject, owner_bounds, cx)
            }
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
            "edit:delete" | "clip:delete" | "automation:delete-selected-points" => {
                self.delete_selected_clip_or_track(cx)
            }
            // Automation editor commands. select-all / deselect are automation
            // aware so they act on points when the selected track is in
            // Automation mode, and fall through harmlessly otherwise.
            "edit:select-all" | "automation:select-all-points" => {
                self.select_all_automation_points(cx)
            }
            "edit:deselect-all" | "automation:clear-selection" => {
                self.clear_automation_selection(cx)
            }
            "automation:toggle-mode" => self.toggle_selected_track_automation_mode(cx),
            "automation:cycle-target" => self.cycle_selected_track_automation_target(cx),
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

            // ── Tools — switch the active timeline tool. UI-only; never dirties
            // the engine. The piano roll owns its own tool keys when focused.
            "tools:select-pointer"
            | "tools:select-pen"
            | "tools:select-cut"
            | "tools:select-glue"
            | "tools:select-time"
            | "tools:select-automation" => {
                use components::timeline::timeline_state::TimelineTool;
                let tool = match command_id {
                    "tools:select-pen" => TimelineTool::Pen,
                    "tools:select-cut" => TimelineTool::Cut,
                    "tools:select-glue" => TimelineTool::Glue,
                    "tools:select-time" => TimelineTool::Time,
                    "tools:select-automation" => TimelineTool::Automation,
                    _ => TimelineTool::Pointer,
                };
                let _ = self.timeline.update(cx, |timeline, cx| {
                    if timeline.state.active_tool != tool {
                        timeline.state.active_tool = tool;
                        cx.notify();
                    }
                });
            }

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
            Arc::new(move |_: &(), window: &mut Window, cx: &mut gpui::App| {
                let bounds = window.bounds();
                let _ = this.update(cx, |this, cx| {
                    this.dispatch_command_id_from_bounds(command_id, Some(bounds), cx);
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

        if let Some(ref engine) = self.audio_engine {
            let desired_pdc = schema.playback.latency_compensation;
            if engine.pdc_enabled() != desired_pdc {
                engine.set_pdc_enabled(desired_pdc);
                self.schedule_audio_project_sync(cx, false, "pdc_setting");
            }
        }

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
    /// Resolve a key event to a command id under the active shortcut profile.
    /// Profiles are data-driven (`packages/keymaps/*.json`); `Ctrl+E` keeps a
    /// special case for opening the MIDI editor since that command has no menu
    /// accelerator in the bundled map.
    fn shortcut_command_id(&self, event: &KeyDownEvent) -> Option<String> {
        if let Some(command) = self.active_keymap.command_for_event(event) {
            return Some(command.to_string());
        }
        // Fallback: Ctrl/Cmd+E opens the MIDI editor (not in the menu manifest).
        let mods = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if (mods.control || mods.platform)
            && !mods.alt
            && !mods.function
            && matches!(key, "e" | "E")
        {
            return Some("midi:open-editor".to_string());
        }
        None
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
