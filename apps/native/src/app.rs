use crate::window::studio_window_options;
use gpui::{App, AppContext};
use sphere_ui_components::assets;
use sphere_ui_components::layout::StudioLayout;

pub fn setup(cx: &mut App) {
    eprintln!("[boot] register fonts");
    assets::register_fonts(cx);

    eprintln!("[boot] open studio window");
    let options = studio_window_options();
    cx.open_window(options, |_window, cx| {
        eprintln!("[boot] build StudioLayout");
        let layout = cx.new(|cx| StudioLayout::new(cx));
        eprintln!("[boot] StudioLayout built");
        layout
    })
    .expect("failed to open studio window");
    eprintln!("[boot] open_window returned");
}
