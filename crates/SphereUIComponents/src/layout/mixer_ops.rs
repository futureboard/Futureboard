use gpui::{Context, Entity, Window, WindowHandle};

use crate::components::edit::EditCommand;
use crate::components::mixer_panel::{
    clamp_mixer_section_height_px, MixerCallbacks, MixerSplitAction, MixerSplitTarget,
    MIXER_INSERT_SECTION_DEFAULT_PX, MIXER_SEND_SECTION_DEFAULT_PX,
};
use crate::components::timeline::timeline_state::{self, TrackState};
use crate::components::{external_mixer_debug, MixerSnapshot};

use super::engine_snapshot::volume_norm_to_linear;
use super::{ContextTarget, MixerWindow, OpenPopover, StudioLayout};

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
        }
    }
}

impl StudioLayout {
    pub(crate) fn notify_mixer_window(&mut self, cx: &mut Context<Self>) {
        self.push_mixer_snapshot_to_window(cx);
    }

    pub(crate) fn build_mixer_snapshot(&self, cx: &gpui::App) -> MixerSnapshot {
        let timeline = self.timeline.read(cx);
        MixerSnapshot {
            tracks: timeline.state.tracks.clone(),
            master: timeline.state.master.clone(),
            selected_track_id: timeline.state.selection.selected_track_id.clone(),
            mixer_scroll_x: self.mixer_view.scroll_x,
            mixer_insert_section_px: clamp_mixer_section_height_px(self.mixer_view.insert_section_px),
            mixer_send_section_px: clamp_mixer_section_height_px(self.mixer_view.send_section_px),
            mixer_split_active_target: self.mixer_view.split_active_target,
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
        cx.notify();
    }

    pub(crate) fn mixer_window_handle(&self) -> Option<WindowHandle<MixerWindow>> {
        self.external_windows.mixer.clone()
    }

    pub(super) fn mixer_panel_chrome_visible(&self) -> bool {
        self.panels.mixer_docked || self.external_windows.mixer.is_some()
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
            external_mixer_debug(&format!(
                "mixer command dispatched set_volume id={id} v={v:.3}"
            ));
            timeline_vol.update(cx, |t, cx| {
                t.state.set_track_volume(&id, v);
                cx.notify();
            });
            StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
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
            external_mixer_debug(&format!(
                "mixer command dispatched set_pan id={id} v={v:.3}"
            ));
            timeline_pan.update(cx, |t, cx| {
                t.state.set_track_pan(&id, v);
                cx.notify();
            });
            StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
                this.mark_dirty();
                this.push_mixer_snapshot_to_window(cx);
            });
            if let Some(engine) = audio_engine.as_ref() {
                let _ = engine.update_track_param(&id, "pan", v as f64);
            }
        });

        let audio_engine = self.audio_bridge.engine.clone();
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
                StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
                    this.mark_dirty();
                    this.push_mixer_snapshot_to_window(cx);
                });
                if let Some(engine) = audio_engine.as_ref() {
                    let _ = engine.update_track_param(&id, "mute", if muted { 1.0 } else { 0.0 });
                }
            });

        let audio_engine = self.audio_bridge.engine.clone();
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
                StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
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
            external_mixer_debug(&format!("mixer command dispatched master_volume v={v:.3}"));
            timeline_master.update(cx, |t, cx| {
                t.state.set_master_volume(v);
                cx.notify();
            });
            StudioLayout::defer_update(&owner_dirty, cx, |this, cx| {
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
            let this = owner.clone();
            std::sync::Arc::new(move |(track_id, x, y): &(String, f32, f32), _w, cx| {
                let track_id = track_id.clone();
                let x = *x;
                let y = *y;
                StudioLayout::defer_update(&this, cx, move |this, cx| {
                    let _ = this.timeline.update(cx, |timeline, cx| {
                        timeline.state.select_track(&track_id);
                        cx.notify();
                    });
                    this.menu_bar.open_menu_id = None;
                    this.menu_bar.submenu_path.clear();
                    this.project_switcher.is_open = false;
                    this.overlay.open_popover = Some(OpenPopover::Context {
                        target: ContextTarget::Mixer(track_id),
                        x,
                        y,
                    });
                    cx.notify();
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
                    this.open_insert_picker(&track_id, window, cx);
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
            std::sync::Arc::new(move |(track_id, x, y): &(String, f32, f32), _w, cx| {
                let track_id = track_id.clone();
                let x = *x;
                let y = *y;
                StudioLayout::defer_update(&this, cx, move |this, cx| {
                    this.overlay.open_popover = Some(OpenPopover::Context {
                        target: ContextTarget::SendPicker { track_id },
                        x,
                        y,
                    });
                    cx.notify();
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
            on_reorder_insert,
            on_open_insert_editor,
            on_add_send,
            on_remove_send,
        }
    }
}
