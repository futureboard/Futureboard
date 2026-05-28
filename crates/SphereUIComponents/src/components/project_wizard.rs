use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, size, svg, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Point, Render,
    StatefulInteractiveElement, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind,
};

use crate::assets;
use crate::components::combo_box::{combo_box_menu, combo_box_trigger, ComboBoxOption};
use crate::components::context_menu::context_menu_overlay;
use crate::components::controls::{
    fb_button, fb_form_row, fb_section_label, fb_stepper_button, FbButtonKind,
};
use crate::components::title_bar::external_window_titlebar;
use crate::components::text_input::{
    text_field_with_callbacks, text_input_context_entries, TextInputAction, TextInputCallbacks,
    TextInputState,
};
use crate::overlay::{
    compute_overlay_position, form_combo_trigger_bounds, refresh_form_anchor,
    wizard_form_column, OverlayAnchor, OverlayPlacement, OverlaySize, COMBO_TRIGGER_HEIGHT,
};
use crate::theme::{self, Colors};

const WIZARD_WIDTH: f32 = 900.0;
const WIZARD_HEIGHT: f32 = 620.0;
const COMBO_MENU_ESTIMATE_HEIGHT: f32 = 160.0;

const TIME_SIGNATURE_OPTIONS: &[ComboBoxOption<(u32, u32)>] = &[
    ComboBoxOption {
        label: "4/4",
        value: (4, 4),
    },
    ComboBoxOption {
        label: "3/4",
        value: (3, 4),
    },
    ComboBoxOption {
        label: "2/4",
        value: (2, 4),
    },
    ComboBoxOption {
        label: "6/8",
        value: (6, 8),
    },
    ComboBoxOption {
        label: "7/8",
        value: (7, 8),
    },
    ComboBoxOption {
        label: "12/8",
        value: (12, 8),
    },
];

const BEAT_GRID_OPTIONS: &[ComboBoxOption<u32>] = &[
    ComboBoxOption {
        label: "1/4",
        value: 4,
    },
    ComboBoxOption {
        label: "1/8",
        value: 8,
    },
    ComboBoxOption {
        label: "1/16",
        value: 16,
    },
    ComboBoxOption {
        label: "1/32",
        value: 32,
    },
];

const SAMPLE_RATE_OPTIONS: &[ComboBoxOption<u32>] = &[
    ComboBoxOption {
        label: "44.1 kHz",
        value: 44100,
    },
    ComboBoxOption {
        label: "48 kHz",
        value: 48000,
    },
    ComboBoxOption {
        label: "88.2 kHz",
        value: 88200,
    },
    ComboBoxOption {
        label: "96 kHz",
        value: 96000,
    },
    ComboBoxOption {
        label: "192 kHz",
        value: 192000,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WizardCombo {
    TimeSignature,
    BeatGrid,
    SampleRate,
}

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

    pub fn description(self) -> &'static str {
        match self {
            Self::Empty => "A clean session with the master bus ready.",
            Self::Recording => "Audio tracks, monitoring, and record-ready routing.",
            Self::BeatMaking => "MIDI lanes for drums, bass, keys, and texture.",
            Self::Mixing => "Audio channels organized for edit and mix work.",
            Self::Scoring => "MIDI-first layout for cues and arrangement sketches.",
        }
    }

    pub fn metadata(self) -> &'static str {
        match self {
            Self::Empty => "No tracks | 120 BPM",
            Self::Recording => "4 audio | 120 BPM",
            Self::BeatMaking => "4 MIDI | 140 BPM",
            Self::Mixing => "8 audio | 120 BPM",
            Self::Scoring => "8 MIDI | 120 BPM",
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
        if matches!(self, Self::BeatMaking) {
            140.0
        } else {
            120.0
        }
    }

    pub fn all() -> [Self; 5] {
        [
            Self::Empty,
            Self::Recording,
            Self::BeatMaking,
            Self::Mixing,
            Self::Scoring,
        ]
    }
}

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

