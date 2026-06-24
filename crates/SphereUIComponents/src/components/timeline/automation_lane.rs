use crate::components::sidebar::SIDEBAR_WIDTH;
use crate::components::timeline::timeline_state::{
    automation_value_to_y, automation_y_to_value, evaluate_automation, AutomationLaneState,
    AutomationMarquee, AutomationTarget, TimelineState, HEADER_WIDTH,
};
use crate::theme::Colors;
use gpui::{
    canvas, div, fill, point, px, size, Bounds, InteractiveElement, IntoElement, ParentElement,
    Pixels, Styled,
};

/// Top chrome height above the timeline ruler — mirrors `timeline.rs` so a
/// window-space click can be mapped into a sub-lane-local value.
const APP_CHROME_HEIGHT: f32 = 36.0;

/// Left inset for automation sub-lane header content. Keeps lane titles visually
/// nested under the parent track without shifting the timeline grid.
const AUTOMATION_SUBLANE_HEADER_INDENT: f32 = 28.0;

/// X-position of the vertical child-lane guide inside the header column.
const AUTOMATION_SUBLANE_RAIL_X: f32 = 18.0;

/// Action fired from a sub-lane header control. One callback handles them all so
/// new lane controls land without re-threading the whole call stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationLaneAction {
    /// Focus the editor on this lane (header body click).
    Activate,
    /// Toggle the lane's read on/off (enabled flag).
    ToggleEnable,
    /// Remove every point but keep the lane.
    Clear,
    /// Hide the sub-lane row (lane + points preserved).
    Hide,
}

/// Sub-lane mouse-down payload: `(track_id, lane_id, beat, value_norm, additive)`.
pub type AutomationDownCallback = std::sync::Arc<
    dyn Fn(&(String, String, f32, f32, bool), &mut gpui::Window, &mut gpui::App) + 'static,
>;

/// Sub-lane header action payload: `(track_id, lane_id, action)`.
pub type AutomationLaneActionCallback = std::sync::Arc<
    dyn Fn(&(String, String, AutomationLaneAction), &mut gpui::Window, &mut gpui::App) + 'static,
>;

/// Human category shown under the lane name in the sub-lane header.
fn target_category(target: &AutomationTarget) -> &'static str {
    match target {
        AutomationTarget::TrackVolume
        | AutomationTarget::TrackPan
        | AutomationTarget::TrackMute => "Track",
        AutomationTarget::PluginParameter { .. } => "Plugin Parameter",
        AutomationTarget::SendLevel { .. } => "Send",
    }
}

