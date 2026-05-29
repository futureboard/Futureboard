//! PluginPicker — Phase 2b insert plugin selection overlay.
//!
//! A centered modal (same design language as `add_track_dialog`) that lists
//! the registry's insert-capable plugins, with a category filter rail and a
//! name/vendor search box. Picking a row appends a new insert slot to the
//! target track via the layout's existing `add_insert`/`set_insert_plugin`
//! mutations.
//!
//! No audio thread interaction — this is pure UI selection. The registry scan
//! that populates the list runs lazily on the UI thread (see
//! `StudioLayout::ensure_plugins_scanned`).

use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, svg, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window,
};

use crate::assets;
use crate::components::text_input::{text_field_with_callbacks, TextInputCallbacks, TextInputState};
use crate::theme::Colors;
use sphere_plugin_host::RegistryPlugin;

type VoidCb = Arc<dyn Fn(&(), &mut Window, &mut App) + 'static>;
type StringCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;

/// Special plugin id used to insert the documented stub effect when the
/// registry has no insert-capable plugin. Keeps the project round-trip
/// exercisable on a clean dev box. Mirrors the Phase 2a fallback id.
pub const STUB_PLUGIN_ID: &str = "futureboard.stub.gain";

/// Sentinel category meaning "no filter".
pub const CATEGORY_ALL: &str = "All";

#[derive(Debug, Clone)]
pub struct PluginPickerState {
    pub is_open: bool,
    /// Track that receives the new insert when a plugin is picked.
    pub track_id: String,
    /// Active category filter; `None` = all categories.
    pub category: Option<String>,
    /// Current search query (kept in sync with the search input).
    pub query: String,
}

impl PluginPickerState {
    pub fn closed() -> Self {
        Self {
            is_open: false,
            track_id: String::new(),
            category: None,
            query: String::new(),
        }
    }

    pub fn open_for(track_id: &str) -> Self {
        Self {
            is_open: true,
            track_id: track_id.to_string(),
            category: None,
            query: String::new(),
        }
    }
}

#[derive(Clone)]
pub struct PluginPickerCallbacks {
    /// Dismiss without inserting.
    pub on_close: VoidCb,
    /// Select a category filter. Empty string / `CATEGORY_ALL` clears it.
    pub on_select_category: StringCb,
    /// Pick a plugin by `RegistryPlugin.id` (or [`STUB_PLUGIN_ID`]).
    pub on_pick: StringCb,
}

/// Insert-capable subset of the registry, in display order.
pub fn insert_capable(plugins: &[RegistryPlugin]) -> Vec<&RegistryPlugin> {
    plugins.iter().filter(|p| p.supports_insert()).collect()
}

/// Unique, sorted category labels across the insert-capable plugins.
fn categories(plugins: &[&RegistryPlugin]) -> Vec<String> {
    let mut cats: Vec<String> = plugins
        .iter()
        .map(|p| p.display_category())
        .filter(|c| !c.is_empty())
        .collect();
    cats.sort();
    cats.dedup();
    cats
}

/// Apply the active category + query filter.
fn apply_filter<'a>(
    plugins: &[&'a RegistryPlugin],
    category: Option<&str>,
    query: &str,
) -> Vec<&'a RegistryPlugin> {
    let q = query.trim().to_lowercase();
    plugins
        .iter()
        .filter(|p| match category {
            Some(cat) => p.display_category() == cat,
            None => true,
        })
        .filter(|p| {
            if q.is_empty() {
                return true;
            }
            p.name.to_lowercase().contains(&q) || p.vendor.to_lowercase().contains(&q)
        })
        .copied()
        .collect()
}

fn icon(path: &'static str, size: f32, color: gpui::Rgba) -> impl IntoElement {
    svg().path(path).w(px(size)).h(px(size)).text_color(color)
}

fn category_pill(label: String, active: bool, value: String, cb: StringCb) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(24.0))
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
        .text_size(px(10.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(if active {
            Colors::text_primary()
        } else {
            Colors::text_muted()
        })
        .id(gpui::SharedString::from(format!("plugin-picker-cat-{value}")))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_hover()))
        .on_click(move |_, window, cx| cb(&value, window, cx))
        .child(label)
}

fn format_badge(label: &str) -> impl IntoElement {
    div()
        .rounded_sm()
        .px(px(4.0))
        .py(px(1.0))
        .bg(Colors::with_alpha(Colors::text_primary(), 0.06))
        .text_size(px(8.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(Colors::text_faint())
        .child(label.to_string())
}

fn plugin_row(index: usize, plugin: &RegistryPlugin, cb: StringCb) -> impl IntoElement {
    let id = plugin.id.clone();
    let name = plugin.name.clone();
    let vendor = plugin.vendor.clone();
    let category = plugin.display_category();
    let format = plugin.format.label();

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .h(px(40.0))
        .px(px(10.0))
        .rounded_md()
        .id(("plugin-picker-row", index))
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|s| s.bg(Colors::surface_hover()))
        .on_click(move |_, window, cx| cb(&id, window, cx))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(26.0))
                .h(px(26.0))
                .rounded_md()
                .border(px(1.0))
                .border_color(Colors::slot_border())
                .bg(Colors::surface_canvas())
                .child(icon(assets::ICON_CPU_PATH, 13.0, Colors::text_muted())),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(1.0))
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(Colors::text_primary())
                        .child(name),
                )
                .child(
                    div()
                        .text_size(px(9.0))
                        .text_color(Colors::text_faint())
                        .child(if vendor.is_empty() {
                            category.clone()
                        } else {
                            format!("{vendor} · {category}")
                        }),
                ),
        )
        .child(format_badge(format))
}

