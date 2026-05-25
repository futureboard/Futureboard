use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, rgba, svg, App, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::components::text_input::{text_field, TextInputState};
use crate::theme::Colors;

// ── Template presets ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTemplate {
    Empty,
    Recording,
    BeatMaking,
    Mixing,
    Scoring,
}

impl ProjectTemplate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Recording => "Recording",
            Self::BeatMaking => "Beat Making",
            Self::Mixing => "Mixing",
            Self::Scoring => "Scoring",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Empty => "Blank canvas",
            Self::Recording => "4 audio tracks",
            Self::BeatMaking => "4 MIDI tracks",
            Self::Mixing => "8 audio tracks",
            Self::Scoring => "8 MIDI tracks",
        }
    }

    pub fn icon_path(self) -> &'static str {
        match self {
            Self::Empty => assets::ICON_FILE_PATH,
            Self::Recording => assets::ICON_MIC_PATH,
            Self::BeatMaking => assets::ICON_MUSIC_PATH,
            Self::Mixing => assets::ICON_SLIDERS_HORIZONTAL_PATH,
            Self::Scoring => assets::ICON_PENCIL_PATH,
        }
    }

    pub fn audio_tracks(self) -> u32 {
        match self {
            Self::Empty => 0,
            Self::Recording => 4,
            Self::BeatMaking => 0,
            Self::Mixing => 8,
            Self::Scoring => 0,
        }
    }

    pub fn midi_tracks(self) -> u32 {
        match self {
            Self::Empty | Self::Recording | Self::Mixing => 0,
            Self::BeatMaking => 4,
            Self::Scoring => 8,
        }
    }

    pub fn default_bpm(self) -> f64 {
        if matches!(self, Self::BeatMaking) { 140.0 } else { 120.0 }
    }

    pub fn all() -> [Self; 5] {
        [Self::Empty, Self::Recording, Self::BeatMaking, Self::Mixing, Self::Scoring]
    }
}

// ── Result ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProjectWizardResult {
    pub name: String,
    pub location: PathBuf,
    pub template: ProjectTemplate,
    pub bpm: f64,
    pub time_sig_num: u32,
    pub time_sig_den: u32,
    pub sample_rate: u32,
}

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProjectWizardState {
    pub is_open: bool,
    pub name: String,
    pub location: PathBuf,
    pub template: ProjectTemplate,
    /// String form for the BPM text input.
    pub bpm_text: String,
    pub time_sig_num: u32,
    pub time_sig_den: u32,
    pub sample_rate: u32,
}

impl ProjectWizardState {
    pub fn closed() -> Self {
        Self {
            is_open: false,
            name: "Untitled Project".to_string(),
            location: crate::project::default_projects_dir(),
            template: ProjectTemplate::Empty,
            bpm_text: "120".to_string(),
            time_sig_num: 4,
            time_sig_den: 4,
            sample_rate: 48000,
        }
    }

    pub fn open() -> Self {
        Self { is_open: true, ..Self::closed() }
    }

    pub fn bpm(&self) -> f64 {
        self.bpm_text.parse::<f64>().unwrap_or(120.0).clamp(20.0, 300.0)
    }

    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty() && self.bpm() >= 20.0
    }

    pub fn apply_template(&mut self, t: ProjectTemplate) {
        self.template = t;
        self.bpm_text = t.default_bpm().to_string();
    }

    pub fn result(&self) -> ProjectWizardResult {
        ProjectWizardResult {
            name: self.name.trim().to_string(),
            location: self.location.clone(),
            template: self.template,
            bpm: self.bpm(),
            time_sig_num: self.time_sig_num,
            time_sig_den: self.time_sig_den,
            sample_rate: self.sample_rate,
        }
    }
}

// ── Callbacks ─────────────────────────────────────────────────────────────────

pub struct ProjectWizardCallbacks {
    pub on_close: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
    pub on_create: Arc<dyn Fn(&ProjectWizardResult, &mut Window, &mut App) + 'static>,
    pub on_template: Arc<dyn Fn(&ProjectTemplate, &mut Window, &mut App) + 'static>,
    /// Stepper delta: +1 or −1 (or ±5 for shift+click in the future).
    pub on_bpm_step: Arc<dyn Fn(&i32, &mut Window, &mut App) + 'static>,
    pub on_time_sig_num: Arc<dyn Fn(&u32, &mut Window, &mut App) + 'static>,
    pub on_time_sig_den: Arc<dyn Fn(&u32, &mut Window, &mut App) + 'static>,
    pub on_sample_rate: Arc<dyn Fn(&u32, &mut Window, &mut App) + 'static>,
    pub on_browse_location: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
}

