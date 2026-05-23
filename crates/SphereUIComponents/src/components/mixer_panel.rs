use gpui::{div, px, rgba, svg, InteractiveElement, IntoElement, ParentElement, Styled, StatefulInteractiveElement};
use crate::theme::Colors;
use crate::assets;

// Total stacked content height inside a channel strip. The mixer panel must
// give each strip at least this much room or scroll vertically to expose the
// remainder; otherwise inner sections collide and clip each other.
//   2 (top accent)
// + 46 (header w/ swatch + name)
// + 44 (INSERTS: section header + slot row)
// + 44 (SENDS: section header + slot row)
// + 76 (pan section)
// + ~170 (fader area: dB readout + TRACK_H + padding)
// + 50 (M/S/R/I + M/S buttons)
// + 28 (footer)
// ~= 460
const STRIP_MIN_HEIGHT: f32 = 460.0;
const STRIP_WIDTH: f32 = 108.0;

// ─── Fader geometry (mirrors web MixerFader constants) ───────────────────────

const TRACK_H: f32 = 130.0; // fader track height px
const THUMB_H: f32 = 10.0;  // fader cap height px
const USABLE:  f32 = TRACK_H - THUMB_H; // 120.0 px

// Scale marks: (dB value, display label)
// Mirrors web SCALE_MARKS — db: -54 shown as "∞"
const SCALE_MARKS: [(f32, &str); 7] = [
    (  0.0, "0"),
    ( -6.0, "6"),
    (-12.0, "12"),
    (-18.0, "18"),
    (-24.0, "24"),
    (-36.0, "36"),
    (-54.0, "∞"),
];

// Center Y of a dB mark inside the TRACK_H container.
// Mirrors web `thumbCenterStyle`: top = (1-t)*(TRACK_H-THUMB_H) + THUMB_H/2
fn db_to_center_y(db: f32) -> f32 {
    let t = ((db + 60.0) / 60.0).max(0.0).min(1.0);
    (1.0 - t) * USABLE + THUMB_H / 2.0
}

// Top edge of fader thumb for a given t.
fn t_to_thumb_top(t: f32) -> f32 {
    (1.0 - t) * USABLE
}

fn db_to_thumb_top(db: f32) -> f32 {
    let t = ((db + 60.0) / 60.0).max(0.0).min(1.0);
    t_to_thumb_top(t)
}

// ─── Placeholder channel data ─────────────────────────────────────────────────
// (name, ch_num, db_str, volume_db, meter_l, meter_r, has_insert, insert_name)
type ChRow = (&'static str, &'static str, &'static str, f32, f32, f32, bool, &'static str);

const CHANNELS: [ChRow; 5] = [
    ("Audio Track 1", "01", "-37.0", -37.0, 0.65, 0.70, true,  "Pro-Q 4"),
    ("Audio Track 2", "02",  "-1.9",  -1.9, 0.45, 0.50, false, ""),
    ("Audio Track 3", "03",  "-1.9",  -1.9, 0.30, 0.28, false, ""),
    ("Audio Track 4", "04",  "-1.9",  -1.9, 0.55, 0.60, false, ""),
    ("Audio Track 5", "05",  "-1.9",  -1.9, 0.40, 0.35, false, ""),
];

// All strips share the default Futureboard teal accent (as seen in screenshot)
fn strip_accent() -> gpui::Rgba { Colors::accent_primary() }

// ─── Mixer sub-header ("Mixer  6 ch") ─────────────────────────────────────────
// Mirrors web: <SlidersHorizontal /> + "Mixer" + "6 ch" badge

fn mixer_sub_header() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .h(px(30.0))
        .px(px(10.0))
        .border_b(px(1.0))
        .border_color(rgba(0xFFFFFF0F_u32))
        .bg(rgba(0x0F131800_u32))
        // SlidersHorizontal icon
        .child(
            svg()
                .path(assets::ICON_SLIDERS_HORIZONTAL_PATH)
                .w(px(14.0))
                .h(px(14.0))
                .text_color(rgba(0xFFFFFF47_u32))
        )
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(Colors::text_primary())
                .child("Mixer"),
        )
        // "6 ch" badge
        .child(
            div()
                .flex()
                .items_center()
                .px(px(5.0))
                .py(px(1.0))
                .rounded_md()
                .bg(rgba(0xFFFFFF08_u32))
                .border(px(1.0))
                .border_color(rgba(0xFFFFFF12_u32))
                .text_size(px(9.0))
                .text_color(rgba(0xFFFFFF59_u32))
                .child("6 ch"),
        )
}