#[derive(Debug, Clone)]
pub struct ProjectWizardState {
    pub is_open: bool,
    pub name: String,
    pub location: PathBuf,
    pub template: ProjectTemplate,
    pub bpm_text: String,
    pub time_sig_num: u32,
    pub time_sig_den: u32,
    pub sample_rate: u32,
    pub beat_value: u32,
    pub error: Option<String>,
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
            beat_value: 4,
            error: None,
        }
    }

    pub fn open() -> Self {
        Self {
            is_open: true,
            ..Self::closed()
        }
    }

    pub fn bpm(&self) -> f64 {
        self.bpm_text
            .parse::<f64>()
            .unwrap_or(120.0)
            .clamp(20.0, 300.0)
    }

    pub fn apply_template(&mut self, t: ProjectTemplate) {
        self.template = t;
        self.bpm_text = format!("{:.0}", t.default_bpm());
        self.error = None;
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

    pub fn validation_error(&self) -> Option<String> {
        validate_project_result(&self.result())
    }
}

pub type ProjectCreateCallback =
    Arc<dyn Fn(ProjectWizardResult, &mut App) -> Result<(), String> + 'static>;

pub struct ProjectWizardWindow {
    state: ProjectWizardState,
    name_input: TextInputState,
    location_input: TextInputState,
    bpm_input: TextInputState,
    focus_handle: FocusHandle,
    open_combo: Option<WizardCombo>,
    combo_anchor: Option<OverlayAnchor>,
    text_menu: Option<WizardTextMenu>,
    on_create: ProjectCreateCallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WizardTextTarget {
    Name,
    Location,
    Bpm,
}

#[derive(Clone, Copy, Debug)]
struct WizardTextMenu {
    target: WizardTextTarget,
    x: f32,
    y: f32,
}

impl ProjectWizardWindow {
    pub fn new(on_create: ProjectCreateCallback, cx: &mut Context<Self>) -> Self {
        let mut state = ProjectWizardState::open();
        let mut name_input =
            TextInputState::new("wizard-project-name", cx.focus_handle()).with_placeholder("Name");
        name_input.set_value(state.name.clone());
        name_input.select_all();

        let mut location_input = TextInputState::new("wizard-project-location", cx.focus_handle())
            .with_placeholder("~/Documents/Futureboard Studio/Projects");
        location_input.set_value(state.location.to_string_lossy().to_string());

        let mut bpm_input =
            TextInputState::new("wizard-project-bpm", cx.focus_handle()).with_placeholder("120");
        bpm_input.set_value(state.bpm_text.clone());

        state.error = state.validation_error();
        Self {
            state,
            name_input,
            location_input,
            bpm_input,
            focus_handle: cx.focus_handle(),
            open_combo: None,
            combo_anchor: None,
            text_menu: None,
            on_create,
        }
    }

    fn sync_inputs_to_state(&mut self) {
        self.state.name = self.name_input.value.clone();
        self.state.location = PathBuf::from(self.location_input.value.trim());
        self.state.bpm_text = self.bpm_input.value.clone();
        self.state.error = self.state.validation_error();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_inputs_to_state();
        if let Some(error) = self.state.validation_error() {
            self.state.error = Some(error);
            cx.notify();
            return;
        }

        match (self.on_create)(self.state.result(), cx) {
            Ok(()) => window.remove_window(),
            Err(error) => {
                self.state.error = Some(error);
                cx.notify();
            }
        }
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key.as_str() == "escape" && self.text_menu.take().is_some() {
            cx.notify();
            return;
        }

        let name_focused = self.name_input.is_focused(window);
        let location_focused = self.location_input.is_focused(window);
        let bpm_focused = self.bpm_input.is_focused(window);

        if name_focused || location_focused || bpm_focused {
            let action = if name_focused {
                self.name_input.handle_key_with_clipboard(event, Some(cx))
            } else if location_focused {
                self.location_input
                    .handle_key_with_clipboard(event, Some(cx))
            } else {
                self.bpm_input.handle_key_with_clipboard(event, Some(cx))
            };
            self.sync_inputs_to_state();
            match action {
                TextInputAction::Submit => self.submit(window, cx),
                TextInputAction::Cancel => window.remove_window(),
                TextInputAction::Consumed | TextInputAction::Pass => cx.notify(),
            }
            return;
        }

        match event.keystroke.key.as_str() {
            "escape" => {
                if self.open_combo.take().is_some() {
                    self.combo_anchor = None;
                    cx.notify();
                } else {
                    window.remove_window();
                }
            }
            "enter" | "numpad_enter" => self.submit(window, cx),
            _ => {}
        }
    }

