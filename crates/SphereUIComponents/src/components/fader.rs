//! Vertical normalized fader used by the mixer channel strips.
//!
//! Drag pattern matches [`super::slider`] — start a drag on mouse-down,
//! receive value updates via `on_drag_move`, never on plain click.
//!
//! The rail/scale/thumb geometry now uses `h_full` instead of a hard pixel
//! height: the parent (mixer fader area) is the flex_1 slot inside the channel
//! strip, so resizing the bottom panel makes the fader travel grow/shrink with
//! the remaining space. Thumb position uses a flex-spacer pair sized
//! proportionally to `norm`, so the thumb stays anchored on the rail at any
//! container height. Tick labels and rail ticks use `top(relative(pct))`,
//! which lays out as a fraction of parent height.

use gpui::{
    div, px, relative, App, AppContext, DragMoveEvent, Empty, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};

use crate::components::timeline::timeline_state::volume;
use crate::theme::Colors;

/// Minimum recommended rail travel height. The fader will still render at
/// smaller heights, but below this the dB labels start to crowd.
pub const FADER_TRACK_HEIGHT: f32 = 130.0;
pub const FADER_THUMB_HEIGHT: f32 = 10.0;
const RAIL_CENTER_X: f32 = 12.0;
const RAIL_W: f32 = 2.0;
const THUMB_W: f32 = 22.0;
const ACCENT_LINE_H: f32 = 2.0;

#[derive(Clone, Debug)]
pub struct FaderDrag {
    pub id: String,
}

impl Render for FaderDrag {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// dB tick marks. Used by [`db_scale_column`] and the fader rail so the scale
/// tape lines up with the shared `volume::db_to_norm` mapping.
pub const SCALE_MARKS: [(f32, &str); 8] = [
    (volume::MAX_DB, "+6"),
    (0.0, "0"),
    (-6.0, "6"),
    (-12.0, "12"),
    (-24.0, "24"),
    (-36.0, "36"),
    (-48.0, "48"),
    (volume::MIN_DB, "∞"),
];

/// Fraction down from the top of the rail for a dB mark (0.0 = top, 1.0 = bot).
fn db_to_top_fraction(db: f32) -> f32 {
    1.0 - volume::db_to_norm(db)
}

fn pointer_y_to_norm(pointer_y: f32, bounds_y: f32, bounds_h: f32) -> f32 {
    let rail_top = FADER_THUMB_HEIGHT / 2.0;
    let rail_h = (bounds_h - FADER_THUMB_HEIGHT).max(1.0);
    let rail_y = (pointer_y - bounds_y - rail_top).clamp(0.0, rail_h);
    1.0 - rail_y / rail_h
}

/// dB scale column — uses `h_full` so it stretches with the strip's flex_1
/// fader slot. Labels are anchored via fractional `top` positions; a small
/// negative `mt` centers each ~7px label vertically on its tick.
pub fn db_scale_column() -> gpui::Div {
    let mut col = div().relative().w(px(15.0)).h_full();
    for &(db, label) in SCALE_MARKS.iter() {
        let pct = db_to_top_fraction(db);
        col = col.child(
            div()
                .absolute()
                .top(relative(pct))
                .right(px(0.0))
                .mt(-px(4.0))
                .text_size(px(7.5))
                .text_color(if db == 0.0 || db == volume::MAX_DB {
                    Colors::text_primary()
                } else {
                    Colors::fader_scale_text()
                })
                .child(label),
        );
    }
    col
}

/// Render the vertical rail + ticks + thumb at `value_norm`.
fn fader_rail(value_norm: f32, accent: gpui::Rgba) -> gpui::Div {
    let value = value_norm.clamp(0.0, 1.0);

    let thumb_accent = Colors::with_alpha(accent, 0.9); // Approved: dynamic accent thumb outline

    let top_basis = (1.0 - value).clamp(0.0, 1.0);
    let bot_basis = value.clamp(0.0, 1.0);

    let mut col = div()
        .relative()
        .w(px(24.0))
        .h_full()
        .flex()
        .flex_col()
        .items_center();

    // Background rail (absolute, layered).
    col = col.child(
        div()
            .absolute()
            .top(px(FADER_THUMB_HEIGHT / 2.0))
            .bottom(px(FADER_THUMB_HEIGHT / 2.0))
            .left(px(RAIL_CENTER_X - RAIL_W / 2.0))
            .w(px(RAIL_W))
            .bg(Colors::fader_rail())
            .border(px(1.0))
            .border_color(Colors::fader_groove())
            .rounded_full(),
    );

    // Tick marks (absolute, layered) at fractional positions on the rail.
    for &(db, _) in SCALE_MARKS.iter() {
        let pct = db_to_top_fraction(db);
        let w = if db == 0.0 || db == volume::MAX_DB {
            14.0_f32
        } else {
            9.0_f32
        };
        let left = RAIL_CENTER_X - w / 2.0;
        col = col.child(
            div()
                .absolute()
                .top(relative(pct))
                .left(px(left))
                .h(px(1.0))
                .w(px(w))
                .bg(if db == 0.0 || db == volume::MAX_DB {
                    Colors::fader_tick()
                } else {
                    Colors::with_alpha(Colors::fader_tick(), 0.3) // Approved: minor tick marks alpha
                }),
        );
    }

    // Flex flow: top spacer / thumb / bot spacer.
    col.child(div().w(px(0.0)).flex_basis(relative(top_basis)))
        .child(
            div()
                .flex_none()
                .w(px(THUMB_W))
                .h(px(FADER_THUMB_HEIGHT))
                .rounded_sm()
                .bg(Colors::surface_input())
                .border(px(1.0))
                .border_color(Colors::fader_thumb_border())
                .relative()
                .child(
                    div()
                        .absolute()
                        .top(px(1.0))
                        .left(px(1.0))
                        .right(px(1.0))
                        .h(px(1.0))
                        .bg(Colors::with_alpha(Colors::text_primary(), 0.15)), // Approved: thumb top highlight
                )
                .child(
                    div()
                        .absolute()
                        .top(px((FADER_THUMB_HEIGHT - ACCENT_LINE_H) / 2.0))
                        .left(px(2.0))
                        .right(px(2.0))
                        .h(px(ACCENT_LINE_H))
                        .bg(thumb_accent),
                ),
        )
        .child(div().w(px(0.0)).flex_basis(relative(bot_basis)))
}

/// Bordered dB readout pill. Use this above the fader instead of plain text so
/// the value reads as a proper integrated control.
pub fn db_value_pill(db_text: impl Into<gpui::SharedString>, highlight: bool) -> impl IntoElement {
    let border = if highlight {
        Colors::border_default()
    } else {
        Colors::panel_border()
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
        .bg(Colors::slot_bg())
        .border(px(1.0))
        .border_color(border)
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_primary())
                .child(db_text.into()),
        )
        .child(
            div()
                .text_size(px(7.5))
                .text_color(Colors::text_muted())
                .child("dB"),
        )
}