/// One expanded automation sub-lane rendered directly below its parent track.
/// Left header carries the target name/category + lane controls; the right area
/// draws the envelope (line + points) and captures point edits scoped to this
/// lane's own row bounds.
#[allow(clippy::too_many_arguments)]
pub fn automation_lane(
    track_id: &str,
    lane: &AutomationLaneState,
    track_color: gpui::Rgba,
    is_active: bool,
    lane_y_abs: f32,
    lane_height: f32,
    state: &TimelineState,
    on_automation_down: Option<AutomationDownCallback>,
    on_lane_action: Option<AutomationLaneActionCallback>,
    marquee: Option<&AutomationMarquee>,
) -> impl IntoElement {
    let track_id = track_id.to_string();
    let lane_id = lane.id.clone();
    let id_num = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        track_id.hash(&mut hasher);
        lane.id.hash(&mut hasher);
        hasher.finish() as usize
    };

    let category = target_category(&lane.target);
    let category_color = if is_active {
        Colors::with_alpha(Colors::accent_primary(), 0.72)
    } else {
        Colors::with_alpha(Colors::text_muted(), 0.62)
    };
    let mut accent = track_color;
    accent.a = if is_active { 0.92 } else { 0.55 };

    // ── Left header (indented child lane — timeline grid stays flush right) ───
    let activate_action = on_lane_action.clone();
    let mut header = div()
        .relative()
        .w(px(HEADER_WIDTH))
        .h_full()
        .flex_none()
        .border_r(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(if is_active {
            Colors::with_alpha(Colors::accent_primary(), 0.07)
        } else {
            Colors::surface_panel()
        })
        .id(("automation-lane-header", id_num))
        .cursor(gpui::CursorStyle::PointingHand);
    if let Some(cb) = activate_action {
        let tid = track_id.clone();
        let lid = lane_id.clone();
        header = header.on_mouse_down(gpui::MouseButton::Left, move |_e, window, cx| {
            cx.stop_propagation();
            cb(&(tid.clone(), lid.clone(), AutomationLaneAction::Activate), window, cx);
        });
    }

    // Nesting gutter — slightly darker band in the indent column so child lanes
    // read as belonging to the parent, not as peer tracks.
    header = header.child(
        div()
            .absolute()
            .left_0()
            .top_0()
            .bottom_0()
            .w(px(AUTOMATION_SUBLANE_HEADER_INDENT))
            .bg(Colors::with_alpha(Colors::surface_base(), 0.35)),
    );

    // Vertical child-lane guide shared by every automation sub-row.
    header = header.child(
        div()
            .absolute()
            .left(px(AUTOMATION_SUBLANE_RAIL_X))
            .top(px(9.0))
            .bottom(px(9.0))
            .w(px(1.0))
            .bg(Colors::with_alpha(track_color, if is_active { 0.55 } else { 0.28 })),
    );

    // Accent bar + title — indented, smaller than the parent track name.
    let name_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .min_w(px(0.0))
        .child(div().w(px(2.0)).h(px(9.0)).rounded_full().bg(accent))
        .child(
            div()
                .text_size(px(9.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(if lane.enabled {
                    Colors::text_secondary()
                } else {
                    Colors::text_muted()
                })
                .overflow_hidden()
                .child(lane.name.clone()),
        );

    let category_row = div()
        .text_size(px(8.0))
        .text_color(category_color)
        .pl(px(6.0))
        .child(category);

    // Lane controls stay flush to the right edge of the header column.
    let control_buttons = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(3.0))
        .child(lane_button(
            ("automation-lane-enable", id_num).into(),
            "E",
            lane.enabled,
            track_id.clone(),
            lane_id.clone(),
            AutomationLaneAction::ToggleEnable,
            on_lane_action.clone(),
        ))
        .child(lane_button(
            ("automation-lane-clear", id_num).into(),
            "C",
            false,
            track_id.clone(),
            lane_id.clone(),
            AutomationLaneAction::Clear,
            on_lane_action.clone(),
        ))
        .child(lane_button(
            ("automation-lane-hide", id_num).into(),
            "x",
            false,
            track_id.clone(),
            lane_id.clone(),
            AutomationLaneAction::Hide,
            on_lane_action.clone(),
        ));

    let header = header.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h_full()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap(px(1.0))
                    .pl(px(AUTOMATION_SUBLANE_HEADER_INDENT))
                    .pr(px(4.0))
                    .child(name_row)
                    .child(category_row),
            )
            .child(div().flex_none().pr(px(8.0)).child(control_buttons)),
    );

    // ── Right envelope + interaction area ────────────────────────────────────
    let envelope = lane_envelope(lane, state, lane_height, marquee);

    let interaction = on_automation_down.clone().map(|cb| {
        let state_for = state.clone();
        let tid = track_id.clone();
        let lid = lane_id.clone();
        div()
            .absolute()
            .inset_0()
            .id(("automation-lane-hit", id_num))
            .on_mouse_down(
                gpui::MouseButton::Left,
                move |event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let wx: f32 = event.position.x.into();
                    let wy: f32 = event.position.y.into();
                    let lane_x = wx - SIDEBAR_WIDTH - HEADER_WIDTH;
                    let raw_beat = state_for.x_to_beats(lane_x);
                    let snapped_sec = state_for.snap_time(raw_beat * state_for.seconds_per_beat());
                    let beat = (snapped_sec / state_for.seconds_per_beat()).max(0.0);
                    let content_y = wy - APP_CHROME_HEIGHT - state_for.arrangement_content_top()
                        + state_for.viewport.scroll_y;
                    let local_y = content_y - lane_y_abs;
                    let value = automation_y_to_value(local_y, lane_height);
                    let additive = event.modifiers.shift || event.modifiers.control;
                    cb(&(tid.clone(), lid.clone(), beat, value, additive), window, cx);
                },
            )
    });

    let lane_area = div()
        .flex_1()
        .h_full()
        .relative()
        .overflow_hidden()
        .child(envelope)
        .children(interaction);

    div()
        .flex()
        .flex_row()
        .w_full()
        .h(px(lane_height))
        // Sub-lane rows stay flatter than parent tracks so the arrangement
        // lane remains visually dominant.
        .bg(if is_active {
            Colors::with_alpha(Colors::accent_primary(), 0.03)
        } else {
            Colors::with_alpha(Colors::surface_base(), 0.45)
        })
        .border_b(px(1.0))
        .border_color(Colors::with_alpha(Colors::border_subtle(), 0.85))
        .child(header)
        .child(lane_area)
}

