use gpui::{div, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};

use crate::components;
use crate::components::mixer_panel::MixerCallbacks;
use crate::components::timeline::timeline_state::TrackState;
use crate::components::{BottomPanelResizeDrag, BottomPanelState};
use crate::theme::{self, Colors};

pub struct StudioLayout {
    active_bottom_tab: components::BottomTab,
    bottom_panel_state: BottomPanelState,
    timeline: Entity<components::timeline::Timeline>,
}

impl StudioLayout {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let timeline = cx.new(|_| components::timeline::Timeline::new());
        Self {
            active_bottom_tab: components::BottomTab::Mixer,
            bottom_panel_state: BottomPanelState::default(),
            timeline,
        }
    }
}

impl StudioLayout {
    /// Build the callback bundle used by the mixer. Every mutation lands in
    /// the same `TimelineState` instance owned by the Timeline entity, so the
    /// TrackHeader and Mixer always read identical values.
    fn build_mixer_callbacks(&self) -> MixerCallbacks {
        let timeline_select = self.timeline.clone();
        let on_select_track: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_select.update(cx, |t, cx| {
                    t.state.select_track(&id);
                    cx.notify();
                });
            });

        let timeline_vol = self.timeline.clone();
        let on_volume_change: std::sync::Arc<dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
                let id = id.clone();
                let v = *v;
                timeline_vol.update(cx, |t, cx| {
                    t.state.set_track_volume(&id, v);
                    cx.notify();
                });
            });

        let timeline_pan = self.timeline.clone();
        let on_pan_change: std::sync::Arc<dyn Fn(&(String, f32), &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |(id, v): &(String, f32), _w, cx| {
                let id = id.clone();
                let v = *v;
                timeline_pan.update(cx, |t, cx| {
                    t.state.set_track_pan(&id, v);
                    cx.notify();
                });
            });

        let timeline_mute = self.timeline.clone();
        let on_toggle_mute: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_mute.update(cx, |t, cx| {
                    t.state.toggle_track_mute(&id);
                    cx.notify();
                });
            });

        let timeline_solo = self.timeline.clone();
        let on_toggle_solo: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_solo.update(cx, |t, cx| {
                    t.state.toggle_track_solo(&id);
                    cx.notify();
                });
            });

        let timeline_arm = self.timeline.clone();
        let on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_arm.update(cx, |t, cx| {
                    t.state.toggle_track_arm(&id);
                    cx.notify();
                });
            });

        let timeline_input = self.timeline.clone();
        let on_toggle_input: std::sync::Arc<dyn Fn(&String, &mut Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(move |id: &String, _w, cx| {
                let id = id.clone();
                timeline_input.update(cx, |t, cx| {
                    t.state.toggle_track_input_monitor(&id);
                    cx.notify();
                });
            });

        MixerCallbacks {
            on_select_track,
            on_volume_change,
            on_pan_change,
            on_toggle_mute,
            on_toggle_solo,
            on_toggle_arm,
            on_toggle_input,
        }
    }
}

impl Render for StudioLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_tab_click = cx.listener(|this, tab: &components::BottomTab, _window, cx| {
            this.active_bottom_tab = *tab;
            cx.notify();
        });

        let on_resize_start = cx.listener(
            |this, event: &gpui::MouseDownEvent, window, cx| {
                let bs = &mut this.bottom_panel_state;
                bs.is_resizing = true;
                bs.resize_start_y = f32::from(event.position.y);
                bs.resize_start_height = bs.height_px;
                let window_h: f32 = window.bounds().size.height.into();
                bs.max_height_px = (window_h * 0.70).max(bs.min_height_px + 40.0);
                cx.notify();
            },
        );

        let on_resize_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<BottomPanelResizeDrag>, _window, cx| {
                let bs = &mut this.bottom_panel_state;
                let cur_y: f32 = event.event.position.y.into();
                let delta = bs.resize_start_y - cur_y;
                let new_h = (bs.resize_start_height + delta).clamp(bs.min_height_px, bs.max_height_px);
                if (new_h - bs.height_px).abs() > 0.5 {
                    bs.height_px = new_h;
                    cx.notify();
                }
            },
        );

        // Pull the live track list and current selection out of the Timeline so
        // the Mixer and Inspector render against the same data the TrackHeader
        // sees. Cloning the Vec is cheap relative to a full render.
        let (tracks, selected_track_id, selected_clip_id) = {
            let t = self.timeline.read(cx);
            (
                t.state.tracks.clone(),
                t.state.selection.selected_track_id.clone(),
                t.state.selection.selected_clip_ids.first().cloned(),
            )
        };

        let panel_state = self.bottom_panel_state;
        let mixer_callbacks = self.build_mixer_callbacks();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(Colors::surface_base())
            .font_family(theme::FONT_FAMILY)
            .child(components::app_chrome(window))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(components::sidebar())
                    .child(self.timeline.clone())
                    .child(crate::components::panel::inspector_panel(
                        &tracks,
                        selected_track_id.as_deref(),
                        selected_clip_id.as_deref(),
                        find_clip_summary(&tracks, selected_clip_id.as_deref()),
                    )),
            )
            .child(components::bottom_panel(
                self.active_bottom_tab,
                panel_state,
                &tracks,
                selected_track_id.as_deref(),
                mixer_callbacks,
                on_tab_click,
                on_resize_start,
                on_resize_move,
            ))
            .child(components::status_bar())
    }
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
