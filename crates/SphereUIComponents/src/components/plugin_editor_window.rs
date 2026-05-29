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

use std::time::Duration;

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Point, Render, StatefulInteractiveElement, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
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

/// How many ~32 ms ticks we wait for the GPUI window to produce a valid native
/// handle + non-zero content bounds before giving up and surfacing a visible
/// error. ~5 s — generous, but never an infinite silent spin.
const MAX_WAIT_TICKS: u32 = 150;

fn plugin_view_debug() -> bool {
    std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some()
}

/// Explicit lifecycle state for the embedded editor. The UI renders a distinct
/// surface for each state so a blank panel never appears unless we are actually
/// `Attached` with a live native child.
#[derive(Clone, Debug, PartialEq)]
enum PluginEditorStatus {
    /// Window just opened; native handle/bounds not yet probed.
    Opening,
    /// Native parent HWND or content bounds not ready — retrying.
    WaitingForHostHandle,
    /// Bounds are ready; about to create the native child + attach.
    Attaching,
    /// Native editor attached and visible.
    Attached,
    /// Attach failed — fallback panel with Retry / Close.
    Failed(String),
}

pub struct PluginEditorWindow {
    pub track_id: String,
    pub insert_id: String,
    plugin_path: String,
    class_id: String,
    display_name: String,
    /// Embed session handle from the C++ backend; `None` until first attach.
    embed_handle: Option<u64>,
    status: PluginEditorStatus,
    /// Number of waiting ticks elapsed (reset on retry).
    wait_ticks: u32,
    /// Whether a deferred re-render tick is already queued (avoids spawning a
    /// timer on every render frame while waiting).
    tick_scheduled: bool,
    /// Logged the "host region mounted" line once bounds first went non-zero.
    host_mounted_logged: bool,
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
            status: PluginEditorStatus::Opening,
            wait_ticks: 0,
            tick_scheduled: false,
            host_mounted_logged: false,
            last_region: None,
            focus_handle: cx.focus_handle(),
        }
    }

    fn editor_id(&self) -> String {
        format!("{}::{}", self.track_id, self.insert_id)
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

    /// Schedule one deferred re-render tick (~32 ms) so the state machine keeps
    /// advancing while we wait for the native handle / layout bounds. Guarded so
    /// we never queue more than one pending tick at a time.
    fn schedule_tick(&mut self, cx: &mut Context<Self>) {
        if self.tick_scheduled {
            return;
        }
        self.tick_scheduled = true;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(Duration::from_millis(32)).await;
            let _ = this.update(cx, |this, cx| {
                this.tick_scheduled = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn note_waiting(&mut self, reason: &str, cx: &mut Context<Self>) {
        self.wait_ticks += 1;
        if self.wait_ticks > MAX_WAIT_TICKS {
            let msg = format!("host region never became ready ({reason})");
            if plugin_view_debug() {
                eprintln!("[plugin-view] attach failed error={msg} editor_id={}", self.editor_id());
            }
            self.status = PluginEditorStatus::Failed(msg);
            cx.notify();
            return;
        }
        if self.status != PluginEditorStatus::WaitingForHostHandle {
            self.status = PluginEditorStatus::WaitingForHostHandle;
        }
        if plugin_view_debug() {
            eprintln!(
                "[plugin-view] waiting ({reason}) editor_id={} tick={}/{MAX_WAIT_TICKS}",
                self.editor_id(),
                self.wait_ticks
            );
        }
        self.schedule_tick(cx);
    }

    /// Drive the attach lifecycle. Called at the top of every render (which has
    /// both the live `Window` and `Context`). Never blocks; transitions through
    /// explicit states and defers via `schedule_tick` until bounds are ready.
    fn drive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.status.clone() {
            PluginEditorStatus::Attached => {
                self.sync_region(window);
                return;
            }
            PluginEditorStatus::Failed(_) => return,
            PluginEditorStatus::Attaching => {
                self.perform_attach(window, cx);
                return;
            }
            PluginEditorStatus::Opening | PluginEditorStatus::WaitingForHostHandle => {}
        }

        // Phase 4/6: require a valid native parent handle before attaching.
        let Some(parent) = Self::native_parent_handle(window) else {
            self.note_waiting("no native parent handle", cx);
            return;
        };
        if plugin_view_debug() {
            eprintln!("[plugin-view] top hwnd=0x{parent:x} editor_id={}", self.editor_id());
        }

        // Phase 7: require real (>0) content bounds before attaching.
        let region = Self::host_region(window);
        if region.width <= 0 || region.height <= 0 {
            self.note_waiting("host bounds not ready (0x0)", cx);
            return;
        }

        if !self.host_mounted_logged {
            self.host_mounted_logged = true;
            if plugin_view_debug() {
                eprintln!(
                    "[plugin-view] host region mounted bounds={{x:{},y:{},w:{},h:{}}} editor_id={}",
                    region.x, region.y, region.width, region.height, self.editor_id()
                );
            }
        }

        // Bounds are ready — move to a visible Attaching state, then let the
        // next tick perform the (potentially blocking) attach so the UI can
        // first paint "Attaching plugin editor…".
        self.wait_ticks = 0;
        self.status = PluginEditorStatus::Attaching;
        if plugin_view_debug() {
            eprintln!(
                "[plugin-view] attach requested editor_id={} parent=0x{parent:x} size={}x{}",
                self.editor_id(),
                region.width,
                region.height
            );
        }
        self.schedule_tick(cx);
        cx.notify();
    }

    fn perform_attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = Self::native_parent_handle(window) else {
            // Lost the handle between scheduling and now — go back to waiting.
            self.status = PluginEditorStatus::WaitingForHostHandle;
            self.note_waiting("native parent handle lost before attach", cx);
            return;
        };
        let region = Self::host_region(window);
        if region.width <= 0 || region.height <= 0 {
            self.status = PluginEditorStatus::WaitingForHostHandle;
            self.note_waiting("host bounds not ready before attach", cx);
            return;
        }
        match attach_editor_into_parent(parent, &self.plugin_path, &self.class_id, region) {
            Ok(handle) => {
                self.embed_handle = Some(handle);
                self.last_region = Some((region.x, region.y, region.width, region.height));
                self.status = PluginEditorStatus::Attached;
                if plugin_view_debug() {
                    eprintln!(
                        "[plugin-view] attach ok editor_id={} handle=0x{handle:x} parent=0x{parent:x}",
                        self.editor_id()
                    );
                }
            }
            Err(err) => {
                if plugin_view_debug() {
                    eprintln!(
                        "[plugin-view] attach failed error={err} editor_id={}",
                        self.editor_id()
                    );
                }
                self.status = PluginEditorStatus::Failed(err);
            }
        }
        cx.notify();
    }

    /// User-initiated retry from the failure panel: tear down any partial state
    /// and restart the lifecycle from `Opening`.
    fn retry(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.embed_handle.take() {
            detach_editor(handle);
        }
        self.status = PluginEditorStatus::Opening;
        self.wait_ticks = 0;
        self.host_mounted_logged = false;
        self.last_region = None;
        if plugin_view_debug() {
            eprintln!("[plugin-view] retry requested editor_id={}", self.editor_id());
        }
        cx.notify();
    }

    fn sync_region(&mut self, window: &Window) {
        let Some(handle) = self.embed_handle else {
            return;
        };
        let region = Self::host_region(window);
        let tuple = (region.x, region.y, region.width, region.height);
        if self.last_region != Some(tuple) {
            self.last_region = Some(tuple);
            if plugin_view_debug() {
                eprintln!(
                    "[plugin-view] resize host bounds={{x:{},y:{},w:{},h:{}}} editor_id={}",
                    region.x, region.y, region.width, region.height, self.editor_id()
                );
            }
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
                    "[plugin-view] close editor_id={} handle=0x{handle:x} (drop → detach)",
                    self.editor_id()
                );
            }
        }
    }
}

