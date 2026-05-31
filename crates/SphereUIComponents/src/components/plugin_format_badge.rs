//! Shared plug-in format badge — VST3/CLAP brand icons, text fallback for other formats.

use gpui::{div, px, svg, AnyElement, IntoElement, ParentElement, Styled};

use crate::assets;
use crate::theme::Colors;
use sphere_plugin_host::PluginFormat;

const FORMAT_ICON_SIZE: f32 = 22.0;

pub fn plugin_format_badge(format: PluginFormat) -> AnyElement {
    match format {
        PluginFormat::Vst3 => format_icon_badge(assets::ICON_PLUGIN_VST3_PATH),
        PluginFormat::Clap => format_icon_badge(assets::ICON_PLUGIN_CLAP_PATH),
        _ => text_format_badge(format),
    }
}

fn format_icon_badge(path: &'static str) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .w(px(FORMAT_ICON_SIZE))
        .h(px(FORMAT_ICON_SIZE))
        .child(
            svg()
                .path(path)
                .w(px(FORMAT_ICON_SIZE))
                .h(px(FORMAT_ICON_SIZE))
                .text_color(Colors::text_primary()),
        )
        .into_any_element()
}

fn text_format_badge(format: PluginFormat) -> AnyElement {
    let (fg, bg, border) = match format {
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
        .child(format.label())
        .into_any_element()
}
