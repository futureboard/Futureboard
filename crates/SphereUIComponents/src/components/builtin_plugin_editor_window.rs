//! Floating shell window for a built-in plugin's CEF editor.
//!
//! GPUI draws only the chrome: a compact titlebar and, when the host is
//! unavailable, an explanatory panel. The editor itself is a native CEF child
//! window parented into a dedicated `WS_CHILD` content host, exactly like the
//! VST3 editor path — GPUI never paints over the browser's rect.
//!
//! ## Lifecycle
//!
//! ```text
//! open  → GPUI window exists, native handle not yet valid  (WaitingForHandle)
//!       → content child created, CEF browser created       (Attached)
//! close → view closed, content child destroyed
//! ```
//!
//! CEF's message loop is pumped from a GPUI timer for as long as this window is
//! alive; without that the browser never paints or handles input.

use std::time::Duration;

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, IntoElement, ParentElement, Pixels, Point,
    Render, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
};

use crate::components::builtin_plugin_editor::{
    self as host, HostAvailability, ViewEvent, ViewId, ViewRect,
};
use crate::components::plugin_content_host::{ContentChildHwnd, ContentRect};
use crate::components::title_bar::{external_window_titlebar, TITLEBAR_HEIGHT};
use crate::theme::Colors;

pub const BUILTIN_EDITOR_WIDTH: f32 = 1180.0;
pub const BUILTIN_EDITOR_HEIGHT: f32 = 760.0;
pub const BUILTIN_EDITOR_MIN_WIDTH: f32 = 900.0;
pub const BUILTIN_EDITOR_MIN_HEIGHT: f32 = 620.0;

/// Height of the GPUI-drawn header strip above the browser rect. Uses the
/// shared external-dialog titlebar height so the browser rect and the chrome
/// can never disagree about where the content starts.
const HEADER_H: f32 = TITLEBAR_HEIGHT;

/// CEF pump interval. 8 ms keeps the editor responsive without spinning the UI
/// thread; CEF coalesces its own work internally.
const PUMP_INTERVAL: Duration = Duration::from_millis(8);

#[derive(Debug, Clone, PartialEq)]
enum Status {
    /// GPUI window created; native parent handle not yet valid.
    WaitingForHandle { ticks: u32 },
    /// The content HWND exists and browser creation is queued.
    Attaching,
    /// Browser created and parented.
    Attached,
    /// Browser close is queued; keep the shell/parent HWND alive until CEF has
    /// processed it.
    Closing,
    /// CEF has processed close and the GPUI shell may now be removed.
    Closed,
    /// Host unavailable or browser creation failed — the reason is shown.
    Failed(String),
}

/// How many pump ticks to wait for a usable native handle before surfacing an
/// error rather than spinning forever.
const MAX_HANDLE_TICKS: u32 = 150;

struct PumpTick {
    keep_going: bool,
    content_to_drop: Option<ContentChildHwnd>,
}

pub struct BuiltinPluginEditorWindow {
    view_id: ViewId,
    editor_id: String,
    plugin_id: String,
    display_name: String,
    status: Status,
    content: Option<ContentChildHwnd>,
    last_rect: Option<ViewRect>,
}

impl BuiltinPluginEditorWindow {
    pub fn new(
        editor_id: String,
        plugin_id: String,
        display_name: String,
        cx: &mut Context<Self>,
    ) -> Self {
        // Refuse early and clearly when the host cannot serve this plugin, so
        // the window shows a reason instead of an empty rect.
        let status = match host::availability(&plugin_id) {
            HostAvailability::Ready => Status::WaitingForHandle { ticks: 0 },
            other => Status::Failed(other.to_string()),
        };

        if matches!(status, Status::WaitingForHandle { .. }) {
            Self::spawn_pump(cx);
        }

        Self {
            view_id: host::allocate_view_id(),
            editor_id,
            plugin_id,
            display_name,
            status,
            content: None,
            last_rect: None,
        }
    }

    /// Drive CEF and, until it succeeds, keep retrying the attach.
    fn spawn_pump(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PUMP_INTERVAL).await;

                // CEF synchronously pumps Win32 messages. It must run before
                // `this.update`, while GPUI holds neither the AppCell nor this
                // entity's RefCell; otherwise a nested GPUI message double-borrows
                // the app and panics in AsyncApp::update_entity.
                host::pump();

