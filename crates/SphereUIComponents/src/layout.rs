use gpui::{div, Context, IntoElement, ParentElement, Render, Styled, Window};
use crate::components;
use crate::theme::{self, Colors};

pub struct StudioLayout {
    active_bottom_tab: components::BottomTab,
}

impl StudioLayout {
    pub fn new() -> Self {
        Self {
            active_bottom_tab: components::BottomTab::Mixer,
        }
    }
}

impl Render for StudioLayout {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on_tab_click = cx.listener(|this, tab: &components::BottomTab, _window, cx| {
            this.active_bottom_tab = *tab;
            cx.notify();
        });

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
                    .child(components::timeline_shell())
                    // Right inspector panel
                    .child(components::right_panel()),
            )
            // Bottom Panel
            .child(components::bottom_panel(self.active_bottom_tab, on_tab_click))
            // Bottom status bar
            .child(components::status_bar())
    }
}
