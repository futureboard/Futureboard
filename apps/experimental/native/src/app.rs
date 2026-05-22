use gpui::{App, AppContext};
use sphere_ui_components::layout::StudioLayout;
use crate::window::studio_window_options;

pub fn setup(cx: &mut App) {
    let options = studio_window_options();
    cx.open_window(options, |_window, cx| {
        cx.new(|_cx| StudioLayout)
    })
    .expect("failed to open studio window");
}