                let tick = this.update(cx, |this, cx| this.tick(cx));
                match tick {
                    Ok(tick) => {
                        // Destroying an HWND also dispatches Win32 messages, so
                        // release failed/closed content after the entity update.
                        drop(tick.content_to_drop);
                        if !tick.keep_going {
                            break;
                        }
                    }
                    // Window gone. If it disappeared during the CEF call above,
                    // `Drop` queued close after that pump drained its command
                    // snapshot. Give the queue one final borrow-free pass.
                    Err(_) => {
                        host::pump();
                        break;
                    }
                }
            }
        })
        .detach();
    }

    /// One pump tick. Consumes completion events without invoking CEF.
    fn tick(&mut self, cx: &mut Context<Self>) -> PumpTick {
        let mut content_to_drop = None;
        for event in host::take_view_events(self.view_id) {
            match event {
                ViewEvent::Opened if matches!(self.status, Status::Attaching) => {
                    self.status = Status::Attached;
                    cx.notify();
                }
                ViewEvent::OpenFailed(error) if matches!(self.status, Status::Attaching) => {
                    self.status = Status::Failed(format!("CEF failed to open the editor: {error}"));
                    content_to_drop = self.content.take();
                    cx.notify();
                }
                ViewEvent::Closed => {
                    self.status = Status::Closed;
                    content_to_drop = self.content.take();
                    cx.notify();
                }
                // An open completion can race a close requested from a nested
                // Win32 callback. Closing dominates; the queued close is handled
                // by the next pump.
                ViewEvent::Opened | ViewEvent::OpenFailed(_) => {}
            }
        }

        if let Status::WaitingForHandle { ticks } = self.status {
            let ticks = ticks + 1;
            if ticks > MAX_HANDLE_TICKS {
                self.status = Status::Failed(
                    "the editor window never produced a usable native handle".to_string(),
                );
                cx.notify();
            } else {
                self.status = Status::WaitingForHandle { ticks };
            }
        }

        PumpTick {
            keep_going: matches!(
                self.status,
                Status::WaitingForHandle { .. }
                    | Status::Attaching
                    | Status::Attached
                    | Status::Closing
            ),
            content_to_drop,
        }
    }

    /// Create the content child and the CEF browser inside it. Called from the
    /// render pass, which is the first place a valid native handle and real
    /// content bounds are both available.
    fn attach(&mut self, window: &mut Window, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let Some(top_hwnd) = native_hwnd(window) else {
            return;
        };

        let scale = window.scale_factor();
        let rect = content_rect(bounds, scale);
        if rect.width <= 0 || rect.height <= 0 {
            return;
        }

        let content = match self.content.as_ref() {
            Some(content) if content.is_valid() => content,
            _ => {
                let Some(created) = ContentChildHwnd::create(
                    top_hwnd,
                    ContentRect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    },
                ) else {
                    self.status =
                        Status::Failed("could not create the editor content window".to_string());
                    cx.notify();
                    return;
                };
                self.content = Some(created);
                self.content.as_ref().expect("just installed")
            }
        };

        // CEF fills its parent's client area, so the browser is placed at the
        // content child's origin, not the shell's.
        let view_rect = ViewRect {
            x: 0,
            y: 0,
            width: rect.width,
            height: rect.height,
        };
        match host::open_view(
            self.view_id,
            &self.editor_id,
            &self.plugin_id,
            content.hwnd(),
            view_rect,
        ) {
            Ok(()) => {
                self.status = Status::Attaching;
                self.last_rect = Some(rect);
                cx.notify();
            }
            Err(err) => {
                self.status = Status::Failed(err.to_string());
                cx.notify();
            }
        }
    }

    /// Keep the content child and the browser matched to the shell's content
    /// rect. Only issues native calls when the rect actually changed.
    fn resync_bounds(&mut self, window: &Window, bounds: Bounds<Pixels>) {
        let rect = content_rect(bounds, window.scale_factor());
        if rect.width <= 0 || rect.height <= 0 || self.last_rect == Some(rect) {
            return;
        }
        if let Some(content) = self.content.as_ref() {
            content.set_bounds(ContentRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            });
        }
        host::set_view_bounds(
            self.view_id,
            ViewRect {
                x: 0,
                y: 0,
                width: rect.width,
                height: rect.height,
            },
        );
        self.last_rect = Some(rect);
    }

    /// Begin an asynchronous close. The shell remains alive until the CEF pump
    /// confirms it processed the close, preserving the native parent HWND for
    /// the browser's entire lifetime.
    pub(crate) fn request_close(&mut self, cx: &mut Context<Self>) {
        match self.status {
            Status::Closing | Status::Closed => return,
            Status::WaitingForHandle { .. } | Status::Failed(_) => {
                self.status = Status::Closed;
            }
            Status::Attaching | Status::Attached => {
                host::close_view(self.view_id);
                self.status = Status::Closing;
            }
        }
        cx.notify();
    }
}

impl Drop for BuiltinPluginEditorWindow {
    fn drop(&mut self) {
        // Fallback for forced application/window teardown. Normal close travels
        // through `Closing` and waits for the pump's `Closed` event.
        if !matches!(self.status, Status::Closed | Status::Failed(_)) {
            host::close_view(self.view_id);
        }
    }
}

/// The physical-pixel rect the browser occupies inside the shell's client area:
/// everything below the GPUI-drawn header.
fn content_rect(bounds: Bounds<Pixels>, scale: f32) -> ViewRect {
    let width: f32 = bounds.size.width.into();
    let height: f32 = bounds.size.height.into();
    let phys = |v: f32| (v * scale).round() as i32;
    ViewRect {
        x: 0,
        y: phys(HEADER_H),
        width: phys(width),
        height: (phys(height) - phys(HEADER_H)).max(0),
    }
}

