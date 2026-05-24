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
    // GPUI's `uniform_list` only constructs elements for the visible
    // index range, then lays them out at a fixed item height. With
    // hundreds of expanded browser nodes, this is the difference
    // between O(N) per-frame allocation/layout and O(visible_rows).
    //
    // The render closure is `'static + Fn`, so `nodes` and every
    // callback must be `'static`. We share them via `Arc` clones —
    // `Arc::clone` is one atomic increment, far cheaper than
    // re-allocating each row each frame.
    let nodes = Arc::new(state.visible_nodes());
    let count = nodes.len();
    crate::perf::count("browser_rows", count as u64);
    let scroll_for_thumb = scroll.0.borrow().base_handle.clone();
    let on_toggle_l = on_toggle.clone();
    let on_select_l = on_select.clone();
    let on_activate_l = on_activate_file.clone();
    let on_context_l = on_context_menu.clone();
    let nodes_for_list = nodes.clone();

    let listing_scroll = uniform_list(
        "browser-tree",
        count,
        move |range, _window, _cx| {
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
        },
    )
    .track_scroll(&scroll)
    .size_full()
    .px(px(2.0))
    .py(px(3.0));

    // Custom scrollbar thumb. Reads through the uniform list's
    // base scroll handle so it tracks the same offset GPUI is using
    // internally.
    let thumb = scrollbar_thumb(scroll_for_thumb);

    let listing = div()
        .flex_1()
        .min_h_0()
        .relative()
        .child(listing_scroll)
        .child(thumb);

    // Per-directory errors render as inline rows inside the tree
    // (`placeholder_row` in file_browser.rs). No top-level error banner
    // is needed now that the browser has no single "current_dir".
    let error_banner: Option<gpui::AnyElement> = None;

    div()
        .flex()
        .flex_col()
        .w(px(SIDEBAR_WIDTH))
        .h_full()
        .bg(Colors::surface_panel())
        .border_r(px(1.0))
        .border_color(Colors::border_subtle())
        .child(header)
        .child(path_row)
        .children(error_banner)
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
    let is_section = node.kind == BrowserNodeKind::Section;
    // Sections are *categories*, not filesystem folders. Keep the two
    // concepts visually and behaviorally separate.
    let is_folder = node.kind == BrowserNodeKind::Folder;
    let is_file = node.kind == BrowserNodeKind::File;
    let is_audio = node.is_audio();
    let is_midi = node.is_midi();
    let depth = node.depth as f32;

    // Uniform row height across all kinds so row boundaries, indent
    // guides, and the selection bar line up regardless of node type.
    let row_height = TREE_ROW_HEIGHT;

    // Sections never paint a selection background — they are toggle-only
    // category headers, not selectable assets. This avoids the "merged
    // / wrong highlight" effect when clicking a section.
    let bg = if selected && !is_section {
        Colors::accent_soft()
    } else if is_section {
        gpui::rgba(0xFFFFFF06).into()
    } else {
        gpui::transparent_black().into()
    };

    let text_color = if selected && !is_section {
        Colors::text_primary()
    } else if is_section {
        Colors::text_secondary()
    } else if is_audio || is_midi || is_folder {
        Colors::text_muted()
    } else {
        Colors::text_faint()
    };

    let icon_path = if is_section || is_folder {
        assets::ICON_FOLDER_PATH
    } else {
        assets::ICON_FILE_PATH
    };

    let icon_color = if is_section {
        Colors::accent_primary()
    } else if selected {
        Colors::accent_primary()
    } else if is_folder {
        Colors::text_muted()
    } else if is_audio {
        Colors::status_success()
    } else if is_midi {
        Colors::status_warning()
    } else {
        Colors::text_faint()
    };

    // Depth indent guides — thin vertical bars at each parent level, so the
    // user can scan the hierarchy at a glance. Skipped for section/depth-0
    // rows (they own the visual heading).
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
                    .bg(gpui::rgba(0xFFFFFF0A)),
            );
        }
    }

    // Disclosure cell is rendered for every row so file and folder labels
    // line up. Only expandable rows get a visible arrow.
    let disclosure = div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(DISCLOSURE_W))
        .h_full()
        .child(disclosure_icon(expandable, expanded));

    let row_label_size = if is_section { 10.0 } else { 11.0 };
    let label_weight = if is_section {
        gpui::FontWeight::BOLD
    } else if is_folder {
        gpui::FontWeight::MEDIUM
    } else {
        gpui::FontWeight::NORMAL
    };

    // Section labels render uppercased to match DAW tree conventions; file /
    // folder labels keep their original casing.
    let display_label = if is_section {
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
        .child(if selected && !is_section {
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

    // Click semantics, by node kind:
    //   - Section: toggle expand only. Never mutates `state.selected` —
    //     a category header is not a selectable asset, and selecting it
    //     would paint the section row with `accent_soft` on top of its
    //     existing band, producing the "merged highlight" artifact.
    //   - Folder: select + toggle expand on a single click (FL-style).
    //   - File:   select on single click; double-click on audio/MIDI
    //             imports it onto the timeline.
    let toggle_for_click = on_toggle.clone();
    let toggle_id = id.clone();
    let toggle_path = path_for_toggle.clone();
    let select_path = path_for_select.clone();
    row = row.on_click(move |event, w, cx| {
        if is_section {
            if expandable {
                toggle_for_click(&(toggle_id.clone(), toggle_path.clone()), w, cx);
            }
            return;
        }
        if let Some(p) = select_path.as_ref() {
            on_select(p, w, cx);
        }
        if expandable {
            toggle_for_click(&(toggle_id.clone(), toggle_path.clone()), w, cx);
        } else if is_file && event.click_count() >= 2 && (is_audio || is_midi) {
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

/// Render a subtle vertical scrollbar thumb on the right edge of the
/// scroll viewport. The thumb is sized as `(viewport_h / content_h)` and
/// positioned as `(-offset_y / content_h)` — reading the values that
/// `track_scroll` populated during the previous paint. When the content
/// fits, the thumb is hidden.
fn scrollbar_thumb(scroll: ScrollHandle) -> impl IntoElement {
    let viewport_h: f32 = scroll.bounds().size.height.into();
    let max_y: f32 = scroll.max_offset().y.into();
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
        .bg(gpui::rgba(0xFFFFFF33))
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
    if s.len() <= max {
        s.to_string()
    } else {
        let tail = &s[s.len().saturating_sub(max - 1)..];
        format!("...{}", tail)
    }
}
