use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, size, svg, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Point, Render, StatefulInteractiveElement, Styled,
    Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
};

use crate::assets;
use crate::components::text_input::{
    text_field_with_callbacks, TextInputCallbacks, TextInputState,
};
use crate::components::title_bar::external_window_titlebar;
use crate::components::timeline::timeline_state::TrackType;
use crate::theme::{self, Colors};

type VoidCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;
type KindCb = Arc<dyn Fn(&AddTrackKind, &mut Window, &mut App) + 'static>;
type U32Cb = Arc<dyn Fn(&u32, &mut Window, &mut App) + 'static>;
type BoolCb = Arc<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTrackKind {
    Audio,
    Instrument,
    Midi,
    Plugin,
    Bus,
    Return,
    Group,
    Master,
}

impl AddTrackKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Audio => "Audio Track",
            Self::Instrument => "Instrument Track",
            Self::Midi => "MIDI Track",
            Self::Plugin => "Plugin Track",
            Self::Bus => "Bus Track",
            Self::Return => "Return Track",
            Self::Group => "Group Track",
            Self::Master => "Master Track",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Audio => "WAV - MP3 - AIFF",
            Self::Instrument => "VST3 - CLAP - Piano Roll",
            Self::Midi => "Piano Roll - CC",
            Self::Plugin => "VST3 - CLAP",
            Self::Bus => "Sends - Groups",
            Self::Return => "FX Returns - Aux",
            Self::Group => "Sub-mix - Stem",
            Self::Master => "Main Output",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Audio => "Record and arrange audio clips",
            Self::Instrument => "MIDI clips routed to an instrument plugin",
            Self::Midi => "Sequence instruments with notes",
            Self::Plugin => "Host virtual instruments & effects",
            Self::Bus => "Route and blend multiple channels",
            Self::Return => "Receive sends from other tracks",
            Self::Group => "Group and process multiple tracks",
            Self::Master => "Final output and master bus",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Audio => assets::ICON_MIC_PATH,
            Self::Instrument => assets::ICON_CPU_PATH,
            Self::Midi => assets::ICON_MUSIC_PATH,
            Self::Plugin => assets::ICON_CPU_PATH,
            Self::Bus => assets::ICON_GIT_MERGE_PATH,
            Self::Return => assets::ICON_CORNER_DOWN_LEFT_PATH,
            Self::Group => assets::ICON_GIT_FORK_PATH,
            Self::Master => assets::ICON_VOLUME_2_PATH,
        }
    }

    pub fn native_track_type(self) -> Option<TrackType> {
        match self {
            Self::Audio => Some(TrackType::Audio),
            Self::Instrument => Some(TrackType::Instrument),
            Self::Midi => Some(TrackType::Midi),
            Self::Plugin | Self::Bus | Self::Return | Self::Group | Self::Master => None,
        }
    }

    pub fn default_input(self) -> &'static str {
        match self {
            Self::Midi | Self::Instrument => "All MIDI Inputs",
            Self::Audio => "System Input (Stereo)",
            _ => "None",
        }
    }

    pub fn all() -> [Self; 8] {
        [
            Self::Audio,
            Self::Instrument,
            Self::Midi,
            Self::Plugin,
            Self::Bus,
            Self::Return,
            Self::Group,
            Self::Master,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct AddTrackDialogState {
    pub is_open: bool,
    pub selected_kind: AddTrackKind,
    pub track_name: String,
    pub count: u32,
    pub color_index: usize,
    pub channel_count: u32,
    pub volume: f32,
    pub pan: f32,
    pub arm_track: bool,
    pub monitor_mode: &'static str,
    pub next_number: usize,
    pub has_master_track: bool,
}

impl AddTrackDialogState {
    pub fn closed() -> Self {
        Self {
            is_open: false,
            selected_kind: AddTrackKind::Audio,
            track_name: String::new(),
            count: 1,
            color_index: 0,
            channel_count: 2,
            volume: 0.8,
            pan: 0.0,
            arm_track: false,
            monitor_mode: "off",
            next_number: 1,
            has_master_track: false,
        }
    }

    pub fn open_for(track_count: usize, has_master_track: bool) -> Self {
        let next_number = track_count + 1;
        Self {
            is_open: true,
            selected_kind: AddTrackKind::Audio,
            track_name: format!("Audio Track {}", next_number),
            count: 1,
            color_index: track_count % Colors::TRACK_COLORS.len(),
            channel_count: 2,
            volume: 0.8,
            pan: 0.0,
            arm_track: false,
            monitor_mode: "off",
            next_number,
            has_master_track,
        }
    }

    pub fn selected_color(&self) -> gpui::Rgba {
        track_color(self.color_index)
    }

    pub fn is_valid(&self) -> bool {
        self.selected_kind.native_track_type().is_some()
    }
}

#[derive(Clone)]
pub struct AddTrackDialogCallbacks {
    pub on_close: VoidCb,
    pub on_confirm: VoidCb,
    pub on_select_kind: KindCb,
    pub on_count_delta: Arc<dyn Fn(&i32, &mut Window, &mut App) + 'static>,
    pub on_channel_count: U32Cb,
    pub on_color_index: U32Cb,
    pub on_arm: BoolCb,
    pub on_monitor: Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>,
}

pub fn track_color(index: usize) -> gpui::Rgba {
    Colors::track_color_for_index(index)
}

fn option_supported(kind: AddTrackKind, state: &AddTrackDialogState) -> bool {
    kind.native_track_type().is_some() && !(kind == AddTrackKind::Master && state.has_master_track)
}

fn unsupported_badge(kind: AddTrackKind, state: &AddTrackDialogState) -> &'static str {
    if kind == AddTrackKind::Master && state.has_master_track {
        "Exists"
    } else {
        "Soon"
    }
}

fn icon(path: &'static str, size: f32, color: gpui::Rgba) -> impl IntoElement {
    svg().path(path).w(px(size)).h(px(size)).text_color(color)
}

fn option_card(
    state: &AddTrackDialogState,
    kind: AddTrackKind,
    callbacks: &AddTrackDialogCallbacks,
    index: usize,
) -> impl IntoElement {
    let active = state.selected_kind == kind;
    let supported = option_supported(kind, state);
    let cb = callbacks.on_select_kind.clone();
    let border = if active {
        Colors::with_alpha(Colors::accent_primary(), 0.48)
    } else {
        Colors::divider()
    };
    let bg = if active {
        Colors::with_alpha(Colors::accent_primary(), 0.07)
    } else {
        Colors::surface_input()
    };
    let icon_bg = if active {
        Colors::accent_soft()
    } else {
        Colors::surface_canvas()
    };
    let icon_border = if active {
        Colors::with_alpha(Colors::accent_primary(), 0.3)
    } else {
        Colors::slot_border()
    };
    let icon_color = if active {
        Colors::accent_primary()
    } else {
        Colors::text_muted()
    };

    let mut card = div()
        .relative()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .h(px(94.0))
        .flex_1()
        .rounded_lg()
        .border(px(1.0))
        .border_color(border)
        .bg(bg)
        .p(px(10.0))
        .id(("add-track-kind", index))
        .opacity(if supported { 1.0 } else { 0.4 })
        .child(
            div()
                .absolute()
                .right(px(8.0))
                .top(px(8.0))
                .children(if supported {
                    Some(
                        div()
                            .rounded_sm()
                            .px(px(4.0))
                            .py(px(1.0))
                            .bg(Colors::accent_soft())
                            .text_size(px(8.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(Colors::accent_primary())
                            .child("Ready"),
                    )
                } else {
                    Some(
                        div()
                            .rounded_sm()
                            .px(px(4.0))
                            .py(px(1.0))
                            .bg(Colors::with_alpha(Colors::text_primary(), 0.05))
                            .text_size(px(8.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(Colors::text_faint())
                            .child(unsupported_badge(kind, state)),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(28.0))
                .h(px(28.0))
                .rounded_lg()
                .border(px(1.0))
                .border_color(icon_border)
                .bg(icon_bg)
                .child(icon(kind.icon(), 13.0, icon_color)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(if active {
                            Colors::text_primary()
                        } else {
                            Colors::text_muted()
                        })
                        .child(kind.label()),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(Colors::text_faint())
                        .child(kind.detail()),
                ),
        );

    if supported {
        card = card
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| s.bg(Colors::surface_hover()).border_color(Colors::border_default()))
            .on_click(move |_, window, cx| {
                cb(&kind, window, cx);
            });
    }

    card
}

fn option_group(label: &'static str, child: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .gap(px(6.0))
        .child(
            div()
                .h(px(12.0))
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_faint())
                .child(label),
        )
        .child(child)
}

fn pill(
    label: &'static str,
    active: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(27.0))
        .flex_1()
        .min_w(px(52.0))
        .px(px(10.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(if active {
            Colors::with_alpha(Colors::accent_primary(), 0.48)
        } else {
            Colors::slot_border()
        })
        .bg(if active {
            Colors::with_alpha(Colors::accent_primary(), 0.14)
        } else {
            Colors::surface_input()
        })
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(if active {
            Colors::text_primary()
        } else {
            Colors::text_muted()
        })
        .id(label)
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_hover()))
        .on_click(on_click)
        .child(label)
}

fn spinner(state: &AddTrackDialogState, callbacks: &AddTrackDialogCallbacks) -> impl IntoElement {
    let down = callbacks.on_count_delta.clone();
    let up = callbacks.on_count_delta.clone();
    div()
        .flex()
        .flex_row()
        .gap(px(5.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(27.0))
                .h(px(27.0))
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::slot_border())
                .bg(Colors::surface_input())
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_muted())
                .id("add-track-count-minus")
                .cursor(gpui::CursorStyle::PointingHand)
                .on_click(move |_, window, cx| down(&-1, window, cx))
                .child("-"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(27.0))
                .flex_1()
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::slot_border())
                .bg(Colors::surface_input())
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_primary())
                .child(state.count.to_string()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(27.0))
                .h(px(27.0))
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::slot_border())
                .bg(Colors::surface_input())
                .text_size(px(12.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_muted())
                .id("add-track-count-plus")
                .cursor(gpui::CursorStyle::PointingHand)
                .on_click(move |_, window, cx| up(&1, window, cx))
                .child("+"),
        )
}

