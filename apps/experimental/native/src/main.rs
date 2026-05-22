mod app;
mod window;

fn main() {
    gpui::Application::new().run(app::setup);
}
