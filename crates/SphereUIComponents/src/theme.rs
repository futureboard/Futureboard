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

pub mod menu {
    pub const PANEL_MIN_WIDTH: f32 = 210.0;
    pub const PANEL_MAX_WIDTH: f32 = 340.0;
    pub const PANEL_PAD: f32 = 4.0;
    pub const ROW_HEIGHT: f32 = 25.0;
    pub const ROW_PAD_X: f32 = 10.0;
    pub const CHECK_SLOT_W: f32 = 18.0;
    pub const ICON_SIZE: f32 = 11.0;
    pub const CHEVRON_SIZE: f32 = 11.0;
    pub const LABEL_TEXT_SIZE: f32 = 10.5;
    pub const META_TEXT_SIZE: f32 = 10.0;
    pub const HEADER_TEXT_SIZE: f32 = 10.0;
    pub const HEADER_HEIGHT: f32 = 21.0;
    pub const SEPARATOR_MARGIN_Y: f32 = 3.0;
    pub const ITEM_GAP: f32 = 1.0;
}

pub struct Colors;

impl Colors {
    // Backgrounds — JetBrains Fleet Dark inspired palette
    pub fn surface_base() -> Rgba {
        rgb(0x1E1F22)
    }

    pub fn surface_panel() -> Rgba {
        rgb(0x25262B)
    }

    pub fn surface_panel_alt() -> Rgba {
        rgb(0x1B1C20)
    }

    pub fn surface_panel_raised() -> Rgba {
        rgb(0x2B2D33)
    }

    pub fn surface_canvas() -> Rgba {
        rgb(0x15161A)
    }

    pub fn surface_raised() -> Rgba {
        rgb(0x2B2D33)
    }

    pub fn surface_input() -> Rgba {
        rgb(0x181A1F)
    }

    pub fn surface_window() -> Rgba {
        rgb(0x15161A)
    }

    pub fn surface_titlebar() -> Rgba {
        rgb(0x1B1C20)
    }

    pub fn surface_card() -> Rgba {
        rgb(0x202126)
    }

    pub fn surface_hover() -> Rgba {
        rgb(0x30323A)
    }

    pub fn surface_active() -> Rgba {
        rgb(0x2B2D33)
    }

    pub fn surface_control_hover() -> Rgba {
        rgb(0x292B31)
    }

    pub fn surface_overlay() -> Rgba {
        rgba(0x00000085)
    }

    // Borders
    pub fn border_subtle() -> Rgba {
        rgba(0xFFFFFF14)
    }

    pub fn border_default() -> Rgba {
        rgba(0xFFFFFF1F)
    }

    pub fn border_strong() -> Rgba {
        rgb(0x4C505C)
    }

    pub fn border_focus() -> Rgba {
        rgba(0x7B61FFB8)
    }

    pub fn border_accent() -> Rgba {
        rgba(0x7B61FF80)
    }

    pub fn divider() -> Rgba {
        rgba(0xFFFFFF0F)
    }

    // Text
    pub fn text_primary() -> Rgba {
        rgb(0xDFE1E5)
    }

    pub fn text_secondary() -> Rgba {
        rgb(0xC3C7D0)
    }

    pub fn text_muted() -> Rgba {
        rgb(0x8E96A3)
    }

    pub fn text_faint() -> Rgba {
        rgba(0xFFFFFF45)
    }

    pub fn text_dim() -> Rgba {
        rgba(0xFFFFFF66)
    }

    pub fn text_disabled() -> Rgba {
        rgba(0xFFFFFF3B)
    }

    pub fn text_inverse() -> Rgba {
        rgb(0x1E1F22)
    }

    // Accent — Fleet-style violet/blue
    pub fn accent_primary() -> Rgba {
        rgb(0x7B61FF)
    }

    pub fn accent_primary_hover() -> Rgba {
        rgb(0x8D78FF)
    }

    pub fn accent_soft() -> Rgba {
        rgba(0x7B61FF30)
    }

    pub fn accent_muted() -> Rgba {
        rgba(0x7B61FF20)
    }

    pub fn accent_pressed() -> Rgba {
        rgba(0x7B61FF28)
    }

    pub fn on_accent() -> Rgba {
        rgb(0xFFFFFF)
    }

    // Status / Alert Accents
    pub fn status_error() -> Rgba {
        rgb(0xFF6B68)
    }

    pub fn status_warning() -> Rgba {
        rgb(0xE5C07B)
    }

    pub fn status_success() -> Rgba {
        rgb(0x6FCF97)
    }

    pub fn accent_success() -> Rgba {
        rgb(0x6FCF97)
    }

    pub fn accent_warning() -> Rgba {
        rgb(0xE5C07B)
    }

    pub fn accent_danger() -> Rgba {
        rgb(0xFF6B68)
    }

    pub fn accent_purple() -> Rgba {
        rgb(0xBB86FC)
    }

    // DAW-specific
    pub fn meter_bg() -> Rgba {
        rgba(0xFFFFFF0D)
    }

    pub fn meter_low() -> Rgba {
        rgb(0x6FCF97)
    }

    pub fn meter_mid() -> Rgba {
        rgb(0xE5C07B)
    }

    pub fn meter_high() -> Rgba {
        rgb(0xFF6B68)
    }

    pub fn fader_rail() -> Rgba {
        rgba(0xFFFFFF0F)
    }

    pub fn fader_thumb() -> Rgba {
        rgb(0xDFE1E5)
    }

    pub fn fader_tick() -> Rgba {
        rgba(0xFFFFFF1F)
    }

    pub fn fader_scale_text() -> Rgba {
        rgba(0xFFFFFF38)
    }