// ── Sub-components ────────────────────────────────────────────────────────────

fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .mb(px(8.0))
        .child(
            div()
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgba(0x4A556680))
                .child(text),
        )
        .child(
            div()
                .flex_1()
                .h(px(1.0))
                .bg(rgba(0xFFFFFF08)),
        )
}

fn field_row(
    label: &'static str,
    child: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .h(px(32.0))
        .child(
            div()
                .w(px(84.0))
                .flex_shrink_0()
                .text_size(px(10.5))
                .text_color(rgba(0x8090A8CC))
                .child(label),
        )
        .child(child)
}

fn template_card(
    tmpl: ProjectTemplate,
    active: bool,
    index: usize,
    cb: Arc<dyn Fn(&ProjectTemplate, &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let border = if active { rgba(0x5FCED0B0) } else { rgba(0xFFFFFF0F) };
    let bg = if active { rgba(0x0D2030FF) } else { rgba(0x12161EFF) };
    let icon_color = if active { Colors::accent_primary() } else { rgba(0x5A6A8080) };
    let name_color = if active { Colors::text_primary() } else { rgba(0xB0C0D0CC) };

    div()
        .id(("wizard-tmpl", index))
        .flex()
        .flex_col()
        .items_start()
        .justify_between()
        .flex_1()
        .h(px(78.0))
        .rounded_lg()
        .border(px(1.0))
        .border_color(border)
        .bg(bg)
        .px(px(10.0))
        .py(px(10.0))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| {
            s.bg(rgba(0x16202EFF))
                .border_color(rgba(0xFFFFFF1E))
        })
        .on_click(move |_, window, cx| cb(&tmpl, window, cx))
        // Top: icon
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(26.0))
                .h(px(26.0))
                .rounded_md()
                .bg(if active { rgba(0x5FCED018) } else { rgba(0xFFFFFF08) })
                .child(
                    svg()
                        .path(tmpl.icon_path())
                        .w(px(13.0))
                        .h(px(13.0))
                        .text_color(icon_color),
                ),
        )
        // Bottom: name + subtitle
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(name_color)
                        .child(tmpl.label()),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(rgba(0x4A556660))
                        .child(tmpl.subtitle()),
                ),
        )
}

