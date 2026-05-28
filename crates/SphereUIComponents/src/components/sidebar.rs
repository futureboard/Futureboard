//! Browser sidebar — left dock of the studio shell.
//!
//! The panel renders a real filesystem-backed TreeView. Root sections mirror
//! the WebUI Browser categories, while expanded folders are read lazily from
//! disk through `file_browser.rs`.
//!
//! Visual direction: closer to an FL-Studio-style DAW asset browser — dense
//! rows, clear disclosure arrows, single-click expand on folders, light
//! depth-indent guides — wrapped in the Futureboard dark theme.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, px, svg, uniform_list, App, AppContext, Empty, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollHandle, StatefulInteractiveElement, Styled,
    UniformListScrollHandle, Window,
};

use crate::assets;
use crate::components::file_browser::{BrowserNodeKind, BrowserVisibleNode, FileBrowserState};
use crate::components::text_input::{text_field_with_callbacks, TextInputCallbacks, TextInputState, TextInputContextCb};
use crate::theme::Colors;

pub const SIDEBAR_WIDTH: f32 = 272.0;
/// Compact row height — FL-like density without losing readability.
const TREE_ROW_HEIGHT: f32 = 22.0;
/// Per-depth indent. Smaller than a generic file explorer so deep trees
/// still fit the 272 px sidebar.
const TREE_INDENT: f32 = 12.0;
/// Width of the left padding before the disclosure arrow at depth 0.
const TREE_LEFT_PAD: f32 = 6.0;
/// Width of the disclosure arrow column. Keeps icons aligned across rows
/// whether or not the row is expandable.
const DISCLOSURE_W: f32 = 12.0;

pub type ActivateFileCb = Arc<dyn Fn(&PathBuf, &mut Window, &mut App) + 'static>;
pub type SelectEntryCb = Arc<dyn Fn(&PathBuf, &mut Window, &mut App) + 'static>;
pub type ToggleNodeCb = Arc<dyn Fn(&(String, Option<PathBuf>), &mut Window, &mut App) + 'static>;
pub type BrowserContextCb =
    Arc<dyn Fn(&(Option<PathBuf>, f32, f32), &mut Window, &mut App) + 'static>;

#[derive(Clone, Debug)]
pub struct BrowserDragItem {
    pub path: PathBuf,
    pub label: String,
}

pub struct BrowserDragPreview {
    label: String,
}

impl Render for BrowserDragPreview {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .py(px(5.0))
            .rounded_md()
            .border(px(1.0))
            .border_color(Colors::border_subtle())
            .bg(Colors::surface_raised())
            .shadow_lg()
            .child(
                svg()
                    .path(assets::ICON_FILE_PATH)
                    .w(px(12.0))
                    .h(px(12.0))
                    .text_color(Colors::status_success()),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(Colors::text_primary())
                    .child(self.label.clone()),
            )
    }
}