// ─── Section header ("| INSERTS +" / "| SENDS +") ────────────────────────────
// Mirrors web SectionHeader component

fn section_header(label: &'static str, accent: gpui::Rgba) -> impl IntoElement {
    let icon_path = match label {
        "INSERTS" => Some(assets::ICON_PLUG_PATH),
        "SENDS" => Some(assets::ICON_ROUTE_PATH),
        _ => None,
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(4.0))
        .px(px(8.0))
        .py(px(5.0))
        // Left: accent dot + label
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.0))
                // 2px accent dot (opacity ~55%)
                .child(
                    div()
                        .w(px(2.0))
                        .h(px(9.0))
                        .rounded_full()
                        .bg(rgba(
                            (((accent.r * 255.0) as u32) << 24)
                            | (((accent.g * 255.0) as u32) << 16)
                            | (((accent.b * 255.0) as u32) << 8)
                            | 0x8C, // 55% alpha
                        )),
                )
                .children(icon_path.map(|path| {
                    svg()
                        .path(path)
                        .w(px(11.0))
                        .h(px(11.0))
                        .text_color(rgba(0xDCE8F066_u32))
                }))
                .child(
                    div()
                        .text_size(px(8.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgba(0xDCE8F066_u32)) // rgba(220,232,240, 0.40)
                        .child(label),
                ),
        )
        // Right: "+" add button placeholder
        .child(
            div()
                .w(px(16.0))
                .h(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .bg(rgba(0xFFFFFF00_u32)) // transparent (hover shows bg)
                .child(
                    svg()
                        .path(assets::ICON_PLUS_PATH)
                        .w(px(11.0))
                        .h(px(11.0))
                        .text_color(rgba(0xFFFFFF38_u32))
                ),
        )
}

// ─── Insert row (filled slot) ─────────────────────────────────────────────────
// Mirrors web InsertRow — left 2px border in accent color

fn insert_row(name: &'static str, accent: gpui::Rgba) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .border_l(px(2.0))
        .border_color(accent)
        .px(px(8.0))
        .py(px(3.0))
        .child(
            div()
                .flex_1()
                .text_size(px(10.0))
                .text_color(rgba(0xFFFFFFB8_u32)) // ~72%
                .child(name),
        )
}

// ─── Empty slot (dashed outline) ─────────────────────────────────────────────
// Mirrors web EmptySlotRow

fn empty_slot() -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .mx(px(4.0))
        .mb(px(4.0))
        .py(px(3.0))
        .rounded_sm()
        .border(px(1.0))
        .border_dashed()
        .border_color(rgba(0xFFFFFF0D_u32)) // ~5%
        .text_size(px(8.5))
        .text_color(rgba(0xFFFFFF38_u32)) // ~22%
        .child("empty")
}

// ─── Pan knob section ─────────────────────────────────────────────────────────
// Mirrors web: size-40 Knob + L/R labels

fn pan_section(accent: gpui::Rgba) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(4.0))
        .py(px(8.0))
        .border_b(px(1.0))
        .border_color(rgba(0xFFFFFF0B_u32))
        // Circular knob: 40×40, matches web Knob size={40}
        .child(
            div()
                .w(px(40.0))
                .h(px(40.0))
                .rounded_full()
                .bg(Colors::surface_raised())
                .border(px(1.5))
                .border_color(rgba(
                    (((accent.r * 255.0) as u32) << 24)
                    | (((accent.g * 255.0) as u32) << 16)
                    | (((accent.b * 255.0) as u32) << 8)
                    | 0x3D, // ~24% alpha
                ))
                .flex()
                .items_center()
                .justify_center()
                .relative()
                // Center indicator dot
                .child(
                    div()
                        .w(px(4.0))
                        .h(px(4.0))
                        .rounded_full()
                        .bg(accent),
                ),
        )
        // L / R labels
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .w(px(64.0))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(7.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgba(0xFFFFFF38_u32))
                        .child("L"),
                )
                .child(
                    div()
                        .text_size(px(7.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgba(0xFFFFFF38_u32))
                        .child("R"),
                ),
        )
}

