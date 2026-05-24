use gpui::{px, size, Bounds, Point, TitlebarOptions, WindowBounds, WindowOptions};

pub fn studio_window_options() -> WindowOptions {
    WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: None,
            // appears_transparent → on Windows, GPUI sets
            // `hide_title_bar = true` and routes WM_NCHITTEST through
            // its own callback so `WindowControlArea::Drag` regions in
            // the chrome become HTCAPTION hits. Without this the OS
            // draws its own title bar and our drag region is dead.
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::default(),
            size: size(px(1400.0), px(900.0)),
        })),
        focus: true,
        show: true,
        // Pin transport-relevant flags explicitly. `is_movable=false`
        // short-circuits the NCHITTEST drag path on Windows
        // (`platform/windows/events.rs:863`) — losing this defensively
        // is a silent "titlebar can't drag" regression.
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        ..Default::default()
    }
}