fn routing_row(label: &'static str, value: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .w(px(56.0))
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_faint())
                .child(label),
        )
        .child(value)
}

fn select_box(text: String) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .flex_1()
        .h(px(27.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(Colors::slot_border())
        .bg(Colors::surface_input())
        .px(px(8.0))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(Colors::text_muted())
                .child(text),
        )
        .child(icon(
            assets::ICON_CHEVRON_DOWN_PATH,
            10.0,
            Colors::text_faint(),
        ))
}

fn summary_text(state: &AddTrackDialogState) -> String {
    let n = if state.count == 1 {
        String::new()
    } else {
        format!("{} ", state.count)
    };
    let plural = if state.count > 1 { "s" } else { "" };
    let out = if state.selected_kind == AddTrackKind::Midi {
        "none"
    } else {
        "Master"
    };
    match state.selected_kind {
        AddTrackKind::Audio => {
            let ch = if state.channel_count == 1 {
                "mono"
            } else {
                "stereo"
            };
            let mon = if state.monitor_mode != "off" {
                format!(" - Mon {}", state.monitor_mode)
            } else {
                String::new()
            };
            format!("Add {n}{ch} audio track{plural} - stereo in -> {out}{mon}")
        }
        AddTrackKind::Midi => format!("Add {n}MIDI track{plural} - All MIDI Inputs, all channels"),
        AddTrackKind::Instrument => {
            format!(
                "Add {n}instrument track{plural} - All MIDI Inputs -> instrument plugin -> {out}"
            )
        }
        AddTrackKind::Plugin => "Plugin tracks are not wired in Native yet".to_string(),
        AddTrackKind::Bus => "Bus tracks are not wired in Native yet".to_string(),
        AddTrackKind::Return => "Return tracks are not wired in Native yet".to_string(),
        AddTrackKind::Group => "Group tracks are not wired in Native yet".to_string(),
        AddTrackKind::Master => "Native uses a managed master bus".to_string(),
    }
}

