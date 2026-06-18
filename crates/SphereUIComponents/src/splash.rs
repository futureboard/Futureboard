//! Standalone boot splash window.
//!
//! Shown immediately at process launch, before Welcome or Studio windows exist.
//! Displays only the splash artwork at logical 670×350 (source PNG is @2x).

use gpui::{
    div, img, px, App, AppContext, Context, IntoElement, ObjectFit, ParentElement, Render,
    SharedString, Styled, StyledImage, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions,
};

use crate::embedded_assets::{splash_image_available, SPLASH_IMAGE_PATH};
use crate::theme::{self, Colors};
use crate::window_position::centered_window_bounds;

/// Splash window size in logical pixels (source asset is 1340×700 @2x).
pub const SPLASH_WIDTH: f32 = 670.0;
pub const SPLASH_HEIGHT: f32 = 350.0;

pub struct SplashWindow {
    image_available: bool,
}

impl SplashWindow {
    pub fn new() -> Self {
        let image_available = splash_image_available();
        if !image_available {
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| {
                eprintln!(
                    "[splash] missing splash asset at {SPLASH_IMAGE_PATH}; using fallback panel"
                );
            });
        }
        Self { image_available }
    }
}

impl Render for SplashWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if self.image_available {
            div()
                .size_full()
                .overflow_hidden()
                .bg(gpui::transparent_black())
                .child(
                    img(SharedString::from(SPLASH_IMAGE_PATH))
                        .w(px(SPLASH_WIDTH))
                        .h(px(SPLASH_HEIGHT))
                        .object_fit(ObjectFit::Contain),
                )
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(Colors::surface_base())
                .font(theme::ui_font())
                .child(
                    div()
                        .text_size(px(16.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child("Futureboard Studio"),
                )
        }
    }
}

/// Borderless centered splash shell. `PopUp` uses `WS_EX_TOOLWINDOW` on Windows
/// so the splash does not claim a separate taskbar button.
pub fn splash_window_options(cx: &mut App) -> WindowOptions {
    let bounds = centered_window_bounds(
        None,
        gpui::size(px(SPLASH_WIDTH), px(SPLASH_HEIGHT)),
        cx,
    );
    WindowOptions {
        titlebar: None,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_background: WindowBackgroundAppearance::Transparent,
        window_decorations: None,
        ..Default::default()
    }
}

pub struct SplashWindowHandle {
    window: WindowHandle<SplashWindow>,
}

impl SplashWindowHandle {
    pub fn open(cx: &mut App) -> Result<Self, String> {
        let options = splash_window_options(cx);
        let handle = cx
            .open_window(options, |_window, cx| {
                cx.new(|_| SplashWindow::new())
            })
            .map_err(|e| e.to_string())?;
        crate::boot::log("splash window shown");
        Ok(Self { window: handle })
    }

    pub fn close(self, cx: &mut App) {
        let _ = self.window.update(cx, |_splash, window, _cx| {
            window.remove_window();
        });
        crate::boot::log("splash window closed");
    }
}
