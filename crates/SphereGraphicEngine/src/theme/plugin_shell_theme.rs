//! Generic window-chrome theme: colors, metrics, and font policy for a native
//! titlebar/border shell.
//!
//! This is **not** plugin-specific. It is the reusable description of what a
//! dark, compact native window chrome looks like. Concrete applications build a
//! [`ChromeTheme`] (or keep their own COLORREF struct) and feed individual
//! values into the DWM / painter / text APIs.
//!
//! Colors are stored as a platform-neutral [`Color`] so this module never has
//! to depend on the Windows crate. Convert to a Win32 `COLORREF` via
//! [`Color::to_colorref`] at the call site.

/// An 8-bit-per-channel RGB color, independent of any platform color encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Pack into a Win32 `COLORREF` layout (`0x00BBGGRR`).
    pub const fn to_colorref(self) -> u32 {
        self.r as u32 | ((self.g as u32) << 8) | ((self.b as u32) << 16)
    }
}

/// Font metrics + family policy for a chrome surface.
#[derive(Debug, Clone, Copy)]
pub struct ChromeFontTheme {
    pub family_primary: &'static str,
    pub family_fallback: &'static str,
    pub title_size: f32,
    pub body_size: f32,
    pub weight_title: u32,
    pub weight_body: u32,
}

/// A complete dark window-chrome theme: colors + metrics + font policy.
///
/// Metrics are logical pixels (scale by window DPI at use). This is a generic
/// preset; callers may override any field.
#[derive(Debug, Clone, Copy)]
pub struct ChromeTheme {
    pub titlebar_bg: Color,
    pub content_bg: Color,
    pub border: Color,
    pub title_text: Color,
    pub status_text: Color,
    pub error_text: Color,
    pub glyph: Color,
    pub glyph_active: Color,
    pub button_hover: Color,
    pub close_hover: Color,
    pub titlebar_h: i32,
    pub border_px: i32,
    pub button_w: i32,
    pub resize_grab: i32,
    pub title_pad: i32,
    pub font: ChromeFontTheme,
}

impl ChromeTheme {
    /// A neutral dark chrome palette. Applications can use this as a starting
    /// point and override individual fields to match their brand.
    pub const fn dark_default(font: ChromeFontTheme) -> Self {
        Self {
            titlebar_bg: Color::rgb(24, 25, 28),
            content_bg: Color::rgb(0, 0, 0),
            border: Color::rgb(44, 46, 51),
            title_text: Color::rgb(220, 221, 225),
            status_text: Color::rgb(150, 152, 158),
            error_text: Color::rgb(229, 115, 115),
            glyph: Color::rgb(205, 206, 210),
            glyph_active: Color::rgb(245, 246, 248),
            button_hover: Color::rgb(45, 47, 53),
            close_hover: Color::rgb(196, 43, 43),
            titlebar_h: 32,
            border_px: 1,
            button_w: 46,
            resize_grab: 6,
            title_pad: 12,
            font,
        }
    }
}