pub fn sidebar(
    state: &FileBrowserState,
    scroll: UniformListScrollHandle,
    search_input: &TextInputState,
    search_focused: bool,
    on_search_context_menu: TextInputContextCb,
    on_toggle: ToggleNodeCb,
    on_select: SelectEntryCb,
    on_activate_file: ActivateFileCb,
    on_context_menu: BrowserContextCb,
) -> impl IntoElement {
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .py(px(8.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .text_color(Colors::text_primary())
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::BOLD)
                .child("BROWSER"),
        )
        .child(
            div()
                .text_size(px(9.0))
                .text_color(Colors::text_faint())
                .child(format!("{} items", state.visible_node_count())),
        );

    // Search bar above content
    let search_callbacks = TextInputCallbacks {
        on_context_menu: Some(on_search_context_menu),
        on_mouse: None,
    };
    let search_container = div()
        .px(px(8.0))
        .py(px(5.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_panel())
        .child(
            text_field_with_callbacks(
                search_input,
                search_focused,
                search_callbacks,
            )
        );

    let selected_label = state
        .selected
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "No file selected".to_string());

    let path_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .py(px(4.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_input())
        .child(
            div()
                .text_size(px(8.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_faint())
                .child("SEL"),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .truncate()
                .text_size(px(10.0))
                .text_color(Colors::text_muted())
                .child(truncate_path(&selected_label, 42)),
        );

    // ── Row virtualization ──────────────────────────────────────────
    let nodes = Arc::new(state.visible_nodes());
    let count = nodes.len();
    crate::perf::count("browser_rows", count as u64);
    let scroll_for_thumb = scroll.0.borrow().base_handle.clone();
    let on_toggle_l = on_toggle.clone();
    let on_select_l = on_select.clone();
    let on_activate_l = on_activate_file.clone();
    let on_context_l = on_context_menu.clone();
    let nodes_for_list = nodes.clone();

    let listing_scroll = uniform_list("browser-tree", count, move |range, _window, _cx| {
        let nodes = nodes_for_list.clone();
        let on_toggle = on_toggle_l.clone();
        let on_select = on_select_l.clone();
        let on_activate = on_activate_l.clone();
        let on_context = on_context_l.clone();
        range
            .map(|i| {
                tree_row(
                    i,
                    &nodes[i],
                    on_toggle.clone(),
                    on_select.clone(),
                    on_activate.clone(),
                    on_context.clone(),
                )
                .into_any_element()
            })
            .collect::<Vec<_>>()
    })
    .track_scroll(scroll)
    .size_full()
    .px(px(2.0))
    .py(px(3.0));

    // Custom scrollbar thumb
    let thumb = scrollbar_thumb(scroll_for_thumb);

    let listing = div()
        .flex_1()
        .min_h_0()
        .relative()
        .child(listing_scroll)
        .child(thumb);

    div()
        .flex()
        .flex_col()
        .w(px(SIDEBAR_WIDTH))
        .h_full()
        .bg(Colors::surface_panel())
        .border_r(px(1.0))
        .border_color(Colors::border_subtle())
        .child(header)
        .child(search_container)
        .child(path_row)
        .child(listing)
}

fn tree_row(
    index: usize,
    node: &BrowserVisibleNode,
    on_toggle: ToggleNodeCb,
    on_select: SelectEntryCb,
    on_activate_file: ActivateFileCb,
    on_context_menu: BrowserContextCb,
) -> impl IntoElement {
    let id = node.id.clone();
    let path = node.path.clone();
    let path_for_select = node.path.clone();
    let path_for_activate = node.path.clone();
    let path_for_toggle = node.path.clone();
    let path_for_context = node.path.clone();
    let label = node.label.clone();
    let expandable = node.expandable;
    let expanded = node.expanded;
    let selected = node.selected;
    let is_folder = node.kind == BrowserNodeKind::Folder;
    let is_file = node.kind == BrowserNodeKind::File;
    let is_audio = node.is_audio();
    let is_midi = node.is_midi();
    let depth = node.depth as f32;
    let extension = node.extension.clone();

    // Uniform row height
    let row_height = TREE_ROW_HEIGHT;

    // Sections and project folders paint a background differently
    let bg = if selected {
        Colors::accent_soft()
    } else {
        gpui::transparent_black().into()
    };

    let text_color = if selected {
        Colors::text_primary()
    } else if is_folder {
        Colors::text_secondary()
    } else if is_audio || is_midi {
        Colors::text_muted()
    } else {
        Colors::text_faint()
    };

    let icon_path = if is_folder && depth == 0.0 {
        match label.as_str() {
            "Audio Files" => assets::ICON_MUSIC_PATH,
            "Plug-ins" => assets::ICON_PLUG_PATH,
            "Instruments" => assets::ICON_CPU_PATH,
            "Projects" => assets::ICON_SAVE_PATH,
            "Samples" => assets::ICON_SHARE_PATH,
            "User Library" => assets::ICON_FOLDER_PATH,
            s if s.starts_with("PROJECT:") => assets::ICON_FOLDER_PATH,
            _ => assets::ICON_FOLDER_PATH,
        }
    } else if is_folder {
        if expanded {
            assets::ICON_FOLDER_OPEN_PATH
        } else {
            assets::ICON_FOLDER_PATH
        }
    } else {
        if is_audio {
            assets::ICON_MUSIC_PATH
        } else if is_midi {
            assets::ICON_FILE_PATH
        } else if extension == "fbproj" {
            assets::ICON_SAVE_PATH
        } else if extension == "vst3" {
            assets::ICON_PLUG_PATH
        } else {
            assets::ICON_FILE_PATH
        }
    };

    let icon_color = if selected {
        Colors::accent_primary()
    } else if is_folder {
        Colors::text_muted()
    } else if is_audio {
        Colors::status_success()
    } else if is_midi {
        Colors::status_warning()
    } else if extension == "fbproj" {
        Colors::accent_primary()
    } else if extension == "vst3" {
        Colors::status_warning()
    } else {
        Colors::text_faint()
    };

    // Depth indent guides
    let mut indent_guides = Vec::new();
    if depth > 0.0 {
        for level in 0..(node.depth) {
            let x = TREE_LEFT_PAD + (level as f32) * TREE_INDENT + DISCLOSURE_W * 0.5;
            indent_guides.push(
                div()
                    .absolute()
                    .top(px(0.0))
                    .bottom(px(0.0))
                    .left(px(x))
                    .w(px(1.0))
                    .bg(Colors::divider()),
            );
        }
    }

    // Disclosure cell
    let disclosure = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(DISCLOSURE_W))
        .h_full()
        .child(disclosure_icon(expandable, expanded));

    let row_label_size = if depth == 0.0 { 10.0 } else { 11.0 };
    let label_weight = if depth == 0.0 {
        gpui::FontWeight::BOLD
    } else if is_folder {
        gpui::FontWeight::MEDIUM
    } else {
        gpui::FontWeight::NORMAL
    };

    // Section labels render uppercased
    let display_label = if depth == 0.0 {
        label.to_uppercase()
    } else {
        label.clone()
    };

    let mut row = div()
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .h(px(row_height))
        .w_full()
        .gap(px(3.0))
        .pl(px(TREE_LEFT_PAD + depth * TREE_INDENT))
        .pr(px(6.0))
        .bg(bg)
        .id(("browser-tree-row", index))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_hover()))
        .children(indent_guides)
        .child(if selected {
            div()
                .absolute()
                .left(px(0.0))
                .top(px(3.0))
                .bottom(px(3.0))
                .w(px(2.0))
                .bg(Colors::accent_primary())
                .into_any_element()
        } else {
            Empty.into_any_element()
        })
        .child(disclosure)
        .child(
            svg()
                .path(icon_path)
                .w(px(12.0))
                .h(px(12.0))
                .text_color(icon_color),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .truncate()
                .text_size(px(row_label_size))
                .font_weight(label_weight)
                .text_color(text_color)
                .child(display_label),
        )
        .children(node.error.as_ref().map(|_| {
            div()
                .text_size(px(9.0))
                .text_color(Colors::status_error())
                .child("unavailable")
        }));

    // Clicks and Toggles
    let toggle_for_click = on_toggle.clone();
    let toggle_id = id.clone();
    let toggle_path = path_for_toggle.clone();
    let select_path = path_for_select.clone();
    row = row.on_click(move |event, w, cx| {
        if let Some(p) = select_path.as_ref() {
            on_select(p, w, cx);
        }
        if expandable {
            toggle_for_click(&(toggle_id.clone(), toggle_path.clone()), w, cx);
        } else if is_file && event.click_count() >= 2 {
            if let Some(p) = path_for_activate.as_ref() {
                on_activate_file(p, w, cx);
            }
        }
    });

    row = row.on_mouse_down(
        gpui::MouseButton::Right,
        move |event: &gpui::MouseDownEvent, window, cx| {
            let x: f32 = event.position.x.into();
            let y: f32 = event.position.y.into();
            on_context_menu(&(path_for_context.clone(), x, y), window, cx);
        },
    );

    if is_audio {
        let drag_label = label.clone();
        if let Some(path) = path {
            row = row.on_drag(
                BrowserDragItem {
                    path,
                    label: drag_label,
                },
                |drag, _offset, _window, cx| {
                    cx.new(|_| BrowserDragPreview {
                        label: drag.label.clone(),
                    })
                },
            );
        }
    }

    row
}