    fn text_input_mut(&mut self, target: WizardTextTarget) -> &mut TextInputState {
        match target {
            WizardTextTarget::Name => &mut self.name_input,
            WizardTextTarget::Location => &mut self.location_input,
            WizardTextTarget::Bpm => &mut self.bpm_input,
        }
    }

    fn text_input(&self, target: WizardTextTarget) -> &TextInputState {
        match target {
            WizardTextTarget::Name => &self.name_input,
            WizardTextTarget::Location => &self.location_input,
            WizardTextTarget::Bpm => &self.bpm_input,
        }
    }
}

pub fn open_project_wizard_window(
    owner_bounds: Bounds<gpui::Pixels>,
    on_create: ProjectCreateCallback,
    cx: &mut App,
) -> Result<WindowHandle<ProjectWizardWindow>, String> {
    let parent_x: f32 = owner_bounds.origin.x.into();
    let parent_y: f32 = owner_bounds.origin.y.into();
    let parent_w: f32 = owner_bounds.size.width.into();
    let parent_h: f32 = owner_bounds.size.height.into();
    let origin = Point {
        x: px(parent_x + ((parent_w - WIZARD_WIDTH) / 2.0).max(24.0)),
        y: px(parent_y + ((parent_h - WIZARD_HEIGHT) / 2.0).max(24.0)),
    };

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(WIZARD_WIDTH), px(WIZARD_HEIGHT)),
    }));
    options.kind = WindowKind::Floating;
    options.is_resizable = false;
    options.is_minimizable = false;
    options.window_background = WindowBackgroundAppearance::Transparent;
    options.window_min_size = Some(size(px(WIZARD_WIDTH), px(WIZARD_HEIGHT)));

    cx.open_window(
        options,
        |_window, cx| cx.new(|cx| ProjectWizardWindow::new(on_create, cx)),
    )
    .map_err(|error| error.to_string())
}

fn validate_project_result(result: &ProjectWizardResult) -> Option<String> {
    let name = result.name.trim();
    if name.is_empty() {
        return Some("Project name is required.".to_string());
    }
    if result.location.as_os_str().is_empty() {
        return Some("Choose a project location.".to_string());
    }
    if !is_path_usable(&result.location) {
        return Some("Project location is not a valid folder path.".to_string());
    }
    if result.bpm < 20.0 || result.bpm > 300.0 {
        return Some("Tempo must be between 20 and 300 BPM.".to_string());
    }

    let folder = result
        .location
        .join(crate::project::sanitize_project_name(name));
    if folder.exists() {
        return Some("A project with this name already exists at that location.".to_string());
    }
    None
}

fn is_path_usable(path: &Path) -> bool {
    !path
        .components()
        .any(|component| component.as_os_str().is_empty())
}

fn icon(path: &'static str, size_px: f32, color: gpui::Rgba) -> impl IntoElement {
    svg()
        .path(path)
        .w(px(size_px))
        .h(px(size_px))
        .text_color(color)
}

fn section_label(label: &'static str) -> impl IntoElement {
    fb_section_label(label)
}

