use super::mixer_panel::{mixer_panel as render_mixer_panel, MixerCallbacks};
use crate::assets;
use crate::components::timeline::timeline_state::{MasterBusState, TrackState};
use crate::theme::Colors;
use gpui::{
    div, px, svg, App, AppContext, Empty, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Mixer,
    Editor,
    EffectEditor,
}

/// Persistent state for the resizable bottom panel.
/// `resize_start_*` are transient — recorded on mouse-down so the on_drag_move
/// handler can recompute height as a pure function of the current mouse Y.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BottomPanelState {
    pub height_px: f32,
    pub min_height_px: f32,
    pub max_height_px: f32,
    pub is_resizing: bool,
    pub resize_start_y: f32,
    pub resize_start_height: f32,
}

impl Default for BottomPanelState {
    fn default() -> Self {
        Self {
            height_px: 280.0,
            min_height_px: 180.0,
            max_height_px: 720.0,
            is_resizing: false,
            resize_start_y: 0.0,
            resize_start_height: 0.0,
        }
    }
}

/// Zero-sized marker used as the drag payload for the bottom panel resize handle.
#[derive(Clone, Copy, Debug, Default)]
pub struct BottomPanelResizeDrag;

impl Render for BottomPanelResizeDrag {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
}

// ─── Sub-components for Editor ───────────────────────────────────────────────

fn piano_key(is_black: bool) -> impl IntoElement {
    let bg_color = if is_black {
        Colors::surface_base()
    } else {
        Colors::text_primary()
    };
    div()
        .h(px(14.0))
        .w_full()
        .bg(bg_color)
        .border_b(px(1.0))
        .border_color(Colors::panel_border())
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
        .border_color(Colors::with_alpha(Colors::accent_primary(), 0.8)) // Approved: MIDI note state border opacity
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
                .border_color(Colors::panel_border())
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
                .child(piano_key(false)),
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
                        })),
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
                        })),
                )
                // Render mock MIDI notes
                .child(midi_note(60.0, 20.0, 14.0))
                .child(midi_note(80.0, 90.0, 42.0))
                .child(midi_note(40.0, 180.0, 70.0))
                .child(midi_note(120.0, 240.0, 28.0)),
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
        .border_color(if is_active {
            Colors::accent_primary()
        } else {
            Colors::border_subtle()
        })
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
                .child(div().w(px(8.0)).h(px(8.0)).rounded_full().bg(if is_active {
                    Colors::accent_primary()
                } else {
                    Colors::text_muted()
                })),
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
                                .rounded_full(),
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
        .border_color(Colors::panel_border())
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
    icon_path: &'static str,
    tab: BottomTab,
    active_tab: BottomTab,
    on_click: std::sync::Arc<impl Fn(&BottomTab, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let active = tab == active_tab;
    let on_click_clone = on_click.clone();
    let text_color = if active {
        Colors::text_primary()
    } else {
        Colors::text_muted()
    };

    let mut btn = div()
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .h(px(24.0))
        .px(px(10.0))
        .rounded_md()
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        // Apply text color at the button level too so the label (a plain
        // string child, not an svg) inherits the right color. Previously
        // only the icon got `.text_color`, so inactive labels rendered with
        // the default (black) text color.
        .text_color(text_color)
        .id(label)
        .on_click(move |_, window, cx| {
            on_click_clone(&tab, window, cx);
        })
        .child(
            svg()
                .path(icon_path)
                .w(px(14.0))
                .h(px(14.0))
                .text_color(text_color),
        )
        .child(label);

    if active {
        btn = btn
            .bg(Colors::surface_hover())
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
        btn = btn.hover(|style| {
            style
                .bg(Colors::surface_hover())
                .text_color(Colors::text_secondary())
        });
    }

    btn
}

pub fn bottom_panel(
    active_tab: BottomTab,
    panel_state: BottomPanelState,
    tracks: &[TrackState],
    master: &MasterBusState,
    selected_track_id: Option<&str>,
    mixer_callbacks: MixerCallbacks,
    mixer_scroll_x: f32,
    mixer_viewport_width: f32,
    on_mixer_scroll: std::sync::Arc<dyn Fn(f32, &mut gpui::Window, &mut gpui::App) + 'static>,
    editor_content: Option<gpui::AnyElement>,
    on_tab_click: impl Fn(&BottomTab, &mut Window, &mut App) + 'static,
    on_resize_start: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    on_resize_move: impl Fn(&gpui::DragMoveEvent<BottomPanelResizeDrag>, &mut Window, &mut App)
        + 'static,
    on_resize_end: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_tab_click = std::sync::Arc::new(on_tab_click);
    let mut editor_content = editor_content;
    div()
        .flex()
        .flex_col()
        .h(px(panel_state.height_px))
        .w_full()
        .border_t(px(1.0))
        .border_color(Colors::panel_border())
        .bg(Colors::bottom_panel_bg())
        .relative()
        // While dragging, listen for move events on the whole panel.
        .on_drag_move::<BottomPanelResizeDrag>(on_resize_move)
        .on_mouse_up(gpui::MouseButton::Left, on_resize_end)
        // Resize handle — 5px strip pinned to the top edge.
        .child(
            div()
                .absolute()
                .top(px(-2.0))
                .left_0()
                .right_0()
                .h(px(5.0))
                .id("bottom-panel-resize-handle")
                .cursor(gpui::CursorStyle::ResizeUpDown)
                .hover(|s| s.bg(Colors::accent_soft()))
                .on_mouse_down(gpui::MouseButton::Left, on_resize_start)
                .on_drag(BottomPanelResizeDrag, |_drag, _offset, _window, cx| {
                    cx.new(|_| BottomPanelResizeDrag)
                }),
        )
        // Tab Header
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .h(px(28.0))
                .px(px(8.0))
                .border_b(px(1.0))
                .border_color(Colors::panel_border())
                .bg(Colors::bottom_panel_header_bg())
                .child(tab_button(
                    "Mixer",
                    assets::ICON_SLIDERS_HORIZONTAL_PATH,
                    BottomTab::Mixer,
                    active_tab,
                    on_tab_click.clone(),
                ))
                .child(tab_button(
                    "Editor",
                    assets::ICON_PENCIL_PATH,
                    BottomTab::Editor,
                    active_tab,
                    on_tab_click.clone(),
                ))
                .child(tab_button(
                    "Effect Editor",
                    assets::ICON_SPARKLES_PATH,
                    BottomTab::EffectEditor,
                    active_tab,
                    on_tab_click,
                )),
        )
        // Tab Content — must declare itself as a flex container so the
        // active panel can `size_full` into the remaining space below the
        // tab header. Without `flex().flex_col()` the panel collapsed to
        // its content height and left a gap at the bottom.
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(match active_tab {
                    BottomTab::Mixer => render_mixer_panel(
                        tracks,
                        master,
                        selected_track_id,
                        mixer_callbacks,
                        mixer_scroll_x,
                        mixer_viewport_width,
                        on_mixer_scroll,
                    )
                    .into_any_element(),
                    BottomTab::Editor => editor_content
                        .take()
                        .unwrap_or_else(|| editor_panel().into_any_element()),
                    BottomTab::EffectEditor => effect_editor_panel().into_any_element(),
                }),
        )
}