/// Small square control button used in the sub-lane header.
#[allow(clippy::too_many_arguments)]
fn lane_button(
    id: gpui::ElementId,
    label: &'static str,
    active: bool,
    track_id: String,
    lane_id: String,
    action: AutomationLaneAction,
    cb: Option<AutomationLaneActionCallback>,
) -> impl IntoElement {
    let mut btn = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(14.0))
        .h(px(14.0))
        .rounded_sm()
        .text_size(px(8.0))
        .font_weight(gpui::FontWeight::BOLD)
        .id(id)
        .cursor(gpui::CursorStyle::PointingHand);
    if active {
        btn = btn
            .bg(Colors::accent_primary())
            .text_color(Colors::text_inverse());
    } else {
        btn = btn
            .bg(Colors::with_alpha(Colors::text_primary(), 0.05))
            .text_color(Colors::text_secondary())
            .hover(|s| s.bg(Colors::surface_hover()));
    }
    if let Some(cb) = cb {
        btn = btn.on_mouse_down(gpui::MouseButton::Left, move |_e, window, cx| {
            cx.stop_propagation();
            cb(&(track_id.clone(), lane_id.clone(), action), window, cx);
        });
    }
    btn.child(label)
}

/// Draw the automation line + points for one lane inside its sub-lane area.
/// Pure render of state. The curve is sampled per visible column so Hold steps
/// and Linear ramps are both correct.
fn lane_envelope(
    lane: &AutomationLaneState,
    state: &TimelineState,
    lane_height: f32,
    marquee: Option<&AutomationMarquee>,
) -> impl IntoElement {
    let default_value = lane.target.default_value();
    let points = lane.points.clone();

    let lane_w = state.viewport.viewport_width.max(1.0);
    let num_cols = lane_w.ceil().max(1.0) as usize;

    let mut samples: Vec<f32> = Vec::with_capacity(num_cols + 1);
    for col in 0..=num_cols {
        let beat = state.x_to_beat(col as f32);
        let v = evaluate_automation(&points, beat, default_value);
        samples.push(automation_value_to_y(v, lane_height));
    }
    let baseline_y = automation_value_to_y(default_value, lane_height);

    let enabled = lane.enabled;
    let line_color = if enabled {
        Colors::accent_primary()
    } else {
        Colors::with_alpha(Colors::accent_primary(), 0.35)
    };
    let baseline_color = Colors::with_alpha(Colors::text_primary(), 0.10);

    let line = canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            let bl = Bounds::new(
                bounds.origin + point(px(0.0), px(baseline_y)),
                size(px(lane_w), px(1.0)),
            );
            window.paint_quad(fill(bl, baseline_color));
            for col in 0..num_cols {
                let y0 = samples[col];
                let y1 = samples[col + 1];
                let top = y0.min(y1);
                let h = (y0 - y1).abs().max(1.6);
                let r = Bounds::new(
                    bounds.origin + point(px(col as f32), px(top)),
                    size(px(1.0), px(h)),
                );
                window.paint_quad(fill(r, line_color));
            }
        },
    )
    .absolute()
    .inset_0();

    let markers: Vec<_> = points
        .iter()
        .filter_map(|p| {
            let x = state.beats_to_x(p.beat);
            if x < -8.0 || x > lane_w + 8.0 {
                return None;
            }
            let y = automation_value_to_y(p.value, lane_height);
            let (fill_color, ring) = if p.selected {
                (Colors::text_primary(), Colors::accent_primary())
            } else {
                (Colors::accent_primary(), Colors::text_primary())
            };
            let size_px = if p.selected { 9.0 } else { 7.0 };
            Some(
                div()
                    .absolute()
                    .left(px(x - size_px / 2.0))
                    .top(px(y - size_px / 2.0))
                    .w(px(size_px))
                    .h(px(size_px))
                    .rounded_full()
                    .bg(fill_color)
                    .border(px(1.0))
                    .border_color(ring),
            )
        })
        .collect();

    let marquee_el = marquee.filter(|m| m.lane_id == lane.id).map(|m| {
        let x0 = state.beats_to_x(m.start_beat.min(m.cur_beat));
        let x1 = state.beats_to_x(m.start_beat.max(m.cur_beat));
        let y0 = automation_value_to_y(m.start_value.max(m.cur_value), lane_height);
        let y1 = automation_value_to_y(m.start_value.min(m.cur_value), lane_height);
        div()
            .absolute()
            .left(px(x0))
            .top(px(y0))
            .w(px((x1 - x0).max(1.0)))
            .h(px((y1 - y0).max(1.0)))
            .bg(Colors::with_alpha(Colors::accent_primary(), 0.14))
            .border(px(1.0))
            .border_color(Colors::with_alpha(Colors::accent_primary(), 0.7))
    });

    div()
        .absolute()
        .inset_0()
        .child(line)
        .children(markers)
        .children(marquee_el)
}
