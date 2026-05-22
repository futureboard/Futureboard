use gpui::{div, Context, IntoElement, ParentElement, Render, Styled, Window};
use crate::components;
use crate::theme::Colors;

pub struct StudioLayout;

impl Render for StudioLayout {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(Colors::bg_base())
            // Title bar (custom chrome)
            .child(components::title_bar())
            // Transport bar (menus + playback controls)
            .child(components::transport_bar())
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
