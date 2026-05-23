//! Ableton-style knob for bipolar values (pan).
//!
//! Visual language:
//! * dark circular body with a thin outer border
//! * a *dotted arc track* covering the full sweep (-135° ... +135°) in muted dots
//! * an active arc highlighted in the track's accent color from 0° (top center)
//!   to the current value angle — fills clockwise for positive values,
//!   counter-clockwise for negative
//! * a bright pointer dot at the indicator angle
//! * small center highlight dot
//!
//! Below the disk sits a small bordered value pill. The pill is part of the
//! same component so that a "knob with value" can be dropped into any layout
//! and always stay in visual sync with the underlying state.
//!
//! Interaction:
//! * vertical drag within the knob's enlarged hit area moves the value
//!   (positional: top = max, bottom = min)
//! * double-click on the knob resets the value to `reset_to` (typically 0.0)

use gpui::{
    div, px, rgba, App, AppContext, DragMoveEvent, Empty, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};

use crate::theme::Colors;

pub const KNOB_DIAMETER: f32 = 30.0;
pub const KNOB_HIT_HEIGHT: f32 = 56.0;
pub const KNOB_HIT_WIDTH: f32 = 56.0;

/// Number of dots used to draw the indicator arc. Higher = smoother but more
/// elements per knob. 21 is the sweet spot for a 30 px disk.
const ARC_DOTS: usize = 21;

/// Sweep of the indicator arc in degrees, measured from the top center.
const SWEEP_DEG: f32 = 135.0;

#[derive(Clone, Debug)]
pub struct KnobDrag {
    pub id: String,
}

impl Render for KnobDrag {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// Render a bipolar knob (`value ∈ [-1, 1]`) without a value pill.
///
/// Use [`knob_with_value`] when you also want the bordered pan-position
/// readout — that is the canonical mixer-strip pan knob.
pub fn knob(
    id: impl Into<gpui::SharedString>,
    value: f32,
    accent: gpui::Rgba,
    on_change: impl Fn(&f32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    knob_inner(id, value, accent, 0.0, on_change)
}

/// Render a pan knob with a bordered value pill underneath.
///
/// `value_label` must already be formatted by the caller (e.g. "C", "L25",
/// "R50"). The pill is purely a presentation surface; it does not interact.
pub fn knob_with_value(
    id: impl Into<gpui::SharedString>,
    value: f32,
    accent: gpui::Rgba,
    value_label: impl Into<gpui::SharedString>,
    border_highlight: bool,
    on_change: impl Fn(&f32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id_str: gpui::SharedString = id.into();
    let label: gpui::SharedString = value_label.into();

    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.0))
        .child(knob_inner(id_str, value, accent, 0.0, on_change))
        .child(value_pill(label, accent, border_highlight))
}

/// Small bordered presentation pill, e.g. for "C", "L25", "R50".
pub fn value_pill(
    text: impl Into<gpui::SharedString>,
    accent: gpui::Rgba,
    highlight: bool,
) -> impl IntoElement {
    let mut border = if highlight {
        let mut c = accent;
        c.a = 0.55;
        c
    } else {
        rgba(0xFFFFFF1F_u32)
    };
    if !highlight {
        border.a = 0.25;
    }

    div()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(28.0))
        .px(px(6.0))
        .h(px(14.0))
        .rounded_sm()
        .bg(rgba(0x0000003A_u32))
        .border(px(1.0))
        .border_color(border)
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_secondary())
        .child(text.into())
}

