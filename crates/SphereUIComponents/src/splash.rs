//! Boot splash window.
//!
//! A small borderless window showing the splash artwork with a status line at
//! the bottom-center. Shown immediately at launch and closed once the main
//! Studio window's first frame is ready (see `apps/native/app.rs`). The status
//! string is updated as boot phases progress ("Initializing audio…", etc.).

use gpui::{
    div, img, px, Context, IntoElement, ParentElement, Render, SharedString, Styled, Window,
};

use crate::embedded_assets::SPLASH_IMAGE_PATH;
use crate::theme::{self, Colors};

/// Splash window size in logical pixels.
pub const SPLASH_WIDTH: f32 = 670.0;
pub const SPLASH_HEIGHT: f32 = 350.0;

pub struct SplashWindow {
    status: SharedString,
}

impl SplashWindow {
    pub fn new(status: impl Into<SharedString>) -> Self {
        Self {
            status: status.into(),
        }
    }

    /// Update the status line shown at the bottom of the splash. The caller is
    /// responsible for `cx.notify()` (done via the window handle's `update`).
    pub fn set_status(&mut self, status: impl Into<SharedString>) {
        self.status = status.into();
    }
}

impl Render for SplashWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(Colors::surface_base())
            .font(theme::ui_font())
            // Splash artwork fills the window.
            .child(
                img(SharedString::from(SPLASH_IMAGE_PATH))
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(SPLASH_WIDTH))
                    .h(px(SPLASH_HEIGHT)),
            )
            // Status text, bottom-center over the artwork.
            .child(
                div()
                    .absolute()
                    .bottom(px(18.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Colors::text_secondary())
                            .child(self.status.clone()),
                    ),
            )
    }
}
