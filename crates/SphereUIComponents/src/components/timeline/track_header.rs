use gpui::{div, px, rgba, svg, IntoElement, ParentElement, Styled, InteractiveElement};
use crate::theme::Colors;
use crate::components::timeline::timeline_state::{TrackState, TimelineState, HEADER_WIDTH, TRACK_HEIGHT};
use crate::components::timeline::vu_meter::vu_meter;
use crate::assets;

fn volume_to_db(v: f32) -> String {
    if v <= 0.001 {
        "-∞ dB".to_string()
    } else {
        let db = 20.0 * v.log10();
        if db >= 0.0 {
            format!("+{:.1} dB", db)
        } else {
            format!("{:.1} dB", db)
        }
    }
}

pub fn track_header(
    track: &TrackState,
    index: usize,
    state: &TimelineState,
    on_select_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_toggle_mute: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_toggle_solo: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_toggle_arm: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_delete_track: std::sync::Arc<dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static>,
    on_volume_change: std::sync::Arc<dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static>,
) -> impl IntoElement {
    let track_id = track.id.clone();
    let is_selected = state.selection.selected_track_id.as_ref() == Some(&track.id);
    let header_bg = if is_selected { gpui::rgb(0x252c35) } else { gpui::rgb(0x1c2028) };
    
    let on_select = on_select_track.clone();
    let track_id_select = track_id.clone();
    
    let on_mute = on_toggle_mute.clone();
    let track_id_mute = track_id.clone();
    
    let on_solo = on_toggle_solo.clone();
    let track_id_solo = track_id.clone();
    
    let on_arm = on_toggle_arm.clone();
    let track_id_arm = track_id.clone();

    let on_delete = on_delete_track.clone();
    let track_id_delete = track_id.clone();

    let on_vol = on_volume_change.clone();
    let track_id_vol = track_id.clone();
    let current_volume = track.volume;

    let id_num = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        track.id.hash(&mut hasher);
        hasher.finish() as usize
    };

    div()
        .flex()
        .flex_row()
        .w(px(HEADER_WIDTH))
        .h(px(TRACK_HEIGHT))
        .bg(header_bg)
        .border_r(px(1.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .id(("track-header", id_num))
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            on_select(&track_id_select, window, cx);
        })
        .child(
            // Left Color Strip
            div()
                .w(px(4.0))
                .h_full()
                .bg(track.color)
        )
        .child(
            // Main content area
            div()
                .flex()
                .flex_col()
                .justify_between()
                .flex_1()
                .px(px(8.0))
                .py(px(6.0))
                // Row 1: Name, Type Badge, Controls
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.0))
                                .child(
                                    svg()
                                        .path(assets::ICON_GRIP_VERTICAL_PATH)
                                        .w(px(10.0))
                                        .h(px(10.0))
                                        .text_color(Colors::text_faint())
                                )
                                .child(
                                    // Name + Badge
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(3.0))
                                                .child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                                        .text_color(Colors::text_primary())
                                                        .child(track.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .px(px(2.0))
                                                        .py(px(0.5))
                                                        .rounded_sm()
                                                        .bg({
                                                            let mut c = track.color;
                                                            c.a = 0.15;
                                                            c
                                                        })
                                                        .text_color(track.color)
                                                        .text_size(px(8.0))
                                                        .font_weight(gpui::FontWeight::BOLD)
                                                        .child(match track.track_type {
                                                            crate::components::timeline::timeline_state::TrackType::Audio => "AUD",
                                                            crate::components::timeline::timeline_state::TrackType::Midi => "MID",
                                                            crate::components::timeline::timeline_state::TrackType::Instrument => "INS",
                                                            crate::components::timeline::timeline_state::TrackType::Master => "MAS",
                                                        })
                                                )
                                        )
                                        .child(
                                            div()
                                                .text_size(px(8.0))
                                                .text_color(Colors::text_muted())
                                                .child(format!("CH {:02} · {} clips", index + 1, track.clips.len())),
                                        )
                                )
                        )
                        .child(
                            // M/S/R/Delete Buttons Block
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(2.0))
                                .px(px(2.0))
                                .py(px(2.0))
                                .rounded_md()
                                .bg(gpui::rgba(0x0000003A)) // black/15
                                .border(px(1.0))
                                .border_color(gpui::rgba(0xFFFFFF0F))
                                // Mute Button
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(16.0))
                                        .h(px(16.0))
                                        .rounded_sm()
                                        .cursor(gpui::CursorStyle::PointingHand)
                                        .bg(if track.muted { gpui::rgb(0xf3c969) } else { gpui::rgba(0xFFFFFF0D) })
                                        .text_color(if track.muted { gpui::rgb(0x101216) } else { Colors::text_secondary() })
                                        .text_size(px(9.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .id(("mute-btn", id_num))
                                        .on_mouse_down(gpui::MouseButton::Left, move |event: &gpui::MouseDownEvent, window, cx| {
                                            on_mute(&track_id_mute, window, cx);
                                        })
                                        .child("M")
                                )
                                // Solo Button
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(16.0))
                                        .h(px(16.0))
                                        .rounded_sm()
                                        .cursor(gpui::CursorStyle::PointingHand)
                                        .bg(if track.solo { gpui::rgb(0x7bd88f) } else { gpui::rgba(0xFFFFFF0D) })
                                        .text_color(if track.solo { gpui::rgb(0x101216) } else { Colors::text_secondary() })
                                        .text_size(px(9.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .id(("solo-btn", id_num))
                                        .on_mouse_down(gpui::MouseButton::Left, move |event: &gpui::MouseDownEvent, window, cx| {
                                            on_solo(&track_id_solo, window, cx);
                                        })
                                        .child("S")
                                )
                                // Arm Button
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(16.0))
                                        .h(px(16.0))
                                        .rounded_sm()
                                        .cursor(gpui::CursorStyle::PointingHand)
                                        .bg(if track.armed { gpui::rgb(0xf06a61) } else { gpui::rgba(0xFFFFFF0D) })
                                        .text_color(if track.armed { gpui::rgb(0x101216) } else { Colors::text_secondary() })
                                        .text_size(px(9.0))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .id(("arm-btn", id_num))
                                        .on_mouse_down(gpui::MouseButton::Left, move |event: &gpui::MouseDownEvent, window, cx| {
                                            on_arm(&track_id_arm, window, cx);
                                        })
                                        .child("R")
                                )
                                // Delete Button
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(16.0))
                                        .h(px(16.0))
                                        .rounded_sm()
                                        .cursor(gpui::CursorStyle::PointingHand)
                                        .bg(gpui::rgba(0xFFFFFF0D))
                                        .text_color(Colors::text_secondary())
                                        .text_size(px(9.0))
                                        .id(("del-btn", id_num))
                                        .on_mouse_down(gpui::MouseButton::Left, move |event: &gpui::MouseDownEvent, window, cx| {
                                            on_delete(&track_id_delete, window, cx);
                                        })
                                        .child(
                                            svg()
                                                .path(assets::ICON_X_PATH)
                                                .w(px(10.0))
                                                .h(px(10.0))
                                                .text_color(Colors::text_secondary())
                                        )
                                )
                        )
                )
                // Row 2: Volume slider, Pan indicator, VU Meter, dB readout
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .w_full()
                        .px(px(4.0))
                        .py(px(2.0))
                        .rounded_md()
                        .bg(gpui::rgba(0x0000002A)) // black/10
                        .border(px(1.0))
                        .border_color(gpui::rgba(0xFFFFFF09))
                        // Volume Fader Rail
                        .child(
                            div()
                                .flex_1()
                                .h(px(6.0))
                                .bg(gpui::rgba(0xFFFFFF0D))
                                .rounded_full()
                                .relative()
                                .cursor(gpui::CursorStyle::ResizeLeftRight)
                                .id(("vol-slider", id_num))
                                // Click changes or cycles volume level
                                .on_mouse_down(gpui::MouseButton::Left, move |event: &gpui::MouseDownEvent, window, cx| {
                                    // Cycles volume: 0.0 -> 0.3 -> 0.6 -> 0.9 -> 1.0 -> 0.0
                                    let next_vol = if current_volume >= 0.95 {
                                        0.0
                                    } else if current_volume >= 0.85 {
                                        1.0
                                    } else if current_volume >= 0.55 {
                                        0.9
                                    } else if current_volume >= 0.25 {
                                        0.6
                                    } else {
                                        0.3
                                    };
                                    on_vol(&(track_id_vol.clone(), next_vol), window, cx);
                                })
                                // Volume Fill bar
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(track.volume * 50.0)) // Fader length factor
                                        .bg(track.color)
                                        .rounded_full()
                                )
                                // Fader Handle
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(track.volume * 50.0 - 3.0))
                                        .top(px(-2.0))
                                        .w(px(6.0))
                                        .h(px(10.0))
                                        .rounded_sm()
                                        .bg(Colors::text_primary())
                                        .border(px(1.0))
                                        .border_color(Colors::border_strong())
                                )
                        )
                        // Pan indicator dot/knob representation
                        .child(
                            div()
                                .w(px(12.0))
                                .h(px(12.0))
                                .rounded_full()
                                .bg(Colors::surface_raised())
                                .border(px(1.0))
                                .border_color(Colors::border_subtle())
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .w(px(3.0))
                                        .h(px(3.0))
                                        .rounded_full()
                                        .bg(track.color)
                                )
                        )
                        // VU Meter
                        .child(vu_meter(&track.id))
                        // dB Readout text
                        .child(
                            div()
                                .w(px(36.0))
                                .text_align(gpui::TextAlign::Right)
                                .text_size(px(8.5))
                                .text_color(Colors::text_muted())
                                .child(volume_to_db(track.volume))
                        )
                )
        )
}
