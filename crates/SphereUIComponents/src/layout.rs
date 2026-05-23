use gpui::{div, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};
use crate::components;
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
                // Clamp max to ~70% of current window height so the panel can't eat the timeline.
                let window_h: f32 = window.bounds().size.height.into();
                bs.max_height_px = (window_h * 0.70).max(bs.min_height_px + 40.0);
                cx.notify();
            },
        );

        let on_resize_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<BottomPanelResizeDrag>, _window, cx| {
                let bs = &mut this.bottom_panel_state;
                let cur_y: f32 = event.event.position.y.into();
                // Drag UP (cur_y decreasing) grows the panel.
                let delta = bs.resize_start_y - cur_y;
                let new_h = (bs.resize_start_height + delta).clamp(bs.min_height_px, bs.max_height_px);
                if (new_h - bs.height_px).abs() > 0.5 {
                    bs.height_px = new_h;
                    cx.notify();
                }
            },
        );

        let panel_state = self.bottom_panel_state;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(Colors::surface_base())
            .font_family(theme::FONT_FAMILY)
            // Unified top chrome: menus + project title + transport + window controls
            .child(components::app_chrome(window))
            // Main content area
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    // Left sidebar / browser
                    .child(components::sidebar())
                    // Central timeline area
                    .child(self.timeline.clone())
                    // Right inspector panel
                    .child(components::right_panel()),
            )
            // Bottom Panel
            .child(components::bottom_panel(
                self.active_bottom_tab,
                panel_state,
                on_tab_click,
                on_resize_start,
                on_resize_move,
            ))
            // Bottom status bar
            .child(components::status_bar())
    }
}
