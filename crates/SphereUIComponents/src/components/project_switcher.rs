use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, px, rgba, svg, App, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::theme::Colors;

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
    pub anchor_x: f32,
    pub query: String,
    pub selected_index: usize,
    pub current_project: ProjectSummary,
    pub recent_projects: Vec<ProjectSummary>,
}

impl Default for ProjectSwitcherState {
    fn default() -> Self {
        Self {
            is_open: false,
            anchor_x: 0.0,
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

const PANEL_WIDTH: f32 = 288.0;
const PANEL_MAX_HEIGHT: f32 = 430.0;
const EDGE_GAP: f32 = 8.0;
const TOP_OFFSET: f32 = 40.0;
const ROW_HEIGHT: f32 = 38.0;

pub fn project_switcher_popover(
    state: &ProjectSwitcherState,
    viewport_width: f32,
    _viewport_height: f32,
    on_command: ProjectSwitcherCommandCb,
    on_close: ProjectSwitcherCloseCb,
) -> impl IntoElement {
    let left = (state.anchor_x - 8.0).clamp(
        EDGE_GAP,
        (viewport_width - PANEL_WIDTH - EDGE_GAP).max(EDGE_GAP),
    );
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
        .child(panel(state, left, on_command))
}

fn panel_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: rgba(0x0000008C_u32).into(),
        offset: gpui::point(px(0.0), px(12.0)),
        blur_radius: px(40.0),
        spread_radius: px(0.0),
    }]
}

fn panel(
    state: &ProjectSwitcherState,
    left: f32,
    on_command: ProjectSwitcherCommandCb,
) -> impl IntoElement {
    let filtered = filtered_recent(state);
    let query = state.query.clone();
    let selected_index = state.selected_index;

    div()
        .absolute()
        .left(px(left))
        .top(px(TOP_OFFSET))
        .w(px(PANEL_WIDTH))
        .max_h(px(PANEL_MAX_HEIGHT))
        .rounded_lg()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_panel())
        .shadow(panel_shadow())
        .occlude()
        .flex()
        .flex_col()
        .child(header_row(state))
        .child(search_row(&query))
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
                    "project:switch-current",
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
                                "project:open-recent",
                            )
                            .into_any_element()
                        })
                        .collect()
                }),
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

fn header_row(state: &ProjectSwitcherState) -> impl IntoElement {
    let status = if state.current_project.is_dirty {
        "Unsaved"
    } else {
        "Saved"
    };
    div()
        .h(px(34.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .w(px(20.0))
                .h(px(20.0))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.0))
                .text_color(Colors::text_faint())
                .child("•••"),
        )
        .child(
            div()
                .min_w(px(0.0))
                .flex_1()
                .h(px(22.0))
                .rounded_md()
                .px(px(7.0))
                .flex()
                .items_center()
                .gap(px(5.0))
                .bg(Colors::surface_raised())
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child(state.current_project.name.clone()),
                )
                .child(
                    svg()
                        .path(assets::ICON_CHEVRON_DOWN_PATH)
                        .w(px(11.0))
                        .h(px(11.0))
                        .text_color(Colors::text_faint()),
                ),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_muted())
                .child(status),
        )
}

fn search_row(query: &str) -> impl IntoElement {
    div()
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .px(px(8.0))
        .py(px(6.0))
        .child(
            div()
                .h(px(25.0))
                .w_full()
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::border_subtle())
                .bg(Colors::surface_input())
                .px(px(8.0))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .w(px(9.0))
                        .h(px(9.0))
                        .rounded_full()
                        .border(px(1.0))
                        .border_color(Colors::text_faint()),
                )
                .child(
                    div()
                        .min_w(px(0.0))
                        .flex_1()
                        .truncate()
                        .text_size(px(11.0))
                        .text_color(if query.is_empty() {
                            Colors::text_faint()
                        } else {
                            Colors::text_primary()
                        })
                        .child(if query.is_empty() {
                            "Search projects...".to_string()
                        } else {
                            query.to_string()
                        }),
                )
                .children((!query.is_empty()).then(|| {
                    svg()
                        .path(assets::ICON_X_PATH)
                        .w(px(10.0))
                        .h(px(10.0))
                        .text_color(Colors::text_faint())
                })),
        )
}

fn section_label(label: &'static str) -> impl IntoElement {
    div()
        .px(px(8.0))
        .pt(px(6.0))
        .pb(px(2.0))
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_faint())
        .child(label.to_uppercase())
}

fn divider() -> impl IntoElement {
    div().my(px(4.0)).h(px(1.0)).bg(Colors::border_subtle())
}

fn project_row(
    index: usize,
    project: &ProjectSummary,
    active: bool,
    selected: bool,
    on_command: ProjectSwitcherCommandCb,
    command: &'static str,
) -> impl IntoElement {
    let command = command.to_string();
    let disabled = active;
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
            Colors::surface_hover()
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
                    div().into_any_element()
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
                        .text_color(Colors::text_primary())
                        .child(project.name.clone()),
                )
                .child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_size(px(9.0))
                        .text_color(Colors::text_faint())
                        .child(project.subtitle.clone()),
                ),
        );

    if !disabled {
        row = row
            .cursor(gpui::CursorStyle::PointingHand)
            .hover(|s| s.bg(Colors::surface_hover()))
            .on_click(move |_, w, cx| on_command(&command, w, cx));
    }

    row
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