pub fn add_track_dialog(
    state: &AddTrackDialogState,
    track_name_input: &TextInputState,
    track_name_focused: bool,
    track_name_callbacks: TextInputCallbacks,
    callbacks: AddTrackDialogCallbacks,
) -> impl IntoElement {
    let close_backdrop = callbacks.on_close.clone();
    let close_button = callbacks.on_close.clone();
    let confirm = callbacks.on_confirm.clone();
    let selected_color = state.selected_color();

    let mut option_rows = Vec::new();
    let all = AddTrackKind::all();
    for row in 0..2 {
        let mut row_el = div().flex().flex_row().gap(px(6.0));
        for col in 0..4 {
            let index = row * 4 + col;
            row_el = row_el.child(option_card(state, all[index], &callbacks, index));
        }
        option_rows.push(row_el.into_any_element());
    }

    let channel_controls = if matches!(
        state.selected_kind,
        AddTrackKind::Audio
            | AddTrackKind::Plugin
            | AddTrackKind::Bus
            | AddTrackKind::Return
            | AddTrackKind::Group
    ) {
        option_group(
            "Channels",
            div()
                .flex()
                .flex_row()
                .gap(px(6.0))
                .child({
                    let cb = callbacks.on_channel_count.clone();
                    pill("Mono", state.channel_count == 1, move |_, window, cx| {
                        cb(&1, window, cx);
                    })
                })
                .child({
                    let cb = callbacks.on_channel_count.clone();
                    pill("Stereo", state.channel_count == 2, move |_, window, cx| {
                        cb(&2, window, cx);
                    })
                }),
        )
        .into_any_element()
    } else {
        div().into_any_element()
    };

    let routing = if state.selected_kind == AddTrackKind::Audio {
        let off = callbacks.on_monitor.clone();
        let auto = callbacks.on_monitor.clone();
        let input = callbacks.on_monitor.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .border_t(px(1.0))
            .border_color(Colors::divider())
            .px(px(12.0))
            .py(px(10.0))
            .child(routing_row(
                "Monitor",
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .gap(px(5.0))
                    .child(pill(
                        "Off",
                        state.monitor_mode == "off",
                        move |_, window, cx| off(&"off".to_string(), window, cx),
                    ))
                    .child(pill(
                        "Auto",
                        state.monitor_mode == "auto",
                        move |_, window, cx| auto(&"auto".to_string(), window, cx),
                    ))
                    .child(pill(
                        "In",
                        state.monitor_mode == "in",
                        move |_, window, cx| input(&"in".to_string(), window, cx),
                    )),
            ))
            .child(routing_row("Input", select_box("System Input".to_string())))
            .child(routing_row("Output", select_box("Master".to_string())))
            .child({
                let cb = callbacks.on_arm.clone();
                let next = !state.arm_track;
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .mt(px(2.0))
                    .id("add-track-arm")
                    .cursor(gpui::CursorStyle::PointingHand)
                    .on_click(move |_, window, cx| cb(&next, window, cx))
                    .child(
                        div()
                            .w(px(12.0))
                            .h(px(12.0))
                            .rounded_sm()
                            .border(px(1.0))
                            .border_color(if state.arm_track {
                                Colors::status_error()
                            } else {
                                Colors::border_default()
                            })
                            .bg(if state.arm_track {
                                Colors::status_error()
                            } else {
                                Colors::surface_input()
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Colors::text_muted())
                            .child("Arm for recording"),
                    )
            })
            .into_any_element()
    } else if state.selected_kind == AddTrackKind::Midi
        || state.selected_kind == AddTrackKind::Instrument
    {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .border_t(px(1.0))
            .border_color(Colors::divider())
            .px(px(12.0))
            .py(px(10.0))
            .child(routing_row(
                if state.selected_kind == AddTrackKind::Instrument {
                    "MIDI In"
                } else {
                    "Input"
                },
                select_box(state.selected_kind.default_input().to_string()),
            ))
            .when(state.selected_kind == AddTrackKind::Midi, |this| {
                this.child(routing_row(
                    "Channel",
                    select_box("All Channels".to_string()),
                ))
            })
            .when(state.selected_kind == AddTrackKind::Instrument, |this| {
                this.child(routing_row("Output", select_box("Master".to_string())))
            })
            .into_any_element()
    } else {
        div()
            .border_t(px(1.0))
            .border_color(Colors::divider())
            .px(px(12.0))
            .py(px(10.0))
            .text_size(px(10.0))
            .text_color(Colors::text_faint())
            .child(state.selected_kind.description())
            .into_any_element()
    };

    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .flex()
        .items_start()
        .justify_center()
        .pt(px(56.0))
        .px(px(18.0))
        .pb(px(32.0))
        .id("add-track-modal-overlay")
        .bg(gpui::transparent_black())
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            close_backdrop(&(), window, cx);
        })
        .child(
            div()
                .flex()
                .flex_col()
                .w(px(620.0))
                .max_w(px(620.0))
                .max_h(px(760.0))
                .overflow_hidden()
                .rounded_xl()
                .border(px(1.0))
                .border_color(Colors::border_default())
                .bg(Colors::surface_window())
                .shadow_xl()
                .on_mouse_down(gpui::MouseButton::Left, |_, _window, cx| {
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .h(px(40.0))
                        .px(px(16.0))
                        .border_b(px(1.0))
                        .border_color(Colors::divider())
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(icon(assets::ICON_PLUS_PATH, 13.0, Colors::accent_primary()))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Colors::text_primary())
                                        .child("New Track"),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(24.0))
                                .h(px(24.0))
                                .rounded_md()
                                .id("add-track-close")
                                .cursor(gpui::CursorStyle::PointingHand)
                                .hover(|s| s.bg(Colors::surface_control_hover()))
                                .on_click(move |_, window, cx| close_button(&(), window, cx))
                                .child(icon(assets::ICON_X_PATH, 13.0, Colors::text_faint())),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .p(px(12.0))
                        .children(option_rows),
                )
                .child(
                    div()
                        .border_t(px(1.0))
                        .border_color(Colors::divider())
                        .px(px(12.0))
                        .py(px(8.0))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.0))
                                .h(px(34.0))
                                .child(icon(state.selected_kind.icon(), 12.0, Colors::text_faint()))
                                .child(div().flex_1().min_w_0().child(text_field_with_callbacks(
                                    track_name_input,
                                    track_name_focused,
                                    track_name_callbacks,
                                ))),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .border_t(px(1.0))
                        .border_color(Colors::divider())
                        .px(px(14.0))
                        .py(px(10.0))
                        .child(option_group("Amount", spinner(state, &callbacks)))
                        .child(channel_controls),
                )
                .child(routing)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .border_t(px(1.0))
                        .border_color(Colors::divider())
                        .px(px(12.0))
                        .py(px(10.0))
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_faint())
                                .child(summary_text(state)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(12.0))
                                .child({
                                    let mut swatches = div().flex().flex_row().gap(px(5.0));
                                    for i in 0..Colors::TRACK_COLORS.len() {
                                        let cb = callbacks.on_color_index.clone();
                                        let active = i == state.color_index;
                                        let color = track_color(i);
                                        swatches = swatches.child(
                                            div()
                                                .relative()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .w(px(20.0))
                                                .h(px(20.0))
                                                .rounded_full()
                                                .border(px(2.0))
                                                .border_color(color)
                                                .bg(if active { color } else { gpui::transparent_black().into() })
                                                .opacity(if active { 1.0 } else { 0.5 })
                                                .id(("add-track-color", i))
                                                .cursor(gpui::CursorStyle::PointingHand)
                                                .on_click(move |_, window, cx| {
                                                    cb(&(i as u32), window, cx);
                                                })
                                                .children(if active {
                                                    Some(icon(
                                                        assets::ICON_CIRCLE_DOT_PATH,
                                                        12.0,
                                                        Colors::with_alpha(Colors::surface_canvas(), 0.6),
                                                    ))
                                                } else {
                                                    None
                                                }),
                                        );
                                    }
                                    swatches
                                })
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .h(px(28.0))
                                                .px(px(12.0))
                                                .rounded_md()
                                                .border(px(1.0))
                                                .border_color(Colors::slot_border())
                                                .text_size(px(11.0))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(Colors::text_faint())
                                                .id("add-track-cancel")
                                                .cursor(gpui::CursorStyle::PointingHand)
                                                .hover(|s| s.bg(Colors::surface_hover()))
                                                .on_click({
                                                    let cb = callbacks.on_close.clone();
                                                    move |_, window, cx| cb(&(), window, cx)
                                                })
                                                .child("Cancel"),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(6.0))
                                                .h(px(28.0))
                                                .px(px(12.0))
                                                .rounded_md()
                                                .bg(selected_color)
                                                .opacity(if state.is_valid() { 1.0 } else { 0.45 })
                                                .text_size(px(11.0))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(Colors::text_inverse())
                                                .id("add-track-confirm")
                                                .when(state.is_valid(), |this| {
                                                    this.cursor(gpui::CursorStyle::PointingHand)
                                                        .on_click(move |_, window, cx| {
                                                            confirm(&(), window, cx);
                                                        })
                                                })
                                                .child(icon(
                                                    assets::ICON_PLUS_PATH,
                                                    12.0,
                                                    Colors::text_inverse(),
                                                ))
                                                .child(if state.count == 1 {
                                                    "Add Track".to_string()
                                                } else {
                                                    format!("Add {} Tracks", state.count)
                                                }),
                                        ),
                                ),
                        ),
                ),
        )
}