// ─── dB scale column (left of fader) ─────────────────────────────────────────
// Mirrors web: right-aligned labels aligned to thumb centers

fn db_scale_column() -> gpui::Div {
    let mut col = div()
        .relative()
        .w(px(15.0))
        .h(px(TRACK_H));

    for &(db, label) in SCALE_MARKS.iter() {
        let cy = db_to_center_y(db);
        // Position text so its vertical center aligns with tick center (~4px half-height for 7.5px font)
        let top = (cy - 3.5).max(0.0);
        col = col.child(
            div()
                .absolute()
                .top(px(top))
                .right(px(0.0))
                .text_size(px(7.5))
                .text_color(if db == 0.0 { rgba(0xFFFFFF59_u32) } else { rgba(0xFFFFFF2E_u32) })
                .child(label),
        );
    }
    col
}

// ─── Fader center column (rail + tick marks + thumb) ─────────────────────────
// The column is 24px wide; everything inside is absolutely positioned.
// Mirrors web fader track internals.

fn fader_center_column(thumb_top: f32) -> impl IntoElement {
    // In 24px wide column:
    //   center = 12px
    //   rail (3px):  left = 12 - 1.5 = 10.5 → 11px
    //   tick 0dB (13px): left = 12 - 6.5 = 5.5 → 6px
    //   tick other (9px): left = 12 - 4.5 = 7.5 → 8px
    //   thumb (24px):  left = 0 (full column width)

    let mut col = div()
        .relative()
        .w(px(24.0))
        .h(px(TRACK_H))
        // Rail (from THUMB_H/2 to TRACK_H - THUMB_H/2)
        .child(
            div()
                .absolute()
                .top(px(THUMB_H / 2.0))
                .left(px(11.0))
                .w(px(3.0))
                .h(px(USABLE))
                .bg(rgba(0xFFFFFF0F_u32))
                .rounded_full(),
        );

    // Tick marks at each dB mark
    for &(db, _) in SCALE_MARKS.iter() {
        let cy = db_to_center_y(db);
        let (w, left) = if db == 0.0 { (13.0f32, 6.0f32) } else { (9.0f32, 8.0f32) };
        col = col.child(
            div()
                .absolute()
                .top(px(cy)) // 1px height, top at center is fine
                .left(px(left))
                .h(px(1.0))
                .w(px(w))
                .bg(if db == 0.0 { rgba(0xFFFFFF4D_u32) } else { rgba(0xFFFFFF1F_u32) }),
        );
    }

    // Fader thumb
    col.child(
        div()
            .absolute()
            .top(px(thumb_top))
            .left(px(0.0))
            .w(px(24.0))
            .h(px(THUMB_H))
            .rounded_sm()
            // Approximate the web's gradient thumb
            .bg(rgba(0xFFFFFF26_u32)) // ~15% white
            .border(px(1.0))
            .border_color(rgba(0xFFFFFF38_u32)) // ~22% white
    )
}

// ─── VU meter bar (single column) ────────────────────────────────────────────

fn meter_bar_col(level: f32) -> gpui::Div {
    let green_max  = 0.80 * TRACK_H;
    let yellow_max = 0.95 * TRACK_H;
    let fill = (level * TRACK_H).min(TRACK_H);
    let green  = fill.min(green_max);
    let yellow = if fill > green_max  { (fill - green_max).min(yellow_max - green_max) } else { 0.0 };
    let red    = if fill > yellow_max { fill - yellow_max } else { 0.0 };

    div()
        .w(px(5.0))
        .h(px(TRACK_H))
        .bg(rgba(0xFFFFFF0A_u32)) // very dark meter bg
        .rounded_sm()
        .relative()
        .child(div().absolute().bottom(px(0.0)).w_full().h(px(green)).bg(rgba(0x85E0A3CC_u32)))  // green ~80%
        .child(div().absolute().bottom(px(green)).w_full().h(px(yellow)).bg(rgba(0xF4CF7ACC_u32))) // yellow
        .child(div().absolute().bottom(px(green + yellow)).w_full().h(px(red)).bg(rgba(0xF4877FCC_u32)))  // red
}

// ─── Fader area (dB readout + scale column + fader column + meter) ────────────
// flex_1 container; inner layout mirrors web MixerFader

