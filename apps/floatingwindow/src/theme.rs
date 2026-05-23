//! Exact color tokens mirroring apps/web/src/theme.ts
//! Every color matches the Futureboard WebUI design system.
#![allow(dead_code)]

use egui::{Color32, FontFamily, FontId, RichText, Stroke, Visuals};

// ── base surfaces ─────────────────────────────────────────────────────────────
pub const BG: Color32 = Color32::from_rgb(0x17, 0x1B, 0x22);          // #171B22
pub const SUNKEN: Color32 = Color32::from_rgb(0x11, 0x15, 0x1B);      // #11151B
pub const SURFACE: Color32 = Color32::from_rgb(0x20, 0x26, 0x31);     // #202631
pub const SURFACE_HIGH: Color32 = Color32::from_rgb(0x2A, 0x32, 0x40); // #2A3240
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x31, 0x3A, 0x49); // #313A49
pub const SURFACE_ACTIVE: Color32 = Color32::from_rgb(0x39, 0x44, 0x56); // #394456

// ── text ──────────────────────────────────────────────────────────────────────
pub const TEXT: Color32 = Color32::from_rgb(0xF1, 0xF5, 0xF9);        // #F1F5F9
pub const TEXT_SOFT: Color32 = Color32::from_rgb(0xD2, 0xDB, 0xE6);   // #D2DBE6
pub const DIM: Color32 = Color32::from_rgb(0x9A, 0xA7, 0xB8);         // #9AA7B8
pub const FAINT: Color32 = Color32::from_rgb(0x6B, 0x78, 0x88);       // #6B7888

// ── accent / brand ────────────────────────────────────────────────────────────
pub const ACCENT: Color32 = Color32::from_rgb(0x5F, 0xCE, 0xD0);      // #5FCED0
pub const ACCENT_HARD: Color32 = Color32::from_rgb(0x8A, 0xE9, 0xEB); // #8AE9EB
pub fn accent_soft() -> Color32 {
    Color32::from_rgba_unmultiplied(0x5F, 0xCE, 0xD0, 46) // rgba(95,206,208,0.18)
}

// ── borders ───────────────────────────────────────────────────────────────────
pub const BORDER: Color32 = Color32::from_rgb(0x3A, 0x45, 0x54);      // #3A4554
pub const BORDER_HARD: Color32 = Color32::from_rgb(0x53, 0x61, 0x73); // #536173
pub fn border_soft() -> Color32 {
    Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 19) // rgba(255,255,255,0.075)
}

// ── status ────────────────────────────────────────────────────────────────────
pub const GREEN: Color32 = Color32::from_rgb(0x85, 0xE0, 0xA3);   // #85E0A3  solo active
pub const RED: Color32 = Color32::from_rgb(0xF4, 0x87, 0x7F);     // #F4877F  armed / clip
pub const YELLOW: Color32 = Color32::from_rgb(0xF4, 0xCF, 0x7A);  // #F4CF7A  mute active
pub const ORANGE: Color32 = Color32::from_rgb(0xEF, 0xA6, 0x6D);  // #EFA66D
pub const VIOLET: Color32 = Color32::from_rgb(0xB7, 0xAB, 0xFF);  // #B7ABFF

// ── VU meter segments ─────────────────────────────────────────────────────────
pub const METER_LOW: Color32 = Color32::from_rgb(0x3A, 0x9F, 0xA1);  // dark cyan (0-9)
pub const METER_MID: Color32 = Color32::from_rgb(0x56, 0xC7, 0xC9);  // bright cyan (10-14)
pub const METER_HIGH: Color32 = Color32::from_rgb(0xE8, 0xBE, 0x58); // yellow (15-17)
pub const METER_CLIP: Color32 = Color32::from_rgb(0xE9, 0x75, 0x6E); // red (18-19)
pub fn meter_off() -> Color32 {
    Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 11) // rgba(255,255,255,0.045)
}