/// Body-only layout for embedding in an external window.
///
/// This intentionally omits the modal backdrop/occlusion layer and the inner
/// "New Track" title row because the external window provides its own chrome.
pub fn add_track_dialog_body(
    state: &AddTrackDialogState,
    track_name_input: &TextInputState,
    track_name_focused: bool,
    track_name_callbacks: TextInputCallbacks,
    callbacks: AddTrackDialogCallbacks,
) -> gpui::Div {
    let confirm = callbacks.on_confirm.clone();
    let selected_color = state.selected_color();

    let mut option_rows = Vec::new();
    let all = AddTrackKind::all();
    for row in 0..2 {
        let mut row_el = div().flex().flex_row().gap(px(6.0));
        for col in 0..4 {
            let index = row * 4 + col;
            row_el = row_el.child(option_card(state, all[index], &callbacks, index));
        }
        option_rows.push(row_el.into_any_element());
    }

    let channel_controls = if matches!(
        state.selected_kind,
        AddTrackKind::Audio
            | AddTrackKind::Plugin
            | AddTrackKind::Bus
            | AddTrackKind::Return
            | AddTrackKind::Group
    ) {
        option_group(
            "Channels",
            div()
                .flex()
                .flex_row()
                .gap(px(6.0))
                .child({
                    let cb = callbacks.on_channel_count.clone();
                    pill("Mono", state.channel_count == 1, move |_, window, cx| {
                        cb(&1, window, cx);
                    })
                })
                .child({
                    let cb = callbacks.on_channel_count.clone();
                    pill("Stereo", state.channel_count == 2, move |_, window, cx| {
                        cb(&2, window, cx);
                    })
                }),
        )
        .into_any_element()
    } else {
        div().into_any_element()
    };

    let routing = if state.selected_kind == AddTrackKind::Audio {
        let off = callbacks.on_monitor.clone();
        let auto = callbacks.on_monitor.clone();
        let input = callbacks.on_monitor.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .border_t(px(1.0))
            .border_color(Colors::divider())
            .px(px(12.0))
            .py(px(10.0))
            .child(routing_row(
                "Monitor",
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .gap(px(5.0))
                    .child(pill(
                        "Off",
                        state.monitor_mode == "off",
                        move |_, window, cx| off(&"off".to_string(), window, cx),
                    ))
                    .child(pill(
                        "Auto",
                        state.monitor_mode == "auto",
                        move |_, window, cx| auto(&"auto".to_string(), window, cx),
                    ))
                    .child(pill(
                        "In",
                        state.monitor_mode == "in",
                        move |_, window, cx| input(&"in".to_string(), window, cx),
                    )),
            ))
            .child(routing_row("Input", select_box("System Input".to_string())))
            .child(routing_row("Output", select_box("Master".to_string())))
            .child({
                let cb = callbacks.on_arm.clone();
                let next = !state.arm_track;
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .mt(px(2.0))
                    .id("add-track-arm")
                    .cursor(gpui::CursorStyle::PointingHand)
                    .on_click(move |_, window, cx| cb(&next, window, cx))
                    .child(
                        div()
                            .w(px(12.0))
                            .h(px(12.0))
                            .rounded_sm()
                            .border(px(1.0))
                            .border_color(if state.arm_track {
                                Colors::status_error()
                            } else {
                                Colors::divider()
                            })
                            .bg(if state.arm_track {
                                Colors::status_error()
                            } else {
                                Colors::surface_input()
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Colors::text_muted())
                            .child("Arm for recording"),
                    )
            })
            .into_any_element()
    } else if state.selected_kind == AddTrackKind::Midi || state.selected_kind == AddTrackKind::Instrument
    {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .border_t(px(1.0))
            .border_color(Colors::divider())
            .px(px(12.0))
            .py(px(10.0))
            .child(routing_row(
                if state.selected_kind == AddTrackKind::Instrument {
                    "MIDI In"
                } else {
                    "Input"
                },
                select_box(AddTrackKind::Instrument.default_input().to_string()),
            ))
            .child(routing_row("Output", select_box("Master".to_string())))
            .into_any_element()
    } else {
        div().into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .size_full()
        .min_h_0()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .p(px(12.0))
                .children(option_rows),
        )
        .child(
            div()
                .border_t(px(1.0))
                .border_color(Colors::divider())
                .px(px(12.0))
                .py(px(8.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .h(px(34.0))
                        .child(icon(state.selected_kind.icon(), 12.0, Colors::text_faint()))
                        .child(div().flex_1().min_w_0().child(text_field_with_callbacks(
                            track_name_input,
                            track_name_focused,
                            track_name_callbacks,
                        ))),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(8.0))
                .border_t(px(1.0))
                .border_color(Colors::divider())
                .px(px(14.0))
                .py(px(10.0))
                .child(option_group("Amount", spinner(state, &callbacks)))
                .child(channel_controls),
        )
        .child(routing)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .border_t(px(1.0))
                .border_color(Colors::divider())
                .px(px(12.0))
                .py(px(10.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(Colors::text_faint())
                        .child(summary_text(state)),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.0))
                        .child({
                            let mut swatches = div().flex().flex_row().gap(px(5.0));
                            for i in 0..Colors::TRACK_COLORS.len() {
                                let cb = callbacks.on_color_index.clone();
                                let active = i == state.color_index;
                                let color = track_color(i);
                                swatches = swatches.child(
                                    div()
                                        .relative()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(20.0))
                                        .h(px(20.0))
                                        .rounded_full()
                                        .border(px(2.0))
                                        .border_color(color)
                                        .bg(if active { color } else { gpui::transparent_black().into() })
                                        .opacity(if active { 1.0 } else { 0.5 })
                                        .id(("add-track-color", i))
                                        .cursor(gpui::CursorStyle::PointingHand)
                                        .on_click(move |_, window, cx| {
                                            cb(&(i as u32), window, cx);
                                        })
                                        .children(if active {
                                            Some(icon(
                                                assets::ICON_CIRCLE_DOT_PATH,
                                                12.0,
                                                Colors::with_alpha(Colors::surface_canvas(), 0.6),
                                            ))
                                        } else {
                                            None
                                        }),
                                );
                            }
                            swatches
                        })
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .h(px(28.0))
                                        .px(px(12.0))
                                        .rounded_md()
                                        .border(px(1.0))
                                        .border_color(Colors::slot_border())
                                        .text_size(px(11.0))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(Colors::text_faint())
                                        .id("add-track-cancel")
                                        .cursor(gpui::CursorStyle::PointingHand)
                                        .hover(|s| s.bg(Colors::surface_hover()))
                                        .on_click({
                                            let cb = callbacks.on_close.clone();
                                            move |_, window, cx| cb(&(), window, cx)
                                        })
                                        .child("Cancel"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        .h(px(28.0))
                                        .px(px(12.0))
                                        .rounded_md()
                                        .bg(selected_color)
                                        .opacity(if state.is_valid() { 1.0 } else { 0.45 })
                                        .text_size(px(11.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Colors::text_inverse())
                                        .id("add-track-confirm")
                                        .when(state.is_valid(), |this| {
                                            this.cursor(gpui::CursorStyle::PointingHand)
                                                .on_click(move |_, window, cx| {
                                                    confirm(&(), window, cx);
                                                })
                                        })
                                        .child(icon(
                                            assets::ICON_PLUS_PATH,
                                            12.0,
                                            Colors::text_inverse(),
                                        ))
                                        .child(if state.count == 1 {
                                            "Add Track".to_string()
                                        } else {
                                            format!("Add {} Tracks", state.count)
                                        }),
                                ),
                        ),
                ),
        )
}