fn fader_area(db_str: &'static str, thumb_top: f32, level_l: f32, level_r: f32) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .px(px(6.0))
        .py(px(6.0))
        // dB readout — matches web: value + "dB" unit
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_center()
                .gap(px(2.0))
                .pb(px(4.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgba(0xEEF2F5B8_u32)) // ~72%
                        .child(db_str),
                )
                .child(
                    div()
                        .text_size(px(7.5))
                        .text_color(rgba(0xFFFFFF38_u32)) // ~22%
                        .child("dB"),
                ),
        )
        // Scale + fader + meter row (fixed height = TRACK_H)
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(2.0))
                .h(px(TRACK_H))
                .child(db_scale_column())
                // Fader: flex-1 centering wrapper → 24px center col inside
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .h_full()
                        .items_center()
                        .justify_center()
                        .child(fader_center_column(thumb_top)),
                )
                // Stereo meter (L + R)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(2.0))
                        .h_full()
                        .child(meter_bar_col(level_l))
                        .child(meter_bar_col(level_r)),
                ),
        )
}

// ─── M / S / R / I button row + M/S button ───────────────────────────────────
// Mirrors web button grid (grid-cols-4 gap-1) + PreviewModeMenu "M/S"

fn msri_button(label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(px(20.0))
        .flex_1()
        .rounded_sm()
        .bg(rgba(0xFFFFFF08_u32))
        .border(px(1.0))
        .border_color(rgba(0xFFFFFF17_u32))
        .text_size(px(9.0))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgba(0xDCE8F085_u32)) // ~52%
        .child(label)
}

fn button_row() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .px(px(6.0))
        .pt(px(4.0))
        .pb(px(4.0))
        .border_t(px(1.0))
        .border_color(rgba(0xFFFFFF0B_u32))
        // M / S / R / I  (4 equal buttons)
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(3.0))
                .child(msri_button("M"))
                .child(msri_button("S"))
                .child(msri_button("R"))
                .child(msri_button("I")),
        )
        // M/S preview mode button (centered, wider)
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h(px(18.0))
                        .px(px(10.0))
                        .rounded_sm()
                        .bg(rgba(0xFFFFFF08_u32))
                        .border(px(1.0))
                        .border_color(rgba(0xFFFFFF17_u32))
                        .text_size(px(9.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(rgba(0xDCE8F085_u32))
                        .child("M/S"),
                ),
        )
}

// ─── Channel strip ────────────────────────────────────────────────────────────

