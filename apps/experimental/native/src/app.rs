use gpui::{App, AppContext};
use sphere_ui_components::assets;
use sphere_ui_components::layout::StudioLayout;
use crate::window::studio_window_options;

pub fn setup(cx: &mut App) {
    assets::register_fonts(cx);

    let options = studio_window_options();
    cx.open_window(options, |_window, cx| {
        cx.new(|_cx| StudioLayout::new())
    })
    .expect("failed to open studio window");
}