pub const ADD_TRACK_WINDOW_WIDTH: f32 = 960.0;
pub const ADD_TRACK_WINDOW_HEIGHT: f32 = 660.0;
pub const ADD_TRACK_WINDOW_MIN_WIDTH: f32 = 860.0;
pub const ADD_TRACK_WINDOW_MIN_HEIGHT: f32 = 560.0;

pub struct AddTrackWindow {
    pub state: AddTrackDialogState,
    track_name_input: TextInputState,
    focus_handle: FocusHandle,
    /// Called when the user confirms (creates tracks).
    on_confirm_request: Arc<dyn Fn(AddTrackDialogState, String, &mut App) + 'static>,
}

impl AddTrackWindow {
    pub fn new(
        initial_state: AddTrackDialogState,
        on_confirm_request: Arc<dyn Fn(AddTrackDialogState, String, &mut App) + 'static>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut track_name_input = TextInputState::new("add-track-window-name", cx.focus_handle());
        track_name_input.set_value(initial_state.track_name.clone());
        track_name_input.select_all();
        Self {
            state: initial_state,
            track_name_input,
            focus_handle: cx.focus_handle(),
            on_confirm_request,
        }
    }

    pub fn set_context(&mut self, kind: AddTrackKind, track_count: usize, has_master: bool) {
        let mut dialog = AddTrackDialogState::open_for(track_count, has_master);
        dialog.selected_kind = kind;
        dialog.track_name = format!("{} {}", kind.label(), dialog.next_number);
        self.track_name_input.set_value(dialog.track_name.clone());
        self.track_name_input.select_all();
        self.state = dialog;
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.is_valid() {
            return;
        }
        self.state.track_name = self.track_name_input.value.clone();
        let req = self.state.clone();
        let name = self.track_name_input.value.clone();
        let cb = self.on_confirm_request.clone();
        cb(req, name, cx);
        window.remove_window();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.track_name_input.is_focused(window) {
            let action = self
                .track_name_input
                .handle_key_with_clipboard(event, Some(cx));
            self.state.track_name = self.track_name_input.value.clone();
            match action {
                crate::components::text_input::TextInputAction::Submit => {
                    self.confirm(window, cx);
                }
                crate::components::text_input::TextInputAction::Cancel => window.remove_window(),
                crate::components::text_input::TextInputAction::Consumed
                | crate::components::text_input::TextInputAction::Pass => cx.notify(),
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => window.remove_window(),
            "enter" | "numpad_enter" => self.confirm(window, cx),
            _ => {}
        }
    }
}

impl Render for AddTrackWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = cx.entity().clone();
        let search_focused = self.track_name_input.is_focused(window);

