// CentralPanel::show(ctx) is deprecated in favour of show_inside(ui), but
// inside deferred viewport callbacks we only have &egui::Context, not &mut Ui.
#![allow(deprecated)]
use std::collections::VecDeque;

use crossbeam_channel::Sender;
use egui::RichText;
use serde_json::json;

use crate::protocol::{MidiDevice, MidiEvent, OutgoingMessage};
use crate::theme;

const MAX_EVENTS: usize = 500;
const ROW_H: f32 = 14.0; // MIDI row height matching WebUI MidiEditorPanel
const PIANO_W: f32 = 40.0; // piano keyboard sidebar width

#[derive(Debug, Clone, PartialEq, Eq)]
enum MidiTab {
    Monitor,
    Devices,
    PianoRoll,
}

pub struct MidiWindow {
    devices: Vec<MidiDevice>,
    events: VecDeque<MidiEvent>,
    active_tab: MidiTab,
    auto_scroll: bool,
}

impl MidiWindow {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            events: VecDeque::new(),
            active_tab: MidiTab::Monitor,
            auto_scroll: true,
        }
    }

    pub fn update_devices(&mut self, devices: Vec<MidiDevice>) {
        self.devices = devices;
    }

    pub fn push_event(&mut self, event: MidiEvent) {
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    pub fn show(&mut self, ctx: &egui::Context, out_tx: &Sender<OutgoingMessage>, _win_id: &str) {
        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: theme::BG,
                inner_margin: egui::Margin::same(0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                // Tab bar
                egui::Frame {
                    fill: theme::SURFACE,
                    inner_margin: egui::Margin { left: 8, right: 8, top: 6, bottom: 6 },
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        tab_button(ui, "MONITOR", self.active_tab == MidiTab::Monitor, || {
                            self.active_tab = MidiTab::Monitor;
                        });
                        tab_button(ui, "DEVICES", self.active_tab == MidiTab::Devices, || {
                            self.active_tab = MidiTab::Devices;
                        });
                        tab_button(ui, "PIANO ROLL", self.active_tab == MidiTab::PianoRoll, || {
                            self.active_tab = MidiTab::PianoRoll;
                        });
                    });
                });

                ui.add_space(0.0);

                egui::Frame {
                    fill: theme::BG,
                    inner_margin: egui::Margin::same(8),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    match self.active_tab {
                        MidiTab::Monitor => self.show_monitor(ui),
                        MidiTab::Devices => self.show_devices(ui, out_tx),
                        MidiTab::PianoRoll => self.show_piano_roll(ui),
                    }
                });
            });
    }

    fn show_monitor(&mut self, ui: &mut egui::Ui) {
        // Header row
        egui::Frame {
            fill: theme::SURFACE,
            inner_margin: egui::Margin { left: 6, right: 6, top: 4, bottom: 4 },
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                col_header(ui, "TIME", 72.0);
                col_header(ui, "DEVICE", 100.0);
                col_header(ui, "CH", 28.0);
                col_header(ui, "TYPE", 72.0);
                col_header(ui, "DATA", 0.0); // takes rest
            });
        });

        // Event list
        let scroll = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll);

        scroll.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;

            if self.events.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        RichText::new("No MIDI events — waiting for input")
                            .size(10.0)
                            .color(theme::FAINT),
                    );
                });
            } else {
                for (idx, event) in self.events.iter().enumerate() {
                    let ts = format_timestamp(event.timestamp);
                    let kcolor = kind_color(&event.kind);
                    let data = format_event_data(event);

                    let row_fill = if idx % 2 == 0 { theme::BG } else { theme::SURFACE };

                    egui::Frame {
                        fill: row_fill,
                        inner_margin: egui::Margin { left: 6, right: 6, top: 2, bottom: 2 },
                        ..Default::default()
                    }
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            mono_cell(ui, &ts, 72.0, theme::DIM);
                            mono_cell(ui, truncate(&event.device_id, 14), 100.0, theme::TEXT_SOFT);
                            mono_cell(ui, &format!("{}", event.channel), 28.0, theme::DIM);
                            mono_cell(ui, &event.kind, 72.0, kcolor);
                            mono_cell_rest(ui, &data, theme::TEXT);
                        });
                    });
                }
            }
        });

        // Footer
        ui.separator();
        egui::Frame {
            fill: theme::SURFACE,
            inner_margin: egui::Margin { left: 8, right: 8, top: 4, bottom: 4 },
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.auto_scroll, RichText::new("Auto-scroll").size(9.0).color(theme::DIM));
                ui.add_space(8.0);
                if ui
                    .add(egui::Button::new(RichText::new("Clear").size(9.0)).fill(theme::SURFACE_HIGH))
                    .clicked()
                {
                    self.events.clear();
                }
            });
        });
    }

    fn show_devices(&mut self, ui: &mut egui::Ui, out_tx: &Sender<OutgoingMessage>) {
        ui.horizontal(|ui| {
            ui.label(theme::header_rt("MIDI DEVICES"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Refresh").size(9.0).color(theme::ACCENT))
                            .fill(theme::SURFACE_HIGH),
                    )
                    .clicked()
                {
                    let _ = out_tx.send(OutgoingMessage::Command {
                        command_id: "midi.refreshDevices".into(),
                        payload: json!({}),
                    });
                }
            });
        });
        ui.add_space(6.0);

        if self.devices.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(
                    RichText::new("No MIDI devices found")
                        .size(10.0)
                        .color(theme::FAINT),
                );
            });
        } else {
            for device in &mut self.devices {
                egui::Frame {
                    fill: theme::SURFACE,
                    inner_margin: egui::Margin::same(8),
                    corner_radius: egui::CornerRadius::same(4),
                    stroke: egui::Stroke::new(1.0, theme::border_soft()),
                    ..Default::default()
                }
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // IN / OUT badge
                        let (badge_text, badge_color) = if device.is_input {
                            ("IN", theme::GREEN)
                        } else {
                            ("OUT", theme::ORANGE)
                        };
                        ui.label(
                            RichText::new(badge_text)
                                .size(8.0)
                                .color(badge_color)
                                .strong(),
                        );
                        ui.add_space(6.0);
                        ui.label(RichText::new(&device.name).size(10.0).color(theme::TEXT));

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let prev = device.enabled;
                            ui.checkbox(&mut device.enabled, "");
                            if device.enabled != prev {
                                let _ = out_tx.send(OutgoingMessage::Command {
                                    command_id: "midi.setDeviceEnabled".into(),
                                    payload: json!({
                                        "deviceId": device.id,
                                        "enabled": device.enabled
                                    }),
                                });
                            }
                        });
                    });
                });
                ui.add_space(2.0);
            }
        }

        ui.add_space(12.0);
        if ui
            .add(
                egui::Button::new(
                    RichText::new("MIDI Panic — All Notes Off")
                        .size(9.0)
                        .color(theme::RED),
                )
                .fill(theme::SURFACE_HIGH)
                .stroke(egui::Stroke::new(1.0, theme::RED)),
            )
            .clicked()
        {
            let _ = out_tx.send(OutgoingMessage::Command {
                command_id: "midi.panic".into(),
                payload: json!({}),
            });
        }
    }

    fn show_piano_roll(&self, ui: &mut egui::Ui) {
        ui.label(theme::header_rt("PIANO ROLL"));
        ui.add_space(4.0);

        // Timeline ruler
        let ruler_height = 18.0;
        let available_w = ui.available_width();
        let (ruler_rect, _) = ui.allocate_exact_size(
            egui::vec2(available_w, ruler_height),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(ruler_rect, 0.0, theme::SUNKEN);

        // Bar numbers
        let bar_width = 64.0;
        let bars = (ruler_rect.width() / bar_width) as u32 + 1;
        for i in 0..bars {
            let x = ruler_rect.min.x + PIANO_W + i as f32 * bar_width;
            if x > ruler_rect.max.x { break; }
            ui.painter().text(
                egui::pos2(x + 4.0, ruler_rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{}", i + 1),
                egui::FontId::monospace(8.0),
                theme::DIM,
            );
            // bar line
            ui.painter().line_segment(
                [egui::pos2(x, ruler_rect.min.y), egui::pos2(x, ruler_rect.max.y)],
                egui::Stroke::new(1.0, theme::grid_major()),
            );
        }

        // Main area: piano sidebar + note grid
        let avail = ui.available_size();
        let grid_h = avail.y;
        let note_count = 128_u32;
        let visible_rows = (grid_h / ROW_H).ceil() as u32;
        let start_note = note_count.saturating_sub(visible_rows); // top = highest notes

        let (area_rect, _) = ui.allocate_exact_size(
            egui::vec2(avail.x, grid_h),
            egui::Sense::hover(),
        );

        let piano_rect = egui::Rect::from_min_size(
            area_rect.min,
            egui::vec2(PIANO_W, grid_h),
        );
        let grid_rect = egui::Rect::from_min_size(
            egui::pos2(area_rect.min.x + PIANO_W, area_rect.min.y),
            egui::vec2(area_rect.width() - PIANO_W, grid_h),
        );

        ui.painter().rect_filled(grid_rect, 0.0, theme::BG);

        for row in 0..visible_rows {
            let note = (note_count - 1 - start_note - row) as u8;
            let pitch = note % 12;
            let is_black = matches!(pitch, 1 | 3 | 6 | 8 | 10);
            let is_c = pitch == 0;

            let y = area_rect.min.y + row as f32 * ROW_H;
            let row_r = egui::Rect::from_min_size(
                egui::pos2(grid_rect.min.x, y),
                egui::vec2(grid_rect.width(), ROW_H),
            );

            // Row background
            let row_bg = if is_black {
                theme::midi_black_key()
            } else {
                theme::midi_white_key()
            };
            ui.painter().rect_filled(row_r, 0.0, row_bg);

            // Divider line
            let div_color = if is_c {
                theme::midi_c_divider()
            } else {
                theme::midi_divider()
            };
            ui.painter().line_segment(
                [egui::pos2(grid_rect.min.x, y), egui::pos2(grid_rect.max.x, y)],
                egui::Stroke::new(1.0, div_color),
            );

            // Piano key sidebar
            let key_r = egui::Rect::from_min_size(
                egui::pos2(piano_rect.min.x, y),
                egui::vec2(PIANO_W, ROW_H),
            );
            let key_fill = if is_black { theme::SURFACE } else { theme::SURFACE_HIGH };
            ui.painter().rect_filled(key_r, 0.0, key_fill);

            // C note label
            if is_c {
                let octave = note / 12;
                ui.painter().text(
                    egui::pos2(piano_rect.min.x + 3.0, y + ROW_H * 0.5),
                    egui::Align2::LEFT_CENTER,
                    format!("C{}", octave.saturating_sub(1)),
                    egui::FontId::monospace(7.0),
                    theme::DIM,
                );
                // C divider on sidebar too
                ui.painter().line_segment(
                    [egui::pos2(piano_rect.min.x, y), egui::pos2(piano_rect.max.x, y)],
                    egui::Stroke::new(1.0, theme::midi_c_divider()),
                );
            }
        }

        // Bar grid lines on the note area
        for i in 0..bars {
            let x = grid_rect.min.x + i as f32 * bar_width;
            if x > grid_rect.max.x { break; }
            ui.painter().line_segment(
                [egui::pos2(x, grid_rect.min.y), egui::pos2(x, grid_rect.max.y)],
                egui::Stroke::new(1.0, theme::grid_major()),
            );
        }

        // Placeholder label
        ui.painter().text(
            grid_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Piano Roll — Full editor coming soon",
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgba_unmultiplied(0x9A, 0xA7, 0xB8, 80),
        );
    }
}

impl Default for MidiWindow {
    fn default() -> Self {
        Self::new()
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn tab_button(ui: &mut egui::Ui, label: &str, active: bool, on_click: impl FnOnce()) {
    let fill = if active { theme::SURFACE_ACTIVE } else { theme::SURFACE_HIGH };
    let text_color = if active { theme::ACCENT } else { theme::TEXT_SOFT };
    let stroke = if active {
        egui::Stroke::new(1.0, theme::ACCENT)
    } else {
        egui::Stroke::new(1.0, theme::border_soft())
    };

    if ui
        .add(
            egui::Button::new(RichText::new(label).size(9.0).color(text_color).strong())
                .fill(fill)
                .stroke(stroke)
                .corner_radius(egui::CornerRadius::same(3)),
        )
        .clicked()
    {
        on_click();
    }
}

fn col_header(ui: &mut egui::Ui, text: &str, width: f32) {
    if width > 0.0 {
        ui.add_sized(
            [width, 14.0],
            egui::Label::new(RichText::new(text).size(8.0).color(theme::FAINT).strong()),
        );
    } else {
        ui.label(RichText::new(text).size(8.0).color(theme::FAINT).strong());
    }
}

fn mono_cell(ui: &mut egui::Ui, text: &str, width: f32, color: egui::Color32) {
    ui.add_sized(
        [width, ROW_H],
        egui::Label::new(
            RichText::new(text)
                .font(egui::FontId::monospace(9.0))
                .color(color),
        ),
    );
}

fn mono_cell_rest(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    ui.label(
        RichText::new(text)
            .font(egui::FontId::monospace(9.0))
            .color(color),
    );
}

fn format_timestamp(ts: f64) -> String {
    let secs = (ts / 1000.0) as u64;
    let ms = (ts % 1000.0) as u32;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}.{:03}", m, s, ms)
}

fn kind_color(kind: &str) -> egui::Color32 {
    match kind {
        "noteOn" => theme::GREEN,
        "noteOff" => theme::DIM,
        "cc" => theme::ACCENT,
        "pitchBend" => theme::YELLOW,
        "aftertouch" => theme::VIOLET,
        _ => theme::FAINT,
    }
}

fn format_event_data(event: &MidiEvent) -> String {
    match event.kind.as_str() {
        "noteOn" | "noteOff" => {
            let note = event.note.unwrap_or(0);
            let vel = event.velocity.unwrap_or(0);
            format!("{} vel={}", note_name(note), vel)
        }
        "cc" => {
            let cc = event.cc.unwrap_or(0);
            let val = event.value.unwrap_or(0);
            format!("CC#{} val={}", cc, val)
        }
        _ => String::new(),
    }
}

fn note_name(note: u8) -> String {
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = note / 12;
    let pitch = (note % 12) as usize;
    format!("{}{}", names[pitch], octave.saturating_sub(1))
}

fn truncate(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars { s } else { &s[..max_chars] }
}
