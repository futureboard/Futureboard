use gpui::{px, size, Bounds, Point, TitlebarOptions, WindowBounds, WindowOptions};

pub fn studio_window_options() -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(1400.0), px(900.0)),
        })),
        focus: true,
        show: true,
        ..Default::default()
    }
}