        let callbacks = AddTrackDialogCallbacks {
            on_close: Arc::new(|_: &(), window: &mut Window, _cx: &mut App| window.remove_window()),
            on_confirm: Arc::new({
                let target = target.clone();
                move |_: &(), window, cx| {
                    let _ = target.update(cx, |this, cx| this.confirm(window, cx));
                }
            }),
            on_select_kind: Arc::new({
                let target = target.clone();
                move |kind: &AddTrackKind, _w, cx| {
                    let kind = *kind;
                    let _ = target.update(cx, |this, cx| {
                        this.state.selected_kind = kind;
                        this.state.track_name =
                            format!("{} {}", kind.label(), this.state.next_number);
                        this.track_name_input.set_value(this.state.track_name.clone());
                        this.track_name_input.select_all();
                        cx.notify();
                    });
                }
            }),
            on_count_delta: Arc::new({
                let target = target.clone();
                move |delta: &i32, _w, cx| {
                    let delta = *delta;
                    let _ = target.update(cx, |this, cx| {
                        let current = this.state.count as i32;
                        this.state.count = (current + delta).clamp(1, 32) as u32;
                        cx.notify();
                    });
                }
            }),
            on_channel_count: Arc::new({
                let target = target.clone();
                move |channels: &u32, _w, cx| {
                    let channels = *channels;
                    let _ = target.update(cx, |this, cx| {
                        this.state.channel_count = channels.clamp(1, 2);
                        cx.notify();
                    });
                }
            }),
            on_color_index: Arc::new({
                let target = target.clone();
                move |index: &u32, _w, cx| {
                    let index = *index as usize;
                    let _ = target.update(cx, |this, cx| {
                        this.state.color_index = index;
                        cx.notify();
                    });
                }
            }),
            on_arm: Arc::new({
                let target = target.clone();
                move |armed: &bool, _w, cx| {
                    let armed = *armed;
                    let _ = target.update(cx, |this, cx| {
                        this.state.arm_track = armed;
                        cx.notify();
                    });
                }
            }),
            on_monitor: Arc::new({
                let target = target.clone();
                move |mode: &String, _w, cx| {
                    let mode = match mode.as_str() {
                        "auto" => "auto",
                        "in" => "in",
                        _ => "off",
                    };
                    let _ = target.update(cx, |this, cx| {
                        this.state.monitor_mode = mode;
                        cx.notify();
                    });
                }
            }),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FONT_FAMILY)
            .bg(Colors::surface_base())
            .overflow_hidden()
            .capture_key_down({
                let target = target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
                }
            })
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar(
                "New Track",
                "add-track-window-close",
                move |window, _cx| window.remove_window(),
            ))
            .child(add_track_dialog_body(
                &self.state,
                &self.track_name_input,
                search_focused,
                TextInputCallbacks::default(),
                callbacks,
            ))
    }
}