fn empty_state(has_plugins: bool, on_pick: StringCb) -> impl IntoElement {
    let message = if has_plugins {
        "No plugins match your filter."
    } else {
        "No scanned plugins found. Insert a stub effect to exercise the chain."
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(10.0))
        .py(px(28.0))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(Colors::text_faint())
                .child(message),
        )
        .when(!has_plugins, |this| {
            this.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .h(px(28.0))
                    .px(px(12.0))
                    .rounded_md()
                    .border(px(1.0))
                    .border_color(Colors::slot_border())
                    .bg(Colors::surface_input())
                    .text_size(px(11.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(Colors::text_muted())
                    .id("plugin-picker-stub")
                    .cursor(gpui::CursorStyle::PointingHand)
                    .hover(|s| s.bg(Colors::surface_hover()))
                    .on_click(move |_, window, cx| {
                        on_pick(&STUB_PLUGIN_ID.to_string(), window, cx)
                    })
                    .child(icon(assets::ICON_PLUS_PATH, 12.0, Colors::text_muted()))
                    .child("Insert Stub Effect"),
            )
        })
}

/// Full-screen modal overlay. Render last so it sits above the mixer.
pub fn plugin_picker_overlay(
    state: &PluginPickerState,
    plugins: &[RegistryPlugin],
    search_input: &TextInputState,
    search_focused: bool,
    search_callbacks: TextInputCallbacks,
    callbacks: PluginPickerCallbacks,
) -> impl IntoElement {
    let close_backdrop = callbacks.on_close.clone();
    let close_button = callbacks.on_close.clone();

    let capable = insert_capable(plugins);
    let has_plugins = !capable.is_empty();
    let cats = categories(&capable);
    let active_cat = state.category.as_deref();
    let filtered = apply_filter(&capable, active_cat, &state.query);

    // Category rail: "All" + each discovered category.
    let mut cat_rail = div().flex().flex_row().flex_wrap().gap(px(5.0)).child({
        let cb = callbacks.on_select_category.clone();
        category_pill(
            CATEGORY_ALL.to_string(),
            active_cat.is_none(),
            CATEGORY_ALL.to_string(),
            cb,
        )
    });
    for cat in &cats {
        let active = active_cat == Some(cat.as_str());
        let cb = callbacks.on_select_category.clone();
        cat_rail = cat_rail.child(category_pill(cat.clone(), active, cat.clone(), cb));
    }

    let list: gpui::AnyElement = if filtered.is_empty() {
        empty_state(has_plugins, callbacks.on_pick.clone()).into_any_element()
    } else {
        let mut col = div().flex().flex_col().gap(px(2.0));
        for (index, plugin) in filtered.iter().enumerate() {
            col = col.child(plugin_row(index, plugin, callbacks.on_pick.clone()));
        }
        col.into_any_element()
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
        .pt(px(64.0))
        .px(px(18.0))
        .pb(px(32.0))
        .id("plugin-picker-overlay")
        .bg(gpui::transparent_black())
        .occlude()
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            close_backdrop(&(), window, cx);
        })
        .child(
            div()
                .flex()
                .flex_col()
                .w(px(440.0))
                .max_w(px(440.0))
                .max_h(px(520.0))
                .overflow_hidden()
                .rounded_xl()
                .border(px(1.0))
                .border_color(Colors::border_default())
                .bg(Colors::surface_window())
                .shadow_xl()
                .on_mouse_down(gpui::MouseButton::Left, |_, _window, cx| {
                    cx.stop_propagation();
                })
                // Titlebar
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
                                .child(icon(
                                    assets::ICON_CPU_PATH,
                                    13.0,
                                    Colors::accent_primary(),
                                ))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(Colors::text_primary())
                                        .child("Add Insert"),
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
                                .id("plugin-picker-close")
                                .cursor(gpui::CursorStyle::PointingHand)
                                .hover(|s| s.bg(Colors::surface_control_hover()))
                                .on_click(move |_, window, cx| close_button(&(), window, cx))
                                .child(icon(assets::ICON_X_PATH, 13.0, Colors::text_faint())),
                        ),
                )
                // Search
                .child(
                    div()
                        .border_b(px(1.0))
                        .border_color(Colors::divider())
                        .px(px(10.0))
                        .py(px(8.0))
                        .child(text_field_with_callbacks(
                            search_input,
                            search_focused,
                            search_callbacks,
                        )),
                )
                // Category rail
                .child(
                    div()
                        .border_b(px(1.0))
                        .border_color(Colors::divider())
                        .px(px(10.0))
                        .py(px(8.0))
                        .child(cat_rail),
                )
                // List
                .child(
                    div()
                        .flex_1()
                        .min_h(px(0.0))
                        .id("plugin-picker-scroll")
                        .overflow_y_scroll()
                        .p(px(6.0))
                        .child(list),
                ),
        )
}
