use gpui::{div, Context, IntoElement, ParentElement, Render, Styled, Window};
use crate::components;
use crate::theme::{self, Colors};

pub struct StudioLayout;

impl Render for StudioLayout {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(Colors::bg_base())
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
            // Bottom status bar
            .child(components::status_bar())
    }
}
