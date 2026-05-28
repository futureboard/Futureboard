use crate::theme::Colors;
use gpui::{canvas, div, fill, px, size, svg, Bounds, IntoElement, ParentElement, Pixels, Styled};

/// Content-viewport playhead line (no head/marker) at a precomputed x.
pub fn playhead_line_at(x: f32) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("PlayheadLine");
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(x))
        .w(px(1.0))
        .bg(Colors::timeline_playhead())
}

/// Ruler-only playhead head/marker (no vertical line) at a precomputed x.
pub fn playhead_head_at(x: f32) -> impl IntoElement {
    let _s = crate::perf::PerfScope::enter("PlayheadHead");
    svg()
        .path(crate::assets::ICON_PLAYHEAD_HANDLE_PATH)
        .absolute()
        .top_0()
        .left(px(x - 5.5))
        .w(px(12.0))
        .h(px(12.0))
        .text_color(Colors::timeline_playhead())
}

/// Dedicated foreground overlay: playhead vertical body line.
/// Rendered after grid/content so it cannot be covered.
pub fn playhead_body_overlay_at(x: f32) -> impl IntoElement {
    let color = Colors::timeline_playhead();

    // Canvas draws the body-only line in a dedicated paint layer.
    let line = canvas(
        |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            let w: f32 = bounds.size.width.into();
            let h: f32 = bounds.size.height.into();
            if x < -2.0 || x > w + 2.0 || h <= 0.0 {
                return;
            }

            if std::env::var_os("FUTUREBOARD_PLAYHEAD_LAYER_DEBUG").is_some() {
                eprintln!("[playhead body] x={x:.1} w={w:.1} h={h:.1}");
            }

            window.paint_layer(bounds, |window| {
                let line_bounds = Bounds::new(
                    bounds.origin + gpui::point(px(x), px(0.0)),
                    size(px(1.0), px(h.max(0.0))),
                );
                window.paint_quad(fill(line_bounds, color));
            });
        },
    )
    .absolute()
    .inset_0()
    .into_any_element();

    div()
        .absolute()
        .inset_0()
        .child(line)
}

pub fn playhead_head_overlay_at(x: f32) -> impl IntoElement {
    let color = Colors::timeline_playhead();
    let y = 2.0; // keep the head inside the ruler strip
    div()
        .absolute()
        .inset_0()
        .child(
            svg()
                .path(crate::assets::ICON_PLAYHEAD_HANDLE_PATH)
                .absolute()
                .top(px(y))
                .left(px(x - 5.5))
                .w(px(12.0))
                .h(px(12.0))
                .text_color(color),
        )
}
