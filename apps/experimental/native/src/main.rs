mod app;
mod window;

use sphere_ui_components::embedded_assets::EmbeddedAssets;

fn main() {
    gpui::Application::new()
        .with_assets(EmbeddedAssets::new())
        .run(app::setup);
}

