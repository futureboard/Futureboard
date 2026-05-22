use gpui::{rgb, rgba, Rgba};

pub struct Colors;

impl Colors {
    // Backgrounds — near-black blue-gray DAW palette
    pub fn bg_base() -> Rgba { rgb(0x171B22) }
    pub fn surface_panel() -> Rgba { rgb(0x1C2028) }
    pub fn surface_raised() -> Rgba { rgb(0x232A34) }
    pub fn surface_high() -> Rgba { rgb(0x2A3240) }

    // Borders
    pub fn border_subtle() -> Rgba { rgba(0xFFFFFF12) }
    pub fn border_strong() -> Rgba { rgba(0xFFFFFF22) }

    // Text
    pub fn text_primary() -> Rgba { rgb(0xC5CED9) }
    pub fn text_secondary() -> Rgba { rgb(0x8892A0) }
    pub fn text_dim() -> Rgba { rgb(0x4A5568) }

    // Accent
    pub fn accent_primary() -> Rgba { rgb(0x3B82F6) }

    // Status
    pub fn status_error() -> Rgba { rgb(0xEF4444) }
    pub fn status_warning() -> Rgba { rgb(0xF59E0B) }
    pub fn status_success() -> Rgba { rgb(0x22C55E) }
}
