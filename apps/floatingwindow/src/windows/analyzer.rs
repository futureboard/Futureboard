// CentralPanel::show(ctx) — see note in mixer.rs
#![allow(deprecated)]
use egui::RichText;

use crate::theme;

pub struct AnalyzerWindow {
    dummy_bins: Vec<f32>,
}

impl AnalyzerWindow {
    pub fn new() -> Self {
        let bins: Vec<f32> = (0..64)
            .map(|i| {
                let x = i as f32 / 64.0;
                let base = (1.0 - x) * 0.6;
                let peak = if i == 8 { 0.9 } else if i == 20 { 0.75 } else { 0.0 };
                (base + peak).min(1.0)
            })
            .collect();
        Self { dummy_bins: bins }
    }

    pub fn show(&self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: theme::BG,
                inner_margin: egui::Margin::same(8),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.label(theme::header_rt("SPECTRUM ANALYZER"));
                ui.add_space(6.0);

                let available = ui.available_size();
                let display_h = (available.y - 44.0).max(100.0);
                let display_w = available.x;

                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(display_w, display_h),
                    egui::Sense::hover(),
                );

                // Background
                ui.painter().rect_filled(rect, egui::CornerRadius::same(3), theme::SUNKEN);

                // dB grid lines
                for db in &[-12.0f32, -24.0, -36.0, -48.0] {
                    let level = theme::db_to_linear(*db);
                    let y = rect.max.y - level * rect.height();
                    ui.painter().line_segment(
                        [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                        egui::Stroke::new(1.0, theme::grid_minor()),
                    );
                    ui.painter().text(
                        egui::pos2(rect.min.x + 4.0, y - 2.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("{:.0}dB", db),
                        egui::FontId::monospace(7.0),
                        theme::FAINT,
                    );
                }

                // Spectrum bars — gradient from METER_LOW → METER_CLIP
                let bin_count = self.dummy_bins.len();
                let bar_w = display_w / bin_count as f32;

                for (i, &level) in self.dummy_bins.iter().enumerate() {
                    let x = rect.min.x + i as f32 * bar_w;
                    let bar_h = level * rect.height();
                    let bar_rect = egui::Rect::from_min_size(
                        egui::pos2(x, rect.max.y - bar_h),
                        egui::vec2((bar_w - 1.0).max(1.0), bar_h),
                    );

                    let color = if level > 0.9 {
                        theme::METER_CLIP
                    } else if level > 0.75 {
                        theme::METER_HIGH
                    } else if level > 0.5 {
                        theme::METER_MID
                    } else {
                        theme::METER_LOW
                    };
                    ui.painter().rect_filled(bar_rect, 0.0, color);
                }

                // Border
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(1.0, theme::BORDER),
                    egui::StrokeKind::Outside,
                );

                ui.add_space(8.0);
                ui.label(
                    RichText::new("Spectrum Analyzer — placeholder (live FFT not connected)")
                        .size(9.0)
                        .color(theme::FAINT),
                );
            });
    }
}

impl Default for AnalyzerWindow {
    fn default() -> Self {
        Self::new()
    }
}
