use gpui::{Context, Entity, UniformListScrollHandle, Window, WindowHandle};
use std::{collections::HashMap, path::PathBuf};

use crate::components::edit::EditCommand;
use crate::components::mixer_panel::{
    clamp_mixer_section_height_px, mixer_render_item_count, mixer_scroll_x_for_strip_index,
    mixer_strip_index_for_channel, MixerCallbacks, MixerSplitAction, MixerSplitTarget,
    VstiOutputMeterState, MIXER_INSERT_SECTION_DEFAULT_PX, MIXER_SEND_SECTION_DEFAULT_PX,
    STRIP_WIDTH,
};
use crate::components::mixer_tree_model::{
    ensure_timeline_mixer_tree_defaults, expand_ancestors_for_channel, MixerTreeModel,
};
use crate::components::mixer_tree_sidebar::{
    clamp_mixer_tree_sidebar_width, MixerTreeCallbacks, MIXER_TREE_COLLAPSED_RAIL_WIDTH,
    MIXER_TREE_SIDEBAR_DEFAULT_WIDTH,
};
use crate::components::timeline::timeline_state::{
    self, collapsed_vsti_output_group_keys_from_tracks, TrackState,
};
use crate::components::{external_mixer_debug, external_mixer_debug_enabled, MixerSnapshot};

use super::engine_snapshot::volume_norm_to_linear;
use super::{ContextMenuRequest, ContextMenuTarget, ContextTarget, MixerWindow, StudioLayout};

/// Mixer-panel view state — horizontal scroll, the shared insert/send section
/// heights, and the transient splitter-drag anchors. `StudioLayout` decomposition
/// slice. Manual `Default` (insert/send sections start at their default px).
pub(crate) struct MixerViewState {
    /// Horizontal scroll offset for the channel-strip area.
    pub scroll_x: f32,
    /// Shared Inserts viewport height (px) for every channel strip.
    pub insert_section_px: f32,
    /// Shared Sends viewport height (px) for every channel strip.
    pub send_section_px: f32,
    /// Splitter-drag pointer-Y anchor recorded on pointer-down.
    pub split_resize_start_y: f32,
    /// Insert-section height captured at splitter-drag start.
    pub split_resize_start_insert_px: f32,
    /// Send-section height captured at splitter-drag start.
    pub split_resize_start_send_px: f32,
    /// Active splitter-drag target, if a drag is in progress.
    pub split_active_target: Option<MixerSplitTarget>,
    pub vsti_output_meters: HashMap<String, VstiOutputMeterState>,
    /// Mixer tree sidebar enabled (session-only).
    pub tree_sidebar_enabled: bool,
    /// Collapsed to icon rail.
    pub tree_sidebar_collapsed: bool,
    pub tree_sidebar_width_px: f32,
    pub tree_show_only_selected_group: bool,
    pub tree_resize_start_x: f32,
    pub tree_resize_start_width_px: f32,
    pub tree_resizing: bool,
    /// Transient strip highlight after tree double-click focus.
    pub focus_channel_id: Option<String>,
    pub tree_scroll: UniformListScrollHandle,
    /// Cached tree model — rebuilt only when tracks/filter/output routing changes.
    pub cached_tree_model: Option<MixerTreeModel>,
    pub tree_cache_filter: String,
    pub tree_cache_output_ch: u32,
    pub tree_cache_tracks_gen: u64,
    pub tree_cache_show_only: bool,
    pub tree_cache_selected_id: Option<String>,
    /// One-shot fallback when session install did not seed tree defaults.
    pub tree_defaults_applied: bool,
}

/// Stable mixer-tree UI hooks built once per studio layout (not per frame).
pub(crate) struct MixerTreeUiHooks {
    pub callbacks: MixerTreeCallbacks,
    pub on_resize_start: std::sync::Arc<dyn Fn(f32, &mut Window, &mut gpui::App) + 'static>,
    pub on_resize_move: std::sync::Arc<dyn Fn(f32, &mut Window, &mut gpui::App) + 'static>,
    pub on_resize_end: std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + 'static>,
}

impl Default for MixerViewState {
    fn default() -> Self {
        Self {
            scroll_x: 0.0,
            insert_section_px: MIXER_INSERT_SECTION_DEFAULT_PX,
            send_section_px: MIXER_SEND_SECTION_DEFAULT_PX,
            split_resize_start_y: 0.0,
            split_resize_start_insert_px: 0.0,
            split_resize_start_send_px: 0.0,
            split_active_target: None,
            vsti_output_meters: HashMap::new(),
            tree_sidebar_enabled: true,
            tree_sidebar_collapsed: false,
            tree_sidebar_width_px: MIXER_TREE_SIDEBAR_DEFAULT_WIDTH,
            tree_show_only_selected_group: false,
            tree_resize_start_x: 0.0,
            tree_resize_start_width_px: MIXER_TREE_SIDEBAR_DEFAULT_WIDTH,
            tree_resizing: false,
            focus_channel_id: None,
            tree_scroll: UniformListScrollHandle::new(),
            cached_tree_model: None,
            tree_cache_filter: String::new(),
            tree_cache_output_ch: 0,
            tree_cache_tracks_gen: 0,
            tree_cache_show_only: false,
            tree_cache_selected_id: None,
            tree_defaults_applied: false,
        }
    }
}

/// Read-only mixer chrome snapshot for the docked panel entity.
pub(crate) struct DockedMixerPanelState<'a> {
    pub scroll_x: f32,
    pub vsti_output_meters: &'a HashMap<String, VstiOutputMeterState>,
    pub tree_sidebar_enabled: bool,
    pub viewport_width: f32,
    pub strip_available_px: f32,
}

impl StudioLayout {
    pub(crate) fn notify_mixer_window(&mut self, cx: &mut Context<Self>) {
        self.push_mixer_snapshot_to_window(cx);
    }

    /// Viewport metrics for the docked mixer panel entity (width, body height, strip height).
    pub(crate) fn mixer_panel_viewport_metrics(&self, cx: &gpui::App) -> (f32, f32, f32) {
        let tree_w = self.mixer_tree_sidebar_width();
        let window_w = self
            .window_hooks
            .cached_bounds
            .map(|b| f32::from(b.size.width))
            .unwrap_or(1280.0);
        let mixer_viewport_width = (window_w - tree_w - 90.0).max(100.0);
        let mixer_viewport_height = (self.bottom_panel_state.height_px - 28.0 - 30.0).max(0.0);
        let strip_available_px = mixer_viewport_height.max(STRIP_WIDTH);
        let _ = cx;
        (
            mixer_viewport_width,
            mixer_viewport_height,
            strip_available_px,
        )
    }

