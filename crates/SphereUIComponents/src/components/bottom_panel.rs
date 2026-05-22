use gpui::{div, px, InteractiveElement, StatefulInteractiveElement, IntoElement, ParentElement, Styled, Window, App};
use crate::theme::Colors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Mixer,
    Editor,
    EffectEditor,
}

// ─── Sub-components for Mixer ────────────────────────────────────────────────

fn mixer_meter() -> impl IntoElement {
    div()
        .w(px(6.0))
        .h(px(100.0))
        .bg(Colors::surface_input())
        .rounded_sm()
        .relative()
        // Green active signal bar
        .child(
            div()
                .absolute()
                .bottom(px(0.0))
                .w_full()
                .h(px(70.0))
                .bg(Colors::status_success()),
        )
        // Yellow warning peak
        .child(
            div()
                .absolute()
                .bottom(px(70.0))
                .w_full()
                .h(px(15.0))
                .bg(Colors::status_warning()),
        )
}

fn mixer_fader(track_color: gpui::Rgba) -> impl IntoElement {
    div()
        .w(px(20.0))
        .h(px(100.0))
        .flex()
        .justify_center()
        .relative()
        // Fader track line
        .child(
            div()
                .w(px(2.0))
                .h_full()
                .bg(Colors::surface_input()),
        )
        // Fader handle (positioned in middle for default level)
        .child(
            div()
                .absolute()
                .top(px(40.0))
                .w(px(16.0))
                .h(px(12.0))
                .rounded_sm()
                .bg(Colors::text_primary())
                .border(px(1.0))
                .border_color(track_color)
                .hover(|style| style.bg(Colors::text_secondary())),
        )
}

fn channel_strip(
    name: &'static str,
    track_color: gpui::Rgba,
    is_master: bool,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(90.0))
        .h_full()
        .border_r(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(if is_master { Colors::surface_raised() } else { Colors::surface_panel() })
        // Top track accent
        .child(
            div()
                .w_full()
                .h(px(2.0))
                .bg(track_color),
        )
        // Track Name
        .child(
            div()
                .px(px(6.0))
                .py(px(4.0))
                .text_color(Colors::text_primary())
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(name),
        )
        // Fader & Meter Area
        .child(
            div()
                .flex()
                .flex_row()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(mixer_fader(track_color))
                .child(mixer_meter()),
        )
        // Controls Row: Mute / Solo
        .child(
            div()
                .flex()
                .flex_row()
                .justify_center()
                .gap_1()
                .py(px(4.0))
                .child(
                    div()
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded_sm()
                        .bg(Colors::surface_raised())
                        .text_color(Colors::text_muted())
                        .text_size(px(8.5))
                        .child("M"),
                )
                .child(
                    div()
                        .px(px(4.0))
                        .py(px(1.0))
                        .rounded_sm()
                        .bg(Colors::surface_raised())
                        .text_color(Colors::text_muted())
                        .text_size(px(8.5))
                        .child("S"),
                ),
        )
}

fn mixer_panel() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .size_full()
        .bg(Colors::surface_base())
        .child(channel_strip("Audio 1", gpui::rgb(0x56C7C9), false))
        .child(channel_strip("Audio 2", gpui::rgb(0x7EDB9A), false))
        .child(channel_strip("Synth 3", gpui::rgb(0xF2C96D), false))
        .child(channel_strip("Vocals", gpui::rgb(0xF27E77), false))
        .child(div().flex_1()) // spacer
        .child(channel_strip("Master", gpui::rgb(0x5FCED0), true))
}

// ─── Sub-components for Editor ───────────────────────────────────────────────

fn piano_key(is_black: bool) -> impl IntoElement {
    let bg_color = if is_black { Colors::surface_base() } else { Colors::text_primary() };
    div()
        .h(px(14.0))
        .w_full()
        .bg(bg_color)
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
}

fn midi_note(width: f32, delay: f32, note_y: f32) -> impl IntoElement {
    div()
        .absolute()
        .top(px(note_y))
        .left(px(delay))
        .w(px(width))
        .h(px(10.0))
        .rounded_sm()
        .bg(Colors::accent_primary())
        .border(px(1.0))
        .border_color(gpui::rgb(0x8AE9EB))
}

