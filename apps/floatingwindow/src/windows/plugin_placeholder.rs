// CentralPanel::show(ctx) — see note in mixer.rs
#![allow(deprecated)]
use egui::RichText;

use crate::theme;

pub struct PluginPlaceholderWindow {
    plugin_name: String,
}

impl PluginPlaceholderWindow {
    pub fn new() -> Self {
        Self {
            plugin_name: "Plugin Editor".into(),
        }
    }

    pub fn show(&self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: theme::BG,
                inner_margin: egui::Margin::same(16),
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.label(theme::header_rt("PLUGIN EDITOR"));
                ui.add_space(2.0);
                ui.add(egui::Separator::default().spacing(1.0));
                ui.add_space(20.0);

                ui.vertical_centered(|ui| {
                    // Plugin icon
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(8), theme::SURFACE_HIGH);
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::same(8),
                        egui::Stroke::new(1.0, theme::accent_soft()),
                        egui::StrokeKind::Middle,
                    );
                    // Simple plug icon via lines
                    let cx = rect.center();
                    ui.painter().circle_filled(cx, 18.0, theme::SURFACE_ACTIVE);
                    ui.painter().circle_stroke(
                        cx,
                        18.0,
                        egui::Stroke::new(1.5, theme::ACCENT),
                    );
                    // "plug" prongs
                    let prong_color = theme::ACCENT;
                    ui.painter().line_segment(
                        [egui::pos2(cx.x - 6.0, cx.y - 18.0), egui::pos2(cx.x - 6.0, cx.y - 24.0)],
                        egui::Stroke::new(2.5, prong_color),
                    );
                    ui.painter().line_segment(
                        [egui::pos2(cx.x + 6.0, cx.y - 18.0), egui::pos2(cx.x + 6.0, cx.y - 24.0)],
                        egui::Stroke::new(2.5, prong_color),
                    );

                    ui.add_space(12.0);

                    ui.label(
                        RichText::new(&self.plugin_name)
                            .size(13.0)
                            .color(theme::TEXT),
                    );

                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("Plugin Editor UI — coming soon")
                            .size(10.0)
                            .color(theme::DIM),
                    );

                    ui.add_space(4.0);

                    ui.label(
                        RichText::new("VST / CLAP hosting not yet implemented")
                            .size(9.0)
                            .color(theme::FAINT),
                    );

                    ui.add_space(20.0);

                    // Parameter knobs section
                    ui.label(theme::header_rt("PARAMETERS"));
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        for name in &["Gain", "Pan", "Drive", "Mix"] {
                            param_knob_placeholder(ui, name);
                        }
                    });
                });
            });
    }
}

impl Default for PluginPlaceholderWindow {
    fn default() -> Self {
        Self::new()
    }
}

fn param_knob_placeholder(ui: &mut egui::Ui, label: &str) {
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        let cx = rect.center();

        // Knob body
        ui.painter().circle_filled(cx, 17.0, theme::SURFACE_HIGH);
        ui.painter().circle_stroke(cx, 17.0, egui::Stroke::new(1.0, theme::BORDER));

        // Indicator line (12 o'clock position)
        let tip = egui::pos2(cx.x, rect.min.y + 3.0);
        ui.painter()
            .line_segment([cx, tip], egui::Stroke::new(2.0, theme::ACCENT));

        ui.add_space(2.0);
        ui.label(
            RichText::new(label)
                .size(8.0)
                .color(theme::DIM),
        );
    });
}
