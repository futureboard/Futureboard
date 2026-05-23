//! Browser sidebar — left dock of the studio shell.
//!
//! Pure presentation: receives a [`FileBrowserState`] from the layout and a
//! pair of callbacks (navigate / activate file) and renders the current
//! directory listing. Filesystem reads happen in `file_browser.rs`, not here.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    div, px, svg, App, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window,
};

use crate::assets;
use crate::components::file_browser::{FileBrowserEntry, FileBrowserState, FileEntryKind};
use crate::theme::Colors;

/// Static placeholder categories shown above the live directory listing.
/// They're not file-backed yet — they exist so the panel matches the
/// Electron browser shell while real category logic is wired up later.
const CATEGORIES: &[&str] = &[
    "Audio Files",
    "Plug-ins (VST3/CLAP)",
    "Instruments",
    "Projects",
    "Samples",
    "User Library",
];

pub type NavigateCb = Arc<dyn Fn(&PathBuf, &mut Window, &mut App) + 'static>;
pub type ActivateFileCb = Arc<dyn Fn(&PathBuf, &mut Window, &mut App) + 'static>;
pub type SelectEntryCb = Arc<dyn Fn(&PathBuf, &mut Window, &mut App) + 'static>;

pub fn sidebar(
    state: &FileBrowserState,
    on_navigate: NavigateCb,
    on_select: SelectEntryCb,
    on_activate_file: ActivateFileCb,
    on_up: Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>,
) -> impl IntoElement {
    let header = div()
        .px(px(10.0))
        .py(px(8.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .text_color(Colors::text_primary())
                .text_xs()
                .font_weight(gpui::FontWeight::BOLD)
                .child("Browser"),
        );

    let path_label = state
        .current_dir
        .to_string_lossy()
        .into_owned();

    let path_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .py(px(5.0))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle())
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(20.0))
                .h(px(20.0))
                .rounded_sm()
                .bg(Colors::surface_raised())
                .border(px(1.0))
                .border_color(Colors::border_subtle())
                .cursor(gpui::CursorStyle::PointingHand)
                .id("browser-up-btn")
                .hover(|s| s.bg(Colors::surface_hover()))
                .on_mouse_down(gpui::MouseButton::Left, {
                    let cb = on_up.clone();
                    move |_, w, cx| cb(&(), w, cx)
                })
                .child(
                    svg()
                        .path(assets::ICON_MINUS_PATH)
                        .w(px(10.0))
                        .h(px(10.0))
                        .text_color(Colors::text_secondary()),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(10.0))
                .text_color(Colors::text_muted())
                .child(truncate_path(&path_label, 40)),
        );

    let categories = div()
        .flex_col()
        .px(px(6.0))
        .py(px(4.0))
        .gap_px()
        .children(CATEGORIES.iter().map(|c| category_row(c)))
        .border_b(px(1.0))
        .border_color(Colors::border_subtle());

    let entries: Vec<gpui::AnyElement> = state
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            entry_row(
                i,
                entry,
                state.selected.as_deref() == Some(&entry.path),
                on_navigate.clone(),
                on_select.clone(),
                on_activate_file.clone(),
            )
            .into_any_element()
        })
        .collect();

    let listing = div()
        .flex_1()
        .min_h_0()
        .id("browser-listing")
        .overflow_y_scroll()
        .flex_col()
        .px(px(4.0))
        .py(px(4.0))
        .gap_px()
        .children(entries);

    let error_banner = state.error.as_ref().map(|e| {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .text_size(px(9.0))
            .text_color(Colors::status_error())
            .child(format!("Error: {}", e))
    });

    div()
        .flex()
        .flex_col()
        .w(px(272.0))
        .h_full()
        .bg(Colors::surface_panel())
        .border_r(px(1.0))
        .border_color(Colors::border_subtle())
        .child(header)
        .child(path_row)
        .child(categories)
        .children(error_banner)
        .child(listing)
}

fn category_row(label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(6.0))
        .py(px(3.0))
        .rounded_sm()
        .child(
            svg()
                .path(assets::ICON_FOLDER_PATH)
                .w(px(12.0))
                .h(px(12.0))
                .text_color(Colors::text_faint()),
        )
        .child(
            div()
                .text_color(Colors::text_muted())
                .text_size(px(10.0))
                .child(label),
        )
}

fn entry_row(
    index: usize,
    entry: &FileBrowserEntry,
    selected: bool,
    on_navigate: NavigateCb,
    on_select: SelectEntryCb,
    on_activate_file: ActivateFileCb,
) -> impl IntoElement {
    let is_folder = entry.kind == FileEntryKind::Folder;
    let is_audio = entry.is_audio();
    let is_midi = entry.is_midi();
    let path_navigate = entry.path.clone();
    let path_activate = entry.path.clone();
    let path_select = entry.path.clone();

    let icon_path = if is_folder {
        assets::ICON_FOLDER_PATH
    } else {
        assets::ICON_FILE_PATH
    };

    let icon_color = if is_folder {
        Colors::accent_primary()
    } else if is_audio {
        Colors::status_success()
    } else if is_midi {
        Colors::status_warning()
    } else {
        Colors::text_faint()
    };

    let text_color = if selected {
        Colors::text_primary()
    } else if is_folder || is_audio || is_midi {
        Colors::text_secondary()
    } else {
        Colors::text_muted()
    };

    let bg = if selected {
        Colors::surface_hover()
    } else {
        gpui::transparent_black().into()
    };

    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(6.0))
        .py(px(3.0))
        .rounded_sm()
        .bg(bg)
        .id(("browser-entry", index))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_hover()))
        .child(
            svg()
                .path(icon_path)
                .w(px(13.0))
                .h(px(13.0))
                .text_color(icon_color),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(10.5))
                .text_color(text_color)
                .child(entry.name.clone()),
        );

    // Single click selects (folders also select, but the click handler below
    // navigates immediately so selection is harmless there).
    row = row.on_mouse_down(gpui::MouseButton::Left, move |_, w, cx| {
        on_select(&path_select, w, cx);
    });

    // Click-to-navigate for folders, double-click-to-import for audio.
    row = row.on_click(move |event, w, cx| {
        if is_folder {
            on_navigate(&path_navigate, w, cx);
        } else if event.click_count() >= 2 && (is_audio || is_midi) {
            on_activate_file(&path_activate, w, cx);
        }
    });

    row
}

fn truncate_path(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let tail = &s[s.len().saturating_sub(max - 1)..];
        format!("…{}", tail)
    }
}