// ── 12-color track palette ────────────────────────────────────────────────────
pub const TRACK_COLORS: [Color32; 12] = [
    Color32::from_rgb(0x56, 0xC7, 0xC9), // #56C7C9 cyan/lead
    Color32::from_rgb(0x7E, 0xDB, 0x9A), // #7EDB9A green/drums
    Color32::from_rgb(0xF2, 0xC9, 0x6D), // #F2C96D amber/bass
    Color32::from_rgb(0xF2, 0x7E, 0x77), // #F27E77 coral/vocal
    Color32::from_rgb(0xA9, 0x9C, 0xFF), // #A99CFF violet/synth
    Color32::from_rgb(0x6E, 0xB7, 0xE8), // #6EB7E8 blue/keys
    Color32::from_rgb(0xE8, 0x9B, 0x61), // #E89B61 orange/percussion
    Color32::from_rgb(0xD9, 0x82, 0xB6), // #D982B6 rose/fx
    Color32::from_rgb(0xA8, 0xD3, 0x6F), // #A8D36F lime/guitar
    Color32::from_rgb(0x9C, 0xAF, 0xE8), // #9CAFE8 periwinkle/pads
    Color32::from_rgb(0xC4, 0x9A, 0x6C), // #C49A6C warm brown/acoustic
    Color32::from_rgb(0x71, 0xD6, 0xB5), // #71D6B5 mint/bus
];

// ── grid / timeline ───────────────────────────────────────────────────────────
pub fn grid_minor() -> Color32 {
    Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 11)  // rgba(255,255,255,0.045)
}
pub fn grid_major() -> Color32 {
    Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 24)  // rgba(255,255,255,0.095)
}

// ── piano roll ────────────────────────────────────────────────────────────────
pub fn midi_black_key() -> Color32 {
    Color32::from_rgba_unmultiplied(0, 0, 0, 71) // rgba(0,0,0,0.28)
}
pub fn midi_white_key() -> Color32 {
    Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 5) // rgba(255,255,255,0.018)
}
pub fn midi_c_divider() -> Color32 {
    Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 25) // rgba(255,255,255,0.10)
}
pub fn midi_divider() -> Color32 {
    Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 10) // rgba(255,255,255,0.04)
}

// ── fonts ─────────────────────────────────────────────────────────────────────
pub fn label(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn label_rt(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text).font(label(size)).color(color)
}
pub fn header_rt(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .font(label(9.0))
        .color(DIM)
        .strong()
}

// ── egui Visuals ──────────────────────────────────────────────────────────────

pub fn visuals() -> Visuals {
    let mut v = Visuals::dark();

    v.panel_fill = BG;
    v.window_fill = SURFACE;
    v.faint_bg_color = SURFACE;
    v.extreme_bg_color = SUNKEN;

    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border_soft());
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, DIM);

    v.widgets.inactive.bg_fill = SURFACE_HIGH;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_SOFT);

    v.widgets.hovered.bg_fill = SURFACE_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);

    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.fg_stroke = Stroke::new(1.0, SUNKEN);

    v.widgets.open.bg_fill = SURFACE_ACTIVE;

    v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x5F, 0xCE, 0xD0, 51); // 0.20 alpha
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    v.hyperlink_color = ACCENT;
    v.window_stroke = Stroke::new(1.0, BORDER);

    v
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Draw a 20-segment VU meter bar (bottom-to-top, colored to match WebUI).
pub fn draw_vu_meter(
    ui: &mut egui::Ui,
    level: f32,
    width: f32,
    height: f32,
) {
    const SEGMENTS: usize = 20;
    const GAP: f32 = 1.5;

    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let level = level.clamp(0.0, 1.0);
    let lit = (level * SEGMENTS as f32).round() as usize;

    let seg_h = (height - GAP * (SEGMENTS as f32 - 1.0)) / SEGMENTS as f32;

    for i in 0..SEGMENTS {
        let y = rect.max.y - (i as f32 + 1.0) * seg_h - i as f32 * GAP;
        let seg = egui::Rect::from_min_size(
            egui::pos2(rect.min.x, y),
            egui::vec2(width, seg_h),
        );
        let color = if i >= lit {
            meter_off()
        } else if i >= 18 {
            METER_CLIP
        } else if i >= 15 {
            METER_HIGH
        } else if i >= 10 {
            METER_MID
        } else {
            METER_LOW
        };
        ui.painter().rect_filled(seg, 0.0, color);
    }
}

/// Parse a CSS hex color string (#RRGGBB or #RGB) → Color32.
pub fn parse_hex(hex: &str) -> Color32 {
    let h = hex.trim_start_matches('#');
    if h.len() >= 6 {
        let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(128);
        let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(128);
        let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(128);
        Color32::from_rgb(r, g, b)
    } else {
        DIM
    }
}

/// Convert linear 0..=1 (or 0..=1.5 for gain) to dB string.
pub fn db_str(linear: f32) -> String {
    if linear < 0.00001 {
        "-∞".into()
    } else {
        let db = 20.0 * linear.log10();
        if db >= 0.0 {
            format!("+{:.1}", db)
        } else {
            format!("{:.1}", db)
        }
    }
}

/// dB → linear
pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
