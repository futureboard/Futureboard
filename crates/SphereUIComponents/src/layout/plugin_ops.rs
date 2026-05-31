use gpui::{px, Bounds, Context, Window};

use crate::components::plugin_manager::open_plugin_manager_window;
use crate::components::plugin_picker::{
    ensure_default_highlight, PluginPickerState, STUB_PLUGIN_ID,
};
use sphere_plugin_host::{load_au_cache_state, CatalogLoad};

use super::{PluginCatalogStatus, PluginSearchIndex, StudioLayout};
impl StudioLayout {
    /// Lazily populated cache of registered audio plugins. First call
    /// runs `PluginRegistry::scan(None)` synchronously — the SQLite
    /// cache backing the registry makes subsequent scans fast. The UI
    /// thread blocks here on purpose; the audio thread is untouched.
    /// `None` return = registry has zero insert-capable plugins.
    /// Open the GPUI-hosted native editor window for an insert slot (Phase 4).
    /// GPUI owns a borderless shell; the C++ backend embeds the VST3 IPlugView
    /// in a native child region under it. If already open, this is a no-op (the
    /// window stays up). UI thread only; bad plugin → the editor window shows a
    /// fallback panel, never a crash.
    pub(super) fn open_insert_editor(
        &mut self,
        track_id: &str,
        insert_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::components::timeline::timeline_state::InsertPluginFormat;
        let debug = std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some();
        let key = (track_id.to_string(), insert_id.to_string());

        // One editor window per insert. If a live editor already exists for this
        // slot, focus/raise it instead of opening (or instantiating) a second
        // one. Only drop the handle when its window is actually gone.
        if let Some(handle) = self.open_plugin_editors.get(&key) {
            if handle
                .update(cx, |_, window, _| {
                    window.activate_window();
                })
                .is_ok()
            {
                if debug {
                    eprintln!(
                        "[plugin-view] existing editor found track={track_id} slot={insert_id} \
                         → focus (no new instance)"
                    );
                }
                return;
            }
            if debug {
                eprintln!("[plugin-view] stale editor handle track={track_id} slot={insert_id} → recreating");
            }
            self.open_plugin_editors.remove(&key);
        }

        let descriptor = {
            let timeline = self.timeline.read(cx);
            timeline.state.find_track(track_id).and_then(|t| {
                t.inserts.iter().find(|i| i.id == insert_id).map(|slot| {
                    (
                        slot.plugin_id.clone(),
                        slot.plugin_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned()),
                        slot.plugin_format,
                        slot.display_name.clone(),
                    )
                })
            })
        };
        let Some((plugin_id, plugin_path, plugin_format, display_name)) = descriptor else {
            if debug {
                eprintln!("[plugin-view] no slot track={track_id} slot={insert_id}");
            }
            return;
        };

        let path = plugin_path.filter(|p| !p.trim().is_empty());
        let editable = plugin_format == Some(InsertPluginFormat::Vst3)
            && path.is_some()
            && plugin_id.is_some();
        if !editable {
            if debug {
                eprintln!(
                    "[plugin-view] not editable track={track_id} slot={insert_id} fmt={plugin_format:?}"
                );
            }
            return;
        }

        // The editor attaches to the EXISTING runtime VST3 instance for this
        // insert — never a new component/controller. Look it up from the engine;
        // if the insert has no ready native processor, there is nothing to edit.
        let Some(engine) = self.audio_engine.as_ref() else {
            if debug {
                eprintln!("[plugin-view] no audio engine track={track_id} slot={insert_id}");
            }
            return;
        };
        let Some(processor) = engine.insert_processor(track_id, insert_id) else {
            if debug {
                eprintln!(
                    "[plugin-view] no ready runtime VST3 instance track={track_id} slot={insert_id} \
                     (insert not loaded / not native)"
                );
            }
            return;
        };