impl PluginEditorWindow {
    fn render_status_message(&self, headline: &str) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .items_center()
            .justify_center()
            .size_full()
            .bg(Colors::surface_base())
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
                    .text_color(Colors::text_secondary())
                    .child(headline.to_string()),
            )
            .into_any_element()
    }

    fn render_failure_panel(&self, err: &str, cx: &mut Context<Self>) -> gpui::AnyElement {
        let retry = div()
            .id("plugin-editor-retry")
            .px(px(14.0))
            .py(px(6.0))
            .rounded_md()
            .cursor(gpui::CursorStyle::PointingHand)
            .bg(Colors::accent_muted())
            .text_size(px(11.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(Colors::accent_primary())
            .hover(|s| s.bg(Colors::surface_control_hover()))
            .child("Retry")
            .on_click(cx.listener(|this, _ev, _window, cx| this.retry(cx)));

        let close = div()
            .id("plugin-editor-close")
            .px(px(14.0))
            .py(px(6.0))
            .rounded_md()
            .cursor(gpui::CursorStyle::PointingHand)
            .bg(Colors::surface_raised())
            .text_size(px(11.0))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(Colors::text_secondary())
            .hover(|s| s.bg(Colors::surface_control_hover()))
            .child("Close")
            .on_click(|_ev, window, _cx| window.remove_window());

        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .items_center()
            .justify_center()
            .size_full()
            .bg(Colors::surface_base())
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
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(Colors::status_error())
                    .child("Editor failed to open"),
            )
            .child(
                div()
                    .max_w(px(560.0))
                    .text_size(px(11.0))
                    .text_color(Colors::text_secondary())
                    .child(err.to_string()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.0))
                    .child(retry)
                    .child(close),
            )
            .into_any_element()
    }
}

