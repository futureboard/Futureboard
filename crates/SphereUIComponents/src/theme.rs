use gpui::{rgb, rgba, Rgba};

/// Single font family used across the native app — Inter Variable.
/// The variable TTF (`InterVariable.ttf`) registers under the family name
/// "Inter Variable Text" in GPUI's text system.
pub const FONT_FAMILY: &str = "Inter Variable Text";

/// Alias kept for callsites that want an explicit "display" name. Points at
/// the same variable family.
pub const DISPLAY_FONT_FAMILY: &str = FONT_FAMILY;

/// Recommended text sizes. Kept here so individual components don't drift.
pub mod text {
    /// Caps-style sublabels — INSERTS / SENDS / TRACK.
    pub const CAPS: f32 = 8.0;
    /// Small meta (CH 01, dB scale).
    pub const META: f32 = 9.0;
    /// Standard UI label (track name, button label).
    pub const UI: f32 = 11.0;
    /// Inspector / title text.
    pub const TITLE: f32 = 12.0;
}

pub struct Colors;

impl Colors {
    // Backgrounds — near-black blue-gray DAW palette
    pub fn surface_base() -> Rgba { rgb(0x171B22) }
    pub fn surface_panel() -> Rgba { rgb(0x202631) }
    pub fn surface_raised() -> Rgba { rgb(0x2A3240) }
    pub fn surface_input() -> Rgba { rgb(0x11151B) }
    pub fn surface_hover() -> Rgba { rgb(0x313A49) }

    // Borders
    pub fn border_subtle() -> Rgba { rgba(0xFFFFFF13) }
    pub fn border_strong() -> Rgba { rgb(0x536173) }

    // Text
    pub fn text_primary() -> Rgba { rgb(0xF1F5F9) }
    pub fn text_secondary() -> Rgba { rgb(0xD2DBE6) }
    pub fn text_muted() -> Rgba { rgb(0x9AA7B8) }
    pub fn text_faint() -> Rgba { rgba(0xFFFFFF47) } // ~28% white — sub-labels

    // Accent
    pub fn accent_primary() -> Rgba { rgb(0x5FCED0) }
    pub fn accent_soft() -> Rgba { rgba(0x5FCED02E) }

    // Status
    pub fn status_error() -> Rgba { rgb(0xF4877F) }
    pub fn status_warning() -> Rgba { rgb(0xF4CF7A) }
    pub fn status_success() -> Rgba { rgb(0x85E0A3) }
}

