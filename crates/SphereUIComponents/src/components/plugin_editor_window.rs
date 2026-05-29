//! Native plugin editor window (Phase 4 — GPUI-hosted embedding).
//!
//! Architecture:
//! - GPUI owns a borderless external window and draws **only** the shell/header.
//! - A native WS_CHILD host region is created under this window's HWND by the
//!   C++ backend (`native_editor::attach_editor_into_parent`), and the VST3
//!   `IPlugView` is attached into it. The plugin UI is the native view; GPUI
//!   never draws plugin content.
//! - No audio-thread interaction: attach/resize/detach run on the UI thread.
//! - Editor failure never crashes — a GPUI fallback panel is shown instead.
//!
//! The old C++ NanoVG/D3D top-level window is no longer used on this path.

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Point, Render, Styled, Window, WindowBounds, WindowHandle, WindowKind,
};

use crate::components::title_bar::external_window_titlebar;
use crate::theme::{self, Colors};
use sphere_plugin_host::native_editor::{
    attach_editor_into_parent, detach_editor, set_editor_region_bounds, EmbedRegion,
};

/// Logical-pixel height reserved for the GPUI-drawn header.
const HEADER_H: f32 = 34.0;
pub const EDITOR_WINDOW_WIDTH: f32 = 820.0;
pub const EDITOR_WINDOW_HEIGHT: f32 = 560.0;
pub const EDITOR_WINDOW_MIN_WIDTH: f32 = 360.0;
pub const EDITOR_WINDOW_MIN_HEIGHT: f32 = 200.0;

fn plugin_view_debug() -> bool {
    std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some()
}

pub struct PluginEditorWindow {
    pub track_id: String,
    pub insert_id: String,
    plugin_path: String,
    class_id: String,
    display_name: String,
    /// Embed session handle from the C++ backend; `None` until first attach.
    embed_handle: Option<u64>,
    /// Set once we've attempted attach (success or failure) so render is idempotent.
    attach_attempted: bool,
    error: Option<String>,
    last_region: Option<(i32, i32, i32, i32)>,
    focus_handle: FocusHandle,
}

impl PluginEditorWindow {
    pub fn new(
        track_id: String,
        insert_id: String,
        plugin_path: String,
        class_id: String,
        display_name: String,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            track_id,
            insert_id,
            plugin_path,
            class_id,
            display_name,
            embed_handle: None,
            attach_attempted: false,
            error: None,
            last_region: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Physical-pixel host region under the GPUI window: full client width, from
    /// just below the header to the bottom. Win32 child coords are physical, so
    /// logical sizes are scaled by the window DPI factor.
    fn host_region(window: &Window) -> EmbedRegion {
        let scale = window.scale_factor().max(0.5);
        let viewport = window.viewport_size();
        let w: f32 = viewport.width.into();
        let h: f32 = viewport.height.into();
        let header_px = HEADER_H * scale;
        EmbedRegion {
            x: 0,
            y: header_px.round() as i32,
            width: (w * scale).round().max(1.0) as i32,
            height: ((h * scale) - header_px).round().max(1.0) as i32,
        }
    }

    /// Extract the native window handle (HWND on Windows) from the GPUI window
    /// via the `raw-window-handle` trait. `None` on unsupported platforms or if
    /// the handle is unavailable.
    #[cfg(target_os = "windows")]
    fn native_parent_handle(window: &Window) -> Option<u64> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        // NB: `Window::window_handle()` (inherent) returns gpui's AnyWindowHandle;
        // the raw handle is the same-named trait method — call it qualified.
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(w) => Some(w.hwnd.get() as u64),
            _ => None,
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn native_parent_handle(_window: &Window) -> Option<u64> {
        None
    }

    fn ensure_attached(&mut self, window: &mut Window) {
        if self.attach_attempted {
            return;
        }
        self.attach_attempted = true;

        let Some(parent) = Self::native_parent_handle(window) else {
            self.error = Some("plugin editor embedding unavailable on this platform".to_string());
            if plugin_view_debug() {
                eprintln!("[plugin-view] no native parent handle for editor window");
            }
            return;
        };
        let region = Self::host_region(window);
        match attach_editor_into_parent(parent, &self.plugin_path, &self.class_id, region) {
            Ok(handle) => {
                self.embed_handle = Some(handle);
                self.last_region = Some((region.x, region.y, region.width, region.height));
                if plugin_view_debug() {
                    eprintln!(
                        "[plugin-view] embed attach ok track={} slot={} handle=0x{handle:x} parent=0x{parent:x}",
                        self.track_id, self.insert_id
                    );
                }
            }
            Err(err) => {
                if plugin_view_debug() {
                    eprintln!(
                        "[plugin-view] embed attach FAILED track={} slot={} err={err}",
                        self.track_id, self.insert_id
                    );
                }
                self.error = Some(err);
            }
        }
    }

    fn sync_region(&mut self, window: &Window) {
        let Some(handle) = self.embed_handle else {
            return;
        };
        let region = Self::host_region(window);
        let tuple = (region.x, region.y, region.width, region.height);
        if self.last_region != Some(tuple) {
            self.last_region = Some(tuple);
            set_editor_region_bounds(handle, region);
        }
    }
}

impl Drop for PluginEditorWindow {
    fn drop(&mut self) {
        if let Some(handle) = self.embed_handle.take() {
            detach_editor(handle);
            if plugin_view_debug() {
                eprintln!(
                    "[plugin-view] embed detach (drop) track={} slot={} handle=0x{handle:x}",
                    self.track_id, self.insert_id
                );
            }
        }
    }
}

impl Render for PluginEditorWindow {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Attach on first render (the window/HWND now exists); resync the host
        // region on every subsequent render so window resizes track through.
        self.ensure_attached(window);
        self.sync_region(window);