fn template_card(
    tmpl: ProjectTemplate,
    active: bool,
    disabled: bool,
    index: usize,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let bg = if active {
        Colors::accent_muted()
    } else {
        Colors::surface_card()
    };
    let border = if active {
        Colors::border_accent()
    } else {
        Colors::border_subtle()
    };

    div()
        .id(("project-template", index))
        .relative()
        .flex()
        .flex_col()
        .justify_between()
        .gap(px(9.0))
        .h(px(94.0))
        .rounded_md()
        .border(px(1.0))
        .border_color(border)
        .bg(bg)
        .px(px(12.0))
        .py(px(11.0))
        .opacity(if disabled { 0.42 } else { 1.0 })
        .when(!disabled, |this| {
            this.cursor(gpui::CursorStyle::PointingHand)
                .hover(|s| {
                    s.bg(Colors::surface_control_hover())
                        .border_color(Colors::border_strong())
                })
                .on_click(on_click)
        })
        .when(active, |this| {
            this.shadow_sm().child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(2.0))
                    .bg(Colors::with_alpha(Colors::accent_primary(), 0.44)),
            )
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(9.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(28.0))
                        .h(px(28.0))
                        .rounded_md()
                        .bg(if active {
                            Colors::accent_muted()
                        } else {
                            Colors::surface_input()
                        })
                        .child(icon(
                            tmpl.icon_path(),
                            13.0,
                            if active {
                                Colors::accent_primary()
                            } else {
                                Colors::text_secondary()
                            },
                        )),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(Colors::text_primary())
                                .truncate()
                                .child(tmpl.label()),
                        )
                        .child(
                            div()
                                .text_size(px(9.5))
                                .text_color(Colors::text_muted())
                                .line_clamp(2)
                                .child(tmpl.description()),
                        ),
                ),
        )
        .child(
            div()
                .h(px(18.0))
                .flex()
                .items_center()
                .rounded_md()
                .bg(if active {
                    Colors::accent_muted()
                } else {
                    Colors::surface_input()
                })
                .px(px(7.0))
                .text_size(px(9.5))
                .text_color(if active {
                    Colors::accent_primary()
                } else {
                    Colors::text_faint()
                })
                .child(tmpl.metadata()),
        )
}

fn action_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    enabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    fb_button(
        id,
        label,
        if primary {
            FbButtonKind::Primary
        } else {
            FbButtonKind::Default
        },
        enabled,
        on_click,
    )
}

fn stepper_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    fb_stepper_button(id, label, on_click)
}

fn wizard_combo_menu_position(anchor: OverlayAnchor, window: &Window) -> crate::overlay::OverlayPosition {
    let layout = wizard_form_column(window);
    let refreshed = refresh_form_anchor(anchor, layout);
    compute_overlay_position(
        refreshed.bounds,
        OverlaySize {
            width: layout.value_width,
            height: COMBO_MENU_ESTIMATE_HEIGHT,
        },
        window.bounds(),
        OverlayPlacement::BottomStart,
        4.0,
    )
}

fn settings_row(label: &'static str, child: impl IntoElement) -> impl IntoElement {
    fb_form_row(label, child)
}

fn time_signature_label(num: u32, den: u32) -> String {
    format!("{num}/{den}")
}

fn beat_grid_label(value: u32) -> String {
    format!("1/{value}")
}

fn sample_rate_label(value: u32) -> String {
    SAMPLE_RATE_OPTIONS
        .iter()
        .find(|option| option.value == value)
        .map(|option| option.label.to_string())
        .unwrap_or_else(|| format!("{} kHz", value as f32 / 1000.0))
}

fn summary_line(label: &'static str, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(10.0))
                .text_color(Colors::text_faint())
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_size(px(10.5))
                .text_color(Colors::text_primary())
                .child(value.into()),
        )
}