fn editor_panel() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .size_full()
        .bg(Colors::surface_base())
        // Left Piano Roll Keys
        .child(
            div()
                .w(px(40.0))
                .h_full()
                .bg(Colors::surface_panel())
                .border_r(px(1.0))
                .border_color(Colors::border_subtle())
                .flex_col()
                .child(piano_key(false))
                .child(piano_key(true))
                .child(piano_key(false))
                .child(piano_key(true))
                .child(piano_key(false))
                .child(piano_key(false))
                .child(piano_key(true))
                .child(piano_key(false))
                .child(piano_key(true))
                .child(piano_key(false))
        )
        // Right Grid Area
        .child(
            div()
                .flex_1()
                .h_full()
                .relative()
                // Horizontal grid lanes
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .flex_col()
                        .children((0..10).map(|_| {
                            div()
                                .h(px(14.0))
                                .w_full()
                                .border_b(px(1.0))
                                .border_color(Colors::border_subtle())
                        }))
                )
                // Vertical beat dividers
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .flex_row()
                        .children((0..8).map(|_| {
                            div()
                                .w(px(80.0))
                                .h_full()
                                .border_r(px(1.0))
                                .border_color(Colors::border_subtle())
                        }))
                )
                // Render mock MIDI notes
                .child(midi_note(60.0, 20.0, 14.0))
                .child(midi_note(80.0, 90.0, 42.0))
                .child(midi_note(40.0, 180.0, 70.0))
                .child(midi_note(120.0, 240.0, 28.0))
        )
}

// ─── Sub-components for Effect Editor ────────────────────────────────────────

fn plugin_slot(name: &'static str, is_active: bool) -> impl IntoElement {
    div()
        .w(px(130.0))
        .h(px(80.0))
        .rounded_md()
        .bg(Colors::surface_panel())
        .border(px(1.0))
        .border_color(if is_active { Colors::accent_primary() } else { Colors::border_subtle() })
        .px(px(8.0))
        .py(px(6.0))
        .flex_col()
        .justify_between()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_color(Colors::text_primary())
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(
                    div()
                        .w(px(8.0))
                        .h(px(8.0))
                        .rounded_full()
                        .bg(if is_active { Colors::accent_primary() } else { Colors::text_muted() }),
                ),
        )
        .child(
            // Parameter slider mock
            div()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_color(Colors::text_muted())
                        .text_size(px(8.5))
                        .child("Mix: 80%"),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(3.0))
                        .bg(Colors::surface_input())
                        .rounded_full()
                        .relative()
                        .child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .w(px(80.0))
                                .h_full()
                                .bg(Colors::accent_primary())
                                .rounded_full()
                        ),
                ),
        )
}

fn empty_plugin_slot() -> impl IntoElement {
    div()
        .w(px(130.0))
        .h(px(80.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .border_dashed()
        .flex()
        .items_center()
        .justify_center()
        .text_color(Colors::text_muted())
        .text_size(px(10.0))
        .child("+ Add Effect")
}

fn effect_editor_panel() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .size_full()
        .bg(Colors::surface_base())
        .px(px(12.0))
        .gap_3()
        .child(plugin_slot("Equalizer", true))
        .child(plugin_slot("Compressor", true))
        .child(plugin_slot("Delay Unit", false))
        .child(empty_plugin_slot())
}

// ─── Main Bottom Panel ───────────────────────────────────────────────────────

fn tab_button(
    label: &'static str,
    tab: BottomTab,
    active_tab: BottomTab,
    on_click: std::sync::Arc<impl Fn(&BottomTab, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let active = tab == active_tab;
    let on_click_clone = on_click.clone();
    let mut btn = div()
        .relative()
        .flex()
        .items_center()
        .h(px(24.0))
        .px(px(10.0))
        .rounded_md()
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .id(label)
        .on_click(move |_, window, cx| {
            on_click_clone(&tab, window, cx);
        });

    if active {
        btn = btn
            .bg(Colors::surface_hover())
            .text_color(Colors::text_primary())
            // Accent indicator at the bottom
            .child(
                div()
                    .absolute()
                    .bottom(px(0.0))
                    .left(px(6.0))
                    .right(px(6.0))
                    .h(px(2.0))
                    .bg(Colors::accent_primary()),
            );
    } else {
        btn = btn
            .text_color(Colors::text_muted())
            .hover(|style| {
                style
                    .bg(Colors::surface_hover())
                    .text_color(Colors::text_secondary())
            });
    }

    btn
}

pub fn bottom_panel(
    active_tab: BottomTab,
    on_tab_click: impl Fn(&BottomTab, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_tab_click = std::sync::Arc::new(on_tab_click);
    div()
        .flex()
        .flex_col()
        .h(px(240.0))
        .w_full()
        .border_t(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_panel())
        // Tab Header
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .h(px(28.0))
                .px(px(8.0))
                .border_b(px(1.0))
                .border_color(Colors::border_subtle())
                .bg(gpui::rgb(0x0F1318))
                .child(tab_button("Mixer", BottomTab::Mixer, active_tab, on_tab_click.clone()))
                .child(tab_button("Editor", BottomTab::Editor, active_tab, on_tab_click.clone()))
                .child(tab_button("Effect Editor", BottomTab::EffectEditor, active_tab, on_tab_click)),
        )
        // Tab Content
        .child(
            div()
                .flex_1()
                .min_h_0()
                .child(match active_tab {
                    BottomTab::Mixer => mixer_panel().into_any_element(),
                    BottomTab::Editor => editor_panel().into_any_element(),
                    BottomTab::EffectEditor => effect_editor_panel().into_any_element(),
                }),
        )
}
