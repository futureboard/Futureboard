mod app;
mod displays;
mod fonts;
mod ipc;
mod protocol;
mod theme;
mod window_manager;
mod windows;

use crossbeam_channel::unbounded;

fn main() {
    // Log to stderr — stdout is reserved for IPC JSON
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Futureboard FloatingWindow Runtime starting");

    let (out_tx, out_rx) = unbounded::<protocol::OutgoingMessage>();
    let in_rx = ipc::spawn_ipc(out_rx);
    let egui_app = app::App::new(in_rx, out_tx);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Futureboard FloatingWindow Runtime")
            .with_inner_size([240.0, 96.0])
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "futureboard-floatingwindow",
        options,
        Box::new(|cc| {
            // Install Inter font and Futureboard visuals before first frame
            fonts::setup(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(theme::visuals());
            Ok(Box::new(egui_app))
        }),
    ) {
        tracing::error!("eframe error: {e}");
        std::process::exit(1);
    }
}