pub fn open_add_track_window(
    owner_bounds: Bounds<gpui::Pixels>,
    kind: AddTrackKind,
    track_count: usize,
    has_master_track: bool,
    on_confirm_request: Arc<dyn Fn(AddTrackDialogState, String, &mut App) + 'static>,
    cx: &mut App,
) -> Result<WindowHandle<AddTrackWindow>, String> {
    let parent_x: f32 = owner_bounds.origin.x.into();
    let parent_y: f32 = owner_bounds.origin.y.into();
    let parent_w: f32 = owner_bounds.size.width.into();
    let parent_h: f32 = owner_bounds.size.height.into();
    let origin = Point {
        x: px(parent_x + ((parent_w - ADD_TRACK_WINDOW_WIDTH) / 2.0).max(24.0)),
        y: px(parent_y + ((parent_h - ADD_TRACK_WINDOW_HEIGHT) / 2.0).max(24.0)),
    };

    let mut state = AddTrackDialogState::open_for(track_count, has_master_track);
    state.selected_kind = kind;
    state.track_name = format!("{} {}", kind.label(), state.next_number);

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(ADD_TRACK_WINDOW_WIDTH), px(ADD_TRACK_WINDOW_HEIGHT)),
    }));
    options.kind = WindowKind::Floating;
    options.is_resizable = true;
    options.is_minimizable = false;
    options.window_background = WindowBackgroundAppearance::Transparent;
    options.window_min_size = Some(size(px(ADD_TRACK_WINDOW_MIN_WIDTH), px(ADD_TRACK_WINDOW_MIN_HEIGHT)));

    cx.open_window(options, |_window, cx| {
        cx.new(|cx| AddTrackWindow::new(state, on_confirm_request, cx))
    })
    .map_err(|e| e.to_string())
}
