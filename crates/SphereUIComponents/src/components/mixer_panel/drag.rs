use gpui::{IntoElement, ParentElement, Render, Styled, Window, div, px};

use crate::theme::Colors;

#[derive(Clone, Debug)]
pub(super) struct SendSlotDrag {
    pub(super) track_id: String,
    pub(super) send_id: String,
    pub(super) target_name: String,
}

impl Render for SendSlotDrag {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .h(px(20.0))
            .rounded_sm()
            .bg(Colors::surface_overlay())
            .border(px(1.0))
            .border_color(Colors::accent_primary())
            .text_size(px(10.0))
            .text_color(Colors::text_primary())
            .child(format!("Send -> {}", self.target_name))
    }
}