        let body = if let Some(err) = &self.error {
            // Fallback panel — no native view; show name + error so the user
            // can still close/bypass. Never a crash.
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .items_center()
                .justify_center()
                .size_full()
                .p(px(20.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(Colors::text_primary())
                        .child(self.display_name.clone()),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(Colors::status_error())
                        .child(format!("Editor unavailable: {err}")),
                )
                .into_any_element()
        } else {
            // Empty host region — the native WS_CHILD view renders on top of
            // this GPUI surface. Dark fill avoids a flash before attach paints.
            div().size_full().bg(Colors::surface_base()).into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FONT_FAMILY)
            .bg(Colors::surface_base())
            .overflow_hidden()
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar(
                self.display_name.clone(),
                "plugin-editor-window-close",
                move |window, _cx| window.remove_window(),
            ))
            .child(div().flex_1().min_h(px(0.0)).child(body))
    }
}

/// Open the GPUI-hosted plugin editor window for an insert slot. The caller
/// (StudioLayout) keeps the returned handle to dedupe/close. Drop of the entity
/// detaches the native view.
#[allow(clippy::too_many_arguments)]
pub fn open_plugin_editor_window(
    owner_bounds: Bounds<gpui::Pixels>,
    track_id: String,
    insert_id: String,
    plugin_path: String,
    class_id: String,
    display_name: String,
    cx: &mut App,
) -> Result<WindowHandle<PluginEditorWindow>, String> {
    let parent_x: f32 = owner_bounds.origin.x.into();
    let parent_y: f32 = owner_bounds.origin.y.into();
    let parent_w: f32 = owner_bounds.size.width.into();
    let parent_h: f32 = owner_bounds.size.height.into();
    let origin = Point {
        x: px(parent_x + ((parent_w - EDITOR_WINDOW_WIDTH) / 2.0).max(24.0)),
        y: px(parent_y + ((parent_h - EDITOR_WINDOW_HEIGHT) / 2.0).max(24.0)),
    };

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(EDITOR_WINDOW_WIDTH), px(EDITOR_WINDOW_HEIGHT)),
    }));
    options.kind = WindowKind::Floating;
    options.is_resizable = true;
    options.is_minimizable = false;
    options.window_min_size = Some(size(
        px(EDITOR_WINDOW_MIN_WIDTH),
        px(EDITOR_WINDOW_MIN_HEIGHT),
    ));

    cx.open_window(options, |_window, cx| {
        cx.new(|cx| {
            PluginEditorWindow::new(
                track_id,
                insert_id,
                plugin_path,
                class_id,
                display_name,
                cx,
            )
        })
    })
    .map_err(|e| e.to_string())
}
