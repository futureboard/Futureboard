use gpui::{Context, KeyDownEvent, Window};

use crate::components::context_menu::ContextMenuEntry;
use crate::components::plugin_picker::{
    compute_filter_result, ensure_default_highlight, move_highlight, page_size_for_height,
    sync_selection_from_highlight, visible_plugin_id_at, PluginPickerState,
};
use crate::components::text_input::{TextInputAction, TextInputState};
use crate::components::timeline::timeline_state::ClipType;

use super::helpers::{is_supported_audio_ext, is_text_input_key};
use super::{ContextTarget, StudioLayout, TextMenuTarget};
impl StudioLayout {
    pub(super) fn project_switcher_visible_count(&self) -> usize {
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

    pub(super) fn handle_project_switcher_key(
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

    pub(super) fn handle_browser_key(
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

    pub(super) fn handle_settings_dialog_key(
        &mut self,
        _event: &KeyDownEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> bool {
        // Settings is now an external window that handles its own keyboard events.
        false
    }

    pub(super) fn handle_add_track_dialog_key(
        &mut self,
        _event: &KeyDownEvent,
        _window: &Window,
        _cx: &mut Context<Self>,
    ) -> bool {
        // Add Track is now an external window that handles its own keyboard events.
        false
    }

    pub(super) fn handle_plugin_picker_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.plugin_picker.is_open {
            return false;
        }
        if event.is_held {
            return true;
        }

        let modifiers = &event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();

        if key == "escape" {
            self.plugin_picker = PluginPickerState::closed();
            cx.notify();
            return true;
        }

        if (modifiers.control || modifiers.platform) && key.eq_ignore_ascii_case("f") {
            self.plugin_picker_search_input.focus_handle.focus(window);
            cx.notify();
            return true;
        }

        let Some(index) = self.plugin_search_index.clone() else {
            return self.handle_plugin_picker_text_input(event, window, cx);
        };

        let visible_len = compute_filter_result(
            &index,
            &self.plugin_picker.query,
            &self.plugin_picker.filters,
            &self.plugin_picker_prefs,
            std::env::var_os("FUTUREBOARD_PLUGIN_PICKER_DEBUG").is_some(),
        )
        .indices
        .len();

        match key {
            "enter" => {
                if let Some(id) =
                    visible_plugin_id_at(&self.plugin_picker, &index, &self.plugin_picker_prefs)
                {
                    self.apply_picked_insert(&id, cx);
                }
                return true;
            }
            "up" | "arrowup" => {
                move_highlight(&mut self.plugin_picker, -1, visible_len);
                sync_selection_from_highlight(
                    &mut self.plugin_picker,
                    &index,
                    &self.plugin_picker_prefs,
                );
                cx.notify();
                return true;
            }
            "down" | "arrowdown" => {
                move_highlight(&mut self.plugin_picker, 1, visible_len);
                sync_selection_from_highlight(
                    &mut self.plugin_picker,
                    &index,
                    &self.plugin_picker_prefs,
                );
                cx.notify();
                return true;
            }
            "home" => {
                self.plugin_picker.highlighted_index = 0;
                sync_selection_from_highlight(
                    &mut self.plugin_picker,
                    &index,
                    &self.plugin_picker_prefs,
                );
                cx.notify();
                return true;
            }
            "end" => {
                if visible_len > 0 {
                    self.plugin_picker.highlighted_index = visible_len - 1;
                    sync_selection_from_highlight(
                        &mut self.plugin_picker,
                        &index,
                        &self.plugin_picker_prefs,
                    );
                }
                cx.notify();
                return true;
            }
            "pageup" => {
                let page = page_size_for_height(self.plugin_picker_prefs.window_height);
                move_highlight(&mut self.plugin_picker, -(page as isize), visible_len);
                sync_selection_from_highlight(
                    &mut self.plugin_picker,
                    &index,
                    &self.plugin_picker_prefs,
                );
                cx.notify();
                return true;
            }
            "pagedown" => {
                let page = page_size_for_height(self.plugin_picker_prefs.window_height);
                move_highlight(&mut self.plugin_picker, page as isize, visible_len);
                sync_selection_from_highlight(
                    &mut self.plugin_picker,
                    &index,
                    &self.plugin_picker_prefs,
                );
                cx.notify();
                return true;
            }
            _ => self.handle_plugin_picker_text_input(event, window, cx),
        }
    }

    pub(super) fn handle_plugin_picker_text_input(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.plugin_picker_search_input.is_focused(window) || is_text_input_key(event) {
            let action = self
                .plugin_picker_search_input
                .handle_key_with_clipboard(event, Some(cx));
            self.sync_text_input_target(TextMenuTarget::PluginPickerSearch);
            if let Some(index) = self.plugin_search_index.as_ref() {
                ensure_default_highlight(&mut self.plugin_picker, index, &self.plugin_picker_prefs);
            }
            return !matches!(action, TextInputAction::Pass);
        }
        false
    }

    pub(super) fn text_input_mut(&mut self, target: TextMenuTarget) -> &mut TextInputState {
        match target {
            TextMenuTarget::ProjectSwitcherSearch => &mut self.project_switcher_search_input,
            TextMenuTarget::BrowserSearch => &mut self.browser_search_input,
            TextMenuTarget::PluginPickerSearch => &mut self.plugin_picker_search_input,
        }
    }

    pub(super) fn text_input(&self, target: TextMenuTarget) -> &TextInputState {
        match target {
            TextMenuTarget::ProjectSwitcherSearch => &self.project_switcher_search_input,
            TextMenuTarget::BrowserSearch => &self.browser_search_input,
            TextMenuTarget::PluginPickerSearch => &self.plugin_picker_search_input,
        }
    }

    pub(super) fn sync_text_input_target(&mut self, target: TextMenuTarget) {
        match target {
            TextMenuTarget::ProjectSwitcherSearch => {
                self.project_switcher.query = self.project_switcher_search_input.value.clone();
                self.project_switcher.selected_index = 0;
            }
            TextMenuTarget::BrowserSearch => {
                self.file_browser
                    .set_filter(&self.browser_search_input.value);
            }
            TextMenuTarget::PluginPickerSearch => {
                self.plugin_picker.query = self.plugin_picker_search_input.value.clone();
                if let Some(index) = self.plugin_search_index.as_ref() {
                    self.plugin_picker.highlighted_index = 0;
                    ensure_default_highlight(
                        &mut self.plugin_picker,
                        index,
                        &self.plugin_picker_prefs,
                    );
                }
            }
        }
    }

    pub(super) fn text_input_has_focus(&self, window: &Window) -> bool {
        self.project_switcher_search_input.is_focused(window)
            || self.browser_search_input.is_focused(window)
            || self.plugin_picker_search_input.is_focused(window)
    }

    /// Whether a *live* main-window text field currently owns the keyboard —
    /// i.e. its focus handle is focused AND its overlay is actually open.
    ///
    /// This differs from [`text_input_has_focus`] in that it does NOT trust a
    /// focused search handle whose overlay has closed: GPUI keeps a closed
    /// overlay's `FocusHandle` "focused" (the handle is still ref-counted) even
    /// though its element is no longer rendered. That orphaned focus is exactly
    /// what silently killed every keyboard shortcut — see `reclaim` in render.
    pub(super) fn keyboard_text_capture_live(&self, window: &Window) -> bool {
        (self.project_switcher.is_open && self.project_switcher_search_input.is_focused(window))
            || (self.plugin_picker.is_open && self.plugin_picker_search_input.is_focused(window))
            || self.browser_search_input.is_focused(window)
    }

    pub(super) fn context_entries(
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
                let clip_info = self.timeline.read(cx).state.find_clip(clip_id);
                let exists = clip_info.is_some();
                let is_midi =
                    clip_info.is_some_and(|(_, c)| matches!(c.clip_type, ClipType::Midi { .. }));
                let mut entries = Vec::new();
                if is_midi {
                    entries.push(ContextMenuEntry::item(
                        "Open in Bottom Editor",
                        "editor:open-bottom",
                    ));
                    entries.push(ContextMenuEntry::item(
                        "Open in New MIDI Editor Window",
                        "midi:open-editor",
                    ));
                    entries.push(ContextMenuEntry::Separator);
                }
                entries.push(ContextMenuEntry::disabled_item("Rename", "clip:rename"));
                entries.push(
                    ContextMenuEntry::item("Duplicate", "clip:duplicate").with_shortcut("Ctrl+D"),
                );
                entries.push(ContextMenuEntry::danger_item("Delete", "clip:delete"));
                entries.push(ContextMenuEntry::Separator);
                entries.push(ContextMenuEntry::item(
                    "Split at Playhead",
                    "clip:split-at-playhead",
                ));
                entries.push(ContextMenuEntry::disabled_item(
                    if exists {
                        "Reveal in Browser"
                    } else {
                        "Clip unavailable"
                    },
                    "browser:reveal",
                ));
                entries
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
                            entries.push(ContextMenuEntry::item(
                                "Reveal in Explorer/Finder",
                                "browser:reveal",
                            ));
                            entries.push(ContextMenuEntry::item("Refresh", "browser:refresh"));
                            entries.push(ContextMenuEntry::disabled_item(
                                "New Folder",
                                "browser:new-folder",
                            ));
                            entries
                                .push(ContextMenuEntry::disabled_item("Rename", "browser:rename"));
                            entries.push(ContextMenuEntry::item("Copy Path", "browser:copy-path"));
                        }
                    } else {
                        let ext = path
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_ascii_lowercase())
                            .unwrap_or_default();

                        if is_supported_audio_ext(&ext) {
                            entries.push(ContextMenuEntry::item(
                                "Import to Timeline",
                                "browser:import",
                            ));
                            entries.push(ContextMenuEntry::item(
                                "Reveal in Explorer/Finder",
                                "browser:reveal",
                            ));
                            entries.push(ContextMenuEntry::item("Copy Path", "browser:copy-path"));
                            entries
                                .push(ContextMenuEntry::disabled_item("Rename", "browser:rename"));
                        } else if ext == "fbproj" {
                            entries.push(ContextMenuEntry::item("Open Project", "project:open"));
                            entries.push(ContextMenuEntry::item(
                                "Reveal in Explorer/Finder",
                                "browser:reveal",
                            ));
                            entries.push(ContextMenuEntry::item("Copy Path", "browser:copy-path"));
                        } else {
                            entries.push(ContextMenuEntry::item(
                                "Reveal in Explorer/Finder",
                                "browser:reveal",
                            ));
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
}