        let owner_bounds = window.bounds();
        match crate::components::plugin_editor_window::open_plugin_editor_window(
            owner_bounds,
            track_id.to_string(),
            insert_id.to_string(),
            display_name,
            processor,
            cx,
        ) {
            Ok(handle) => {
                self.open_plugin_editors.insert(key, handle);
                if debug {
                    eprintln!("[plugin-view] open track={track_id} slot={insert_id}");
                }
            }
            Err(err) => {
                if debug {
                    eprintln!(
                        "[plugin-view] open FAILED track={track_id} slot={insert_id} err={err}"
                    );
                }
            }
        }
    }

    /// Close the editor window for a slot if one is open. Idempotent. Removing
    /// the GPUI window drops the entity, which detaches the native view.
    pub(super) fn close_insert_editor(
        &mut self,
        track_id: &str,
        insert_id: &str,
        cx: &mut Context<Self>,
    ) {
        let key = (track_id.to_string(), insert_id.to_string());
        if let Some(handle) = self.open_plugin_editors.remove(&key) {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
            if std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some() {
                eprintln!("[plugin-view] close track={track_id} slot={insert_id}");
            }
        }
    }

    /// Close every open plugin editor and release native embed sessions before
    /// application exit (avoids HWND/VST3 teardown during TLS destruction).
    pub(super) fn shutdown_plugin_editors(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<(String, String)> = self.open_plugin_editors.keys().cloned().collect();
        for (track_id, insert_id) in keys {
            self.close_insert_editor(&track_id, &insert_id, cx);
        }
        sphere_plugin_host::native_editor::detach_all_embedded_editors();
    }

    /// Kick off a background SQLite load of the plug-in catalog. The picker
    /// opens instantly with a skeleton; this task replaces the skeleton once
    /// the catalog is read. Re-entrant: a second call while a load is in
    /// flight is a no-op.
    ///
    /// **Never** invokes the VST3/CLAP scanner; **never** touches plug-in
    /// binaries. The picker's open path must stay UI-only.
    pub(super) fn arm_catalog_load(&mut self, cx: &mut Context<Self>) {
        // Already loaded and not stale → nothing to do.
        if matches!(self.plugin_catalog_status, PluginCatalogStatus::Ready)
            && self.available_plugins.is_some()
        {
            return;
        }
        if matches!(self.plugin_catalog_status, PluginCatalogStatus::Loading)
            && self.available_plugins.is_none()
        {
            // Spawn-in-progress (initial boot path also fires this).
        } else {
            self.plugin_catalog_status = PluginCatalogStatus::Loading;
        }

        let debug = std::env::var_os("FUTUREBOARD_PLUGIN_PICKER_DEBUG").is_some()
            || std::env::var_os("FUTUREBOARD_PLUGIN_DB_DEBUG").is_some();
        let shell_started = std::time::Instant::now();

        cx.spawn(async move |this, cx| {
            let load = cx
                .background_executor()
                .spawn(async { sphere_plugin_host::PluginRegistry::load_catalog() })
                .await;
            let _ = this.update(cx, |this, cx| {
                match load {
                    CatalogLoad::Loaded { catalog, sqlite_ms } => {
                        let count = catalog.plugins.len();
                        let plugins: Vec<sphere_plugin_host::RegistryPlugin> = catalog
                            .plugins
                            .iter()
                            .map(|e| e.to_registry_plugin())
                            .collect();
                        this.available_plugins = Some(plugins.clone());
                        this.plugin_search_index = Some(PluginSearchIndex::from_plugins(plugins));
                        this.plugin_picker_au_error = load_au_cache_state().last_error;
                        this.plugin_cache_present = true;
                        this.plugin_catalog_status = PluginCatalogStatus::Ready;
                        if debug {
                            eprintln!(
                                "[plugin-db] loaded rows={count} sqlite_ms={sqlite_ms} path={} total_ms={}",
                                catalog.source_path.display(),
                                shell_started.elapsed().as_millis(),
                            );
                        }
                    }
                    CatalogLoad::MissingDatabase { path } => {
                        this.available_plugins = Some(Vec::new());
                        this.plugin_cache_present = false;
                        this.plugin_catalog_status = PluginCatalogStatus::MissingDatabase;
                        if debug {
                            eprintln!(
                                "[plugin-db] path={} exists=false",
                                path.display()
                            );
                        }
                    }
                    CatalogLoad::Error { path, message } => {
                        this.available_plugins = Some(Vec::new());
                        this.plugin_cache_present = path.exists();
                        this.plugin_catalog_status =
                            PluginCatalogStatus::Error(message.clone());
                        if debug {
                            eprintln!(
                                "[plugin-db] error path={} message={}",
                                path.display(),
                                message
                            );
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the Phase 2b insert picker for `track_id`. Loads from cached
    /// `.pst` index only (no VST3/CLAP scan, no plug-in binary read) so the
    /// overlay opens instantly even with 1000+ plug-ins. No insert slot is
    /// created until the user picks a plugin.
    pub(super) fn open_insert_picker(
        &mut self,
        track_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::components::timeline::timeline_state::TrackType;

        let debug = std::env::var_os("FUTUREBOARD_PLUGIN_PICKER_DEBUG").is_some();
        let started = std::time::Instant::now();
        let track_info = self
            .timeline
            .read(cx)
            .state
            .find_track(track_id)
            .map(|track| (track.name.clone(), track.track_type, track.inserts.len()));
        let (track_name, track_type, next_slot) =
            track_info.unwrap_or((track_id.to_string(), TrackType::Audio, 0));
        self.plugin_picker = PluginPickerState::open_for(
            track_id,
            &track_name,
            track_type,
            next_slot,
            self.plugin_picker_prefs.show_details,
        );
        self.plugin_picker_search_input.set_value("");
        self.plugin_picker.query = String::new();
        self.plugin_picker_search_input.focus_handle.focus(window);
        if let Some(index) = self.plugin_search_index.as_ref() {
            ensure_default_highlight(&mut self.plugin_picker, index, &self.plugin_picker_prefs);
        }
        // Kick off (or rejoin) the background SQLite load. Picker shell is
        // visible immediately; skeleton rows fill in until the catalog lands.
        if self.available_plugins.is_none()
            || !matches!(self.plugin_catalog_status, PluginCatalogStatus::Ready)
        {
            self.arm_catalog_load(cx);
        }
        if debug {
            let state_label = match &self.plugin_catalog_status {
                PluginCatalogStatus::Loading => "LoadingCatalog",
                PluginCatalogStatus::Ready => "Ready",
                PluginCatalogStatus::MissingDatabase => "MissingDatabase",
                PluginCatalogStatus::Error(_) => "Error",
            };
            eprintln!(
                "[plugin-picker] opened state={state_label} shell_ms={}",
                started.elapsed().as_millis()
            );
        }
        cx.notify();
    }

    /// Apply a picked plugin: append an insert slot to the picker's target
    /// track and bind the chosen descriptor. `plugin_id` is a
    /// `RegistryPlugin.id` or [`STUB_PLUGIN_ID`]. Closes the picker. No audio
    /// thread interaction — the next project sync carries the descriptor down.
    pub(super) fn apply_picked_insert(&mut self, plugin_id: &str, cx: &mut Context<Self>) {
        use crate::components::plugin_picker::validate_insert;
        use crate::components::timeline::timeline_state::InsertPluginFormat;
        use sphere_plugin_host::PluginFormat as RegFmt;

        let track_id = self.plugin_picker.insert_target.track_id.clone();
        if track_id.is_empty() {
            self.plugin_picker = PluginPickerState::closed();
            cx.notify();
            return;
        }

        if plugin_id != STUB_PLUGIN_ID {
            if let Some(plugins) = self.available_plugins.as_ref() {
                if let Some(reg) = plugins.iter().find(|p| p.id == plugin_id) {
                    if validate_insert(reg, &self.plugin_picker.insert_target)
                        != crate::components::plugin_picker::InsertValidation::Ok
                    {
                        cx.notify();
                        return;
                    }
                }
            }
        }

        let descriptor = if plugin_id == STUB_PLUGIN_ID {
            None
        } else {
            self.available_plugins
                .as_ref()
                .and_then(|plugins| plugins.iter().find(|p| p.id == plugin_id))
                .map(|reg| {
                    let format = match reg.format {
                        RegFmt::Vst3 => InsertPluginFormat::Vst3,
                        RegFmt::Clap => InsertPluginFormat::Clap,
                        RegFmt::Au => InsertPluginFormat::Au,
                        RegFmt::Lv2 => InsertPluginFormat::Lv2,
                        _ => InsertPluginFormat::Unknown,
                    };
                    let id = reg.class_id.clone().unwrap_or_else(|| reg.id.clone());
                    (id, Some(reg.path.clone()), format, reg.name.clone())
                })
        };
        let (plugin_id_out, plugin_path, plugin_format, display_name) =
            descriptor.unwrap_or_else(|| {
                (
                    STUB_PLUGIN_ID.to_string(),
                    None,
                    InsertPluginFormat::Vst3,
                    "Stub Effect".to_string(),
                )
            });

        let new_slot_id = self
            .timeline
            .update(cx, |timeline, _cx| timeline.state.add_insert(&track_id));
        if let Some(slot_id) = new_slot_id {
            self.timeline.update(cx, |timeline, _cx| {
                timeline.state.set_insert_plugin(
                    &track_id,
                    &slot_id,
                    plugin_id_out,
                    plugin_path,
                    plugin_format,
                    display_name,
                );
            });
            self.mark_dirty();
            self.engine_project_dirty = true;
            if plugin_id != STUB_PLUGIN_ID {
                self.plugin_picker_prefs.record_recent(plugin_id);
            }
        }
        self.plugin_picker = PluginPickerState::closed();
        cx.notify();
    }

    pub(super) fn open_plugin_manager_external_window(
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
}
