use gpui::{Bounds, Context, Window};

use std::path::PathBuf;
use std::sync::Arc;

use crate::components::add_track_dialog::{
    open_add_track_window, AddTrackDialogState, AddTrackKind, AudioFormat,
};
use crate::components::combo_box::dedupe_preserve_order;
use crate::components::keymap_window::{open_keymap_window, KeymapChangedCb};
use crate::components::midi_editor_window::{midi_editor_debug, open_midi_editor_window};
use crate::components::settings_dialog::{open_settings_window, OnSettingUpdate};
use crate::components::timeline::timeline_state::{
    self, ClipType, CreateTrackOptions, InsertPluginFormat, TrackAudioFormat, TrackInputRouting,
    TrackOutputRouting, TrackType,
};
use crate::components::{external_mixer_debug, open_mixer_window};
use crate::session_shutdown::SessionShutdownSnapshot;
use crate::window_position::resolve_owner_bounds_with_preferred;
use SpherePluginHost::{PluginFormat as RegistryPluginFormat, PluginKind};

use super::helpers::{cleaned_track_name, numbered_name_stem};
use super::{ContextMenuTarget, OpenPopover, StudioLayout};

fn add_track_instrument_plugins_from_catalog(
    catalog: &super::plugin_ops::PluginCatalogState,
) -> Vec<SpherePluginHost::RegistryPlugin> {
    catalog
        .available
        .as_ref()
        .map(|plugins| {
            plugins
                .iter()
                .filter(|plugin| {
                    plugin.kind == PluginKind::Instrument
                        && plugin.supports_insert()
                        && plugin.scan_status.is_usable()
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn dialog_audio_format(format: AudioFormat) -> TrackAudioFormat {
    match format {
        AudioFormat::Mono => TrackAudioFormat::Mono,
        AudioFormat::Stereo => TrackAudioFormat::Stereo,
    }
}

fn dialog_audio_input_routing(
    label: &str,
    format: AudioFormat,
    input_device: Option<&(String, u32)>,
) -> TrackInputRouting {
    match label {
        "None" => TrackInputRouting::None,
        "Input 1" | "Input 2" => {
            let Some((device_id, channels)) = input_device else {
                return TrackInputRouting::AllInputs;
            };
            let channel = if label == "Input 2" { 1 } else { 0 };
            if channel >= *channels {
                return TrackInputRouting::AllInputs;
            }
            match format {
                AudioFormat::Mono => TrackInputRouting::AudioDeviceChannel {
                    device_id: device_id.clone(),
                    channel,
                },
                AudioFormat::Stereo => {
                    if channel + 1 < *channels {
                        TrackInputRouting::AudioDeviceChannels {
                            device_id: device_id.clone(),
                            channels: vec![channel, channel + 1],
                        }
                    } else {
                        TrackInputRouting::AllInputs
                    }
                }
            }
        }
        _ => TrackInputRouting::AllInputs,
    }
}

fn dialog_audio_output_routing(label: &str) -> TrackOutputRouting {
    match label {
        "None" => TrackOutputRouting::None,
        _ => TrackOutputRouting::Main,
    }
}

/// Studio-window / app-integration hooks — this workspace's own window handle,
/// the last known window bounds (used to position child windows without
/// re-entering the root `WindowHandle`), and the app-level "re-open Welcome"
/// hook. `StudioLayout` decomposition slice (all Option → derived `Default`).
#[derive(Default)]
pub(crate) struct StudioWindowHooks {
    /// Handle to this workspace's own window; `None` until wired by the app layer.
    pub self_window: Option<gpui::WindowHandle<StudioLayout>>,
    /// Last known main workspace bounds, updated during render.
    pub cached_bounds: Option<Bounds<gpui::Pixels>>,
    /// App-level hook that re-opens the Welcome window (invoked by close_project).
    pub on_request_welcome: Option<Arc<dyn Fn(&mut gpui::App) + 'static>>,
    /// App-level hook for in-studio project open/replace — keeps the root studio
    /// window alive and swaps the session in place.
    pub on_request_project_load: Option<
        Arc<dyn Fn(PathBuf, super::project_ops::ProjectOpenOptions, &mut gpui::App) + 'static>,
    >,
    /// App-level hook for visible session shutdown (close project).
    pub on_request_session_shutdown: Option<
        Arc<
            dyn Fn(
                    SessionShutdownSnapshot,
                    Option<Bounds<gpui::Pixels>>,
                    Option<gpui::WindowHandle<StudioLayout>>,
                    &mut gpui::App,
                ) + 'static,
        >,
    >,
}

/// Floating MIDI editor window state — the single editor window handle (switches
/// clip on open) and the owner bounds parked for a deferred open. `StudioLayout`
/// decomposition slice (both Option → derived `Default`).
#[derive(Default)]
pub(crate) struct MidiEditorWindowState {
    /// Global floating MIDI editor window; `None` when closed.
    pub window: Option<gpui::WindowHandle<crate::components::midi_editor_window::MidiEditorWindow>>,
    /// Owner bounds for a deferred editor open.
    pub pending_open: Option<Bounds<gpui::Pixels>>,
}

/// Detached / external window handles owned by the studio (settings, mixer,
/// add-track, plugin-manager, export-arrangement) plus the bounds parked for a
/// deferred external-mixer open. `StudioLayout` decomposition slice (all Option
/// → derived `Default`).
#[derive(Default)]
pub(crate) struct ExternalWindows {
    /// External Settings window; `None` when closed.
    pub settings: Option<gpui::WindowHandle<crate::components::settings_dialog::SettingsWindow>>,
    /// Detached mixer window (multi-monitor layouts).
    pub mixer: Option<gpui::WindowHandle<crate::components::MixerWindow>>,
    /// Bounds for an external-mixer open deferred to after the current update.
    pub pending_mixer_open: Option<Bounds<gpui::Pixels>>,
    /// Add Track dialog window.
    pub add_track: Option<gpui::WindowHandle<crate::components::add_track_dialog::AddTrackWindow>>,
    /// Plugin Manager window.
    pub plugin_manager:
        Option<gpui::WindowHandle<crate::components::plugin_manager::PluginManagerWindow>>,
    /// Export Arrangement window.
    pub export_arrangement: Option<gpui::WindowHandle<crate::export::ExportArrangementWindow>>,
    /// Keymap / keyboard shortcuts editor window.
    pub keymap: Option<gpui::WindowHandle<crate::components::keymap_window::KeymapWindow>>,
}

impl StudioLayout {
    pub(super) fn update_add_track_instrument_plugins(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.external_windows.add_track.clone() else {
            return;
        };
        let instrument_plugins = add_track_instrument_plugins_from_catalog(&self.plugin_catalog);
        let _ = handle.update(cx, |add_track, _window, cx| {
            add_track.set_instrument_plugins(instrument_plugins);
            cx.notify();
        });
    }

    pub(super) fn open_add_track_external_window(
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
    pub(super) fn open_add_track_external_window_with_context(
        &mut self,
        kind: AddTrackKind,
        track_count: usize,
        has_master_track: bool,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        // If window is already open, activate and refresh its context.
        let default_monitor_mode = self
            .settings
            .read(cx)
            .current
            .recording
            .default_monitor_mode
            .add_track_value();
        if let Some(handle) = self.external_windows.add_track.clone() {
            if handle
                .update(cx, |win, window, cx| {
                    win.set_instrument_plugins(add_track_instrument_plugins_from_catalog(
                        &self.plugin_catalog,
                    ));
                    win.set_context(kind, track_count, has_master_track, default_monitor_mode);
                    window.activate_window();
                    cx.notify();
                })
                .is_ok()
            {
                return;
            }
            self.external_windows.add_track = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.overlay.open_popover = None;
        self.overlay.text_context_menu = None;

        let owner_bounds =
            resolve_owner_bounds_with_preferred(owner_bounds, self.studio_window_bounds(cx), cx);

        if self.plugin_catalog.available.is_none()
            || !matches!(
                self.plugin_catalog.status,
                crate::components::plugin_picker::CatalogStatus::Ready
            )
        {
            self.arm_catalog_load(cx);
        }
        let instrument_plugins = add_track_instrument_plugins_from_catalog(&self.plugin_catalog);

        let layout = cx.entity().clone();
        let language = self.settings.read(cx).current.general.language.clone();
        let on_confirm_request: Arc<dyn Fn(AddTrackDialogState, String, &mut gpui::App) + 'static> =
            Arc::new(move |dialog, _name, cx| {
                let Some(track_type) = dialog.selected_kind.native_track_type() else {
                    return;
                };
                let _ = layout.update(cx, |this, cx| {
                    this.mark_dirty();
                    let selected_input_device = this.selected_input_device_channels(cx);
                    let _ = this.timeline.update(cx, |timeline, cx| {
                        let count = dialog.count.clamp(1, 128) as usize;
                        let base_name =
                            cleaned_track_name(&dialog.track_name, dialog.selected_kind);
                        let mut selected_track_id = None;
                        let mut created_ids = Vec::new();
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
                            // Auto color → generated palette color per track.
                            // Custom color → the user's chosen color (applied to
                            // every track created in this batch).
                            let color = if dialog.auto_color {
                                timeline
                                    .state
                                    .track_color_for_index(dialog.base_track_count + i)
                            } else if let Some(custom) = dialog.custom_color {
                                custom
                            } else {
                                timeline.state.track_color_for_index(dialog.color_index + i)
                            };
                            let id = timeline.state.create_track(CreateTrackOptions {
                                track_type,
                                name,
                                color,
                                volume: timeline_state::volume::db_to_norm(0.0),
                                pan: 0.0,
                                armed: dialog.selected_kind == AddTrackKind::Audio
                                    && dialog.arm_track,
                                input_monitor: match dialog.monitor_mode {
                                    "input" => timeline_state::InputMonitorMode::Always,
                                    "auto" => timeline_state::InputMonitorMode::WhenRecordArmed,
                                    _ => timeline_state::InputMonitorMode::Off,
                                },
                            });
                            if dialog.selected_kind == AddTrackKind::Audio {
                                let audio_format = dialog_audio_format(dialog.audio_format);
                                let input = dialog_audio_input_routing(
                                    &dialog.input_label,
                                    dialog.audio_format,
                                    selected_input_device.as_ref(),
                                );
                                let output = dialog_audio_output_routing(&dialog.output_label);
                                timeline.state.set_track_audio_format(&id, audio_format);
                                timeline.state.set_track_input_routing(&id, input);
                                timeline.state.set_track_output_routing(&id, output);
                            }
                            if dialog.selected_kind == AddTrackKind::Instrument {
                                if let Some(plugin_id) = dialog.instrument_plugin_id.as_deref() {
                                    let instrument_registry =
                                        add_track_instrument_plugins_from_catalog(
                                            &this.plugin_catalog,
                                        );
                                    if let Some(reg) = instrument_registry.iter().find(|p| {
                                        p.id == plugin_id
                                            || p.class_id.as_deref() == Some(plugin_id)
                                            || p.name.eq_ignore_ascii_case(plugin_id)
                                    }) {
                                        if let Some(slot_id) = timeline.state.add_insert(&id) {
                                            let format = match reg.format {
                                                RegistryPluginFormat::Vst3 => {
                                                    InsertPluginFormat::Vst3
                                                }
                                                RegistryPluginFormat::Clap => {
                                                    InsertPluginFormat::Clap
                                                }
                                                RegistryPluginFormat::Au => InsertPluginFormat::Au,
                                                RegistryPluginFormat::Lv2 => {
                                                    InsertPluginFormat::Lv2
                                                }
                                                RegistryPluginFormat::Unknown => {
                                                    InsertPluginFormat::Unknown
                                                }
                                            };
                                            let plugin_uid = reg
                                                .class_id
                                                .clone()
                                                .unwrap_or_else(|| reg.id.clone());
                                            timeline.state.set_insert_plugin(
                                                &id,
                                                &slot_id,
                                                plugin_uid,
                                                Some(reg.path.clone()),
                                                format,
                                                reg.name.clone(),
                                            );
                                        }
                                    }
                                }
                            }
                            created_ids.push(id.clone());
                            selected_track_id = Some(id);
                        }
                        if let Some(id) = selected_track_id {
                            timeline.state.select_track(&id);
                        }
                        crate::components::add_track_dialog::add_track_debug(&format!(
                            "created tracks kind={} count={} ids={:?}",
                            dialog.selected_kind.tab_label(),
                            count,
                            created_ids
                        ));
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
            default_monitor_mode,
            language,
            instrument_plugins,
            on_confirm_request,
            cx,
        ) {
            Ok(handle) => self.external_windows.add_track = Some(handle),
            Err(err) => eprintln!("[add-track] failed to open window: {err}"),
        }
    }

    pub(super) fn open_settings_dialog(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        // If window is already open, activate it
        if let Some(handle) = self.external_windows.settings.clone() {
            if handle
                .update(cx, |_settings, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.external_windows.settings = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.overlay.open_popover = None;
        self.project_switcher.is_open = false;
        self.overlay.text_context_menu = None;

        let owner_bounds =
            resolve_owner_bounds_with_preferred(owner_bounds, self.studio_window_bounds(cx), cx);
        let settings = self.settings.clone();
        let owner = cx.entity().clone();

        let mut available_inputs = if let Some(ref engine) = self.audio_bridge.engine {
            engine
                .list_input_devices()
                .into_iter()
                .map(|d| d.name)
                .collect::<Vec<_>>()
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
        available_inputs = dedupe_preserve_order(&available_inputs);

        let mut available_outputs = if let Some(ref engine) = self.audio_bridge.engine {
            engine
                .list_output_devices()
                .into_iter()
                .map(|d| d.name)
                .collect::<Vec<_>>()
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
        available_outputs = dedupe_preserve_order(&available_outputs);

        // (device name, channel count) for the read-only channel lists (Phase C).
        let available_input_channels: Vec<(String, u32)> = self
            .audio_bridge
            .engine
            .as_ref()
            .map(|engine| {
                engine
                    .list_input_devices()
                    .into_iter()
                    .map(|d| (d.name, d.channels))
                    .collect()
            })
            .unwrap_or_default();
        let available_output_channels: Vec<(String, u32)> = self
            .audio_bridge
            .engine
            .as_ref()
            .map(|engine| {
                engine
                    .list_output_devices()
                    .into_iter()
                    .map(|d| (d.name, d.channels))
                    .collect()
            })
            .unwrap_or_default();

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

        let engine_for_latency = self.audio_bridge.engine.clone();
        let latency_provider: crate::components::settings_dialog::AudioLatencySnapshotProvider =
            Arc::new(move || {
                engine_for_latency
                    .as_ref()
                    .map(crate::settings::SettingsAudioLatencySnapshot::from_engine)
                    .unwrap_or_else(crate::settings::SettingsAudioLatencySnapshot::unavailable)
            });
        let input_test_start: Option<crate::components::settings_dialog::InputTestStartFn> =
            self.audio_bridge.engine.clone().map(|engine| {
                Arc::new(move |device_id: Option<String>| {
                    let device_id = device_id.filter(|id| !id.trim().is_empty());
                    engine
                        .start_input_test(device_id.as_deref())
                        .map_err(|error| error.to_string())
                }) as crate::components::settings_dialog::InputTestStartFn
            });
        let input_test_stop: Option<crate::components::settings_dialog::InputTestStopFn> =
            self.audio_bridge.engine.clone().map(|engine| {
                Arc::new(move || {
                    engine.stop_input_test();
                }) as crate::components::settings_dialog::InputTestStopFn
            });
        let input_test_level: Option<crate::components::settings_dialog::InputTestLevelFn> =
            self.audio_bridge.engine.clone().map(|engine| {
                Arc::new(move || engine.input_test_level())
                    as crate::components::settings_dialog::InputTestLevelFn
            });

        match open_settings_window(
            owner_bounds,
            settings,
            available_inputs,
            available_outputs,
            available_backends,
            available_input_channels,
            available_output_channels,
            latency_provider,
            input_test_start,
            input_test_stop,
            input_test_level,
            on_update,
            cx,
        ) {
            Ok(handle) => self.external_windows.settings = Some(handle),
            Err(err) => eprintln!("[settings] failed to open settings window: {err}"),
        }
    }

    pub(super) fn close_settings_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.external_windows.settings.take() {
            let _ = handle.update(cx, |_settings, window, _cx| window.remove_window());
        }
        self.overlay.text_context_menu = None;
        cx.notify();
    }

    pub(super) fn open_keymap_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.external_windows.keymap.clone() {
            if handle
                .update(cx, |_keymap, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.external_windows.keymap = None;
        }

        let manager = self.keymap_manager.clone();
        let studio = cx.entity().clone();
        let on_changed: KeymapChangedCb = Arc::new(move |manager, app| {
            let _ = studio.update(app, |layout, cx| {
                layout.keymap_manager = manager;
                cx.notify();
            });
        });

        match open_keymap_window(owner_bounds, manager, on_changed, cx) {
            Ok(handle) => self.external_windows.keymap = Some(handle),
            Err(err) => eprintln!("[keymap] failed to open window: {err}"),
        }
    }

    pub(crate) fn open_mixer_external_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        external_mixer_debug("external mixer open requested");
        let owner_bounds =
            resolve_owner_bounds_with_preferred(owner_bounds, self.studio_window_bounds(cx), cx);
        self.external_windows.pending_mixer_open = owner_bounds;
        self.schedule_pending_mixer_external_open(cx);
        cx.notify();
    }

    pub(super) fn schedule_pending_mixer_external_open(&mut self, cx: &mut Context<Self>) {
        if self.external_windows.pending_mixer_open.is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(0))
                .await;
            let _ = this.update(cx, |layout, cx| {
                layout.flush_pending_mixer_external_open(cx)
            });
        })
        .detach();
    }

    pub(super) fn flush_pending_mixer_external_open(&mut self, cx: &mut Context<Self>) {
        let owner_bounds = resolve_owner_bounds_with_preferred(
            self.external_windows.pending_mixer_open.take(),
            self.studio_window_bounds(cx),
            cx,
        );
        let Some(owner_bounds) = owner_bounds else {
            return;
        };

        self.prune_mixer_window(cx);
        if let Some(handle) = self.external_windows.mixer.clone() {
            if handle
                .update(cx, |_mixer, window, _cx| window.activate_window())
                .is_ok()
            {
                self.panels.mixer_docked = false;
                self.push_mixer_snapshot_to_window(cx);
                cx.notify();
                return;
            }
            self.external_windows.mixer = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.overlay.open_popover = None;
        self.panels.mixer_docked = false;

        let snapshot = self.build_mixer_snapshot(cx);
        let callbacks = self.build_mixer_callbacks(cx.entity().clone());
        let owner = cx.entity().clone();
        let on_close: std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + Send + Sync> =
            std::sync::Arc::new(move |_window, cx| {
                let _ = owner.update(cx, |layout, cx| layout.note_mixer_window_closed(cx));
            });
        let scroll_owner = cx.entity().clone();
        let on_mixer_scroll: std::sync::Arc<
            dyn Fn(f32, &mut Window, &mut gpui::App) + Send + Sync,
        > = std::sync::Arc::new(move |new_x: f32, _w, cx| {
            let _ = scroll_owner.update(cx, |layout, cx| {
                if layout.set_mixer_scroll_x(new_x, cx) {
                    layout.push_mixer_snapshot_to_window(cx);
                }
            });
        });
        let split_owner = cx.entity().clone();
        let on_mixer_split: std::sync::Arc<
            dyn Fn(crate::components::mixer_panel::MixerSplitAction, &mut Window, &mut gpui::App)
                + Send
                + Sync,
        > = std::sync::Arc::new(move |action, _w, cx| {
            let _ =
                split_owner.update(cx, |layout, cx| layout.apply_mixer_split_action(action, cx));
        });

        match open_mixer_window(
            owner_bounds,
            snapshot,
            callbacks,
            on_close,
            on_mixer_scroll,
            on_mixer_split,
            cx,
        ) {
            Ok(handle) => {
                self.external_windows.mixer = Some(handle);
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
        if let Some(handle) = self.external_windows.mixer.take() {
            let _ = handle.update(cx, |_mixer, window, _cx| window.remove_window());
        }
        cx.notify();
    }

    pub(super) fn note_mixer_window_closed(&mut self, cx: &mut Context<Self>) {
        self.external_windows.mixer = None;
        cx.notify();
    }

    pub(super) fn prune_mixer_window(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.external_windows.mixer.clone() else {
            return;
        };
        if handle.update(cx, |_mixer, _window, _cx| ()).is_err() {
            self.external_windows.mixer = None;
        }
    }

    pub(crate) fn open_midi_editor_external_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if self.selected_midi_clip_id(cx).is_none() {
            return;
        }
        let owner_bounds =
            resolve_owner_bounds_with_preferred(owner_bounds, self.studio_window_bounds(cx), cx);
        self.midi_editor.pending_open = owner_bounds;
        self.schedule_pending_midi_editor_open(cx);
        cx.notify();
    }

    pub(super) fn schedule_pending_midi_editor_open(&mut self, cx: &mut Context<Self>) {
        if self.midi_editor.pending_open.is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(0))
                .await;
            let _ = this.update(cx, |layout, cx| layout.flush_pending_midi_editor_open(cx));
        })
        .detach();
    }

    pub(super) fn flush_pending_midi_editor_open(&mut self, cx: &mut Context<Self>) {
        let owner_bounds = resolve_owner_bounds_with_preferred(
            self.midi_editor.pending_open.take(),
            self.studio_window_bounds(cx),
            cx,
        );
        let Some(owner_bounds) = owner_bounds else {
            return;
        };

        if let Some(OpenPopover::Context { request }) = self.overlay.open_popover.as_ref() {
            if let ContextMenuTarget::Clip(clip_id) = &request.target {
                let clip_id = clip_id.clone();
                if self
                    .timeline
                    .read(cx)
                    .state
                    .find_clip(&clip_id)
                    .is_some_and(|(_, c)| matches!(c.clip_type, ClipType::Midi { .. }))
                {
                    self.select_midi_clip(&clip_id, cx);
                }
            }
        }

        self.prune_midi_editor_window(cx);
        if let Some(handle) = self.midi_editor.window.clone() {
            if handle
                .update(cx, |_w, window, _cx| window.activate_window())
                .is_ok()
            {
                midi_editor_debug("focus existing window");
                if let Some(clip_id) = self.selected_midi_clip_id(cx) {
                    if let Some((track, clip)) = self.timeline.read(cx).state.find_clip(&clip_id) {
                        midi_editor_debug(&format!(
                            "switch target clip clip={} track={}",
                            clip.name, track.name
                        ));
                    }
                }
                cx.notify();
                return;
            }
            self.midi_editor.window = None;
        }

        let clip_label = self
            .selected_midi_clip_id(cx)
            .and_then(|id| self.timeline.read(cx).state.find_clip(&id))
            .map(|(t, c)| (c.name.clone(), t.name.clone()));
        if let Some((clip_name, track_name)) = clip_label.as_ref() {
            midi_editor_debug(&format!("open window clip={clip_name} track={track_name}"));
        } else {
            midi_editor_debug("open window (no MIDI clip selected)");
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();

        let timeline = self.timeline.clone();
        let piano_roll = self.piano_roll_floating.clone();
        let virtual_keyboard = self.virtual_keyboard.clone();
        let owner = cx.entity().clone();
        let on_close: Arc<dyn Fn(&mut Window, &mut gpui::App) + Send + Sync> =
            Arc::new(move |_window, cx| {
                StudioLayout::defer_update(&owner, cx, |layout, cx| {
                    layout.note_midi_editor_window_closed(cx);
                });
            });
        let dispatch_owner = cx.entity().clone();
        let dispatch_command: Arc<dyn Fn(&'static str, &mut gpui::App) + Send + Sync> =
            Arc::new(move |command_id, cx| {
                let _ = dispatch_owner.update(cx, |layout, cx| {
                    layout.dispatch_command_id(command_id, cx);
                    cx.notify();
                });
            });

        match open_midi_editor_window(
            Some(owner_bounds),
            timeline,
            piano_roll,
            virtual_keyboard,
            on_close,
            dispatch_command,
            cx,
        ) {
            Ok(handle) => {
                // Register the popout as a musical-typing source so its held
                // notes are released if it closes (register doesn't flush, so it
                // is safe to call directly here).
                let window_id = handle.window_id();
                let _ = self
                    .virtual_keyboard
                    .update(cx, |keyboard, _cx| keyboard.register_window(window_id));
                midi_editor_debug(&format!(
                    "register virtual-keyboard window id={}",
                    window_id.as_u64()
                ));
                self.midi_editor.window = Some(handle);
                cx.notify();
            }
            Err(err) => eprintln!("[midi-editor] failed to open window: {err}"),
        }
    }

    pub(crate) fn close_midi_editor_window(&mut self, cx: &mut Context<Self>) {
        let _ = self.piano_roll_floating.update(cx, |roll, cx| {
            roll.preview_all_notes_off("editor_close", cx);
        });
        if let Some(handle) = self.midi_editor.window.take() {
            // Drop any musical-typing notes the popout still held, and never
            // touch the window handle after it is removed.
            self.unregister_virtual_keyboard_window(handle.window_id(), cx);
            let _ = handle.update(cx, |_w, window, _cx| window.remove_window());
        }
        cx.notify();
    }

    pub(super) fn note_midi_editor_window_closed(&mut self, cx: &mut Context<Self>) {
        let _ = self.piano_roll_floating.update(cx, |roll, cx| {
            roll.preview_all_notes_off("editor_close", cx);
        });
        if let Some(handle) = self.midi_editor.window.as_ref() {
            // The popout closed itself (titlebar X). Reading the stored id never
            // touches the dead window; unregister releases its notes safely.
            let window_id = handle.window_id();
            midi_editor_debug(&format!(
                "unregister virtual-keyboard window id={}",
                window_id.as_u64()
            ));
            self.unregister_virtual_keyboard_window(window_id, cx);
        }
        self.midi_editor.window = None;
        cx.notify();
    }

    pub(super) fn prune_midi_editor_window(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.midi_editor.window.clone() else {
            return;
        };
        if handle.update(cx, |_w, _window, _cx| ()).is_err() {
            self.midi_editor.window = None;
        }
    }
}