/// Render a vertical fader and wire drag updates. Uses `h_full` — the parent
/// must constrain height (e.g. via flex_1) so the rail/thumb scale with the
/// available channel-strip slot.
pub fn fader(
    id: impl Into<gpui::SharedString>,
    value_norm: f32,
    accent: gpui::Rgba,
    on_change: impl Fn(&f32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id_str: gpui::SharedString = id.into();
    let id_string = id_str.to_string();
    let value = value_norm.clamp(0.0, 1.0);

    div()
        .id(gpui::ElementId::Name(id_str.clone()))
        // Hit area: rail width + horizontal slack so users can wander
        // horizontally without losing the drag.
        .w(px(28.0))
        .h_full()
        .relative()
        .cursor(gpui::CursorStyle::ResizeUpDown)
        .flex()
        .flex_row()
        .justify_center()
        .child(fader_rail(value, accent))
        .on_drag(
            FaderDrag {
                id: id_string.clone(),
            },
            move |drag, _offset, _window, cx| {
                cx.new(|_| FaderDrag {
                    id: drag.id.clone(),
                })
            },
        )
        .on_drag_move::<FaderDrag>(move |event: &DragMoveEvent<FaderDrag>, window, cx| {
            if event.drag(cx).id != id_string {
                return;
            }
            let bounds = event.bounds;
            let y: f32 = event.event.position.y.into();
            let oy: f32 = bounds.origin.y.into();
            let oh: f32 = f32::from(bounds.size.height).max(FADER_THUMB_HEIGHT + 1.0);
            let new_value = pointer_y_to_norm(y, oy, oh);
            on_change(&new_value, window, cx);
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_ticks_follow_shared_volume_mapping() {
        assert!((db_to_top_fraction(volume::MAX_DB) - 0.0).abs() < 1.0e-6);
        assert!((db_to_top_fraction(volume::MIN_DB) - 1.0).abs() < 1.0e-6);
        assert!((db_to_top_fraction(0.0) - (1.0 - volume::db_to_norm(0.0))).abs() < 1.0e-6);
    }

    #[test]
    fn pointer_mapping_uses_rail_travel_not_outer_hitbox() {
        let h = 210.0;
        let top = FADER_THUMB_HEIGHT / 2.0;
        let bottom = h - FADER_THUMB_HEIGHT / 2.0;

        assert!((pointer_y_to_norm(top, 0.0, h) - 1.0).abs() < 1.0e-6);
        assert!((pointer_y_to_norm(bottom, 0.0, h) - 0.0).abs() < 1.0e-6);
        assert!((pointer_y_to_norm(h / 2.0, 0.0, h) - 0.5).abs() < 1.0e-6);
    }
}