fn seg_button(
    label: &'static str,
    active: bool,
    id: impl Into<gpui::ElementId>,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(26.0))
        .px(px(9.0))
        .min_w(px(28.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(if active { rgba(0x5FCED080) } else { rgba(0xFFFFFF10) })
        .bg(if active { rgba(0x5FCED018) } else { rgba(0x0E1117FF) })
        .text_size(px(10.5))
        .font_weight(if active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
        .text_color(if active { Colors::text_primary() } else { rgba(0x7080A0AA) })
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(rgba(0x1A2030FF)).border_color(rgba(0xFFFFFF1A)))
        .on_click(on_click)
        .child(label)
}

fn stepper_btn(
    label: &'static str,
    id: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(24.0))
        .h(px(26.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(rgba(0xFFFFFF10))
        .bg(rgba(0x0E1117FF))
        .text_size(px(13.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(rgba(0x7080A0CC))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(rgba(0x1A2030FF)).border_color(rgba(0xFFFFFF18)))
        .on_click(on_click)
        .child(label)
}

// ── Main ──────────────────────────────────────────────────────────────────────

/// Render the New Project wizard overlay.
///
/// `name_input` and `bpm_input` are the live text-input states from `StudioLayout`.
/// `name_focused` / `bpm_focused` are `focus_handle.is_focused(cx)` computed
/// by the caller.
pub fn project_wizard(
    state: &ProjectWizardState,
    name_input: &TextInputState,
    name_focused: bool,
    bpm_input: &TextInputState,
    bpm_focused: bool,
    callbacks: ProjectWizardCallbacks,
) -> impl IntoElement {
    let close_backdrop = callbacks.on_close.clone();
    let close_btn = callbacks.on_close.clone();
    let create_cb = callbacks.on_create.clone();
    let result = state.result();
    let can_create = state.is_valid();

    // ── Template row ──────────────────────────────────────────────────────────
    let template_row = {
        let mut row = div().flex().flex_row().gap(px(5.0));
        for (i, tmpl) in ProjectTemplate::all().into_iter().enumerate() {
            let active = state.template == tmpl;
            let cb = callbacks.on_template.clone();
            row = row.child(template_card(tmpl, active, i, cb));
        }
        row
    };

    // ── Time sig presets ──────────────────────────────────────────────────────
    // Common numerators
    let numerators: &[u32] = &[2, 3, 4, 5, 6, 7, 8];
    let denominators: &[u32] = &[4, 8, 16];
    let sample_rates: &[(u32, &str)] = &[
        (44100, "44.1 kHz"),
        (48000, "48 kHz"),
        (88200, "88.2 kHz"),
        (96000, "96 kHz"),
    ];

    let num_row = {
        let mut row = div().flex().flex_row().gap(px(3.0));
        for &n in numerators {
            let cb = callbacks.on_time_sig_num.clone();
            let active = state.time_sig_num == n;
            row = row.child(seg_button(
                Box::leak(n.to_string().into_boxed_str()),
                active,
                ("wizard-tsn", n),
                move |_, w, cx| cb(&n, w, cx),
            ));
        }
        row
    };
    let den_row = {
        let mut row = div().flex().flex_row().gap(px(3.0));
        for &d in denominators {
            let cb = callbacks.on_time_sig_den.clone();
            let active = state.time_sig_den == d;
            row = row.child(seg_button(
                Box::leak(d.to_string().into_boxed_str()),
                active,
                ("wizard-tsd", d),
                move |_, w, cx| cb(&d, w, cx),
            ));
        }
        row
    };
    let rate_row = {
        let mut row = div().flex().flex_row().gap(px(3.0));
        for &(sr, label) in sample_rates {
            let cb = callbacks.on_sample_rate.clone();
            let active = state.sample_rate == sr;
            row = row.child(seg_button(
                label,
                active,
                ("wizard-sr", sr),
                move |_, w, cx| cb(&sr, w, cx),
            ));
        }
        row
    };

    // ── BPM stepper ───────────────────────────────────────────────────────────
    let bpm_dec = callbacks.on_bpm_step.clone();
    let bpm_inc = callbacks.on_bpm_step.clone();

    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .right_0()
        .flex()
        .items_start()
        .justify_center()
        .pt(px(44.0))
        .px(px(18.0))
        .pb(px(32.0))
        .id("project-wizard-overlay")
        .bg(rgba(0x00000055))
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            close_backdrop(&(), window, cx);
        })
        .child(
            div()
                .flex()
                .flex_col()
                .w(px(560.0))
                .max_w(px(560.0))
                .overflow_hidden()
                .rounded_xl()
                .border(px(1.0))
                .border_color(rgba(0xFFFFFF14))
                .bg(rgba(0x161B24FF))
                .shadow_xl()
                .on_mouse_down(gpui::MouseButton::Left, |_, _w, cx| cx.stop_propagation())
                // ── Title bar ─────────────────────────────────────────────
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .h(px(42.0))
                        .px(px(18.0))
                        .border_b(px(1.0))
                        .border_color(rgba(0xFFFFFF0A))
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
                                        .w(px(22.0))
                                        .h(px(22.0))
                                        .rounded_md()
                                        .bg(rgba(0x5FCED018))
                                        .child(
                                            svg()
                                                .path(assets::ICON_FOLDER_PATH)
                                                .w(px(11.0))
                                                .h(px(11.0))
                                                .text_color(Colors::accent_primary()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Colors::text_primary())
                                        .child("New Project"),
                                ),
                        )
                        .child(
                            div()
                                .id("wizard-close")
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(22.0))
                                .h(px(22.0))
                                .rounded_md()
                                .cursor(gpui::CursorStyle::PointingHand)
                                .hover(|s| s.bg(rgba(0xFFFFFF0F)))
                                .on_click(move |_, w, cx| close_btn(&(), w, cx))
                                .child(
                                    svg()
                                        .path(assets::ICON_X_PATH)
                                        .w(px(12.0))
                                        .h(px(12.0))
                                        .text_color(rgba(0x6070808C)),
                                ),
                        ),
                )
                // ── Body ──────────────────────────────────────────────────
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(18.0))
                        .px(px(18.0))
                        .py(px(16.0))
                        // ── PROJECT DETAILS ───────────────────────────────
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(section_label("PROJECT DETAILS"))
                                .child(field_row("Name", text_field(name_input, name_focused)))
                                .child({
                                    let browse = callbacks.on_browse_location.clone();
                                    field_row(
                                        "Location",
                                        div()
                                            .flex()
                                            .flex_row()
                                            .flex_1()
                                            .gap(px(6.0))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .h(px(28.0))
                                                    .rounded_md()
                                                    .border(px(1.0))
                                                    .border_color(rgba(0xFFFFFF1A))
                                                    .bg(rgba(0x0E1117FF))
                                                    .px(px(10.0))
                                                    .flex()
                                                    .items_center()
                                                    .overflow_hidden()
                                                    .child(
                                                        div()
                                                            .text_size(px(10.5))
                                                            .text_color(rgba(0x6878908C))
                                                            .child(
                                                                state
                                                                    .location
                                                                    .to_string_lossy()
                                                                    .to_string(),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .id("wizard-browse")
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .h(px(28.0))
                                                    .px(px(12.0))
                                                    .rounded_md()
                                                    .border(px(1.0))
                                                    .border_color(rgba(0xFFFFFF10))
                                                    .bg(rgba(0x0E1117FF))
                                                    .text_size(px(11.0))
                                                    .text_color(rgba(0x8090A8CC))
                                                    .cursor(gpui::CursorStyle::PointingHand)
                                                    .hover(|s| s.bg(rgba(0x1A2030FF)))
                                                    .on_click(move |_, w, cx| browse(&(), w, cx))
                                                    .child("Browse…"),
                                            ),
                                    )
                                }),
                        )
                        // ── TEMPLATE ──────────────────────────────────────
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(section_label("TEMPLATE"))
                                .child(template_row),
                        )
                        // ── SETTINGS ─────────────────────────────────────
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(section_label("SETTINGS"))
                                // BPM row
                                .child(field_row(
                                    "Tempo",
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(5.0))
                                        .child(
                                            div()
                                                .w(px(68.0))
                                                .child(text_field(bpm_input, bpm_focused)),
                                        )
                                        .child(stepper_btn("−", "wizard-bpm-dec", move |_, w, cx| {
                                            bpm_dec(&-1, w, cx);
                                        }))
                                        .child(stepper_btn("+", "wizard-bpm-inc", move |_, w, cx| {
                                            bpm_inc(&1, w, cx);
                                        }))
                                        .child(
                                            div()
                                                .text_size(px(10.0))
                                                .text_color(rgba(0x4A556660))
                                                .ml(px(4.0))
                                                .child("BPM"),
                                        ),
                                ))
                                // Time Sig numerator
                                .child(field_row("Time Sig", num_row))
                                // Time Sig denominator
                                .child(field_row("Beat Value", den_row))
                                // Sample Rate
                                .child(field_row("Sample Rate", rate_row)),
                        ),
                )
                // ── Footer ────────────────────────────────────────────────
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .gap(px(8.0))
                        .border_t(px(1.0))
                        .border_color(rgba(0xFFFFFF0A))
                        .px(px(18.0))
                        .py(px(13.0))
                        // Cancel
                        .child(
                            div()
                                .id("wizard-cancel")
                                .flex()
                                .items_center()
                                .justify_center()
                                .h(px(30.0))
                                .px(px(16.0))
                                .rounded_md()
                                .border(px(1.0))
                                .border_color(rgba(0xFFFFFF10))
                                .text_size(px(11.5))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(rgba(0x8090A8CC))
                                .cursor(gpui::CursorStyle::PointingHand)
                                .hover(|s| s.bg(rgba(0x1A2030FF)))
                                .on_click({
                                    let cb = callbacks.on_close.clone();
                                    move |_, w, cx| cb(&(), w, cx)
                                })
                                .child("Cancel"),
                        )
                        // Create Project (primary)
                        .child(
                            div()
                                .id("wizard-create")
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.0))
                                .h(px(30.0))
                                .px(px(16.0))
                                .rounded_md()
                                .bg(if can_create {
                                    Colors::accent_primary()
                                } else {
                                    rgba(0x5FCED030)
                                })
                                .text_size(px(11.5))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(if can_create {
                                    gpui::rgb(0x0A1018)
                                } else {
                                    rgba(0x5FCED060)
                                })
                                .when(can_create, |d| {
                                    d.cursor(gpui::CursorStyle::PointingHand)
                                        .hover(|s| s.bg(gpui::rgb(0x7ADBDD)))
                                        .on_click(move |_, w, cx| create_cb(&result, w, cx))
                                })
                                .child(
                                    svg()
                                        .path(assets::ICON_PLUS_PATH)
                                        .w(px(11.0))
                                        .h(px(11.0))
                                        .text_color(if can_create {
                                            gpui::rgb(0x0A1018)
                                        } else {
                                            rgba(0x5FCED060)
                                        }),
                                )
                                .child("Create Project"),
                        ),
                ),
        )
}