fn channel_strip(
    name:        &'static str,
    ch_num:      &'static str,
    accent:      gpui::Rgba,
    db_str:      &'static str,
    volume_db:   f32,
    level_l:     f32,
    level_r:     f32,
    has_insert:  bool,
    insert_name: &'static str,
    is_master:   bool,
) -> gpui::Div {
    let thumb_top  = db_to_thumb_top(volume_db);
    let strip_bg   = if is_master { rgba(0x5FCED00C_u32) } else { rgba(0xFFFFFF07_u32) };
    let border_col = if is_master { rgba(0xFFFFFF1A_u32) } else { rgba(0xFFFFFF0A_u32) };

    // Type label: "AUDIO" for regular tracks, "MST" for master
    let type_label = if is_master { "MST" } else { "AUDIO" };
    let ch_display = if is_master { "M" } else { ch_num };

    let mut strip = div()
        .flex()
        .flex_col()
        .flex_none()
        .w(px(STRIP_WIDTH))
        .min_h(px(STRIP_MIN_HEIGHT))
        .bg(strip_bg)
        .border_r(px(1.0))
        .border_color(border_col)
        // ── Top accent line (1.5px, gradient-like solid) ──────────────────────
        .child(div().w_full().h(px(2.0)).bg(accent))
        // ── Header: 3px swatch + name + type/ch ──────────────────────────────
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .py(px(6.0))
                .border_b(px(1.0))
                .border_color(rgba(0xFFFFFF0B_u32))
                // h-7 (28px) colored accent bar
                .child(
                    div()
                        .w(px(3.0))
                        .h(px(28.0))
                        .rounded_full()
                        .bg(accent),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        // Track name (truncated by strip width)
                        .child(
                            div()
                                .text_size(px(10.5))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgba(0xFFFFFFCC_u32)) // ~80%
                                .child(name),
                        )
                        // type + CH label
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(3.0))
                                .mt(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(8.0))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgba(0xFFFFFF47_u32)) // ~28%
                                        .child(type_label),
                                )
                                .child(
                                    div()
                                        .text_size(px(8.0))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgba(0xFFFFFF47_u32))
                                        .child(if is_master { "" } else { "CH" }),
                                )
                                .child(
                                    div()
                                        .text_size(px(8.0))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(rgba(0xFFFFFF47_u32))
                                        .child(ch_display),
                                ),
                        ),
                ),
        );

    // ── INSERTS section (full level: show for all channels) ───────────────────
    strip = strip.child(
        div()
            .flex()
            .flex_col()
            .border_b(px(1.0))
            .border_color(rgba(0xFFFFFF0B_u32))
            .child(section_header("INSERTS", accent))
            .child(if has_insert {
                insert_row(insert_name, accent).into_any_element()
            } else {
                empty_slot().into_any_element()
            }),
    );

    // ── SENDS section (non-master only) ───────────────────────────────────────
    if !is_master {
        strip = strip.child(
            div()
                .flex()
                .flex_col()
                .border_b(px(1.0))
                .border_color(rgba(0xFFFFFF0B_u32))
                .child(section_header("SENDS", accent))
                .child(empty_slot()),
        );
    }

    // ── Pan section (non-master only) ─────────────────────────────────────────
    if !is_master {
        strip = strip.child(pan_section(accent));
    } else {
        // Master: thin separator to keep alignment
        strip = strip.child(
            div()
                .h(px(4.0))
                .border_b(px(1.0))
                .border_color(rgba(0xFFFFFF0B_u32)),
        );
    }

    // ── Fader area (flex_1) ───────────────────────────────────────────────────
    strip = strip.child(fader_area(db_str, thumb_top, level_l, level_r));

    // ── M/S/R/I + M/S buttons (non-master) ────────────────────────────────────
    if !is_master {
        strip = strip.child(button_row());
    } else {
        // Master: "master" label + minimal separator
        strip = strip.child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .px(px(6.0))
                .py(px(6.0))
                .border_t(px(1.0))
                .border_color(rgba(0xFFFFFF0B_u32))
                .child(
                    div()
                        .text_size(px(8.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgba(0xFFFFFF38_u32))
                        .child("master"),
                ),
        );
    }

    // ── Name footer ───────────────────────────────────────────────────────────
    strip.child(
        div()
            .flex()
            .items_center()
            .justify_center()
            .px(px(4.0))
            .py(px(5.0))
            .border_t(px(1.0))
            .border_color(rgba(0xFFFFFF0F_u32))
            .bg(rgba(0x0000003A_u32)) // rgba(0,0,0, 0.23)
            .child(
                div()
                    .text_size(px(10.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgba(0xEEF2F5AD_u32)) // ~68%
                    .child(name),
            ),
    )
}

// ─── Public: Mixer Panel ──────────────────────────────────────────────────────

pub fn mixer_panel() -> impl IntoElement {
    let accent = strip_accent();

    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(rgba(0x111418FF_u32))
        // Mixer sub-header ("Mixer  6ch")
        .child(mixer_sub_header())
        // Channel strips scroll area — vertically and horizontally scrollable
        // so strips never overlap when the bottom panel is short.
        .child(
            div()
                .flex_1()
                .min_h_0()
                .id("mixer-strips-scroll")
                .overflow_y_scroll()
                .overflow_x_scroll()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .min_h_full()
                        // Five audio channel strips (fixed width, fixed min-height)
                        .children(CHANNELS.iter().map(|&(name, ch_num, db_str, vol_db, ml, mr, has_ins, ins_name)| {
                            channel_strip(name, ch_num, accent, db_str, vol_db, ml, mr, has_ins, ins_name, false)
                        }))
                        // Spacer pushes master to the right when the row is wider than the strips
                        .child(div().flex_1().min_w(px(0.0)))
                        // Master strip — stronger left separator
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_none()
                                .border_l(px(1.0))
                                .border_color(rgba(0xFFFFFF1A_u32))
                                .child(channel_strip(
                                    "Master", "M", accent, "0.00", 0.0,
                                    0.80, 0.78, false, "", true,
                                )),
                        ),
                ),
        )
}