fn scrollbar_thumb(scroll: ScrollHandle) -> impl IntoElement {
    let viewport_h: f32 = scroll.bounds().size.height.into();
    let max_y: f32 = scroll.max_offset().height.into();
    let raw_y: f32 = scroll.offset().y.into();
    let offset_y: f32 = -raw_y;

    if viewport_h <= 0.0 || max_y <= 0.5 {
        return div().w(px(0.0)).h(px(0.0)).into_any_element();
    }

    let content_h = viewport_h + max_y;
    let min_thumb = 24.0_f32;
    let thumb_h = ((viewport_h / content_h) * viewport_h).max(min_thumb);
    let track_room = (viewport_h - thumb_h).max(0.0);
    let progress = (offset_y / max_y).clamp(0.0, 1.0);
    let thumb_top = progress * track_room;

    div()
        .absolute()
        .top(px(thumb_top))
        .right(px(2.0))
        .w(px(4.0))
        .h(px(thumb_h))
        .rounded_full()
        .bg(Colors::with_alpha(Colors::text_primary(), 0.2))
        .into_any_element()
}

fn disclosure_icon(expandable: bool, expanded: bool) -> impl IntoElement {
    if expandable {
        let icon_path = if expanded {
            assets::ICON_CHEVRON_DOWN_PATH
        } else {
            assets::ICON_CHEVRON_RIGHT_PATH
        };
        svg()
            .path(icon_path)
            .w(px(9.0))
            .h(px(9.0))
            .text_color(Colors::text_muted())
            .into_any_element()
    } else {
        div().w(px(9.0)).h(px(9.0)).into_any_element()
    }
}

fn truncate_path(s: &str, max: usize) -> String {
    let max = max.max(4);
    let char_len = s.chars().count();
    if char_len <= max {
        return s.to_string();
    }

    // Take the last `max - 3` chars (leave room for "...") without slicing in the middle
    // of a UTF-8 codepoint.
    let keep = max.saturating_sub(3);
    let start = char_len.saturating_sub(keep);
    let tail: String = s.chars().skip(start).collect();
    format!("...{tail}")
}