fn compact_path(path: &Path) -> String {
    let text = path.to_string_lossy().to_string();
    let max_chars = 36;
    if text.chars().count() <= max_chars {
        text
    } else {
        let tail: String = text
            .chars()
            .rev()
            .take(max_chars - 3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("...{tail}")
    }
}

impl Render for ProjectWizardWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if window.focused(cx).is_none() {
            self.name_input.focus_handle.focus(window);
        }

        let target = cx.entity().clone();
        let name_focused = self.name_input.is_focused(window);
        let location_focused = self.location_input.is_focused(window);
        let bpm_focused = self.bpm_input.is_focused(window);
        let can_create = self.state.validation_error().is_none();
        let validation_message = self
            .state
            .error
            .clone()
            .or_else(|| self.state.validation_error());

        let template_grid = {
            let mut grid = div().grid().grid_cols(2).gap(px(9.0)).child(template_card(
                ProjectTemplate::Empty,
                self.state.template == ProjectTemplate::Empty,
                false,
                0,
                {
                    let target = target.clone();
                    move |_, _, cx| {
                        let _ = target.update(cx, |this, cx| {
                            this.state.apply_template(ProjectTemplate::Empty);
                            this.bpm_input.set_value(this.state.bpm_text.clone());
                            cx.notify();
                        });
                    }
                },
            ));

            for (index, tmpl) in ProjectTemplate::all().into_iter().enumerate().skip(1) {
                let target = target.clone();
                grid = grid.child(template_card(
                    tmpl,
                    self.state.template == tmpl,
                    false,
                    index,
                    move |_, _, cx| {
                        let tmpl = tmpl;
                        let _ = target.update(cx, |this, cx| {
                            this.state.apply_template(tmpl);
                            this.bpm_input.set_value(this.state.bpm_text.clone());
                            cx.notify();
                        });
                    },
                ));
            }
            grid
        };

        let time_signature_row = combo_box_trigger(
            "project-time-signature-combo",
            time_signature_label(self.state.time_sig_num, self.state.time_sig_den),
            self.open_combo == Some(WizardCombo::TimeSignature),
            {
                let target = target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| {
                        if this.open_combo == Some(WizardCombo::TimeSignature) {
                            this.open_combo = None;
                            this.combo_anchor = None;
                        } else {
                            let layout = wizard_form_column(window);
                            this.combo_anchor = Some(OverlayAnchor {
                                bounds: form_combo_trigger_bounds(
                                    layout,
                                    event,
                                    COMBO_TRIGGER_HEIGHT,
                                ),
                            });
                            this.open_combo = Some(WizardCombo::TimeSignature);
                        }
                        cx.notify();
                    });
                }
            },
        );

        let beat_value_row = combo_box_trigger(
            "project-beat-grid-combo",
            beat_grid_label(self.state.beat_value),
            self.open_combo == Some(WizardCombo::BeatGrid),
            {
                let target = target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| {
                        if this.open_combo == Some(WizardCombo::BeatGrid) {
                            this.open_combo = None;
                            this.combo_anchor = None;
                        } else {
                            let layout = wizard_form_column(window);
                            this.combo_anchor = Some(OverlayAnchor {
                                bounds: form_combo_trigger_bounds(
                                    layout,
                                    event,
                                    COMBO_TRIGGER_HEIGHT,
                                ),
                            });
                            this.open_combo = Some(WizardCombo::BeatGrid);
                        }
                        cx.notify();
                    });
                }
            },
        );

        let sample_rate_row = combo_box_trigger(
            "project-sample-rate-combo",
            sample_rate_label(self.state.sample_rate),
            self.open_combo == Some(WizardCombo::SampleRate),
            {
                let target = target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| {
                        if this.open_combo == Some(WizardCombo::SampleRate) {
                            this.open_combo = None;
                            this.combo_anchor = None;
                        } else {
                            let layout = wizard_form_column(window);
                            this.combo_anchor = Some(OverlayAnchor {
                                bounds: form_combo_trigger_bounds(
                                    layout,
                                    event,
                                    COMBO_TRIGGER_HEIGHT,
                                ),
                            });
                            this.open_combo = Some(WizardCombo::SampleRate);
                        }
                        cx.notify();
                    });
                }
            },
        );

        let combo_overlay = if let (Some(open_combo), Some(anchor)) =
            (self.open_combo, self.combo_anchor)
        {
            let position = wizard_combo_menu_position(anchor, window);
            let close_target = target.clone();
            Some(
            div()
                .absolute()
                .inset_0()
                .id("project-wizard-combo-overlay")
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = close_target.update(cx, |this, cx| {
                        this.open_combo = None;
                        this.combo_anchor = None;
                        cx.notify();
                    });
                })
                .child(match open_combo {
                    WizardCombo::TimeSignature => {
                        let select_target = target.clone();
                        combo_box_menu(
                            "project-time-signature-menu",
                            position,
                            (self.state.time_sig_num, self.state.time_sig_den),
                            TIME_SIGNATURE_OPTIONS,
                            Arc::new(move |(num, den), _, cx| {
                                let _ = select_target.update(cx, |this, cx| {
                                    this.state.time_sig_num = num;
                                    this.state.time_sig_den = den;
                                    this.state.error = None;
                                    this.open_combo = None;
                                    this.combo_anchor = None;
                                    cx.notify();
                                });
                            }),
                        )
                        .into_any_element()
                    }
                    WizardCombo::BeatGrid => {
                        let select_target = target.clone();
                        combo_box_menu(
                            "project-beat-grid-menu",
                            position,
                            self.state.beat_value,
                            BEAT_GRID_OPTIONS,
                            Arc::new(move |value, _, cx| {
                                let _ = select_target.update(cx, |this, cx| {
                                    this.state.beat_value = value;
                                    this.state.error = None;
                                    this.open_combo = None;
                                    this.combo_anchor = None;
                                    cx.notify();
                                });
                            }),
                        )
                        .into_any_element()
                    }
                    WizardCombo::SampleRate => {
                        let select_target = target.clone();
                        combo_box_menu(
                            "project-sample-rate-menu",
                            position,
                            self.state.sample_rate,
                            SAMPLE_RATE_OPTIONS,
                            Arc::new(move |value, _, cx| {
                                let _ = select_target.update(cx, |this, cx| {
                                    this.state.sample_rate = value;
                                    this.state.error = None;
                                    this.open_combo = None;
                                    this.combo_anchor = None;
                                    cx.notify();
                                });
                            }),
                        )
                        .into_any_element()
                    }
                })
            )
        } else {
            None
        };

        let text_input_callbacks = |text_target: WizardTextTarget| TextInputCallbacks {
            on_context_menu: Some(Arc::new({
                let target = target.clone();
                move |(x, y): &(f32, f32), _window, cx| {
                    let x = *x;
                    let y = *y;
                    let _ = target.update(cx, |this, cx| {
                        this.open_combo = None;
                        this.combo_anchor = None;
                        this.text_menu = Some(WizardTextMenu {
                            target: text_target,
                            x,
                            y,
                        });
                        cx.notify();
                    });
                }
            })),
            on_mouse: None,
        };

        let text_menu_overlay = self.text_menu.map(|menu| {
            let clipboard_has_text = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .is_some_and(|text| !text.is_empty());
            let entries =
                text_input_context_entries(self.text_input(menu.target), clipboard_has_text);
            let command_target = target.clone();
            let close_target = target.clone();
            context_menu_overlay(
                entries,
                menu.x,
                menu.y,
                WIZARD_WIDTH,
                WIZARD_HEIGHT,
                Arc::new(move |command: &String, _window, cx| {
                    let command = command.clone();
                    let _ = command_target.update(cx, |this, cx| {
                        if let Some(menu) = this.text_menu {
                            let input = this.text_input_mut(menu.target);
                            let _ = input.apply_context_command(&command, cx);
                            this.sync_inputs_to_state();
                        }
                        this.text_menu = None;
                        cx.notify();
                    });
                }),
                Arc::new(move |_: &(), _window, cx| {
                    let _ = close_target.update(cx, |this, cx| {
                        this.text_menu = None;
                        cx.notify();
                    });
                }),
            )
        });

        div()
            .flex()
            .flex_col()
            .size_full()
            .relative()
            .font_family(theme::FONT_FAMILY)
            .bg(Colors::surface_window())
            .overflow_hidden()
            .capture_key_down({
                let target = target.clone();
                move |event, window, cx| {
                    let _ = target.update(cx, |this, cx| this.handle_key(event, window, cx));
                }
            })
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(
                external_window_titlebar(
                    "New Project",
                    "project-wizard-close",
                    {
                        let target = target.clone();
                        move |window, cx| {
                            let _ = target.update(cx, |this, cx| {
                                this.open_combo = None;
                                this.combo_anchor = None;
                                this.text_menu = None;
                                cx.notify();
                            });
                            window.remove_window();
                        }
                    },
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .gap(px(16.0))
                    .p(px(16.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(14.0))
                            .w(px(540.0))
                            .min_w(px(540.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.0))
                                    .child(section_label("PROJECT"))
                                    .child(fb_form_row(
                                        "Name",
                                        text_field_with_callbacks(
                                            &self.name_input,
                                            name_focused,
                                            text_input_callbacks(WizardTextTarget::Name),
                                        ),
                                    ))
                                    .child(fb_form_row(
                                        "Location",
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .child(text_field_with_callbacks(
                                                        &self.location_input,
                                                        location_focused,
                                                        text_input_callbacks(
                                                            WizardTextTarget::Location,
                                                        ),
                                                    )),
                                            )
                                            .child(
                                                div().w(px(84.0)).child(action_button(
                                                        "project-wizard-browse",
                                                        "Browse",
                                                        false,
                                                        true,
                                                        {
                                                        let target = target.clone();
                                                        move |_, _, cx| {
                                                            let current =
                                                                target.read(cx).state.location.clone();
                                                            let fut = rfd::AsyncFileDialog::new()
                                                                .set_title("Choose Project Location")
                                                                .set_directory(&current)
                                                                .pick_folder();
                                                            let target2 = target.clone();
                                                            cx.spawn(async move |cx| {
                                                                if let Some(handle) = fut.await {
                                                                    let path = handle
                                                                        .path()
                                                                        .to_path_buf();
                                                                    let _ = target2.update(
                                                                        cx,
                                                                        |this, cx| {
                                                                            this.state.location =
                                                                                path.clone();
                                                                            this.location_input
                                                                                .set_value(path
                                                                                    .to_string_lossy()
                                                                                    .to_string());
                                                                            this.sync_inputs_to_state();
                                                                            cx.notify();
                                                                        },
                                                                    );
                                                                }
                                                            })
                                                            .detach();
                                                        }
                                                        },
                                                    )
                                                )
                                                    .into_any_element(),
                                            ),
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(9.0))
                                    .child(section_label("TEMPLATE"))
                                    .child(template_grid),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .rounded_md()
                            .border(px(1.0))
                            .border_color(Colors::border_subtle())
                            .bg(Colors::surface_card())
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(14.0))
                                    .p(px(14.0))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(3.0))
                                            .child(section_label("SESSION SETTINGS"))
                                            .child(
                                                div()
                                                    .text_size(px(10.0))
                                                    .text_color(Colors::text_faint())
                                                    .child("Timing and format for the new session"),
                                            ),
                                    )
                                    .child(settings_row(
                                        "Tempo",
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(div().w(px(76.0)).child(
                                                text_field_with_callbacks(
                                                    &self.bpm_input,
                                                    bpm_focused,
                                                    text_input_callbacks(WizardTextTarget::Bpm),
                                                ),
                                            ))
                                            .child({
                                                let target = target.clone();
                                                stepper_button("project-tempo-down", "-", move |_, _, cx| {
                                                    let _ = target.update(cx, |this, cx| {
                                                        let bpm = (this.state.bpm() - 1.0).clamp(20.0, 300.0);
                                                        this.state.bpm_text = format!("{:.0}", bpm);
                                                        this.bpm_input.set_value(this.state.bpm_text.clone());
                                                        this.sync_inputs_to_state();
                                                        cx.notify();
                                                    });
                                                })
                                            })
                                            .child({
                                                let target = target.clone();
                                                stepper_button("project-tempo-up", "+", move |_, _, cx| {
                                                    let _ = target.update(cx, |this, cx| {
                                                        let bpm = (this.state.bpm() + 1.0).clamp(20.0, 300.0);
                                                        this.state.bpm_text = format!("{:.0}", bpm);
                                                        this.bpm_input.set_value(this.state.bpm_text.clone());
                                                        this.sync_inputs_to_state();
                                                        cx.notify();
                                                    });
                                                })
                                            })
                                            .child(
                                                div()
                                                    .text_size(px(10.5))
                                                    .text_color(Colors::text_muted())
                                                    .child("BPM"),
                                            ),
                                    ))
                                    .child(settings_row("Time Signature", time_signature_row))
                                    .child(settings_row("Beat / Grid", beat_value_row))
                                    .child(settings_row("Sample Rate", sample_rate_row)),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(9.0))
                                    .mx(px(14.0))
                                    .mb(px(14.0))
                                    .rounded_lg()
                                    .border(px(1.0))
                                    .border_color(Colors::border_subtle())
                                    .bg(Colors::surface_input())
                                    .p(px(12.0))
                                    .child(section_label("SUMMARY"))
                                    .child(summary_line(
                                        "Template",
                                        self.state.template.label().to_string(),
                                    ))
                                    .child(summary_line(
                                        "Tracks",
                                        format!(
                                            "{} audio / {} MIDI",
                                            self.state.template.audio_tracks(),
                                            self.state.template.midi_tracks()
                                        ),
                                    ))
                                    .child(summary_line(
                                        "Format",
                                        format!(
                                            "{:.0} BPM, {}/{}, {} kHz",
                                            self.state.bpm(),
                                            self.state.time_sig_num,
                                            self.state.time_sig_den,
                                            self.state.sample_rate as f32 / 1000.0
                                        ),
                                    ))
                                    .child(summary_line(
                                        "Location",
                                        compact_path(&self.state.location),
                                    ))
                                    .child(summary_line(
                                        "Project",
                                        self.state.name.trim().to_string(),
                                    )),
                            )
                            .children(validation_message.map(|message| {
                                div()
                                    .mx(px(14.0))
                                    .mb(px(14.0))
                                    .rounded_md()
                                    .border(px(1.0))
                                    .border_color(Colors::with_alpha(Colors::status_warning(), 0.33))
                                    .bg(Colors::with_alpha(Colors::status_warning(), 0.06))
                                    .px(px(10.0))
                                    .py(px(8.0))
                                    .text_size(px(10.5))
                                    .text_color(Colors::status_warning())
                                    .child(message)
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(54.0))
                    .px(px(16.0))
                                    .border_t(px(1.0))
                                    .border_color(Colors::border_subtle())
                                    .bg(Colors::surface_panel())
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(Colors::text_muted())
                            .child("Creates a local Futureboard binary project folder."),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(action_button(
                                "project-wizard-cancel",
                                "Cancel",
                                false,
                                true,
                                |_, window, _| window.remove_window(),
                            ))
                            .child(action_button(
                                "project-wizard-create",
                                "Create Project",
                                true,
                                can_create,
                                {
                                    let target = target.clone();
                                    move |_, window, cx| {
                                        let _ = target.update(cx, |this, cx| this.submit(window, cx));
                                    }
                                },
                            )),
                    ),
            )
            .children(combo_overlay)
            .children(text_menu_overlay)
    }
}