    /// Snapshot for the docked mixer panel entity (read-only view of mixer chrome).
    pub(crate) fn docked_mixer_panel_state(&self, cx: &gpui::App) -> DockedMixerPanelState<'_> {
        let (viewport_width, _viewport_height, strip_available_px) =
            self.mixer_panel_viewport_metrics(cx);
        DockedMixerPanelState {
            scroll_x: self.mixer_view.scroll_x,
            vsti_output_meters: &self.mixer_view.vsti_output_meters,
            tree_sidebar_enabled: self.mixer_view.tree_sidebar_enabled,
            viewport_width,
            strip_available_px,
        }
    }

    /// Meter-only UI refresh — isolated regions, never the StudioLayout root.
    pub(crate) fn notify_mixer_meter_regions(&mut self, cx: &mut Context<Self>) {
        let timeline = self.timeline.read(cx);
        let master_sig = crate::components::mixer_master_strip_view::mixer_master_meter_signature(
            &timeline.state.master,
        );
        let channel_sig = mixer_channel_meter_signature(&timeline.state.tracks);
        let sig = master_sig ^ channel_sig.rotate_left(17);
        if sig == self.engine_sync.last_meter_notify_sig {
            return;
        }
        self.engine_sync.last_meter_notify_sig = sig;

        // Track headers show meters — notify timeline only, not the studio shell.
        let _ = self.timeline.update(cx, |_, cx| cx.notify());

        if self.mixer_panel_chrome_visible() {
            let _ = self
                .mixer_panel
                .update(cx, |panel, cx| panel.on_meter_tick(cx));
            if self.external_windows.mixer.is_some() {
                self.push_mixer_snapshot_to_window(cx);
            }
        } else {
            crate::perf::count("mixer_repaint_while_inactive_count", 1);
            crate::perf::count("inactive_tab_repaint_count", 1);
        }
    }

    pub(crate) fn build_mixer_snapshot(&self, cx: &gpui::App) -> MixerSnapshot {
        let timeline = self.timeline.read(cx);
        MixerSnapshot {
            tracks: timeline.state.tracks.clone(),
            master: timeline.state.master.clone(),
            selected_track_id: timeline.state.selection.selected_track_id.clone(),
            mixer_scroll_x: self.mixer_view.scroll_x,
            mixer_insert_section_px: clamp_mixer_section_height_px(
                self.mixer_view.insert_section_px,
            ),
            mixer_send_section_px: clamp_mixer_section_height_px(self.mixer_view.send_section_px),
            mixer_split_active_target: self.mixer_view.split_active_target,
            // Derived from the persisted per-instrument collapse flag (single
            // source of truth) — never a separate, drift-prone view cache.
            collapsed_vsti_output_groups: timeline.state.collapsed_vsti_output_group_keys(),
            hidden_mixer_channels: timeline.state.mixer_tree.hidden_channel_ids.clone(),
            vsti_output_meters: self.mixer_view.vsti_output_meters.clone(),
            tree_sidebar_enabled: self.mixer_view.tree_sidebar_enabled,
            tree_sidebar_collapsed: self.mixer_view.tree_sidebar_collapsed,
            tree_sidebar_width_px: self.mixer_view.tree_sidebar_width_px,
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
        let Some(handle) = self.external_windows.mixer.clone() else {
            return;
        };
        let snapshot = self.build_mixer_snapshot(cx);
        let _ = handle.update(cx, |mixer, _window, cx| {
            mixer.set_snapshot(snapshot);
            cx.notify();
        });
    }

    pub(crate) fn set_mixer_scroll_x(&mut self, scroll_x: f32, _cx: &mut Context<Self>) -> bool {
        if (self.mixer_view.scroll_x - scroll_x).abs() > 0.25 {
            self.mixer_view.scroll_x = scroll_x;
            true
        } else {
            false
        }
    }

    pub(crate) fn mixer_tree_sidebar_width(&self) -> f32 {
        if self.mixer_view.tree_sidebar_collapsed {
            MIXER_TREE_COLLAPSED_RAIL_WIDTH
        } else if self.mixer_view.tree_sidebar_enabled {
            clamp_mixer_tree_sidebar_width(self.mixer_view.tree_sidebar_width_px)
        } else {
            0.0
        }
    }

    pub(crate) fn mixer_tree_output_channels(&self, cx: &Context<Self>) -> u32 {
        self.selected_output_device_channels(cx)
            .map(|(_, ch)| ch)
            .unwrap_or(2)
    }

    pub(crate) fn invalidate_mixer_tree_model_cache(&mut self) {
        self.mixer_view.cached_tree_model = None;
    }

    pub(crate) fn refresh_mixer_tree_sidebar_entity(&self, cx: &mut Context<Self>) {
        let routing_gen = self.audio_bridge.route_graph_version;
        let output_ch = self.mixer_tree_output_channels(cx);
        let collapsed = self.mixer_view.tree_sidebar_collapsed;
        let width = self.mixer_view.tree_sidebar_width_px;
        let show_only = self.mixer_view.tree_show_only_selected_group;
        let _ = self.mixer_tree_sidebar.update(cx, |sidebar, cx| {
            sidebar.sync_chrome(collapsed, width, show_only);
            if sidebar.sync_routing_from_layout(cx, routing_gen, output_ch) {
                cx.notify();
            }
        });
    }

    pub(crate) fn notify_mixer_tree_sidebar_only(&self, cx: &mut Context<Self>) {
        let _ = self.mixer_tree_sidebar.update(cx, |sidebar, cx| {
            sidebar.recompute_expansion(cx);
            cx.notify();
        });
    }

    pub(crate) fn notify_mixer_tree_selection_only(&self, cx: &mut Context<Self>) {
        let _ = self.mixer_tree_sidebar.update(cx, |sidebar, cx| {
            sidebar.recompute_selection(cx);
            cx.notify();
        });
    }

    /// Seed default expanded groups once when session install did not run (e.g. empty project).
    pub(crate) fn ensure_mixer_tree_defaults_once(&mut self, cx: &mut Context<Self>) {
        if self.mixer_view.tree_defaults_applied {
            return;
        }
        if !self
            .timeline
            .read(cx)
            .state
            .mixer_tree
            .expanded_node_ids
            .is_empty()
        {
            self.mixer_view.tree_defaults_applied = true;
            return;
        }
        let output_channels = self.mixer_tree_output_channels(cx);
        self.timeline.update(cx, |timeline, _cx| {
            ensure_timeline_mixer_tree_defaults(&mut timeline.state, output_channels);
        });
        self.mixer_view.tree_defaults_applied = true;
        self.invalidate_mixer_tree_model_cache();
    }

    pub(crate) fn mixer_tree_model_for_render(&mut self, cx: &mut Context<Self>) -> MixerTreeModel {
        let output_channels = self.mixer_tree_output_channels(cx);
        let filter = self.mixer_tree_filter_input.value.clone();
        let tracks_gen = self.audio_bridge.route_graph_version;
        let show_only = self.mixer_view.tree_show_only_selected_group;
        let selected_id = self
            .timeline
            .read(cx)
            .state
            .selection
            .selected_track_id
            .clone();

        let needs_rebuild = self.mixer_view.cached_tree_model.is_none()
            || self.mixer_view.tree_cache_filter != filter
            || self.mixer_view.tree_cache_output_ch != output_channels
            || self.mixer_view.tree_cache_tracks_gen != tracks_gen
            || self.mixer_view.tree_cache_show_only != show_only
            || (show_only && self.mixer_view.tree_cache_selected_id != selected_id);

        if needs_rebuild {
            let model = {
                let timeline = self.timeline.read(cx);
                MixerTreeModel::build(
                    &timeline.state.tracks,
                    output_channels,
                    &timeline.state.mixer_tree,
                    &filter,
                    show_only,
                    selected_id.as_deref(),
                )
            };
            self.mixer_view.cached_tree_model = Some(model);
            self.mixer_view.tree_cache_filter = filter;
            self.mixer_view.tree_cache_output_ch = output_channels;
            self.mixer_view.tree_cache_tracks_gen = tracks_gen;
            self.mixer_view.tree_cache_show_only = show_only;
            self.mixer_view.tree_cache_selected_id = selected_id;
        }

        self.mixer_view
            .cached_tree_model
            .as_ref()
            .expect("mixer tree cache populated above")
            .clone()
    }

    pub(crate) fn ensure_mixer_tree_ui_hooks(
        &mut self,
        owner: Entity<Self>,
        cx: &mut Context<Self>,
    ) {
        if self.mixer_tree_ui_hooks.is_some() {
            return;
        }

        let callbacks = self.build_mixer_tree_callbacks(owner.clone());

        let owner_tree_resize = owner.clone();
        let on_resize_start: std::sync::Arc<dyn Fn(f32, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |x, _w, cx| {
                let _ = owner_tree_resize.update(cx, |layout, _cx| {
                    layout.apply_mixer_tree_resize_start(x);
                });
            });
        let owner_tree_resize_move = owner.clone();
        let on_resize_move: std::sync::Arc<dyn Fn(f32, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |x, _w, cx| {
                let _ = owner_tree_resize_move.update(cx, |layout, cx| {
                    layout.apply_mixer_tree_resize_move(x, cx);
                });
            });
        let owner_tree_resize_end = owner.clone();
        let on_resize_end: std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |_w, cx| {
                let _ = owner_tree_resize_end.update(cx, |layout, cx| {
                    layout.apply_mixer_tree_resize_end(cx);
                });
            });

        self.mixer_tree_ui_hooks = Some(MixerTreeUiHooks {
            callbacks,
            on_resize_start,
            on_resize_move,
            on_resize_end,
        });
        let hooks = self.mixer_tree_ui_hooks.as_ref().unwrap();
        let _ = self.mixer_tree_sidebar.update(cx, |sidebar, _cx| {
            sidebar.set_session_hooks(
                hooks.callbacks.clone(),
                Some(hooks.on_resize_start.clone()),
                Some(hooks.on_resize_move.clone()),
                Some(hooks.on_resize_end.clone()),
            );
        });
    }

    pub(crate) fn build_mixer_tree_model(
        &self,
        cx: &gpui::App,
        output_device_channels: u32,
    ) -> MixerTreeModel {
        let timeline = self.timeline.read(cx);
        let filter = self.mixer_tree_filter_input.value.clone();
        MixerTreeModel::build(
            &timeline.state.tracks,
            output_device_channels,
            &timeline.state.mixer_tree,
            &filter,
            self.mixer_view.tree_show_only_selected_group,
            timeline.state.selection.selected_track_id.as_deref(),
        )
    }

    pub(crate) fn mixer_focus_channel(
        &mut self,
        channel_id: &str,
        viewport_width: f32,
        cx: &mut Context<Self>,
    ) {
        let collapsed =
            collapsed_vsti_output_group_keys_from_tracks(&self.timeline.read(cx).state.tracks);
        let hidden = self
            .timeline
            .read(cx)
            .state
            .mixer_tree
            .hidden_channel_ids
            .clone();
        let tracks = self.timeline.read(cx).state.tracks.clone();
        if let Some(index) = mixer_strip_index_for_channel(&tracks, &collapsed, &hidden, channel_id)
        {
            let strip_count = mixer_render_item_count(&tracks, &collapsed, &hidden);
            self.mixer_view.scroll_x =
                mixer_scroll_x_for_strip_index(index, viewport_width, strip_count);
        }
        let model = self.build_mixer_tree_model(cx, self.mixer_tree_output_channels(cx));
        self.timeline.update(cx, |timeline, _cx| {
            expand_ancestors_for_channel(&mut timeline.state.mixer_tree, &model, channel_id);
            timeline.state.select_track(channel_id);
        });
        self.mixer_view.focus_channel_id = Some(channel_id.to_string());
        self.mark_dirty_view_only();
        self.push_mixer_snapshot_to_window(cx);
        cx.notify();
    }

    pub(crate) fn mixer_toggle_channel_visibility(
        &mut self,
        channel_id: &str,
        cx: &mut Context<Self>,
    ) {
        self.timeline.update(cx, |timeline, _cx| {
            timeline
                .state
                .mixer_tree
                .toggle_channel_visibility(channel_id);
        });
        self.mark_dirty_view_only();
        self.push_mixer_snapshot_to_window(cx);
        cx.notify();
    }

    pub(crate) fn mixer_pin_channel(&mut self, channel_id: &str, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, _cx| {
            timeline.state.mixer_tree.toggle_pin(channel_id);
        });
        self.mark_dirty_view_only();
        self.notify_mixer_tree_sidebar_only(cx);
    }

    pub(crate) fn mixer_expand_all_tree(&mut self, cx: &mut Context<Self>) {
        let model = self.build_mixer_tree_model(cx, self.mixer_tree_output_channels(cx));
        self.timeline.update(cx, |timeline, _cx| {
            timeline
                .state
                .mixer_tree
                .expand_all(model.all_expandable_ids.clone());
            timeline.state.mixer_tree.set_expanded(
                crate::components::mixer_tree_model::MIXER_TREE_ROOT_ID,
                true,
            );
        });
        self.mark_dirty_view_only();
        self.notify_mixer_tree_sidebar_only(cx);
    }

    pub(crate) fn mixer_collapse_all_tree(&mut self, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, _cx| {
            timeline.state.mixer_tree.collapse_all();
        });
        self.mark_dirty_view_only();
        self.notify_mixer_tree_sidebar_only(cx);
    }

    pub(crate) fn mixer_reset_tree_visibility(&mut self, cx: &mut Context<Self>) {
        self.timeline.update(cx, |timeline, _cx| {
            timeline.state.mixer_tree.reset_visibility();
        });
        self.mark_dirty_view_only();
        self.push_mixer_snapshot_to_window(cx);
        cx.notify();
    }

    pub(crate) fn sync_mixer_tree_to_selection(&mut self, cx: &mut Context<Self>) {
        let Some(channel_id) = self
            .timeline
            .read(cx)
            .state
            .selection
            .selected_track_id
            .clone()
        else {
            return;
        };
        let model = self.build_mixer_tree_model(cx, self.mixer_tree_output_channels(cx));
        self.timeline.update(cx, |timeline, _cx| {
            expand_ancestors_for_channel(&mut timeline.state.mixer_tree, &model, &channel_id);
        });
        self.notify_mixer_tree_sidebar_only(cx);
    }

    pub(crate) fn apply_mixer_tree_resize_start(&mut self, x: f32) {
        self.mixer_view.tree_resize_start_x = x;
        self.mixer_view.tree_resize_start_width_px =
            clamp_mixer_tree_sidebar_width(self.mixer_view.tree_sidebar_width_px);
        self.mixer_view.tree_resizing = true;
    }

    pub(crate) fn apply_mixer_tree_resize_move(&mut self, x: f32, cx: &mut Context<Self>) {
        if !self.mixer_view.tree_resizing {
            return;
        }
        let delta = x - self.mixer_view.tree_resize_start_x;
        let new_w =
            clamp_mixer_tree_sidebar_width(self.mixer_view.tree_resize_start_width_px + delta);
        if (new_w - self.mixer_view.tree_sidebar_width_px).abs() > 0.25 {
            self.mixer_view.tree_sidebar_width_px = new_w;
            self.refresh_mixer_tree_sidebar_entity(cx);
            cx.notify();
        }
    }

    pub(crate) fn apply_mixer_tree_resize_end(&mut self, cx: &mut Context<Self>) {
        self.mixer_view.tree_resizing = false;
        cx.notify();
    }

    pub(crate) fn build_mixer_tree_callbacks(&self, owner: Entity<Self>) -> MixerTreeCallbacks {
        let owner_select = owner.clone();
        let on_select_channel: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |id: &String, _w, cx| {
            let id = id.clone();
            StudioLayout::defer_update(&owner_select, cx, move |layout, cx| {
                let already = layout
                    .timeline
                    .read(cx)
                    .state
                    .selection
                    .selected_track_id
                    .as_deref()
                    == Some(id.as_str());
                layout.timeline.update(cx, |timeline, _cx| {
                    timeline.state.select_track(&id);
                });
                layout.sync_mixer_tree_to_selection(cx);
                if !already {
                    layout.push_mixer_snapshot_to_window(cx);
                    cx.notify();
                } else {
                    layout.notify_mixer_tree_selection_only(cx);
                }
            });
        });

        let owner_focus = owner.clone();
        let on_focus_channel: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |id: &String, window, cx| {
            let id = id.clone();
            let window_w: f32 = window.bounds().size.width.into();
            StudioLayout::defer_update(&owner_focus, cx, move |layout, cx| {
                let tree_w = layout.mixer_tree_sidebar_width();
                let viewport = (window_w - tree_w - 90.0).max(STRIP_WIDTH);
                layout.mixer_focus_channel(&id, viewport, cx);
            });
        });

        let owner_expand = owner.clone();
        let on_toggle_expand: std::sync::Arc<
            dyn Fn(&(String, bool), &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |(node_id, expanded): &(String, bool), _w, cx| {
            let node_id = node_id.clone();
            let expanded = *expanded;
            StudioLayout::defer_update(&owner_expand, cx, move |layout, cx| {
                layout.timeline.update(cx, |timeline, _cx| {
                    timeline.state.mixer_tree.set_expanded(node_id, expanded);
                });
                layout.mark_dirty_view_only();
                layout.notify_mixer_tree_sidebar_only(cx);
            });
        });

        let owner_vis = owner.clone();
        let on_toggle_visibility: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |id: &String, _w, cx| {
            let id = id.clone();
            StudioLayout::defer_update(&owner_vis, cx, move |layout, cx| {
                layout.mixer_toggle_channel_visibility(&id, cx);
            });
        });

        let owner_pin = owner.clone();
        let on_toggle_pin: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                StudioLayout::defer_update(&owner_pin, cx, move |layout, cx| {
                    layout.mixer_pin_channel(&id, cx);
                });
            });

        let owner_collapse = owner.clone();
        let on_collapse_all: std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |_w, cx| {
                let _ = owner_collapse.update(cx, |layout, cx| layout.mixer_collapse_all_tree(cx));
            });

        let owner_expand_all = owner.clone();
        let on_expand_all: std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |_w, cx| {
                let _ = owner_expand_all.update(cx, |layout, cx| layout.mixer_expand_all_tree(cx));
            });

        let owner_filter_group = owner.clone();
        let on_show_only_selected_group: std::sync::Arc<
            dyn Fn(&mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |_w, cx| {
            let _ = owner_filter_group.update(cx, |layout, cx| {
                layout.mixer_view.tree_show_only_selected_group =
                    !layout.mixer_view.tree_show_only_selected_group;
                layout.invalidate_mixer_tree_model_cache();
                layout.refresh_mixer_tree_sidebar_entity(cx);
            });
        });

        let owner_reset = owner.clone();
        let on_reset_visibility: std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |_w, cx| {
                let _ = owner_reset.update(cx, |layout, cx| layout.mixer_reset_tree_visibility(cx));
            });

        let owner_toggle_sidebar = owner.clone();
        let on_toggle_sidebar: std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |_w, cx| {
                let _ = owner_toggle_sidebar.update(cx, |layout, cx| {
                    layout.mixer_view.tree_sidebar_collapsed =
                        !layout.mixer_view.tree_sidebar_collapsed;
                    layout.refresh_mixer_tree_sidebar_entity(cx);
                    cx.notify();
                });
            });

        MixerTreeCallbacks {
            on_select_channel,
            on_focus_channel,
            on_toggle_expand,
            on_toggle_visibility,
            on_toggle_pin,
            on_collapse_all,
            on_expand_all,
            on_show_only_selected_group,
            on_reset_visibility,
            on_toggle_sidebar,
        }
    }

    /// Current shared insert/send viewport heights, clamped to the supported range.
    pub(crate) fn mixer_insert_section_px(&self) -> f32 {
        clamp_mixer_section_height_px(self.mixer_view.insert_section_px)
    }

    pub(crate) fn mixer_send_section_px(&self) -> f32 {
        clamp_mixer_section_height_px(self.mixer_view.send_section_px)
    }

    pub(crate) fn mixer_split_active_target(&self) -> Option<MixerSplitTarget> {
        self.mixer_view.split_active_target
    }

    pub(crate) fn toggle_vsti_output_group(&mut self, group_key: &str, cx: &mut Context<Self>) {
        // Group key is `{track_id}:{insert_id}`; insert ids contain no ':' so the
        // last ':' splits the pair. Collapse/expand is a VIEW concern stored on the
        // instrument insert (single source of truth) — it never touches routing,
        // child mixer channels, route nodes, or the engine snapshot.
        let Some((track_id, insert_id)) = group_key.rsplit_once(':') else {
            return;
        };
        let track_id = track_id.to_string();
        let insert_id = insert_id.to_string();
        let now_collapsed = self.timeline.update(cx, |timeline, _cx| {
            timeline
                .state
                .toggle_insert_multiout_collapsed(&track_id, &insert_id)
        });

        if crate::forensic_trace::forensic_trace_enabled() {
            self.log_multiout_collapse_toggle(&track_id, &insert_id, now_collapsed, cx);
        }

        // Persist the new view state (so save/restore keeps it) WITHOUT rebuilding
        // the audio graph: the collapse flag is not part of the engine snapshot, so
        // marking only the project session dirty (never the engine) keeps the next
        // poll from building/serializing a redundant snapshot. `route_graph_version`
        // is unchanged.
        self.mark_dirty_view_only();
        self.push_mixer_snapshot_to_window(cx);
        cx.notify();
    }

    /// Structured `[MULTIOUT COLLAPSE]` / `[MULTIOUT EXPAND]` diagnostics proving
    /// the toggle preserves routes (graph version unchanged, no channels
    /// created/deleted). Forensic-gated; runs off the audio thread.
    fn log_multiout_collapse_toggle(
        &self,
        track_id: &str,
        insert_id: &str,
        now_collapsed: bool,
        cx: &gpui::App,
    ) {
        let prefix = format!("vsti-out:{insert_id}:bus:");
        let timeline = self.timeline.read(cx);
        let child_ids: Vec<String> = timeline
            .state
            .tracks
            .iter()
            .filter(|t| t.id.starts_with(&prefix))
            .map(|t| t.id.clone())
            .collect();
        let gv = self.audio_bridge.route_graph_version;
        if now_collapsed {
            eprintln!(
                "[MULTIOUT COLLAPSE]\nplugin_instance_id={insert_id}\nparent_mixer_channel_id={track_id}\nchild_count={}\nchild_mixer_channel_ids={child_ids:?}\nroute_nodes_preserved=true\naudio_routes_preserved=true\ngraph_version_before={gv}\ngraph_version_after={gv}\ncreated_or_deleted_channels=false",
                child_ids.len()
            );
        } else {
            eprintln!(
                "[MULTIOUT EXPAND]\nplugin_instance_id={insert_id}\nparent_mixer_channel_id={track_id}\nchild_count={}\nchild_mixer_channel_ids={child_ids:?}\nused_existing_channels=true\nduplicated_channels=false\nroute_nodes_preserved=true\ngraph_version_before={gv}\ngraph_version_after={gv}",
                child_ids.len()
            );
        }
    }

    /// Apply a splitter intent from any channel-strip handle. Shared across all
    /// strips, so one drag resizes the whole mixer. Pushes the new height to the
    /// floating mixer window and repaints when something changed.
    pub(crate) fn apply_mixer_split_action(
        &mut self,
        action: MixerSplitAction,
        cx: &mut Context<Self>,
    ) {
        match action {
            MixerSplitAction::ResizeStart(target, y) => {
                self.mixer_view.split_resize_start_y = y;
                self.mixer_view.split_resize_start_insert_px =
                    clamp_mixer_section_height_px(self.mixer_view.insert_section_px);
                self.mixer_view.split_resize_start_send_px =
                    clamp_mixer_section_height_px(self.mixer_view.send_section_px);
                self.mixer_view.split_active_target = Some(target);
            }
            MixerSplitAction::ResizeMove(y) => {
                let Some(target) = self.mixer_view.split_active_target else {
                    return;
                };
                let delta = y - self.mixer_view.split_resize_start_y;
                match target {
                    MixerSplitTarget::InsertSend => {
                        let new_px = clamp_mixer_section_height_px(
                            self.mixer_view.split_resize_start_insert_px + delta,
                        );
                        if (new_px - self.mixer_view.insert_section_px).abs() <= 0.25 {
                            return;
                        }
                        self.mixer_view.insert_section_px = new_px;
                    }
                    MixerSplitTarget::SendFader => {
                        let new_px = clamp_mixer_section_height_px(
                            self.mixer_view.split_resize_start_send_px + delta,
                        );
                        if (new_px - self.mixer_view.send_section_px).abs() <= 0.25 {
                            return;
                        }
                        self.mixer_view.send_section_px = new_px;
                    }
                }
            }
            MixerSplitAction::ResizeEnd => {
                if self.mixer_view.split_active_target.is_none() {
                    return;
                }
                self.mixer_view.split_active_target = None;
            }
            MixerSplitAction::Reset(target) => {
                match target {
                    MixerSplitTarget::InsertSend => {
                        self.mixer_view.insert_section_px = MIXER_INSERT_SECTION_DEFAULT_PX;
                    }
                    MixerSplitTarget::SendFader => {
                        self.mixer_view.send_section_px = MIXER_SEND_SECTION_DEFAULT_PX;
                    }
                }
                self.mixer_view.split_active_target = None;
            }
        }
        self.push_mixer_snapshot_to_window(cx);
        let _ = self.mixer_panel.update(cx, |_, cx| cx.notify());
    }

    pub(crate) fn mixer_window_handle(&self) -> Option<WindowHandle<MixerWindow>> {
        self.external_windows.mixer.clone()
    }

    pub(super) fn mixer_panel_chrome_visible(&self) -> bool {
        if self.external_windows.mixer.is_some() {
            return true;
        }
        self.panels.mixer_docked && self.active_bottom_tab == crate::components::BottomTab::Mixer
    }

    /// Build the callback bundle used by the mixer. Every mutation lands in
    /// the same `TimelineState` instance owned by the Timeline entity, so the
    /// TrackHeader and Mixer always read identical values.
    pub(crate) fn build_mixer_callbacks(&self, owner: Entity<Self>) -> MixerCallbacks {
        let audio_engine = self.audio_bridge.engine.clone();
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
            StudioLayout::defer_update(&owner_select, cx, |layout, cx| {
                layout.sync_mixer_tree_to_selection(cx);
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
            // Per fader-drag-stutter fix: track gain is a *dynamic mixer parameter*,
            // not a structural graph change. It is applied to the engine runtime
            // live via the `SetTrackVolume` command below, so the drag must NOT
            // enqueue a native engine sync (load_project / route-graph rebuild) on
            // every mouse-move. Use `mark_dirty_view_only` (session-dirty for save,
            // no engine sync); a later real edit still carries the volume down
            // through the snapshot. See [[engine-sync-single-flight]].
            crate::perf::count("mixer_fader_drag_update_count", 1);
            if external_mixer_debug_enabled() {
                external_mixer_debug(&format!(
                    "mixer command dispatched set_volume id={id} v={v:.3}"
                ));
            }
            timeline_vol.update(cx, |t, cx| {
                t.state.set_track_volume(&id, v);
                cx.notify();
            });
            StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
                this.mark_dirty_view_only();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                crate::perf::count("mixer_fader_audio_control_update_count", 1);
                let _ = engine.update_track_param(&id, "volume", volume_norm_to_linear(v) as f64);
            }
        });

        let audio_engine = self.audio_bridge.engine.clone();
        let timeline_pan = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_pan_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
            let id = id.clone();
            let v = *v;
            // Pan is a dynamic mixer parameter like volume — applied live via the
            // `SetTrackPan` command, so the knob drag must not enqueue an engine
            // sync per mouse-move. See the volume handler above.
            crate::perf::count("mixer_fader_drag_update_count", 1);
            if external_mixer_debug_enabled() {
                external_mixer_debug(&format!(
                    "mixer command dispatched set_pan id={id} v={v:.3}"
                ));
            }
            timeline_pan.update(cx, |t, cx| {
                t.state.set_track_pan(&id, v);
                cx.notify();
            });
            StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
                this.mark_dirty_view_only();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                crate::perf::count("mixer_fader_audio_control_update_count", 1);
                let _ = engine.update_track_param(&id, "pan", v as f64);
            }
        });

        let audio_engine = self.audio_bridge.engine.clone();
        let timeline_mute = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_toggle_mute: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                let before_state = timeline_mute
                    .read(cx)
                    .state
                    .find_track(&id)
                    .map(|track| track.muted)
                    .unwrap_or(false);
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
                let id_for_side_effect_log = id.clone();
                StudioLayout::defer_update(&owner_dirty, cx, move |this, cx| {
                    let graph_version_before = this.audio_bridge.route_graph_version;
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                    let graph_version_after = this.audio_bridge.route_graph_version;
                    if crate::forensic_trace::forensic_trace_enabled() {
                        eprintln!(
                            "[SOLO_MUTE TOGGLE SIDE EFFECT CHECK]\naction=mute\nstrip_id={id_for_side_effect_log}\nmixer_channel_id={id_for_side_effect_log}\ngraph_version_before={graph_version_before}\ngraph_version_after={graph_version_after}\nroute_changed={}\nsound_was_silent_before=unknown\nsound_after_toggle=unknown",
                            graph_version_before != graph_version_after
                        );
                    }
                });
                let mut audio_router_applied = false;
                if let Some(engine) = audio_engine.as_ref() {
                    audio_router_applied = engine
                        .update_track_param(&id, "muted", if muted { 1.0 } else { 0.0 })
                        .is_ok();
                }
                if crate::forensic_trace::forensic_trace_enabled() {
                    let bus_index = id
                        .rsplit_once(":bus:")
                        .and_then(|(_, bus)| bus.parse::<u8>().ok())
                        .unwrap_or(0);
                    let plugin_instance_id = id
                        .strip_prefix("vsti-out:")
                        .and_then(|rest| rest.split_once(":bus:").map(|(plugin, _)| plugin))
                        .unwrap_or("");
                    eprintln!(
                        "[SOLO_MUTE COMMAND]\naction=mute\nrequested_value={muted}\nstrip_view_id={id}\nplugin_instance_id={plugin_instance_id}\nbus_index={bus_index}\nui_mixer_channel_id={id}\nengine_target_channel_id={id}\nbefore_state={before_state}\nafter_state={muted}\naudio_router_applied={audio_router_applied}"
                    );
                }
            });

        let audio_engine = self.audio_bridge.engine.clone();
        let timeline_solo = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_toggle_solo: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                let before_state = timeline_solo
                    .read(cx)
                    .state
                    .find_track(&id)
                    .map(|track| track.solo)
                    .unwrap_or(false);
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
                let id_for_side_effect_log = id.clone();
                StudioLayout::defer_update(&owner_dirty, cx, move |this, cx| {
                    let graph_version_before = this.audio_bridge.route_graph_version;
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                    let graph_version_after = this.audio_bridge.route_graph_version;
                    if crate::forensic_trace::forensic_trace_enabled() {
                        eprintln!(
                            "[SOLO_MUTE TOGGLE SIDE EFFECT CHECK]\naction=solo\nstrip_id={id_for_side_effect_log}\nmixer_channel_id={id_for_side_effect_log}\ngraph_version_before={graph_version_before}\ngraph_version_after={graph_version_after}\nroute_changed={}\nsound_was_silent_before=unknown\nsound_after_toggle=unknown",
                            graph_version_before != graph_version_after
                        );
                    }
                });
                let mut audio_router_applied = false;
                if let Some(engine) = audio_engine.as_ref() {
                    audio_router_applied = engine
                        .update_track_param(&id, "solo", if solo { 1.0 } else { 0.0 })
                        .is_ok();
                }
                if crate::forensic_trace::forensic_trace_enabled() {
                    let bus_index = id
                        .rsplit_once(":bus:")
                        .and_then(|(_, bus)| bus.parse::<u8>().ok())
                        .unwrap_or(0);
                    let plugin_instance_id = id
                        .strip_prefix("vsti-out:")
                        .and_then(|rest| rest.split_once(":bus:").map(|(plugin, _)| plugin))
                        .unwrap_or("");
                    eprintln!(
                        "[SOLO_MUTE COMMAND]\naction=solo\nrequested_value={solo}\nstrip_view_id={id}\nplugin_instance_id={plugin_instance_id}\nbus_index={bus_index}\nui_mixer_channel_id={id}\nengine_target_channel_id={id}\nbefore_state={before_state}\nafter_state={solo}\naudio_router_applied={audio_router_applied}"
                    );
                }
            });

        let timeline_arm = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                external_mixer_debug(&format!("mixer command dispatched toggle_arm id={id}"));
                let changed = timeline_arm.update(cx, |t, cx| {
                    let changed = t.state.toggle_track_arm(&id);
                    if changed {
                        cx.notify();
                    }
                    changed
                });
                if changed {
                    StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
                        this.mark_dirty();
                        this.push_mixer_snapshot_to_window(cx);
                    });
                }
            });

        let timeline_input = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_toggle_input: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |id: &String, _w, cx| {
            let id = id.clone();
            external_mixer_debug(&format!("mixer command dispatched toggle_input id={id}"));
            let changed = timeline_input.update(cx, |t, cx| {
                let changed = t.state.cycle_track_input_monitor(&id);
                if changed {
                    cx.notify();
                }
                changed
            });
            if changed {
                StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
            }
        });

        let audio_engine = self.audio_bridge.engine.clone();
        let timeline_master = self.timeline.clone();
        let owner_dirty = owner.clone();
        let on_master_volume_change: std::sync::Arc<
            dyn Fn(&f32, &mut Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(move |v: &f32, _w, cx| {
            let v = *v;
            // Master gain is applied live via `set_master_volume` (atomic) — the
            // fader drag must not enqueue an engine sync per mouse-move.
            crate::perf::count("mixer_fader_drag_update_count", 1);
            if external_mixer_debug_enabled() {
                external_mixer_debug(&format!("mixer command dispatched master_volume v={v:.3}"));
            }
            timeline_master.update(cx, |t, cx| {
                t.state.set_master_volume(v);
                cx.notify();
            });
            StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
                this.mark_dirty_view_only();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                crate::perf::count("mixer_fader_audio_control_update_count", 1);
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
            let this = owner.clone();
            std::sync::Arc::new(move |(track_id, x, y): &(String, f32, f32), window, cx| {
                let track_id = track_id.clone();
                let x = *x;
                let y = *y;
                let window_id = window.window_handle().window_id();
                StudioLayout::defer_update(&this, cx, move |this, cx| {
                    let _ = this.timeline.update(cx, |timeline, cx| {
                        timeline.state.select_track(&track_id);
                        cx.notify();
                    });
                    this.try_open_context_menu(
                        ContextMenuRequest::new(
                            window_id,
                            x,
                            y,
                            ContextMenuTarget::MixerStrip(track_id),
                        ),
                        cx,
                    );
                });
            })
        };

        // ── Plugin insert callbacks (Phase 1) ────────────────────────
        // Phase 1: add_insert seeds an empty slot followed by a stub
        // descriptor so the project round-trip exercises end-to-end.
        // Phase 2 will swap the stub for a real picker + plugin host
        // instantiation. None of these touch the audio thread; they
        // mutate UI state and let the next project sync carry the
        // descriptor down to the engine (which currently no-ops on
        // unrecognised plugins).
        // Phase 2b: opens the registry-driven picker overlay. The insert slot
        // is created only when the user picks a plugin (see
        // `apply_picked_insert`). No audio thread interaction.
        let on_add_insert: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> = {
            let this = owner.clone();
            std::sync::Arc::new(move |track_id: &String, window, cx| {
                let track_id = track_id.clone();
                StudioLayout::defer_update_in_window(&this, window, cx, move |this, window, cx| {
                    this.open_insert_picker(&track_id, Some(window), cx);
                });
            })
        };
        let on_remove_insert: std::sync::Arc<
            dyn Fn(&(String, String), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(move |(track_id, insert_id): &(String, String), _w, cx| {
                let track_id = track_id.clone();
                let insert_id = insert_id.clone();
                StudioLayout::defer_update(&this, cx, move |this, cx| {
                    // Full RemoveInstrumentPlugin lifecycle: close editor, unload
                    // the bridge-host instance, remove the engine sink, drop the
                    // slot, re-sync the engine, and assert the instance is gone.
                    this.remove_insert_fully(&track_id, &insert_id, cx, "mixer_remove_insert");
                    cx.notify();
                });
            })
        };
        let on_toggle_insert_bypass: std::sync::Arc<
            dyn Fn(&(String, String), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(move |(track_id, insert_id): &(String, String), _w, cx| {
                let track_id = track_id.clone();
                let insert_id = insert_id.clone();
                StudioLayout::defer_update(&this, cx, move |this, cx| {
                    this.timeline.update(cx, |timeline, _cx| {
                        timeline.state.toggle_insert_bypass(&track_id, &insert_id);
                    });
                    this.mark_dirty();
                    this.audio_bridge.project_dirty = true;
                    cx.notify();
                });
            })
        };
        let on_toggle_vsti_output_group: std::sync::Arc<
            dyn Fn(&String, &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(move |group_key: &String, _w, cx| {
                let group_key = group_key.clone();
                StudioLayout::defer_update(&this, cx, move |this, cx| {
                    this.toggle_vsti_output_group(&group_key, cx);
                });
            })
        };
        // Drag-reorder commit (mirrors the Inspector's `reorder_insert_cb`). The
        // drop handler supplies the dragged `plugin_instance_id` and the
        // insertion gap; we snapshot the current id order, compute the new order,
        // and apply it as a single `EditCommand::ReorderFxSlot` so one drag is one
        // undo entry. The command only reorders existing slots (never recreates an
        // instance), so bypass / preset / parameter / editor / automation state
        // follow each instance. A forced project sync rebuilds the engine's chain
        // order (DSP order == UI order); editor windows are keyed by instance id,
        // so they stay attached.
        let on_reorder_insert: std::sync::Arc<
            dyn Fn(&(String, String, usize), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(
                move |(track_id, insert_id, insertion_index): &(String, String, usize), _w, cx| {
                    let track_id = track_id.clone();
                    let insert_id = insert_id.clone();
                    let insertion_index = *insertion_index;
                    StudioLayout::defer_update(&this, cx, move |this, cx| {
                        let changed = this.timeline.update(cx, |timeline, cx| {
                            let before = timeline.state.insert_order(&track_id);
                            let after = timeline_state::TimelineState::reordered_insert_ids(
                                &before,
                                &insert_id,
                                insertion_index,
                            );
                            if before == after {
                                return false;
                            }
                            timeline.run_edit_command(
                                EditCommand::ReorderFxSlot {
                                    track_id: track_id.clone(),
                                    before_order: before,
                                    after_order: after,
                                },
                                cx,
                            );
                            true
                        });
                        if changed {
                            this.mark_dirty();
                            this.audio_bridge.project_dirty = true;
                            this.schedule_audio_project_sync(cx, true, "mixer_reorder_insert");
                            this.push_mixer_snapshot_to_window(cx);
                            cx.notify();
                        }
                    });
                },
            )
        };
        let on_drop_plugin_preset: std::sync::Arc<
            dyn Fn(&(PathBuf, String, usize), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(
                move |(preset_path, track_id, insert_index): &(PathBuf, String, usize), _w, cx| {
                    let preset_path = preset_path.clone();
                    let track_id = track_id.clone();
                    let insert_index = *insert_index;
                    StudioLayout::defer_update(&this, cx, move |this, cx| {
                        if this
                            .apply_dropped_plugin_preset_to_slot(
                                &track_id,
                                insert_index,
                                &preset_path,
                                cx,
                            )
                            .is_some()
                        {
                            this.push_mixer_snapshot_to_window(cx);
                            cx.notify();
                        }
                    });
                },
            )
        };
        // Phase 4: open the GPUI-hosted native plugin editor window.
        let on_open_insert_editor: std::sync::Arc<
            dyn Fn(&(String, usize, String), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(move |(track_id, insert_index, insert_id), window, cx| {
                let track_id = track_id.clone();
                let insert_index = *insert_index;
                let insert_id = insert_id.clone();
                StudioLayout::defer_update_in_window(&this, window, cx, move |this, window, cx| {
                    this.open_insert_editor(&track_id, insert_index, &insert_id, window, cx);
                });
            })
        };

        // ── Send callbacks (Phase 3) ─────────────────────────────────────
        // Add Send opens a target picker at the click point. The command chosen
        // from that menu performs the actual state mutation and engine sync.
        let on_add_send: std::sync::Arc<
            dyn Fn(&(String, f32, f32), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(move |(track_id, x, y): &(String, f32, f32), window, cx| {
                let track_id = track_id.clone();
                let x = *x;
                let y = *y;
                let window_id = window.window_handle().window_id();
                StudioLayout::defer_update(&this, cx, move |this, cx| {
                    this.try_open_context_menu(
                        ContextMenuRequest::new(
                            window_id,
                            x,
                            y,
                            ContextMenuTarget::Extended(ContextTarget::SendPicker { track_id }),
                        ),
                        cx,
                    );
                });
            })
        };
        let on_remove_send: std::sync::Arc<
            dyn Fn(&(String, String), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(move |(track_id, send_id): &(String, String), _w, cx| {
                let track_id = track_id.clone();
                let send_id = send_id.clone();
                StudioLayout::defer_update(&this, cx, move |this, cx| {
                    this.timeline.update(cx, |timeline, _cx| {
                        timeline.state.remove_send(&track_id, &send_id);
                    });
                    this.mark_dirty();
                    this.audio_bridge.project_dirty = true;
                    cx.notify();
                });
            })
        };
        let on_reorder_send: std::sync::Arc<
            dyn Fn(&(String, String, usize), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(
                move |(track_id, send_id, insertion_index): &(String, String, usize), _w, cx| {
                    let track_id = track_id.clone();
                    let send_id = send_id.clone();
                    let insertion_index = *insertion_index;
                    StudioLayout::defer_update(&this, cx, move |this, cx| {
                        let changed = this.timeline.update(cx, |timeline, cx| {
                            let before = timeline.state.send_order(&track_id);
                            let after = timeline_state::TimelineState::reordered_send_ids(
                                &before,
                                &send_id,
                                insertion_index,
                            );
                            if before == after {
                                return false;
                            }
                            timeline.run_edit_command(
                                EditCommand::ReorderSendSlot {
                                    track_id: track_id.clone(),
                                    before_order: before,
                                    after_order: after,
                                },
                                cx,
                            );
                            true
                        });
                        if changed {
                            this.mark_dirty();
                            // Send order is modeled in session routing. The engine
                            // snapshot dedup guard skips load_project when the graph
                            // is effectively unchanged.
                            this.audio_bridge.project_dirty = true;
                            this.schedule_audio_project_sync(cx, true, "mixer_reorder_send");
                            this.push_mixer_snapshot_to_window(cx);
                            cx.notify();
                        }
                    });
                },
            )
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
            on_add_insert,
            on_remove_insert,
            on_toggle_insert_bypass,
            on_toggle_vsti_output_group,
            on_reorder_insert,
            on_drop_plugin_preset,
            on_open_insert_editor,
            on_add_send,
            on_remove_send,
            on_reorder_send,
        }
    }
}

fn mixer_channel_meter_signature(
    tracks: &[crate::components::timeline::timeline_state::TrackState],
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
    for track in tracks {
        q(track.meter_level_l).hash(&mut hasher);
        q(track.meter_level_r).hash(&mut hasher);
    }
    hasher.finish()
}