impl Render for PluginEditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Drive the attach lifecycle every frame; this advances the explicit
        // state machine and resyncs the host region on resize once attached.
        self.drive(window, cx);

        let body = match &self.status {
            PluginEditorStatus::Opening => self.render_status_message("Opening editor…"),
            PluginEditorStatus::WaitingForHostHandle => {
                self.render_status_message("Opening editor… (waiting for host window)")
            }
            PluginEditorStatus::Attaching => {
                self.render_status_message("Attaching plugin editor…")
            }
            PluginEditorStatus::Failed(err) => {
                let err = err.clone();
                self.render_failure_panel(&err, cx)
            }
            PluginEditorStatus::Attached => {
                // TRANSPARENT host region — the native WS_CHILD view renders
                // beneath gpui's (topmost, premultiplied-alpha) composition, so
                // we must NOT paint an opaque background here or the plugin is
                // hidden (blank editor). The child window paints its own black
                // backing, so there is no see-through to the desktop.
                div().size_full().into_any_element()
            }
        };

        // Root stays transparent; only the header (opaque) and the non-attached
        // panels paint a surface. This keeps the attached content region clear
        // so the native plugin child composites through.
        div()
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FONT_FAMILY)
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
    if plugin_view_debug() {
        eprintln!(
            "[plugin-view] open requested plugin={display_name} track={track_id} insert={insert_id} instance={}::{}",
            track_id, insert_id
        );
    }
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
    // The native VST3 editor lives in a WS_CHILD under this window. gpui composes
    // its surface *on top of* child windows (DirectComposition topmost visual),
    // but with premultiplied alpha — so a transparent content region lets the
    // child show through. Opaque content would hide the plugin (blank editor).
    options.window_background = WindowBackgroundAppearance::Transparent;
    options.window_min_size = Some(size(
        px(EDITOR_WINDOW_MIN_WIDTH),
        px(EDITOR_WINDOW_MIN_HEIGHT),
    ));

    let editor_id = format!("{track_id}::{insert_id}");
    let result = cx.open_window(options, |_window, cx| {
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
    });
    if plugin_view_debug() {
        match &result {
            Ok(_) => eprintln!("[plugin-view] gpui window created id={editor_id}"),
            Err(e) => eprintln!("[plugin-view] gpui window create FAILED id={editor_id} err={e}"),
        }
    }
    result.map_err(|e| e.to_string())
}
