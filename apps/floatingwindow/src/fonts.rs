//! Embeds Inter font from packages/shared/fonts/ into the binary.
//! Inter is the WebUI's primary typeface.

use egui::{FontData, FontDefinitions, FontFamily};

// Embed Inter weights — paths are relative to this source file.
static INTER_REGULAR: &[u8] =
    include_bytes!("../../../packages/shared/fonts/Inter-Regular.ttf");
static INTER_MEDIUM: &[u8] =
    include_bytes!("../../../packages/shared/fonts/Inter-Medium.ttf");
static INTER_SEMIBOLD: &[u8] =
    include_bytes!("../../../packages/shared/fonts/Inter-SemiBold.ttf");

/// Register Inter as the primary proportional font and install it into the
/// egui context. Call this once during app creation (CreationContext).
pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "Inter-Regular".into(),
        FontData::from_static(INTER_REGULAR).into(),
    );
    fonts.font_data.insert(
        "Inter-Medium".into(),
        FontData::from_static(INTER_MEDIUM).into(),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".into(),
        FontData::from_static(INTER_SEMIBOLD).into(),
    );

    // Inter-SemiBold → Proportional (labels, headers use `.strong()` automatically)
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .splice(0..0, ["Inter-Regular".into(), "Inter-SemiBold".into()]);

    // Inter-Regular → Monospace (dB readouts, timestamps)
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .splice(0..0, ["Inter-Regular".into()]);

    ctx.set_fonts(fonts);
}
