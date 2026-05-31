use gpui::{Context, Entity, Window, WindowHandle};

use crate::components::mixer_panel::MixerCallbacks;
use crate::components::timeline::timeline_state::{self, TrackState};
use crate::components::{external_mixer_debug, MixerSnapshot};

use super::engine_snapshot::volume_norm_to_linear;
use super::{ContextTarget, MixerWindow, OpenPopover, StudioLayout};
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

    pub(super) fn mixer_panel_chrome_visible(&self) -> bool {
        self.panels.mixer_docked || self.mixer_window.is_some()
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
            external_mixer_debug(&format!(
                "mixer command dispatched set_volume id={id} v={v:.3}"
            ));
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
            external_mixer_debug(&format!(
                "mixer command dispatched set_pan id={id} v={v:.3}"
            ));
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
            let this = owner.clone();
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
                let _ = this.update(cx, |this, cx| {
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
                let _ = this.update(cx, |this, cx| {
                    // Close any open editor window for this slot before dropping
                    // the descriptor — every open pairs with a close.
                    this.close_insert_editor(&track_id, &insert_id, cx);
                    this.timeline.update(cx, |timeline, _cx| {
                        timeline.state.remove_insert(&track_id, &insert_id);
                    });
                    this.mark_dirty();
                    this.engine_project_dirty = true;
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
                let _ = this.update(cx, |this, cx| {
                    this.timeline.update(cx, |timeline, _cx| {
                        timeline.state.toggle_insert_bypass(&track_id, &insert_id);
                    });
                    this.mark_dirty();
                    this.engine_project_dirty = true;
                    cx.notify();
                });
            })
        };
        // Phase 4: open the GPUI-hosted native plugin editor window.
        let on_open_insert_editor: std::sync::Arc<
            dyn Fn(&(String, String), &mut Window, &mut gpui::App) + 'static,
        > = {
            let this = owner.clone();
            std::sync::Arc::new(move |(track_id, insert_id), window, cx| {
                let track_id = track_id.clone();
                let insert_id = insert_id.clone();
                let _ = this.update(cx, |this, cx| {
                    this.open_insert_editor(&track_id, &insert_id, window, cx);
                });
            })
        };

        // ── Send callbacks (Phase 3) ─────────────────────────────────────
        // add_send auto-targets the first eligible Bus/Return (a target picker
        // is a follow-up). Both flip `engine_project_dirty` so the next audio
        // sync carries the send list down to the runtime.
        let on_add_send: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> = {
            let this = owner.clone();
            std::sync::Arc::new(move |track_id: &String, _w, cx| {
                let track_id = track_id.clone();
                let _ = this.update(cx, |this, cx| {
                    let added = this
                        .timeline
                        .update(cx, |timeline, _cx| timeline.state.add_send(&track_id));
                    if added.is_some() {
                        this.mark_dirty();
                        this.engine_project_dirty = true;
                        cx.notify();
                    }
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
                let _ = this.update(cx, |this, cx| {
                    this.timeline.update(cx, |timeline, _cx| {
                        timeline.state.remove_send(&track_id, &send_id);
                    });
                    this.mark_dirty();
                    this.engine_project_dirty = true;
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
            on_open_insert_editor,
            on_add_send,
            on_remove_send,
        }
    }
}
