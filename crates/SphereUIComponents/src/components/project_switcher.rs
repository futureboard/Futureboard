use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, px, svg, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window,
};

use crate::assets;
use crate::components::text_input::{
    text_field_with_callbacks, TextInputCallbacks, TextInputState,
};
use crate::overlay::{
    compute_overlay_position, OverlayAnchor, OverlayPlacement, OverlaySize, OVERLAY_WINDOW_MARGIN,
};
use crate::theme::Colors;

pub const OPEN_RECENT_PATH_PREFIX: &str = "project:open-recent-path:";

pub type ProjectSwitcherCommandCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;
pub type ProjectSwitcherCloseCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;

#[derive(Debug, Clone)]
pub struct ProjectSummary {
    pub name: String,
    pub path: Option<PathBuf>,
    pub is_current: bool,
    pub is_dirty: bool,
    pub subtitle: String,
}

#[derive(Debug, Clone)]
pub struct ProjectSwitcherState {
    pub is_open: bool,
    pub anchor: OverlayAnchor,
    pub query: String,
    pub selected_index: usize,
    pub current_project: ProjectSummary,
    pub recent_projects: Vec<ProjectSummary>,
}

impl Default for ProjectSwitcherState {
    fn default() -> Self {
        Self {
            is_open: false,
            anchor: OverlayAnchor {
                bounds: gpui::bounds(gpui::point(px(0.0), px(0.0)), gpui::size(px(0.0), px(0.0))),
            },
            query: String::new(),
            selected_index: 0,
            current_project: ProjectSummary {
                name: "Untitled Project".to_string(),
                path: None,
                is_current: true,
                is_dirty: false,
                subtitle: "Saved locally".to_string(),
            },
            recent_projects: Vec::new(),
        }
    }
}

const PANEL_WIDTH: f32 = 304.0;
const PANEL_MAX_HEIGHT: f32 = 430.0;
const EDGE_GAP: f32 = OVERLAY_WINDOW_MARGIN;
const ROW_HEIGHT: f32 = 34.0;

pub fn project_switcher_popover(
    state: &ProjectSwitcherState,
    search_input: &TextInputState,
    search_focused: bool,
    search_callbacks: TextInputCallbacks,
    viewport_width: f32,
    viewport_height: f32,
    on_command: ProjectSwitcherCommandCb,
    on_close: ProjectSwitcherCloseCb,
) -> impl IntoElement {
    let window_bounds = gpui::bounds(
        gpui::point(px(0.0), px(0.0)),
        gpui::size(px(viewport_width), px(viewport_height)),
    );
    let pos = compute_overlay_position(
        state.anchor.bounds,
        OverlaySize {
            width: PANEL_WIDTH,
            height: PANEL_MAX_HEIGHT,
        },
        window_bounds,
        OverlayPlacement::BottomStart,
        EDGE_GAP,
    );
    let left: f32 = pos.x.into();
    let top: f32 = pos.y.into();
    let close_backdrop = on_close.clone();

    div()
        .absolute()
        .inset_0()
        .id("project-switcher-overlay")
        .child(
            div()
                .absolute()
                .inset_0()
                .on_mouse_down(gpui::MouseButton::Left, move |_, w, cx| {
                    close_backdrop(&(), w, cx)
                })
                .on_mouse_down(gpui::MouseButton::Right, move |_, w, cx| {
                    on_close(&(), w, cx)
                }),
        )
        .child(panel(
            state,
            search_input,
            search_focused,
            search_callbacks,
            left,
            top,
            on_command,
        ))
}

fn panel_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: Colors::surface_overlay().into(),
        offset: gpui::point(px(0.0), px(12.0)),
        blur_radius: px(40.0),
        spread_radius: px(0.0),
        inset: false,
    }]
}

