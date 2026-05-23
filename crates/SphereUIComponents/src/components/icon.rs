use gpui::{svg, IntoElement, ParentElement, Pixels, Styled, Rgba};

/// Renders an embedded SVG icon at the specified path and size.
pub fn icon(path: &'static str, size: Pixels, color: Rgba) -> impl IntoElement {
    svg()
        .path(path)
        .w(size)
        .h(size)
        .text_color(color)
}
