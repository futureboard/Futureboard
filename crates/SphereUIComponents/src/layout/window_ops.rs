use gpui::{Bounds, Context, Window};

use std::sync::Arc;

use crate::components::add_track_dialog::{
    open_add_track_window, AddTrackDialogState, AddTrackKind,
};
use crate::components::combo_box::dedupe_preserve_order;
use crate::components::midi_editor_window::{midi_editor_debug, open_midi_editor_window};
use crate::components::settings_dialog::{open_settings_window, OnSettingUpdate};
use crate::components::timeline::timeline_state::{
    self, ClipType, CreateTrackOptions, InsertPluginFormat, TrackType,
};
use crate::components::{external_mixer_debug, open_mixer_window};
use crate::window_position::resolve_owner_bounds_with_preferred;
use sphere_plugin_host::{PluginFormat as RegistryPluginFormat, PluginKind};

use super::helpers::{cleaned_track_name, numbered_name_stem};
use super::{ContextTarget, OpenPopover, StudioLayout};
impl StudioLayout {
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
        if let Some(handle) = self.add_track_window.clone() {
            if handle
                .update(cx, |win, window, _cx| {
                    win.set_context(kind, track_count, has_master_track, default_monitor_mode);
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

        let owner_bounds =
            resolve_owner_bounds_with_preferred(owner_bounds, self.studio_window_bounds(cx), cx);

        if self.available_plugins.is_none()
            || !matches!(
                self.plugin_catalog_status,
                crate::components::plugin_picker::CatalogStatus::Ready
            )
        {
            self.arm_catalog_load(cx);
        }
        let instrument_plugins: Vec<sphere_plugin_host::RegistryPlugin> = self
            .available_plugins
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
            .unwrap_or_default();

        let layout = cx.entity().clone();
        let language = self.settings.read(cx).current.general.language.clone();
        let instrument_registry = instrument_plugins.clone();
        let on_confirm_request: Arc<dyn Fn(AddTrackDialogState, String, &mut gpui::App) + 'static> =
            Arc::new(move |dialog, _name, cx| {
                let Some(track_type) = dialog.selected_kind.native_track_type() else {
                    return;
                };
                let _ = layout.update(cx, |this, cx| {
                    this.mark_dirty();
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
                            if dialog.selected_kind == AddTrackKind::Instrument {
                                if let Some(plugin_id) = dialog.instrument_plugin_id.as_deref() {
                                    if let Some(reg) =
                                        instrument_registry.iter().find(|p| p.id == plugin_id)
                                    {
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
            Ok(handle) => self.add_track_window = Some(handle),
            Err(err) => eprintln!("[add-track] failed to open window: {err}"),
        }
    }

    pub(super) fn open_settings_dialog(
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

        let owner_bounds =
            resolve_owner_bounds_with_preferred(owner_bounds, self.studio_window_bounds(cx), cx);
        let settings = self.settings.clone();
        let owner = cx.entity().clone();

        let mut available_inputs = if let Some(ref engine) = self.audio_engine {
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

        let mut available_outputs = if let Some(ref engine) = self.audio_engine {
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
            .audio_engine
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
            .audio_engine
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

        let engine_for_latency = self.audio_engine.clone();
        let latency_provider: crate::components::settings_dialog::AudioLatencySnapshotProvider =
            Arc::new(move || {
                engine_for_latency
                    .as_ref()
                    .map(crate::settings::SettingsAudioLatencySnapshot::from_engine)
                    .unwrap_or_else(crate::settings::SettingsAudioLatencySnapshot::unavailable)
            });
        let input_test_start: Option<crate::components::settings_dialog::InputTestStartFn> =
            self.audio_engine.clone().map(|engine| {
                Arc::new(move |device_id: Option<String>| {
                    let device_id = device_id.filter(|id| !id.trim().is_empty());
                    engine
                        .start_input_test(device_id.as_deref())
                        .map_err(|error| error.to_string())
                }) as crate::components::settings_dialog::InputTestStartFn
            });
        let input_test_stop: Option<crate::components::settings_dialog::InputTestStopFn> =
            self.audio_engine.clone().map(|engine| {
                Arc::new(move || {
                    engine.stop_input_test();
                }) as crate::components::settings_dialog::InputTestStopFn
            });
        let input_test_level: Option<crate::components::settings_dialog::InputTestLevelFn> =
            self.audio_engine.clone().map(|engine| {
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
            Ok(handle) => self.settings_window = Some(handle),
            Err(err) => eprintln!("[settings] failed to open settings window: {err}"),
        }
    }

    pub(super) fn close_settings_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window.take() {
            let _ = handle.update(cx, |_settings, window, _cx| window.remove_window());
        }
        self.text_context_menu = None;
        cx.notify();
    }

    pub(crate) fn open_mixer_external_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        external_mixer_debug("external mixer open requested");
        let owner_bounds =
            resolve_owner_bounds_with_preferred(owner_bounds, self.studio_window_bounds(cx), cx);
        self.pending_mixer_external_open = owner_bounds;
        self.schedule_pending_mixer_external_open(cx);
        cx.notify();
    }

    pub(super) fn schedule_pending_mixer_external_open(&mut self, cx: &mut Context<Self>) {
        if self.pending_mixer_external_open.is_none() {
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
            self.pending_mixer_external_open.take(),
            self.studio_window_bounds(cx),
            cx,
        );
        let Some(owner_bounds) = owner_bounds else {
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
            dyn Fn(
                    crate::components::mixer_panel::MixerSplitAction,
                    &mut Window,
                    &mut gpui::App,
                ) + Send
                + Sync,
        > = std::sync::Arc::new(move |action, _w, cx| {
            let _ = split_owner.update(cx, |layout, cx| layout.apply_mixer_split_action(action, cx));
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

    pub(super) fn note_mixer_window_closed(&mut self, cx: &mut Context<Self>) {
        self.mixer_window = None;
        cx.notify();
    }

    pub(super) fn prune_mixer_window(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.mixer_window.clone() else {
            return;
        };
        if handle.update(cx, |_mixer, _window, _cx| ()).is_err() {
            self.mixer_window = None;
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
        self.pending_midi_editor_open = owner_bounds;
        self.schedule_pending_midi_editor_open(cx);
        cx.notify();
    }

    pub(super) fn schedule_pending_midi_editor_open(&mut self, cx: &mut Context<Self>) {
        if self.pending_midi_editor_open.is_none() {
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
            self.pending_midi_editor_open.take(),
            self.studio_window_bounds(cx),
            cx,
        );
        let Some(owner_bounds) = owner_bounds else {
            return;
        };

        if let Some(OpenPopover::Context {
            target: ContextTarget::Clip(clip_id),
            ..
        }) = self.open_popover.as_ref()
        {
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

        self.prune_midi_editor_window(cx);
        if let Some(handle) = self.midi_editor_window.clone() {
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
            self.midi_editor_window = None;
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
            on_close,
            dispatch_command,
            cx,
        ) {
            Ok(handle) => {
                self.midi_editor_window = Some(handle);
                cx.notify();
            }
            Err(err) => eprintln!("[midi-editor] failed to open window: {err}"),
        }
    }

    pub(crate) fn close_midi_editor_window(&mut self, cx: &mut Context<Self>) {
        let _ = self.piano_roll_floating.update(cx, |roll, cx| {
            roll.preview_all_notes_off("editor_close", cx);
        });
        if let Some(handle) = self.midi_editor_window.take() {
            let _ = handle.update(cx, |_w, window, _cx| window.remove_window());
        }
        cx.notify();
    }

    pub(super) fn note_midi_editor_window_closed(&mut self, cx: &mut Context<Self>) {
        let _ = self.piano_roll_floating.update(cx, |roll, cx| {
            roll.preview_all_notes_off("editor_close", cx);
        });
        self.midi_editor_window = None;
        cx.notify();
    }

    pub(super) fn prune_midi_editor_window(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.midi_editor_window.clone() else {
            return;
        };
        if handle.update(cx, |_w, _window, _cx| ()).is_err() {
            self.midi_editor_window = None;
        }
    }
}