fn panel(
    state: &ProjectSwitcherState,
    search_input: &TextInputState,
    search_focused: bool,
    search_callbacks: TextInputCallbacks,
    left: f32,
    top: f32,
    on_command: ProjectSwitcherCommandCb,
) -> impl IntoElement {
    let filtered = filtered_recent(state);
    let query = state.query.clone();
    let selected_index = state.selected_index;

    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(PANEL_WIDTH))
        .max_h(px(PANEL_MAX_HEIGHT))
        .rounded_lg()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_card())
        .shadow(panel_shadow())
        .occlude()
        .flex()
        .flex_col()
        .child(search_row(search_input, search_focused, search_callbacks))
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .id("project-switcher-scroll")
                .overflow_y_scroll()
                .p(px(4.0))
                .child(section_label("This Window"))
                .child(project_row(
                    0,
                    &state.current_project,
                    true,
                    selected_index == 0,
                    on_command.clone(),
                    RowAction::Command("project:switch-current".to_string()),
                ))
                .child(divider())
                .child(section_label("Recent Projects"))
                .children(if filtered.is_empty() {
                    vec![empty_recent_row(&query).into_any_element()]
                } else {
                    filtered
                        .iter()
                        .enumerate()
                        .map(|(index, project)| {
                            project_row(
                                index + 1,
                                project,
                                false,
                                selected_index == index + 1,
                                on_command.clone(),
                                RowAction::OpenRecentPath,
                            )
                            .into_any_element()
                        })
                        .collect()
                }),
        )
        .child(
            div()
                .border_t(px(1.0))
                .border_color(Colors::border_subtle())
                .p(px(5.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(action_row(
                    assets::ICON_FOLDER_OPEN_PATH,
                    "Open Project...",
                    "project:open",
                    on_command.clone(),
                ))
                .child(action_row(
                    assets::ICON_PLUS_PATH,
                    "New Project",
                    "project:new",
                    on_command,
                )),
        )
}

fn filtered_recent(state: &ProjectSwitcherState) -> Vec<ProjectSummary> {
    let query = state.query.trim().to_lowercase();
    state
        .recent_projects
        .iter()
        .filter(|project| !project.is_current)
        .filter(|project| {
            if query.is_empty() {
                return true;
            }
            let path = project
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            project.name.to_lowercase().contains(&query) || path.contains(&query)
        })
        .cloned()
        .collect()
}

fn search_row(
    search_input: &TextInputState,
    search_focused: bool,
    callbacks: TextInputCallbacks,
) -> impl IntoElement {
    div()
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .px(px(8.0))
        .py(px(5.0))
        .child(text_field_with_callbacks(
            search_input,
            search_focused,
            callbacks,
        ))
}

fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .px(px(8.0))
        .pt(px(6.0))
        .pb(px(3.0))
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_faint())
        .child(label.to_uppercase())
}

fn divider() -> impl IntoElement {
    div().my(px(3.0)).h(px(1.0)).bg(Colors::border_subtle())
}

enum RowAction {
    Command(String),
    OpenRecentPath,
}

fn project_row(
    index: usize,
    project: &ProjectSummary,
    active: bool,
    selected: bool,
    on_command: ProjectSwitcherCommandCb,
    action: RowAction,
) -> impl IntoElement {
    let command = match action {
        RowAction::Command(command) => command,
        RowAction::OpenRecentPath => project
            .path
            .as_ref()
            .map(|path| format!("{OPEN_RECENT_PATH_PREFIX}{}", path.to_string_lossy()))
            .unwrap_or_else(|| "project:open-recent".to_string()),
    };
    let disabled = active || command == "project:open-recent";
    let mut row = div()
        .id(("project-switcher-row", index))
        .h(px(ROW_HEIGHT))
        .rounded_md()
        .px(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .bg(if selected {
            Colors::with_alpha(Colors::accent_primary(), 0.16)
        } else {
            gpui::transparent_black().into()
        })
        .child(
            div()
                .w(px(16.0))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(if active {
                    svg()
                        .path(assets::ICON_CHECK_PATH)
                        .w(px(11.0))
                        .h(px(11.0))
                        .text_color(Colors::accent_primary())
                        .into_any_element()
                } else {
                    svg()
                        .path(assets::ICON_FILE_PATH)
                        .w(px(11.0))
                        .h(px(11.0))
                        .text_color(Colors::text_faint())
                        .into_any_element()
                }),
        )
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .flex()
                .flex_col()
                .items_start()
                .child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if disabled && !active {
                            Colors::text_disabled()
                        } else {
                            Colors::text_primary()
                        })
                        .child(project.name.clone()),
                )
                .child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_size(px(9.0))
                        .text_color(if project.subtitle == "Missing" {
                            Colors::status_warning()
                        } else {
                            Colors::text_faint()
                        })
                        .child(if project.subtitle.is_empty() {
                            project
                                .path
                                .as_ref()
                                .and_then(|path| path.parent())
                                .map(|path| path.to_string_lossy().to_string())
                                .unwrap_or_default()
                        } else {
                            project.subtitle.clone()
                        }),
                ),
        );

    if !disabled {
        row = row
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| s.bg(Colors::surface_control_hover()))
            .on_click(move |_, w, cx| on_command(&command, w, cx));
    }

    row
}

fn action_row(
    icon_path: &'static str,
    label: &'static str,
    command: &'static str,
    on_command: ProjectSwitcherCommandCb,
) -> impl IntoElement {
    let command = command.to_string();
    div()
        .id(label)
        .h(px(28.0))
        .rounded_md()
        .px(px(8.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_control_hover()))
        .on_click(move |_, w, cx| on_command(&command, w, cx))
        .child(
            svg()
                .path(icon_path)
                .w(px(13.0))
                .h(px(13.0))
                .text_color(Colors::text_muted()),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_secondary())
                .child(label),
        )
}

fn empty_recent_row(query: &str) -> impl IntoElement {
    let label = if query.is_empty() {
        "No Recent Projects".to_string()
    } else {
        format!("No projects match \"{}\"", query)
    };
    div()
        .px(px(8.0))
        .py(px(12.0))
        .text_align(gpui::TextAlign::Center)
        .text_size(px(11.0))
        .text_color(Colors::text_faint())
        .child(label)
}