fn knob_inner(
    id: impl Into<gpui::SharedString>,
    value: f32,
    accent: gpui::Rgba,
    reset_to: f32,
    on_change: impl Fn(&f32, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id_str: gpui::SharedString = id.into();
    let id_string = id_str.to_string();
    let value = value.clamp(-1.0, 1.0);
    // Share `on_change` between the drag and double-click handlers.
    let on_change_shared: std::sync::Arc<dyn Fn(&f32, &mut Window, &mut App) + 'static> =
        std::sync::Arc::new(on_change);
    let on_change_drag = on_change_shared.clone();
    let on_change_reset = on_change_shared.clone();

    let cx_disk = KNOB_DIAMETER / 2.0;
    let cy_disk = KNOB_DIAMETER / 2.0;
    let arc_radius = cx_disk - 2.5; // outer track inset
    let active_radius = cx_disk - 2.5; // same radius for accent dots
    let pointer_radius = cx_disk * 0.62;

    // Build the dotted arc track. Each dot is rendered as a 2 px disk.
    // Dots from 0° (top) to the indicator angle are accent; the rest are dim.
    let value_angle_deg = value * SWEEP_DEG;
    let mut arc_children: Vec<gpui::AnyElement> = Vec::with_capacity(ARC_DOTS + 4);

    for i in 0..ARC_DOTS {
        let t = i as f32 / (ARC_DOTS - 1) as f32;
        let deg = -SWEEP_DEG + t * (2.0 * SWEEP_DEG);
        let rad = deg.to_radians();
        let x = cx_disk + rad.sin() * arc_radius;
        let y = cy_disk + -rad.cos() * arc_radius;

        // Highlight rule: a dot is active if it lies between 0° and the
        // current indicator angle (inclusive), on the correct side.
        let active = if value_angle_deg >= 0.0 {
            deg >= -0.001 && deg <= value_angle_deg + 0.001
        } else {
            deg <= 0.001 && deg >= value_angle_deg - 0.001
        };

        let mut color = if active {
            accent
        } else {
            rgba(0xFFFFFF1A_u32)
        };
        if !active {
            color.a = 0.28;
        }

        arc_children.push(
            div()
                .absolute()
                .left(px(x - 1.0))
                .top(px(y - 1.0))
                .w(px(2.0))
                .h(px(2.0))
                .rounded_full()
                .bg(color)
                .into_any_element(),
        );
    }

    // Pointer dot — bright accent at the indicator angle on the inner radius.
    {
        let rad = value_angle_deg.to_radians();
        let x = cx_disk + rad.sin() * pointer_radius;
        let y = cy_disk + -rad.cos() * pointer_radius;
        arc_children.push(
            div()
                .absolute()
                .left(px(x - 1.5))
                .top(px(y - 1.5))
                .w(px(3.0))
                .h(px(3.0))
                .rounded_full()
                .bg(accent)
                .into_any_element(),
        );
    }

    // Center highlight dot.
    arc_children.push(
        div()
            .absolute()
            .left(px(cx_disk - 1.25))
            .top(px(cy_disk - 1.25))
            .w(px(2.5))
            .h(px(2.5))
            .rounded_full()
            .bg(rgba(0xFFFFFF38_u32))
            .into_any_element(),
    );

    // Surrounding bright/dim arc shadow as a single ring under the dots so the
    // knob doesn't read as "floating dots on a black disk".
    let mut ring = accent;
    ring.a = 0.10;

    div()
        .id(gpui::ElementId::Name(id_str.clone()))
        .w(px(KNOB_HIT_WIDTH))
        .h(px(KNOB_HIT_HEIGHT))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .cursor(gpui::CursorStyle::ResizeUpDown)
        .child(
            div()
                .w(px(KNOB_DIAMETER))
                .h(px(KNOB_DIAMETER))
                .rounded_full()
                .bg(rgba(0x0F1419FF_u32))
                .border(px(1.0))
                .border_color(rgba(0xFFFFFF1F_u32))
                .relative()
                // Faint accent halo so the knob doesn't read as a pure black disk.
                .child(
                    div()
                        .absolute()
                        .left(px(3.0))
                        .top(px(3.0))
                        .w(px(KNOB_DIAMETER - 6.0))
                        .h(px(KNOB_DIAMETER - 6.0))
                        .rounded_full()
                        .border(px(1.0))
                        .border_color(ring),
                )
                .children(arc_children),
        )
        .on_drag(KnobDrag { id: id_string.clone() }, move |drag, _offset, _window, cx| {
            cx.new(|_| KnobDrag { id: drag.id.clone() })
        })
        .on_drag_move::<KnobDrag>(move |event: &DragMoveEvent<KnobDrag>, window, cx| {
            if event.drag(cx).id != id_string {
                return;
            }
            let bounds = event.bounds;
            let y: f32 = event.event.position.y.into();
            let oy: f32 = bounds.origin.y.into();
            let oh: f32 = f32::from(bounds.size.height).max(1.0);
            let t = ((y - oy) / oh).clamp(0.0, 1.0);
            let new_value = (1.0 - 2.0 * t).clamp(-1.0, 1.0);
            on_change_drag(&new_value, window, cx);
        })
        // Double-click resets the value to `reset_to` (0.0 for pan).
        .on_click(move |event, window, cx| {
            if event.click_count() >= 2 {
                on_change_reset(&reset_to, window, cx);
            }
        })
}
