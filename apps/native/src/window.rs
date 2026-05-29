use gpui::{
    px, size, App, Bounds, Point, WindowBackgroundAppearance, WindowBounds, WindowDecorations,
    WindowKind, WindowOptions,
};

use sphere_ui_components::platform_chrome;
use sphere_ui_components::splash::{SPLASH_HEIGHT, SPLASH_WIDTH};

pub fn studio_window_options() -> WindowOptions {
    let mut options = platform_chrome::studio_window_options();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin: Point::default(),
        size: size(px(1400.0), px(900.0)),
    }));
    options
}

/// Borderless, fixed-size splash window centered on the primary display.
pub fn splash_window_options(cx: &App) -> WindowOptions {
    let splash_size = size(px(SPLASH_WIDTH), px(SPLASH_HEIGHT));

    // Center on the primary display; fall back to a reasonable offset if the
    // display list is unavailable.
    let origin = cx
        .primary_display()
        .map(|display| {
            let b = display.bounds();
            let ox = f32::from(b.origin.x);
            let oy = f32::from(b.origin.y);
            let dw = f32::from(b.size.width);
            let dh = f32::from(b.size.height);
            Point {
                x: px(ox + (dw - SPLASH_WIDTH).max(0.0) / 2.0),
                y: px(oy + (dh - SPLASH_HEIGHT).max(0.0) / 2.0),
            }
        })
        .unwrap_or(Point {
            x: px(420.0),
            y: px(260.0),
        });

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin,
            size: splash_size,
        })),
        // Borderless: no OS titlebar, app draws the artwork edge-to-edge.
        titlebar: None,
        focus: true,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        kind: WindowKind::PopUp,
        window_background: WindowBackgroundAppearance::Opaque,
        window_decorations: Some(WindowDecorations::Client),
        ..Default::default()
    }
}