    pub fn knob_bg() -> Rgba {
        rgb(0x181A1F)
    }

    pub fn knob_ring() -> Rgba {
        rgb(0x7B61FF)
    }

    pub fn slot_bg() -> Rgba {
        rgba(0xFFFFFF08)
    }

    pub fn slot_border() -> Rgba {
        rgba(0xFFFFFF12)
    }

    pub fn statusbar_bg() -> Rgba {
        rgb(0x1B1C20)
    }

    pub fn statusbar_text() -> Rgba {
        rgb(0x8E96A3)
    }

    pub fn mixer_bg() -> Rgba {
        rgb(0x111418)
    }

    pub fn master_strip_bg() -> Rgba {
        rgb(0x181A1F)
    }

    pub fn timeline_grid_major() -> Rgba {
        rgba(0xFFFFFF12)
    }

    pub fn timeline_grid_minor() -> Rgba {
        rgba(0xFFFFFF08)
    }

    pub fn timeline_grid_bar() -> Rgba {
        rgba(0xFFFFFF1A)
    }

    pub fn timeline_playhead() -> Rgba {
        rgb(0xFF6B68)
    }

    pub fn timeline_background() -> Rgba {
        Self::surface_base()
    }

    pub fn timeline_content_background() -> Rgba {
        Self::surface_base()
    }

    pub fn timeline_region_background() -> Rgba {
        rgba(0xFFFFFF06)
    }

    pub fn timeline_region_background_alt() -> Rgba {
        rgba(0xFFFFFF04)
    }

    pub fn timeline_lane_background() -> Rgba {
        rgba(0xFFFFFF07)
    }

    pub fn timeline_lane_alt_background() -> Rgba {
        rgba(0x00000029)
    }

    pub fn timeline_selected_lane_background() -> Rgba {
        rgba(0xFFFFFF12)
    }

    pub fn timeline_empty_body_background() -> Rgba {
        // Slightly calmer than lane alt so the grid doesn't look "too forward"
        // in empty space below the last track.
        rgba(0x00000024)
    }

    pub fn timeline_ruler_background() -> Rgba {
        Self::surface_panel()
    }

    pub fn timeline_ruler_tick() -> Rgba {
        rgba(0xFFFFFF1F)
    }

    pub fn timeline_ruler_text() -> Rgba {
        Self::text_secondary()
    }

    pub fn timeline_selection() -> Rgba {
        Self::accent_soft()
    }

    // Track colors (fallbacks)
    pub fn track_audio() -> Rgba {
        rgb(0x5FCED0)
    }

    pub fn track_midi() -> Rgba {
        rgb(0xE5C07B)
    }

    pub fn track_instrument() -> Rgba {
        rgb(0xBB86FC)
    }

    pub fn track_bus() -> Rgba {
        rgb(0x7B61FF)
    }

    pub fn track_return() -> Rgba {
        rgb(0x6FCF97)
    }

    pub fn track_master() -> Rgba {
        rgb(0xDFE1E5)
    }

    // Surfaces
    pub fn bottom_panel_bg() -> Rgba {
        rgb(0x25262B)
    }

    pub fn bottom_panel_header_bg() -> Rgba {
        rgb(0x1B1C20)
    }

    pub fn mixer_strip_bg() -> Rgba {
        rgba(0xFFFFFF08)
    }

    pub fn mixer_strip_bg_alt() -> Rgba {
        rgba(0xFFFFFF05)
    }

    pub fn mixer_strip_selected_bg() -> Rgba {
        rgba(0xFFFFFF14)
    }

    pub fn master_strip_header_bg() -> Rgba {
        rgb(0x181A1F)
    }

    // Borders
    pub fn panel_border() -> Rgba {
        rgba(0xFFFFFF14)
    }

    pub fn strip_border() -> Rgba {
        rgba(0xFFFFFF26)
    }

    pub fn strip_border_subtle() -> Rgba {
        rgba(0xFFFFFF0A)
    }

    pub fn master_strip_border() -> Rgba {
        rgba(0xFFFFFF1A)
    }

    // Slots
    pub fn slot_bg_hover() -> Rgba {
        rgba(0xFFFFFF14)
    }

    pub fn slot_empty_text() -> Rgba {
        rgba(0xFFFFFF45)
    }

    // Fader
    pub fn fader_groove() -> Rgba {
        rgb(0x15161A)
    }

    pub fn fader_thumb_border() -> Rgba {
        rgba(0xFFFFFF40)
    }

    // Meters
    pub fn meter_rail() -> Rgba {
        rgba(0xFFFFFF0A)
    }

    pub fn meter_peak() -> Rgba {
        rgb(0xFFD700)
    }

    // Status
    pub fn statusbar_text_muted() -> Rgba {
        rgba(0xFFFFFF66)
    }

    pub fn statusbar_accent() -> Rgba {
        rgb(0x7B61FF)
    }

    pub fn statusbar_warning() -> Rgba {
        rgb(0xE5C07B)
    }

    // Helper to dynamically adjust alpha channel
    pub fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
        Rgba {
            r: color.r,
            g: color.g,
            b: color.b,
            a: alpha,
        }
    }

    pub const TRACK_COLORS: [u32; 12] = [
        0x56C7C9, 0x7EDB9A, 0xF2C96D, 0xF27E77, 0xA99CFF, 0x6EB7E8, 0xE89B61, 0xD982B6, 0xA8D36F,
        0x9CAFE8, 0xC49A6C, 0x71D6B5,
    ];

    pub fn track_color_for_index(index: usize) -> Rgba {
        rgb(Self::TRACK_COLORS[index % Self::TRACK_COLORS.len()])
    }
}
