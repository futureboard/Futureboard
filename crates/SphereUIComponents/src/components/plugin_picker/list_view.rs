//! Plugin list row rendering.

use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, svg, App, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window,
};

use crate::assets;
use crate::components::plugin_picker::category::normalized_category_label;
use crate::components::plugin_picker::insert::is_insertable;
use crate::theme::Colors;
use sphere_plugin_host::{PluginFormat, PluginKind, PluginScanStatus, PluginStatus, RegistryPlugin};

pub const ROW_HEIGHT: f32 = 38.0;

type StringCb = Arc<dyn Fn(&String, &mut Window, &mut App) + 'static>;

pub fn plugin_row(
    list_index: usize,
    plugin: &RegistryPlugin,
    highlighted: bool,
    favorite: bool,
    on_select: StringCb,
    on_pick: StringCb,
    on_toggle_favorite: StringCb,
) -> impl IntoElement {
    let id_select = plugin.id.clone();
    let id_pick = plugin.id.clone();
    let id_fav = plugin.id.clone();
    let name = plugin.name.clone();
    let vendor = plugin.vendor.clone();
    let category = normalized_category_label(plugin);
    let fmt = plugin.format;
    let insertable = is_insertable(plugin);
    let (kind_icon, kind_color) = kind_icon_for(plugin.kind);
    let status = scan_status_label(plugin);

    div()
        .id(("plugin-picker-row", list_index))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(ROW_HEIGHT))
        .px(px(10.0))
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .when(highlighted, |el| {
            el.bg(Colors::accent_muted())
                .border_l(px(2.0))
                .border_color(Colors::accent_primary())
        })
        .when(!highlighted, |el| el.hover(|s| s.bg(Colors::surface_hover())))
        .when(!insertable, |el| el.opacity(0.55))
        .cursor(if insertable {
            gpui::CursorStyle::PointingHand
        } else {
            gpui::CursorStyle::Arrow
        })
        .on_click(move |event, window, cx| {
            if event.click_count() >= 2 {
                if insertable {
                    on_pick(&id_pick, window, cx);
                }
            } else {
                on_select(&id_select, window, cx);
            }
        })
        .child(icon(kind_icon, 12.0, kind_color))
        .child(
            div()
                .w(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .cursor(gpui::CursorStyle::PointingHand)
                .child(icon(
                    assets::ICON_STAR_PATH,
                    11.0,
                    if favorite {
                        Colors::status_warning()
                    } else {
                        Colors::text_faint()
                    },
                ))
                .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                    on_toggle_favorite(&id_fav, window, cx);
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(11.0))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(Colors::text_primary())
                .truncate()
                .child(name),
        )
        .child(
            div()
                .w(px(128.0))
                .text_size(px(10.5))
                .text_color(Colors::text_dim())
                .truncate()
                .child(vendor),
        )
        .child(
            div()
                .w(px(96.0))
                .text_size(px(10.5))
                .text_color(Colors::text_dim())
                .truncate()
                .child(category),
        )
        .child(format_badge(fmt))
        .when_some(status, |el, label| el.child(status_badge(label, true)))
}

fn kind_icon_for(kind: PluginKind) -> (&'static str, gpui::Rgba) {
    match kind {
        PluginKind::Instrument => (assets::ICON_MUSIC_PATH, Colors::accent_primary()),
        PluginKind::Effect => (assets::ICON_SLIDERS_HORIZONTAL_PATH, Colors::status_success()),
    }
}

fn icon(path: &'static str, size: f32, color: gpui::Rgba) -> impl IntoElement {
    svg().path(path).w(px(size)).h(px(size)).text_color(color)
}

pub fn format_badge(fmt: PluginFormat) -> impl IntoElement {
    let (fg, bg, border) = match fmt {
        PluginFormat::Vst3 => (
            Colors::accent_primary(),
            Colors::accent_muted(),
            Colors::border_accent(),
        ),
        PluginFormat::Clap => (
            Colors::status_success(),
            gpui::rgba(0x6FCF9720),
            Colors::status_success(),
        ),
        PluginFormat::Au => (
            Colors::status_warning(),
            gpui::rgba(0xE5C07B18),
            Colors::status_warning(),
        ),
        _ => (
            Colors::text_faint(),
            Colors::surface_input(),
            Colors::border_subtle(),
        ),
    };
    div()
        .px(px(5.0))
        .py(px(1.0))
        .rounded_sm()
        .border(px(1.0))
        .border_color(border)
        .bg(bg)
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(fmt.label())
}

fn status_badge(label: &'static str, tone_warn: bool) -> impl IntoElement {
    let (fg, bg) = if tone_warn {
        (Colors::status_warning(), gpui::rgba(0xE5C07B14))
    } else {
        (Colors::status_success(), gpui::rgba(0x6FCF9714))
    };
    div()
        .px(px(5.0))
        .py(px(1.0))
        .rounded_sm()
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(bg)
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(label)
}

pub fn scan_status_label(plugin: &RegistryPlugin) -> Option<&'static str> {
    match plugin.scan_status {
        PluginScanStatus::Crashed => Some("Crashed"),
        PluginScanStatus::Failed | PluginScanStatus::MetadataOnly => Some("Failed"),
        PluginScanStatus::Skipped | PluginScanStatus::Disabled => Some("Disabled"),
        PluginScanStatus::Success | PluginScanStatus::Ok => {
            if plugin.status == PluginStatus::MissingPreset {
                Some("Missing")
            } else if !plugin.supports_insert() {
                Some("Unsupported")
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn skeleton_row(index: usize) -> impl IntoElement {
    let block = |w: f32, alpha: f32| {
        div()
            .h(px(10.0))
            .w(px(w))
            .rounded_sm()
            .bg(Colors::with_alpha(Colors::text_primary(), alpha))
    };
    let alpha = 0.06 + ((index % 3) as f32) * 0.015;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(ROW_HEIGHT))
        .px(px(10.0))
        .border_b(px(1.0))
        .border_color(Colors::divider())
        .child(
            div()
                .w(px(12.0))
                .h(px(12.0))
                .rounded_sm()
                .bg(Colors::with_alpha(Colors::text_primary(), alpha)),
        )
        .child(div().flex_1().min_w(px(0.0)).child(block(140.0 + (index % 4) as f32 * 20.0, alpha)))
        .child(div().w(px(128.0)).child(block(110.0, alpha)))
        .child(div().w(px(96.0)).child(block(80.0, alpha)))
        .child(div().w(px(54.0)).child(block(36.0, alpha)))
}

pub fn skeleton_body() -> impl IntoElement {
    let mut col = div().flex().flex_col();
    for i in 0..14 {
        col = col.child(skeleton_row(i));
    }
    col
}
