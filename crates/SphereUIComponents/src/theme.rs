use gpui::{rgb, rgba, Rgba};

pub const FONT_FAMILY: &str = "Inter";

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

    // Accent
    pub fn accent_primary() -> Rgba { rgb(0x5FCED0) }
    pub fn accent_soft() -> Rgba { rgba(0x5FCED02E) }

    // Status
    pub fn status_error() -> Rgba { rgb(0xF4877F) }
    pub fn status_warning() -> Rgba { rgb(0xF4CF7A) }
    pub fn status_success() -> Rgba { rgb(0x85E0A3) }
}

