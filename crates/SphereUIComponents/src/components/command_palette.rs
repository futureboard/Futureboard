use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window,
};

use crate::components::text_input::{
    text_field_with_callbacks, TextInputCallbacks, TextInputState,
};
use crate::menu::{MenuItem, MenuItemKind, MenuManifest};
use crate::theme::Colors;

pub type CommandPaletteCommandCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;
pub type CommandPaletteCloseCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;

const PALETTE_W: f32 = 560.0;
const PALETTE_MAX_H: f32 = 440.0;
const ROW_H: f32 = 32.0;
const MAX_RESULTS: usize = 14;

#[derive(Debug, Clone, Default)]
pub struct CommandPaletteState {
    pub is_open: bool,
    pub query: String,
    pub selected_index: usize,
}

#[derive(Debug, Clone)]
pub struct CommandPaletteEntry {
    pub label: String,
    pub command: String,
    pub shortcut: Option<String>,
    pub path: String,
}

impl CommandPaletteState {
    pub fn open(&mut self) {
        self.is_open = true;
        self.query.clear();
        self.selected_index = 0;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.query.clear();
        self.selected_index = 0;
    }
}

pub fn command_palette_entries(query: &str) -> Vec<CommandPaletteEntry> {
    let query = query.trim().to_lowercase();
    let mut entries = Vec::new();
    for menu in &MenuManifest::load().menus {
        collect_menu_items(&menu.items, &menu.label, &query, &mut entries);
    }
    entries.truncate(MAX_RESULTS);
    entries
}

fn collect_menu_items(
    items: &[MenuItem],
    path: &str,
    query: &str,
    entries: &mut Vec<CommandPaletteEntry>,
) {
    for item in items {
        if !item.visible {
            continue;
        }
        match item.kind {
            MenuItemKind::Submenu => {
                if let Some(label) = item.label.as_deref() {
                    let next_path = format!("{path} / {label}");
                    collect_menu_items(&item.children, &next_path, query, entries);
                }
            }
            MenuItemKind::Normal | MenuItemKind::Checkbox | MenuItemKind::Radio => {
                if !item.enabled {
                    continue;
                }
                let Some(command) = item.command.as_ref() else {
                    continue;
                };
                let label = item.label.clone().unwrap_or_else(|| command.clone());
                let haystack = format!(
                    "{} {} {} {}",
                    label,
                    command,
                    path,
                    item.description.clone().unwrap_or_default()
                )
                .to_lowercase();
                if query.is_empty() || haystack.contains(query) {
                    entries.push(CommandPaletteEntry {
                        label,
                        command: command.clone(),
                        shortcut: item.shortcut.clone(),
                        path: path.to_string(),
                    });
                }
            }
            MenuItemKind::Separator => {}
        }
    }
}

pub fn command_palette_overlay(
    state: &CommandPaletteState,
    search_input: &TextInputState,
    search_focused: bool,
    search_callbacks: TextInputCallbacks,
    viewport_width: f32,
    viewport_height: f32,
    on_command: CommandPaletteCommandCb,
    on_close: CommandPaletteCloseCb,
) -> impl IntoElement {
    let entries = command_palette_entries(&state.query);
    let left = ((viewport_width - PALETTE_W) * 0.5).max(12.0);
    let width = PALETTE_W.min((viewport_width - 24.0).max(320.0));
    let top = (viewport_height * 0.16).clamp(36.0, 120.0);
    let close_click = on_close.clone();

    div()
        .absolute()
        .inset_0()
        .id("command-palette-overlay")
        .child(
            div()
                .absolute()
                .inset_0()
                .bg(Colors::with_alpha(Colors::surface_base(), 0.28))
                .on_mouse_down(gpui::MouseButton::Left, move |_, w, cx| {
                    close_click(&(), w, cx)
                }),
        )
        .child(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(width))
                .max_h(px(PALETTE_MAX_H))
                .flex()
                .flex_col()
                .rounded_lg()
                .border(px(1.0))
                .border_color(Colors::border_subtle())
                .bg(Colors::surface_panel())
                .shadow(vec![gpui::BoxShadow {
                    color: Colors::surface_overlay().into(),
                    offset: gpui::point(px(0.0), px(18.0)),
                    blur_radius: px(42.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .occlude()
                .child(
                    div()
                        .p(px(8.0))
                        .border_b(px(1.0))
                        .border_color(Colors::border_subtle())
                        .child(text_field_with_callbacks(
                            search_input,
                            search_focused,
                            search_callbacks,
                        )),
                )
                .child(result_list(entries, state.selected_index, on_command)),
        )
}

fn result_list(
    entries: Vec<CommandPaletteEntry>,
    selected_index: usize,
    on_command: CommandPaletteCommandCb,
) -> impl IntoElement {
    if entries.is_empty() {
        return div()
            .h(px(76.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.0))
            .text_color(Colors::text_faint())
            .child("No commands")
            .into_any_element();
    }

    div()
        .max_h(px(PALETTE_MAX_H - 48.0))
        .id("command-palette-results")
        .overflow_y_scroll()
        .p(px(5.0))
        .flex()
        .flex_col()
        .gap(px(1.0))
        .children(entries.into_iter().enumerate().map(|(index, entry)| {
            command_row(index, entry, index == selected_index, on_command.clone())
                .into_any_element()
        }))
        .into_any_element()
}

fn command_row(
    index: usize,
    entry: CommandPaletteEntry,
    selected: bool,
    on_command: CommandPaletteCommandCb,
) -> impl IntoElement {
    let command = entry.command.clone();
    div()
        .id(("command-palette-row", index))
        .h(px(ROW_H))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(8.0))
        .rounded_md()
        .bg(if selected {
            Colors::accent_muted()
        } else {
            gpui::transparent_black().into()
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_hover()))
        .on_click(move |_, w, cx| on_command(&command, w, cx))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(Colors::text_primary())
                        .child(entry.label),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(10.0))
                        .text_color(Colors::text_faint())
                        .child(entry.path),
                ),
        )
        .when_some(entry.shortcut, |row, shortcut| {
            row.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(10.0))
                    .text_color(Colors::text_muted())
                    .child(shortcut),
            )
        })
}
