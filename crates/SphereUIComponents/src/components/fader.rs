//! Vertical normalized fader used by the mixer channel strips.
//!
//! Same drag pattern as [`super::slider`] — start a drag on mouse-down,
//! receive value updates via `on_drag_move`, never on plain click. The visible
//! geometry (rail, ticks, thumb) matches the Web/Electron mixer fader.

use gpui::{
    div, px, rgba, App, AppContext, DragMoveEvent, Empty, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};

pub const FADER_TRACK_HEIGHT: f32 = 130.0;
pub const FADER_THUMB_HEIGHT: f32 = 10.0;
pub const FADER_USABLE: f32 = FADER_TRACK_HEIGHT - FADER_THUMB_HEIGHT;

#[derive(Clone, Debug)]
pub struct FaderDrag {
    pub id: String,
}

impl Render for FaderDrag {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// dB tick marks. Used by [`db_scale_column`] and [`fader_center_column`] so
/// the scale tape lines up exactly with the fader's `0…-60 dB → norm 1…0`
/// mapping. Note this is the *visual* scale; the underlying `volume` value is
/// the linear normalized fader position.
pub const SCALE_MARKS: [(f32, &str); 7] = [
    (0.0, "0"),
    (-6.0, "6"),
    (-12.0, "12"),
    (-18.0, "18"),
    (-24.0, "24"),
    (-36.0, "36"),
    (-54.0, "∞"),
];

/// Center Y of a dB mark inside the rail. The dB→thumb mapping is approximate
/// (the fader thumb position is driven by `norm` directly, not dB) but matches
/// the Web mixer's visual scale.
pub fn db_to_center_y(db: f32) -> f32 {
    let t = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    (1.0 - t) * FADER_USABLE + FADER_THUMB_HEIGHT / 2.0
}

/// Convert a normalized fader value to the thumb's `top` y inside the rail.
pub fn norm_to_thumb_top(norm: f32) -> f32 {
    (1.0 - norm.clamp(0.0, 1.0)) * FADER_USABLE
}

pub fn db_scale_column() -> gpui::Div {
    let mut col = div().relative().w(px(15.0)).h(px(FADER_TRACK_HEIGHT));
    for &(db, label) in SCALE_MARKS.iter() {
        let cy = db_to_center_y(db);
        let top = (cy - 3.5).max(0.0);
        col = col.child(
            div()
                .absolute()
                .top(px(top))
                .right(px(0.0))
                .text_size(px(7.5))
                .text_color(if db == 0.0 { rgba(0xFFFFFF59_u32) } else { rgba(0xFFFFFF2E_u32) })
                .child(label),
        );
    }
    col
}

/// Render the vertical rail + ticks + thumb at `value_norm`.
///
/// Geometry contract:
/// * rail column is 24 px wide; the rail centerline lives at x = 12 px.
/// * the rail itself is 2 px wide and inset so its center sits on x = 12.
/// * the thumb is 22 px wide and centered on x = 12 (`left = 1`).
/// * tick marks straddle x = 12.
fn fader_rail(thumb_top: f32, accent: gpui::Rgba) -> gpui::Div {
    let rail_center_x = 12.0_f32;
    let rail_w = 2.0_f32;
    let thumb_w = 22.0_f32;
    let thumb_left = rail_center_x - thumb_w / 2.0;
    let accent_line_h = 2.0_f32;

    let mut col = div()
        .relative()
        .w(px(24.0))
        .h(px(FADER_TRACK_HEIGHT))
        // Rail — recessed dark line aligned to the centerline.
        .child(
            div()
                .absolute()
                .top(px(FADER_THUMB_HEIGHT / 2.0))
                .left(px(rail_center_x - rail_w / 2.0))
                .w(px(rail_w))
                .h(px(FADER_USABLE))
                .bg(rgba(0xFFFFFF14_u32))
                .border(px(1.0))
                .border_color(rgba(0x00000038_u32))
                .rounded_full(),
        );

    // Tick marks centered on the rail centerline.
    for &(db, _) in SCALE_MARKS.iter() {
        let cy = db_to_center_y(db);
        let w = if db == 0.0 { 14.0_f32 } else { 9.0_f32 };
        let left = rail_center_x - w / 2.0;
        col = col.child(
            div()
                .absolute()
                .top(px(cy))
                .left(px(left))
                .h(px(1.0))
                .w(px(w))
                .bg(if db == 0.0 {
                    rgba(0xFFFFFF59_u32)
                } else {
                    rgba(0xFFFFFF1F_u32)
                }),
        );
    }

    // Thumb — centered on the rail, with a crisp accent line through the cap
    // anchored on the value position.
    let mut thumb_accent = accent;
    thumb_accent.a = 0.9;

    col.child(
        div()
            .absolute()
            .top(px(thumb_top))
            .left(px(thumb_left))
            .w(px(thumb_w))
            .h(px(FADER_THUMB_HEIGHT))
            .rounded_sm()
            .bg(rgba(0x1F262FFF_u32))
            .border(px(1.0))
            .border_color(rgba(0xFFFFFF66_u32))
            // Top highlight band.
            .child(
                div()
                    .absolute()
                    .top(px(1.0))
                    .left(px(1.0))
                    .right(px(1.0))
                    .h(px(1.0))
                    .bg(rgba(0xFFFFFF26_u32)),
            )
            // Accent stripe through the cap, exactly centered on the thumb.
            .child(
                div()
                    .absolute()
                    .top(px((FADER_THUMB_HEIGHT - accent_line_h) / 2.0))
                    .left(px(2.0))
                    .right(px(2.0))
                    .h(px(accent_line_h))
                    .bg(thumb_accent),
            ),
    )
}

/// Bordered dB readout pill. Use this above the fader instead of plain text so
/// the value reads as a proper integrated control.
pub fn db_value_pill(
    db_text: impl Into<gpui::SharedString>,
    highlight: bool,
) -> impl IntoElement {
    let border = if highlight {
        rgba(0xFFFFFF3A_u32)
    } else {
        rgba(0xFFFFFF1F_u32)
    };

    div()
        .flex()
        .flex_row()
        .items_baseline()
        .justify_center()
        .gap(px(2.0))
        .min_w(px(46.0))
        .h(px(18.0))
        .px(px(6.0))
        .rounded_sm()
        .bg(rgba(0x0000003A_u32))
        .border(px(1.0))
        .border_color(border)
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgba(0xEEF2F5D9_u32))
                .child(db_text.into()),
        )
        .child(
            div()
                .text_size(px(7.5))
                .text_color(rgba(0xFFFFFF47_u32))
                .child("dB"),
        )
}

