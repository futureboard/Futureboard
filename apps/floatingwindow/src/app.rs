use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender};
use egui::ViewportBuilder;

use crate::protocol::*;
use crate::theme;
use crate::windows::{
    analyzer::AnalyzerWindow, midi::MidiWindow, mixer::MixerWindow,
    plugin_placeholder::PluginPlaceholderWindow,
};

// ── window content ────────────────────────────────────────────────────────────

pub enum WindowContent {
    Mixer(Arc<Mutex<MixerWindow>>),
    Midi(Arc<Mutex<MidiWindow>>),
    Analyzer(Arc<Mutex<AnalyzerWindow>>),
    Plugin(Arc<Mutex<PluginPlaceholderWindow>>),
}

pub struct OpenWindow {
    pub title: String,
    pub content: WindowContent,
    pub initial_size: [f32; 2],
    pub always_on_top: bool,
}

// ── app ───────────────────────────────────────────────────────────────────────

pub struct App {
    ipc_rx: Receiver<IncomingMessage>,
    out_tx: Sender<OutgoingMessage>,
    windows: HashMap<String, OpenWindow>,
    pending_closes: Arc<Mutex<Vec<String>>>,
    pending_focuses: Arc<Mutex<Vec<String>>>,
}

impl App {
    pub fn new(ipc_rx: Receiver<IncomingMessage>, out_tx: Sender<OutgoingMessage>) -> Self {
        Self {
            ipc_rx,
            out_tx,
            windows: HashMap::new(),
            pending_closes: Arc::new(Mutex::new(Vec::new())),
            pending_focuses: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn handle_message(&mut self, ctx: &egui::Context, msg: IncomingMessage) {
        match msg {
            IncomingMessage::OpenWindow { window } => self.open_window(ctx, window),
            IncomingMessage::CloseWindow { id } => {
                if self.windows.remove(&id).is_some() {
                    let _ = self.out_tx.send(OutgoingMessage::WindowClosed { id });
                }
            }
            IncomingMessage::FocusWindow { id } => {
                self.pending_focuses.lock().unwrap().push(id);
            }
            IncomingMessage::MixerUpdate { tracks, master } => {
                for win in self.windows.values() {
                    if let WindowContent::Mixer(m) = &win.content {
                        m.lock().unwrap().update_tracks(tracks.clone(), master.clone());
                    }
                }
            }
            IncomingMessage::MidiUpdateDevices { devices } => {
                for win in self.windows.values() {
                    if let WindowContent::Midi(m) = &win.content {
                        m.lock().unwrap().update_devices(devices.clone());
                    }
                }
            }
            IncomingMessage::MidiEvent { event } => {
                for win in self.windows.values() {
                    if let WindowContent::Midi(m) = &win.content {
                        m.lock().unwrap().push_event(event.clone());
                    }
                }
            }
        }
    }

    fn open_window(&mut self, ctx: &egui::Context, desc: FloatingWindowDescriptor) {
        if self.windows.contains_key(&desc.id) {
            ctx.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of(&desc.id),
                egui::ViewportCommand::Focus,
            );
            return;
        }

        let initial_size = desc
            .initial_bounds
            .as_ref()
            .map(|b| [b.width, b.height])
            .unwrap_or_else(|| match desc.kind {
                WindowKind::Mixer => [960.0, 300.0],
                WindowKind::Midi => [740.0, 540.0],
                WindowKind::Analyzer => [640.0, 420.0],
                WindowKind::PluginEditorPlaceholder => [520.0, 440.0],
            });

        let content = match desc.kind {
            WindowKind::Mixer => {
                WindowContent::Mixer(Arc::new(Mutex::new(MixerWindow::new())))
            }
            WindowKind::Midi => {
                WindowContent::Midi(Arc::new(Mutex::new(MidiWindow::new())))
            }
            WindowKind::Analyzer => {
                WindowContent::Analyzer(Arc::new(Mutex::new(AnalyzerWindow::new())))
            }
            WindowKind::PluginEditorPlaceholder => {
                WindowContent::Plugin(Arc::new(Mutex::new(PluginPlaceholderWindow::new())))
            }
        };

        let _ = self
            .out_tx
            .send(OutgoingMessage::WindowOpened { id: desc.id.clone() });

        self.windows.insert(
            desc.id,
            OpenWindow {
                title: desc.title,
                content,
                initial_size,
                always_on_top: desc.always_on_top,
            },
        );
    }

    fn tick(&mut self, ctx: &egui::Context) {
        // Keep visuals in sync (supports hot-reloading in future)
        ctx.set_visuals(theme::visuals());

        while let Ok(msg) = self.ipc_rx.try_recv() {
            self.handle_message(ctx, msg);
        }

        let closes: Vec<String> = self.pending_closes.lock().unwrap().drain(..).collect();
        for id in closes {
            self.windows.remove(&id);
            let _ = self.out_tx.send(OutgoingMessage::WindowClosed { id });
        }

        let focuses: Vec<String> = self.pending_focuses.lock().unwrap().drain(..).collect();
        for id in focuses {
            ctx.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of(&id),
                egui::ViewportCommand::Focus,
            );
        }

        let window_ids: Vec<String> = self.windows.keys().cloned().collect();
        for id in window_ids {
            let Some(win) = self.windows.get(&id) else { continue };

            let mut builder = ViewportBuilder::default()
                .with_title(&win.title)
                .with_inner_size(win.initial_size)
                .with_min_inner_size([300.0, 180.0]);

            if win.always_on_top {
                builder = builder.with_always_on_top();
            }

            let closes = Arc::clone(&self.pending_closes);
            let out_tx = self.out_tx.clone();
            let win_id = id.clone();

            match &win.content {
                WindowContent::Mixer(m) => {
                    let m = Arc::clone(m);
                    ctx.show_viewport_deferred(
                        egui::ViewportId::from_hash_of(&id),
                        builder,
                        move |ctx, _| {
                            handle_close(ctx, &closes, &win_id);
                            m.lock().unwrap().show(ctx, &out_tx, &win_id);
                        },
                    );
                }
                WindowContent::Midi(m) => {
                    let m = Arc::clone(m);
                    ctx.show_viewport_deferred(
                        egui::ViewportId::from_hash_of(&id),
                        builder,
                        move |ctx, _| {
                            handle_close(ctx, &closes, &win_id);
                            m.lock().unwrap().show(ctx, &out_tx, &win_id);
                        },
                    );
                }
                WindowContent::Analyzer(a) => {
                    let a = Arc::clone(a);
                    ctx.show_viewport_deferred(
                        egui::ViewportId::from_hash_of(&id),
                        builder,
                        move |ctx, _| {
                            handle_close(ctx, &closes, &win_id);
                            a.lock().unwrap().show(ctx);
                        },
                    );
                }
                WindowContent::Plugin(p) => {
                    let p = Arc::clone(p);
                    ctx.show_viewport_deferred(
                        egui::ViewportId::from_hash_of(&id),
                        builder,
                        move |ctx, _| {
                            handle_close(ctx, &closes, &win_id);
                            p.lock().unwrap().show(ctx);
                        },
                    );
                }
            }
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tick(&ctx);

        // Host window — status strip
        ui.label(
            egui::RichText::new("Futureboard")
                .size(11.0)
                .color(theme::ACCENT)
                .strong(),
        );
        ui.label(
            egui::RichText::new("FloatingWindow Runtime")
                .size(9.0)
                .color(theme::FAINT),
        );
        ui.separator();
        ui.label(
            egui::RichText::new(format!("{} window(s) open", self.windows.len()))
                .size(9.0)
                .color(theme::DIM),
        );

        let _ = frame;
    }
}

fn handle_close(
    ctx: &egui::Context,
    pending: &Arc<Mutex<Vec<String>>>,
    id: &str,
) {
    if ctx.input(|i| i.viewport().close_requested()) {
        pending.lock().unwrap().push(id.to_string());
    }
}