#[cfg(target_os = "windows")]
fn native_hwnd(window: &Window) -> Option<u64> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get() as u64),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
fn native_hwnd(_window: &Window) -> Option<u64> {
    // CEF child embedding for built-in editors is Windows-only for now; the
    // host reports this rather than opening a blank window.
    None
}

impl Render for BuiltinPluginEditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = window.bounds();

        match &self.status {
            Status::WaitingForHandle { .. } => self.attach(window, bounds, cx),
            Status::Attaching | Status::Attached => self.resync_bounds(window, bounds),
            Status::Closing | Status::Closed | Status::Failed(_) => {}
        }
        if matches!(self.status, Status::Closed) {
            cx.defer_in(window, |_this, window, _cx| window.remove_window());
        }

        let failure = match &self.status {
            Status::Failed(reason) => Some(reason.clone()),
            _ => None,
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(Colors::surface_panel())
            .child(
                // Shared external-dialog titlebar: gives this window the same
                // chrome as every other floating Studio surface, plus the drag
                // region and close button that a borderless shell needs (a
                // hand-rolled header has no way to move the window).
                div().flex_none().child(external_window_titlebar(
                    self.display_name.clone(),
                    "builtin-plugin-editor-close",
                    {
                        let this = cx.weak_entity();
                        move |_window, cx| {
                            let _ = this.update(cx, |this, cx| this.request_close(cx));
                        }
                    },
                )),
            )
            .child(match failure {
                // The browser paints itself into the native child below the
                // header, so on success this area stays deliberately empty.
                None => div().flex_1().min_h(px(0.0)),
                Some(reason) => div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .p(px(16.0))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(Colors::text_primary())
                            .child("This editor could not be opened"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(Colors::text_secondary())
                            .child(reason),
                    ),
            })
    }
}

/// Open the shell window for a built-in plugin editor.
pub fn open_builtin_editor_window(
    owner_bounds: Bounds<Pixels>,
    editor_id: String,
    plugin_id: String,
    display_name: String,
    cx: &mut App,
) -> Result<WindowHandle<BuiltinPluginEditorWindow>, String> {
    let parent_x: f32 = owner_bounds.origin.x.into();
    let parent_y: f32 = owner_bounds.origin.y.into();
    let parent_w: f32 = owner_bounds.size.width.into();
    let parent_h: f32 = owner_bounds.size.height.into();
    let origin = Point {
        x: px(parent_x + ((parent_w - BUILTIN_EDITOR_WIDTH) / 2.0).max(24.0)),
        y: px(parent_y + ((parent_h - BUILTIN_EDITOR_HEIGHT) / 2.0).max(24.0)),
    };

    let mut options = crate::platform_chrome::external_dialog_window_options_partial();
    options.window_bounds = Some(WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(BUILTIN_EDITOR_WIDTH), px(BUILTIN_EDITOR_HEIGHT)),
    }));
    options.kind = WindowKind::Floating;
    options.is_resizable = true;
    options.is_minimizable = false;
    // Opaque: the CEF child composites above the swap chain in the content
    // region, and a transparent shell would show the timeline behind it.
    options.window_background = WindowBackgroundAppearance::Opaque;
    options.window_min_size = Some(size(
        px(BUILTIN_EDITOR_MIN_WIDTH),
        px(BUILTIN_EDITOR_MIN_HEIGHT),
    ));

    cx.open_window(options, |window, cx| {
        let view =
            cx.new(|cx| BuiltinPluginEditorWindow::new(editor_id, plugin_id, display_name, cx));
        let weak = view.downgrade();
        window.on_window_should_close(cx, move |_window, cx| {
            let _ = weak.update(cx, |view, cx| view.request_close(cx));
            // Always veto the platform close. `Closed` removes the shell after
            // CEF has processed its queued close command.
            false
        });
        view
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::point;

    fn bounds(w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(w), px(h)),
        }
    }

    #[test]
    fn content_rect_sits_below_the_header() {
        let rect = content_rect(bounds(1000.0, 700.0), 1.0);
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, HEADER_H as i32);
        assert_eq!(rect.width, 1000);
        assert_eq!(rect.height, 700 - HEADER_H as i32);
    }

    #[test]
    fn content_rect_scales_with_dpi() {
        let rect = content_rect(bounds(1000.0, 700.0), 2.0);
        assert_eq!(rect.y, (HEADER_H * 2.0) as i32);
        assert_eq!(rect.width, 2000);
        assert_eq!(rect.height, 1400 - (HEADER_H * 2.0) as i32);
        // The browser must never be told to draw over the header.
        assert!(rect.y > 0);
    }

    #[test]
    fn a_window_shorter_than_the_header_clamps_to_zero_rather_than_going_negative() {
        let rect = content_rect(bounds(400.0, 10.0), 1.0);
        assert_eq!(rect.height, 0);
        assert!(rect.height >= 0);
    }
}