/// Render a vertical fader and wire drag updates.
pub fn fader(
    id: impl Into<gpui::SharedString>,
    value_norm: f32,
    accent: gpui::Rgba,
    on_change: impl Fn(&f32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id_str: gpui::SharedString = id.into();
    let id_string = id_str.to_string();
    let value = value_norm.clamp(0.0, 1.0);
    let thumb_top = norm_to_thumb_top(value);

    div()
        .id(gpui::ElementId::Name(id_str.clone()))
        // Hit area: the full rail width + a margin so users can wander
        // horizontally without losing the drag.
        .w(px(28.0))
        .h(px(FADER_TRACK_HEIGHT))
        .relative()
        .cursor(gpui::CursorStyle::ResizeUpDown)
        .flex()
        .flex_row()
        .justify_center()
        .child(fader_rail(thumb_top, accent))
        .on_drag(FaderDrag { id: id_string.clone() }, move |drag, _offset, _window, cx| {
            cx.new(|_| FaderDrag { id: drag.id.clone() })
        })
        .on_drag_move::<FaderDrag>(move |event: &DragMoveEvent<FaderDrag>, window, cx| {
            if event.drag(cx).id != id_string {
                return;
            }
            let bounds = event.bounds;
            let y: f32 = event.event.position.y.into();
            let oy: f32 = bounds.origin.y.into();
            let oh: f32 = f32::from(bounds.size.height).max(1.0);
            // Top of rail → 1.0, bottom → 0.0
            let new_value = (1.0 - (y - oy) / oh).clamp(0.0, 1.0);
            on_change(&new_value, window, cx);
        })
}
