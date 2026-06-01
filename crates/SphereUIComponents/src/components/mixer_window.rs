//! Detached mixer window for multi-monitor / float layout.
//!
//! Renders from a cloned [`MixerSnapshot`] — never reads [`StudioLayout`] during
//! `Render` (avoids GPUI entity re-entrancy when the main studio is updating).

use std::sync::Arc;

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Point, Render, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind,
};

use crate::components::mixer_panel::{mixer_panel, MixerCallbacks};
use crate::components::timeline::timeline_state::{MasterBusState, TrackState};
use crate::components::title_bar::external_window_titlebar;
use crate::theme::Colors;

pub const MIXER_WINDOW_WIDTH: f32 = 1180.0;
pub const MIXER_WINDOW_HEIGHT: f32 = 420.0;
pub const MIXER_WINDOW_MIN_WIDTH: f32 = 760.0;
pub const MIXER_WINDOW_MIN_HEIGHT: f32 = 320.0;

/// View-model for the external mixer — cloned from the studio timeline.
#[derive(Clone)]
pub struct MixerSnapshot {
    pub tracks: Vec<TrackState>,
    pub master: MasterBusState,
    pub selected_track_id: Option<String>,
    pub mixer_scroll_x: f32,
}

pub struct MixerWindow {
    snapshot: MixerSnapshot,
    callbacks: MixerCallbacks,
    on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
    on_mixer_scroll: Arc<dyn Fn(f32, &mut Window, &mut App) + Send + Sync>,
    focus_handle: FocusHandle,
}

impl MixerWindow {
    pub fn new(
        snapshot: MixerSnapshot,
        callbacks: MixerCallbacks,
        on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
        on_mixer_scroll: Arc<dyn Fn(f32, &mut Window, &mut App) + Send + Sync>,
        cx: &mut Context<Self>,
    ) -> Self {
        external_mixer_debug(&format!(
            "external mixer window created tracks={}",
            snapshot.tracks.len()
        ));
        Self {
            snapshot,
            callbacks,
            on_close,
            on_mixer_scroll,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn set_snapshot(&mut self, snapshot: MixerSnapshot) {
        external_mixer_debug(&format!(
            "external mixer snapshot updated tracks={}",
            snapshot.tracks.len()
        ));
        self.snapshot = snapshot;
    }
}

impl Render for MixerWindow {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let viewport_width: f32 = window.bounds().size.width.into();
        let MixerSnapshot {
            tracks,
            master,
            selected_track_id,
            mixer_scroll_x,
        } = self.snapshot.clone();
        let mixer_callbacks = self.callbacks.clone();
        let on_mixer_scroll = self.on_mixer_scroll.clone();
        let on_close = self.on_close.clone();
        let mixer_viewport_width = (viewport_width - 90.0).max(100.0);

        div()
            .flex()
            .flex_col()
            .size_full()
            .font(crate::theme::ui_font())
            .bg(Colors::surface_window())
            .overflow_hidden()
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar(
                "Mixer",
                "mixer-window-close",
                move |window, cx| {
                    on_close(window, cx);
                    window.remove_window();
                },
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .bg(Colors::bottom_panel_bg())
                    .child(mixer_panel(
                        &tracks,
                        &master,
                        selected_track_id.as_deref(),
                        mixer_callbacks,
                        mixer_scroll_x,
                        mixer_viewport_width,
                        on_mixer_scroll,
                    )),
            )
    }
}

pub fn open_mixer_window(
    owner_bounds: Bounds<gpui::Pixels>,
    snapshot: MixerSnapshot,
    callbacks: MixerCallbacks,
    on_close: Arc<dyn Fn(&mut Window, &mut App) + Send + Sync>,
    on_mixer_scroll: Arc<dyn Fn(f32, &mut Window, &mut App) + Send + Sync>,
    cx: &mut App,
) -> Result<WindowHandle<MixerWindow>, String> {
    external_mixer_debug(&format!(
        "opening external mixer window tracks={}",
        snapshot.tracks.len()
    ));

    let parent_x: f32 = owner_bounds.origin.x.into();
    let parent_y: f32 = owner_bounds.origin.y.into();
    let parent_w: f32 = owner_bounds.size.width.into();
    let parent_h: f32 = owner_bounds.size.height.into();
    let origin = Point {
        x: px((parent_x + parent_w - MIXER_WINDOW_WIDTH).max(24.0)),
        y: px((parent_y + parent_h - MIXER_WINDOW_HEIGHT).max(24.0)),
    };

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(MIXER_WINDOW_WIDTH), px(MIXER_WINDOW_HEIGHT)),
    }));
    options.kind = WindowKind::Floating;
    options.is_resizable = true;
    options.is_minimizable = true;
    options.window_background = WindowBackgroundAppearance::Opaque;
    options.window_min_size = Some(size(
        px(MIXER_WINDOW_MIN_WIDTH),
        px(MIXER_WINDOW_MIN_HEIGHT),
    ));

    cx.open_window(options, move |_window, cx| {
        cx.new(|cx| MixerWindow::new(snapshot, callbacks, on_close, on_mixer_scroll, cx))
    })
    .map_err(|e| e.to_string())
}

pub(crate) fn external_mixer_debug(message: &str) {
    if std::env::var("FUTUREBOARD_EXTERNAL_MIXER_DEBUG")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        eprintln!("[external-mixer] {message}");
    }
}
