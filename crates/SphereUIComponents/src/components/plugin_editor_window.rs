//! Native plugin editor window (Phase 4 — GPUI-hosted embedding).
//!
//! Architecture:
//! - GPUI owns a borderless external window and draws **only** the shell/header.
//! - A native WS_CHILD host region is created under this window's HWND by the
//!   C++ backend (`native_editor::attach_editor_into_parent`), and the VST3
//!   `IPlugView` is attached into it. The plugin UI is the native view; GPUI
//!   never draws plugin content.
//! - On Windows the native app sets `GPUI_DISABLE_DIRECT_COMPOSITION=1` at boot.
//! - VST3 UI is hosted in an **owned tool window** (`WS_POPUP|WS_EX_TOOLWINDOW`)
//!   aligned to the content region below the GPUI titlebar (default). Set
//!   `FUTUREBOARD_PLUGIN_EDITOR_MODE=child` to force in-client `WS_CHILD` embed.
//! - The GPUI shell uses an opaque background; the tool window carries the plugin UI.
//! - No audio-thread interaction: attach/resize/detach run on the UI thread.
//! - Editor failure never crashes — a GPUI fallback panel is shown instead.
//!
//! The old C++ NanoVG/D3D top-level window is no longer used on this path.

use std::time::{Duration, Instant};

use gpui::{
    div, px, size, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Point, Render, StatefulInteractiveElement, Styled, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind,
};

use crate::components::plugin_content_host::{ContentChildHwnd, ContentRect};
use crate::components::title_bar::{external_window_titlebar, TITLEBAR_HEIGHT};
use crate::theme::{self, Colors};
use sphere_plugin_host::editor_quirk::{match_quirk, PluginEditorQuirk};
use sphere_plugin_host::ipc::HostEvent;
use sphere_plugin_host::native_editor::PluginEditorPresentationMode;
use sphere_plugin_host::plugin_host_client::{
    plugin_host_bridge_enabled, ClientEvent, PluginHostClient,
};

/// Physical-pixel host region under the GPUI window. (Local mirror of the
/// backend's region struct — the editor is now driven by the DAUx runtime
/// instance, not SpherePluginHost.)
#[derive(Debug, Clone, Copy, Default)]
struct EmbedRegion {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

/// Map the DAUx embed host-kind code (0 = WS_CHILD, 1 = owned tool window,
/// 2 = detached top-level) to the shared presentation-mode enum. Exactly one
/// mode is active per editor.
fn presentation_mode_from_host_kind(kind: i32) -> PluginEditorPresentationMode {
    match kind {
        0 => PluginEditorPresentationMode::ChildHwndEmbed,
        2 => PluginEditorPresentationMode::DetachedNativeWindow,
        _ => PluginEditorPresentationMode::OwnedToolWindowFallback,
    }
}

/// State for the separated-process editor backend. Present only when host-process
/// ownership is selected and the host spawned successfully. The main app owns
/// the window + the content child HWND; the host process owns the VST3 view.
struct HostEditorBackend {
    /// One host process per open editor (simplest lifecycle + crash isolation;
    /// a shared host is a later optimization). Drop shuts it down.
    client: PluginHostClient,
    /// Main-app-owned `WS_CHILD` content HWND the host attaches the view into.
    content: Option<ContentChildHwnd>,
    plugin_path: String,
    class_id: String,
    /// Captured from `HostEvent::Ready` for diagnostics.
    host_pid: Option<u32>,
    /// Last content rect pushed to the host (dedup resize spam).
    last_region: Option<ContentRect>,
}

/// Spawn the bridge host and complete a Ping/Pong handshake. Returns `None` on
/// any failure — but the caller's `bridge_required` gate ensures we NEVER fall
/// back to the in-process editor path when the bridge is enabled (spec point 6:
/// no silent fallback). `[plugin-bridge]` diagnostics are emitted throughout.
fn build_host_backend(
    processor: &DAUx::Vst3RuntimeProcessor,
    _display_name: &str,
) -> Option<HostEditorBackend> {
    let plugin_path = processor.plugin_path().map(|s| s.to_string());
    let class_id = processor.class_id().map(|s| s.to_string());

    let mut client = match PluginHostClient::spawn_bridge() {
        Ok(c) => c, // spawn_bridge logged current_exe/resolved/exists/spawned
        Err(_) => return None, // spawn_bridge already logged spawn_failed
    };

    // Liveness handshake before any editor command.
    eprintln!("[plugin-bridge] sending Ping");
    if let Err(e) = client.ping() {
        eprintln!("[plugin-bridge] spawn_failed error=ping send: {e}");
        return None;
    }
    let mut host_pid = Some(client.pid());
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut ponged = false;
    while Instant::now() < deadline {
        match client.try_recv_event() {
            Some(ClientEvent::Host(HostEvent::Pong { pid })) => {
                host_pid = Some(pid);
                ponged = true;
                break;
            }
            Some(ClientEvent::Host(HostEvent::Ready { pid, .. })) => {
                host_pid = Some(pid); // startup Ready; keep waiting for Pong
            }
            Some(ClientEvent::Disconnected) => {
                eprintln!("[plugin-bridge] spawn_failed error=host disconnected during handshake");
                return None;
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    if !ponged {
        eprintln!("[plugin-bridge] spawn_failed error=handshake timeout (no Pong)");
        return None;
    }
    eprintln!("[plugin-bridge] received Pong");

    // The bridge is live. If plugin identity is missing we still go through the
    // bridge (skeleton OpenEditor) — we must never touch the in-process path.
    Some(HostEditorBackend {
        client,
        content: None,
        plugin_path: plugin_path.unwrap_or_default(),
        class_id: class_id.unwrap_or_default(),
        host_pid,
        last_region: None,
    })
}

/// Logical-pixel height reserved for the GPUI-drawn header (matches titlebar).
const HEADER_H: f32 = TITLEBAR_HEIGHT;
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
    /// IPlugView::attached returned ok but no visible plug-in UI yet. We poll
    /// `embed_has_visible_ui` at the Phase-6 milestones below; the editor is
    /// promoted to `Attached` as soon as a visible UI appears. WebView/CEF
    /// editors (UAD Native) regularly land here for hundreds of ms.
    ProbingReady {
        mode: PluginEditorPresentationMode,
        probe_index: u8,
    },
    /// Native editor attached and visible, via exactly one presentation mode.
    Attached(PluginEditorPresentationMode),
    /// Attach failed — fallback panel with Retry / Close.
    Failed(String),
}

/// Phase 6: delays (ms) between visible-UI re-checks after a successful
/// `IPlugView::attached`. Cap at the last entry — anything still blank past
/// that turns into a surfaced failure.
const READY_PROBE_DELAYS_MS: &[u64] = &[100, 500, 1000, 3000, 5000];

pub struct PluginEditorWindow {
    pub track_id: String,
    pub insert_id: String,
    display_name: String,
    /// Clone of the live runtime VST3 instance for this insert. The editor view
    /// is created from THIS instance's controller — never a new one — so GUI
    /// edits drive the actual audio processor. Holding the clone keeps the C++
    /// instance alive while the editor is open.
    processor: DAUx::Vst3RuntimeProcessor,
    /// Editor handle from the embed attach; `None` until first attach.
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
    editor_content_size: Option<(i32, i32)>,
    /// Editor quirk resolved from the plug-in path + name at construction.
    /// Drives the delayed-ready ramp and informs failure messaging.
    quirk: PluginEditorQuirk,
    /// `Some` when the bridge is active and the host spawned. When `None` and
    /// `bridge_required` is false, the in-process path runs unchanged.
    host: Option<HostEditorBackend>,
    /// True when `FUTUREBOARD_PLUGIN_HOST_BRIDGE` is enabled. Hard gate: while
    /// set, the in-process editor path is NEVER used — if `host` is `None` the
    /// window surfaces a failure instead of silently embedding in-process.
    bridge_required: bool,
    focus_handle: FocusHandle,
}

impl PluginEditorWindow {
    pub fn new(
        track_id: String,
        insert_id: String,
        display_name: String,
        processor: DAUx::Vst3RuntimeProcessor,
        cx: &mut Context<Self>,
    ) -> Self {
        let quirk = processor
            .plugin_path()
            .map(|p| match_quirk(std::path::Path::new(p), Some(&display_name), None))
            .unwrap_or_default();
        if plugin_view_debug() {
            eprintln!(
                "[plugin-view] open requested plugin=\"{}\" track={} insert={} quirk={} delayed_ready={} sta={} extra_pump={} plugin_webview_based={}",
                display_name,
                track_id,
                insert_id,
                quirk.name,
                quirk.delayed_ready_check,
                quirk.requires_sta_com,
                quirk.extra_message_pump,
                quirk.plugin_webview_based,
            );
        }
        let bridge_required = plugin_host_bridge_enabled();
        let host = if bridge_required {
            build_host_backend(&processor, &display_name)
        } else {
            None
        };
        // Bridge enabled but host unavailable → fail visibly; never fall back to
        // the in-process path (spec point 6).
        let status = if bridge_required && host.is_none() {
            eprintln!("[plugin-view] editor open failed because bridge enabled but unavailable");
            PluginEditorStatus::Failed(
                "Plugin host bridge is enabled (FUTUREBOARD_PLUGIN_HOST_BRIDGE=1) but the \
                 FutureboardPluginHost-x64 process could not be started. The in-process editor \
                 is disabled while the bridge is enabled."
                    .to_string(),
            )
        } else {
            PluginEditorStatus::Opening
        };
        Self {
            track_id,
            insert_id,
            display_name,
            processor,
            embed_handle: None,
            status,
            wait_ticks: 0,
            tick_scheduled: false,
            host_mounted_logged: false,
            last_region: None,
            editor_content_size: None,
            quirk,
            host,
            bridge_required,
            focus_handle: cx.focus_handle(),
        }
    }

    fn editor_id(&self) -> String {
        format!("{}::{}", self.track_id, self.insert_id)
    }

    /// Physical-pixel host region under the GPUI window: full client width, from
    /// just below the header to the bottom. Win32 child coords are physical, so
    /// logical sizes are scaled by the window DPI factor.
    fn host_region_for(&self, window: &Window) -> EmbedRegion {
        let scale = window.scale_factor().max(0.5);
        let viewport = window.viewport_size();
        let w: f32 = viewport.width.into();
        let h: f32 = viewport.height.into();
        let header_px = HEADER_H * scale;
        if let Some((content_w, content_h)) = self.editor_content_size {
            return EmbedRegion {
                x: 0,
                y: header_px.round() as i32,
                width: content_w.max(1),
                height: content_h.max(1),
            };
        }
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
                eprintln!(
                    "[plugin-view] attach failed error={msg} editor_id={}",
                    self.editor_id()
                );
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
        if self.host.is_some() {
            self.drive_host(window, cx);
            return;
        }
        // Hard gate: bridge enabled but no host process → never touch the
        // in-process editor path. Stay in a surfaced failure (set in `new`).
        if self.bridge_required {
            if !matches!(self.status, PluginEditorStatus::Failed(_)) {
                self.status = PluginEditorStatus::Failed(
                    "Plugin host bridge enabled but host process is unavailable; \
                     in-process editor is disabled."
                        .to_string(),
                );
                cx.notify();
            }
            return;
        }
        match self.status.clone() {
            PluginEditorStatus::Attached(PluginEditorPresentationMode::DetachedNativeWindow) => {
                // The plug-in owns a standalone window; the GPUI shell only
                // watches for the user closing that window (WM_CLOSE) or the
                // native window vanishing, then tears the editor down.
                if self.embed_handle.is_some()
                    && (self.processor.embed_take_user_close() || !self.processor.embed_is_valid())
                {
                    if plugin_view_debug() {
                        eprintln!(
                            "[plugin-view] detached window closed editor_id={} → removing shell",
                            self.editor_id()
                        );
                    }
                    window.remove_window();
                }
                return;
            }
            PluginEditorStatus::Attached(_) => {
                self.sync_region(window);
                return;
            }
            PluginEditorStatus::Failed(_) => return,
            PluginEditorStatus::Attaching => {
                self.perform_attach(window, cx);
                return;
            }
            PluginEditorStatus::ProbingReady { .. } => {
                // The probe scheduler advances the state — keep the host region
                // in sync (parent moves still translate to the embed) while we
                // wait for the WebView/CEF children to materialize.
                self.sync_region(window);
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
            eprintln!(
                "[plugin-view] top hwnd=0x{parent:x} editor_id={}",
                self.editor_id()
            );
        }

        // Phase 7: require real (>0) content bounds before attaching.
        let region = self.host_region_for(window);
        if region.width <= 0 || region.height <= 0 {
            self.note_waiting("host bounds not ready (0x0)", cx);
            return;
        }

        if !self.host_mounted_logged {
            self.host_mounted_logged = true;
            if plugin_view_debug() {
                eprintln!(
                    "[plugin-view] host region mounted bounds={{x:{},y:{},w:{},h:{}}} editor_id={}",
                    region.x,
                    region.y,
                    region.width,
                    region.height,
                    self.editor_id()
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
        let region = self.host_region_for(window);
        if region.width <= 0 || region.height <= 0 {
            self.status = PluginEditorStatus::WaitingForHostHandle;
            self.note_waiting("host bounds not ready before attach", cx);
            return;
        }
        // Attach the editor view of the EXISTING runtime instance into our GPUI
        // window — never create a new VST3 component/controller for the editor.
        match self
            .processor
            .embed_editor(parent, region.x, region.y, region.width, region.height)
        {
            Some(handle) => {
                self.embed_handle = Some(handle);
                // Record the single presentation mode the host selected so we
                // never drive both a child-HWND embed and a tool-window overlay.
                let mode = presentation_mode_from_host_kind(self.processor.embed_host_kind());
                let detached = mode == PluginEditorPresentationMode::DetachedNativeWindow;
                if detached {
                    // The plug-in lives in its own standalone OS window; the GPUI
                    // shell must NOT resize to the plug-in size or push host
                    // bounds (those are no-ops for detached anyway). Leave the
                    // small shell as a control/close surface.
                    self.last_region = None;
                } else {
                    let applied_region = self.apply_native_auto_size(window).unwrap_or(region);
                    self.last_region = Some((
                        applied_region.x,
                        applied_region.y,
                        applied_region.width,
                        applied_region.height,
                    ));
                    // Re-apply bounds + z-order after attach (plugins may resize the host).
                    self.processor.embed_set_bounds(
                        applied_region.x,
                        applied_region.y,
                        applied_region.width,
                        applied_region.height,
                    );
                    self.processor.embed_refresh();
                }
                let visible = self.processor.embed_has_visible_ui();
                if visible {
                    self.status = PluginEditorStatus::Attached(mode);
                    if plugin_view_debug() {
                        eprintln!(
                            "[plugin-view] attach ok editor_id={} handle=0x{handle:x} parent=0x{parent:x} mode={mode:?} visible=immediate (reused runtime instance)",
                            self.editor_id()
                        );
                    }
                } else {
                    // Phase 6: enter the delayed-ready probe. WebView/CEF
                    // editors (UAD Native) routinely take 100–3000 ms before
                    // any visible child window materializes — failing now
                    // would always lose them.
                    self.status = PluginEditorStatus::ProbingReady {
                        mode,
                        probe_index: 0,
                    };
                    if plugin_view_debug() {
                        eprintln!(
                            "[plugin-view] attach ok editor_id={} handle=0x{handle:x} parent=0x{parent:x} mode={mode:?} visible=deferred (probing ready)",
                            self.editor_id()
                        );
                    }
                    self.schedule_ready_probe(0, cx);
                }
            }
            None => {
                let err = self
                    .processor
                    .last_error()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        "failed to attach editor to runtime plugin instance \
                         (no ready VST3 processor for this insert)"
                            .to_string()
                    });
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

    fn apply_native_auto_size(&mut self, window: &mut Window) -> Option<EmbedRegion> {
        let (content_w, content_h) = self.processor.embed_content_size()?;
        self.editor_content_size = Some((content_w, content_h));

        let scale = window.scale_factor().max(0.5);
        let shell_w = (content_w as f32 / scale).max(EDITOR_WINDOW_MIN_WIDTH);
        let shell_h = ((content_h as f32 / scale) + HEADER_H).max(EDITOR_WINDOW_MIN_HEIGHT);
        window.resize(size(px(shell_w), px(shell_h)));

        let region = self.host_region_for(window);
        if plugin_view_debug() {
            eprintln!(
                "[plugin-view] auto_size plugin=\"{}\" shell={:.0}x{:.0} content={}x{} editor_id={}",
                self.display_name,
                shell_w,
                shell_h,
                region.width,
                region.height,
                self.editor_id()
            );
        }
        Some(region)
    }

    /// User-initiated retry from the failure panel: tear down any partial state
    /// and restart the lifecycle from `Opening`.
    fn retry(&mut self, cx: &mut Context<Self>) {
        if self.embed_handle.take().is_some() {
            // Detach the editor view only — the runtime processor keeps running.
            self.processor.embed_detach();
        }
        self.status = PluginEditorStatus::Opening;
        self.wait_ticks = 0;
        self.host_mounted_logged = false;
        self.last_region = None;
        self.editor_content_size = None;
        if plugin_view_debug() {
            eprintln!(
                "[plugin-view] retry requested editor_id={}",
                self.editor_id()
            );
        }
        cx.notify();
    }

    /// Phase 6: schedule a deferred visible-UI re-check. WebView/CEF editors
    /// (UAD Native, Slate, some iZotope) routinely take 100–3000 ms after
    /// `IPlugView::attached()` before any visible child window materializes.
    /// We poll at the Phase-6 milestones (100/500/1000/3000/5000 ms); the
    /// first probe to see visible UI promotes the editor to `Attached`. The
    /// final probe surfaces a failure if everything is still blank.
    fn schedule_ready_probe(&mut self, probe_index: u8, cx: &mut Context<Self>) {
        let idx = probe_index as usize;
        let Some(&delay_ms) = READY_PROBE_DELAYS_MS.get(idx) else {
            // Out of range — caller should have promoted by now.
            return;
        };
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            executor.timer(Duration::from_millis(delay_ms)).await;
            let _ = this.update(cx, |this, cx| {
                this.on_ready_probe(probe_index, cx);
            });
        })
        .detach();
    }

    fn on_ready_probe(&mut self, probe_index: u8, cx: &mut Context<Self>) {
        // Only act if we are still in ProbingReady for *this* probe sequence —
        // a retry or close may have moved the state under us.
        let PluginEditorStatus::ProbingReady {
            mode,
            probe_index: current,
        } = self.status.clone()
        else {
            return;
        };
        if current != probe_index {
            return;
        }
        // Extra refresh nudges any pending message queue and re-applies bounds.
        self.processor.embed_refresh();
        // Quirked plug-ins (UAD Native and other CEF/WebView editors) benefit
        // from a second pump on each probe step — Chromium often delivers its
        // first child window during a later message dispatch.
        if self.quirk.extra_message_pump {
            self.processor.embed_refresh();
        }
        let visible = self.processor.embed_has_visible_ui();
        let is_last = probe_index as usize + 1 >= READY_PROBE_DELAYS_MS.len();
        if plugin_view_debug() {
            eprintln!(
                "[plugin-view] ready-probe editor_id={} step={}/{} delay_ms={} visible={}",
                self.editor_id(),
                probe_index as usize + 1,
                READY_PROBE_DELAYS_MS.len(),
                READY_PROBE_DELAYS_MS[probe_index as usize],
                visible
            );
        }
        if visible {
            self.status = PluginEditorStatus::Attached(mode);
            cx.notify();
            return;
        }
        if is_last {
            // Cap reached and still blank — detach + show fallback panel.
            if self.embed_handle.take().is_some() {
                self.processor.embed_detach();
            }
            let msg = format!(
                "Editor attached but no visible WebView/editor window appeared \
                 after {} ms. The plug-in may host a Chromium/CEF view that did \
                 not initialize. Try Retry, switch to the Owned Tool Window \
                 fallback, or check the plug-in's runtime requirements.",
                READY_PROBE_DELAYS_MS.last().copied().unwrap_or(5000)
            );
            self.status = PluginEditorStatus::Failed(msg);
            cx.notify();
            return;
        }
        // Schedule the next probe in the ramp.
        self.status = PluginEditorStatus::ProbingReady {
            mode,
            probe_index: probe_index + 1,
        };
        self.schedule_ready_probe(probe_index + 1, cx);
    }

    fn sync_region(&mut self, window: &mut Window) {
        if !matches!(
            self.status,
            PluginEditorStatus::Attached(_) | PluginEditorStatus::ProbingReady { .. }
        ) {
            return;
        }
        if self.embed_handle.is_none() || !self.processor.embed_is_valid() {
            return;
        }
        // Detached: the plug-in's standalone window owns its own size/position.
        // Never resize the GPUI shell to it or push host bounds.
        if self.processor.embed_host_kind() == 2 {
            return;
        }
        if let Some(plugin_size) = self.processor.embed_content_size() {
            if self.editor_content_size != Some(plugin_size) {
                self.editor_content_size = Some(plugin_size);
                let scale = window.scale_factor().max(0.5);
                let shell_w = (plugin_size.0 as f32 / scale).max(EDITOR_WINDOW_MIN_WIDTH);
                let shell_h =
                    ((plugin_size.1 as f32 / scale) + HEADER_H).max(EDITOR_WINDOW_MIN_HEIGHT);
                window.resize(size(px(shell_w), px(shell_h)));
                if plugin_view_debug() {
                    eprintln!(
                        "[plugin-view] auto_size plugin=\"{}\" shell={:.0}x{:.0} content={}x{} editor_id={}",
                        self.display_name,
                        shell_w,
                        shell_h,
                        plugin_size.0,
                        plugin_size.1,
                        self.editor_id()
                    );
                }
            }
        }
        let region = self.host_region_for(window);
        let tuple = (region.x, region.y, region.width, region.height);
        // Only push an explicit resize when our client-relative region actually
        // changed (Part D — ignore resize events if the rect is unchanged).
        if self.last_region != Some(tuple) {
            self.last_region = Some(tuple);
            if plugin_view_debug() {
                eprintln!(
                    "[plugin-view] resize host bounds={{x:{},y:{},w:{},h:{}}} editor_id={}",
                    region.x,
                    region.y,
                    region.width,
                    region.height,
                    self.editor_id()
                );
            }
            self.processor
                .embed_set_bounds(region.x, region.y, region.width, region.height);
        }
        // Cheap per-frame poll so the overlay still tracks a *parent window move*
        // (screen coords change while our client-relative region does not). The
        // C++ side compares the recomputed screen rect against the last applied
        // one and no-ops when unchanged, so idle frames do no SetWindowPos /
        // onSize / raise work — no flicker, no resize spam.
        self.processor.embed_refresh();
    }

    // --- Host-process editor path (gated; in-process path above is untouched) ---

    /// Drive the separated-process editor lifecycle. Mirrors `drive` but the
    /// VST3 view lives in `FutureboardPluginHost-x64.exe`: the main app creates a
    /// content child HWND under its GPUI window and hands the handle to the host
    /// over IPC. Attach is event-driven (`HostEvent::EditorAttached`).
    fn drive_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 1. Drain host → client events into the shared status state machine.
        let mut events = Vec::new();
        if let Some(host) = self.host.as_ref() {
            while let Some(ev) = host.client.try_recv_event() {
                events.push(ev);
            }
        }
        for ev in events {
            self.on_host_event(ev, cx);
        }

        match self.status.clone() {
            PluginEditorStatus::Attached(_) => {
                self.sync_host_region(window);
                // Keep a light tick so a host crash (EditorDisconnected) is
                // noticed promptly even with no user interaction.
                self.schedule_tick(cx);
                return;
            }
            PluginEditorStatus::Failed(_) => return,
            PluginEditorStatus::Attaching | PluginEditorStatus::ProbingReady { .. } => {
                // Waiting for EditorAttached / EditorAttachFailed.
                self.schedule_tick(cx);
                return;
            }
            PluginEditorStatus::Opening | PluginEditorStatus::WaitingForHostHandle => {}
        }

        // 2. Need a valid GPUI top HWND before we can parent a content child.
        let Some(top) = Self::native_parent_handle(window) else {
            self.note_waiting("no native parent handle (host mode)", cx);
            return;
        };
        let region = self.host_region_for(window);
        if region.width <= 0 || region.height <= 0 {
            self.note_waiting("host bounds not ready (host mode)", cx);
            return;
        }
        let rect = ContentRect {
            x: region.x,
            y: region.y,
            width: region.width,
            height: region.height,
        };
        let dpi = (window.scale_factor().max(0.5) * 96.0).round() as u32;
        let id = self.editor_id();

        // 3. Create the main-app-owned content child HWND (content != top).
        let Some(content) = ContentChildHwnd::create(top, rect) else {
            self.status =
                PluginEditorStatus::Failed("failed to create content child HWND".to_string());
            cx.notify();
            return;
        };
        let content_hwnd = content.hwnd();
        eprintln!(
            "[plugin-view][host] top_hwnd=0x{top:x} content_hwnd=0x{content_hwnd:x} editor_id={id}"
        );

        // 4. Send OpenEditorWithParentHwnd to the host process.
        let (path, class_id) = {
            let host = self.host.as_ref().unwrap();
            (host.plugin_path.clone(), host.class_id.clone())
        };
        {
            let host = self.host.as_mut().unwrap();
            host.content = Some(content);
            let pid = host
                .host_pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "pending".to_string());
            eprintln!(
                "[plugin-bridge] sending OpenEditorWithParentHwnd instance={id} hwnd=0x{content_hwnd:x}"
            );
            match host.client.open_editor(
                id.clone(),
                path,
                class_id,
                content_hwnd,
                rect.width as u32,
                rect.height as u32,
                dpi,
            ) {
                Ok(()) => {
                    host.last_region = Some(rect);
                    eprintln!(
                        "[plugin-view][host] OpenEditorWithParentHwnd sent editor_id={id} \
                         content_hwnd=0x{content_hwnd:x} host_pid={pid} size={}x{} dpi={dpi}",
                        rect.width, rect.height
                    );
                }
                Err(e) => {
                    self.status = PluginEditorStatus::Failed(format!("send OpenEditor failed: {e}"));
                    cx.notify();
                    return;
                }
            }
        }
        self.wait_ticks = 0;
        self.status = PluginEditorStatus::Attaching;
        self.schedule_tick(cx);
        cx.notify();
    }

    /// Fold a host event into the existing `PluginEditorStatus` state machine.
    fn on_host_event(&mut self, ev: ClientEvent, cx: &mut Context<Self>) {
        let id = self.editor_id();
        match ev {
            ClientEvent::Host(HostEvent::Ready { pid, .. }) => {
                if let Some(host) = self.host.as_mut() {
                    host.host_pid = Some(pid);
                }
                eprintln!("[plugin-view][host] host ready pid={pid} editor_id={id}");
            }
            ClientEvent::Host(HostEvent::Pong { pid }) => {
                if let Some(host) = self.host.as_mut() {
                    host.host_pid = Some(pid);
                }
                eprintln!("[plugin-bridge] received Pong (late) pid={pid} editor_id={id}");
            }
            ClientEvent::Host(HostEvent::EditorAttached {
                result,
                preferred_width,
                preferred_height,
                ..
            }) => {
                eprintln!(
                    "[plugin-view][host] EditorAttached editor_id={id} attached_result={result} \
                     preferred={preferred_width}x{preferred_height}"
                );
                // Content is a WS_CHILD embed under the GPUI window.
                self.status =
                    PluginEditorStatus::Attached(PluginEditorPresentationMode::ChildHwndEmbed);
                cx.notify();
            }
            ClientEvent::Host(HostEvent::EditorAttachFailed { error, .. }) => {
                eprintln!("[plugin-view][host] EditorAttachFailed editor_id={id} error={error}");
                self.status = PluginEditorStatus::Failed(error);
                cx.notify();
            }
            ClientEvent::Host(HostEvent::EditorClosed { .. }) => {
                eprintln!("[plugin-view][host] EditorClosed editor_id={id}");
            }
            ClientEvent::Host(HostEvent::EditorPreferredSize { width, height, .. }) => {
                eprintln!(
                    "[plugin-view][host] EditorPreferredSize editor_id={id} {width}x{height}"
                );
            }
            ClientEvent::Host(HostEvent::PluginUnloaded { .. }) => {}
            ClientEvent::Host(HostEvent::Log { level, message }) => {
                eprintln!("[plugin-view][host][{level}] {message}");
            }
            ClientEvent::Disconnected => {
                eprintln!(
                    "[plugin-view][host] EditorDisconnected editor_id={id} (host process exited/crashed)"
                );
                self.status = PluginEditorStatus::Failed(
                    "Plugin host process disconnected (crashed or exited). \
                     The editor closed; audio is unaffected."
                        .to_string(),
                );
                cx.notify();
            }
        }
    }

    /// Push a resized content rect to both the content child HWND (geometry,
    /// owned by the main app) and the host (`ResizeEditor` → `onSize`).
    fn sync_host_region(&mut self, window: &mut Window) {
        let region = self.host_region_for(window);
        let rect = ContentRect {
            x: region.x,
            y: region.y,
            width: region.width,
            height: region.height,
        };
        let dpi = (window.scale_factor().max(0.5) * 96.0).round() as u32;
        let id = self.editor_id();
        let Some(host) = self.host.as_mut() else {
            return;
        };
        if host.last_region == Some(rect) {
            return;
        }
        host.last_region = Some(rect);
        if let Some(content) = host.content.as_ref() {
            if !content.is_valid() {
                return;
            }
            content.set_bounds(rect);
        }
        let _ = host
            .client
            .resize_editor(id.clone(), rect.width as u32, rect.height as u32, dpi);
        if plugin_view_debug() {
            eprintln!(
                "[plugin-view][host] resize editor_id={id} content=({},{},{}x{})",
                rect.x, rect.y, rect.width, rect.height
            );
        }
    }
}

impl Drop for PluginEditorWindow {
    fn drop(&mut self) {
        if crate::shutdown::ShutdownState::global().is_shutting_down() {
            return;
        }
        // Host-process path: ask the host to remove the view (spec Part 6), then
        // let the backend's Drop tear down the content HWND + the host process.
        if let Some(host) = self.host.as_mut() {
            let id = format!("{}::{}", self.track_id, self.insert_id);
            let _ = host.client.close_editor(id.clone());
            eprintln!("[plugin-view][host] CloseEditor sent editor_id={id} (drop) — tearing down content HWND + host process");
            return;
        }
        if self.embed_handle.take().is_some() {
            // Detach the editor view + destroy the host window. The runtime
            // processor (and audio) keep running — only insert removal destroys it.
            self.processor.embed_detach();
            if plugin_view_debug() {
                eprintln!(
                    "[plugin-view] close editor_id={} (drop → detach view only, processor kept)",
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

        // When attached, GPUI must not paint anything below the titlebar — gpui
        // composites its surface above child HWNDs (DirectComposition topmost). A
        // flex_1 content div would create an opaque compositor layer and hide the
        // native plugin even when attach reports ok. Only draw overlays while
        // opening / waiting / attaching / failed.
        let content_overlay: Option<gpui::AnyElement> = match &self.status {
            PluginEditorStatus::Opening => Some(self.render_status_message("Opening editor…")),
            PluginEditorStatus::WaitingForHostHandle => {
                Some(self.render_status_message("Opening editor… (waiting for host window)"))
            }
            PluginEditorStatus::Attaching => {
                Some(self.render_status_message("Attaching plugin editor…"))
            }
            PluginEditorStatus::ProbingReady { probe_index, .. } => {
                let step = (*probe_index as usize).saturating_add(1);
                let total = READY_PROBE_DELAYS_MS.len();
                Some(self.render_status_message(&format!(
                    "Opening editor… (waiting for plug-in UI, {step}/{total})"
                )))
            }
            PluginEditorStatus::Failed(err) => {
                let err = err.clone();
                Some(self.render_failure_panel(&err, cx))
            }
            PluginEditorStatus::Attached(PluginEditorPresentationMode::DetachedNativeWindow) => {
                // The plug-in is in its own standalone OS window — the GPUI shell
                // has no native plugin region to expose, so fill it with an
                // explanatory panel (closing this shell closes the editor).
                Some(self.render_status_message(
                    "Editor opened in a separate window. Closing this window closes the editor.",
                ))
            }
            PluginEditorStatus::Attached(mode) => {
                // Transparent hole — the single active host HWND is aligned to
                // this region. GPUI must not paint an opaque layer here or it
                // would composite over the native plugin.
                if plugin_view_debug() {
                    Some(
                        div()
                            .absolute()
                            .top(px(HEADER_H))
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(Colors::surface_base())
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(Colors::text_secondary())
                                    .child(format!("External editor overlay active ({mode:?})")),
                            )
                            .into_any_element(),
                    )
                } else {
                    None
                }
            }
        };

        let mut root = div()
            .relative()
            .size_full()
            .font(theme::ui_font())
            .overflow_hidden()
            .child(div().w(px(0.0)).h(px(0.0)).track_focus(&self.focus_handle))
            .child(external_window_titlebar(
                self.display_name.clone(),
                "plugin-editor-window-close",
                move |window, _cx| window.remove_window(),
            ));

        if let Some(overlay) = content_overlay {
            root = root.child(
                div()
                    .absolute()
                    .top(px(HEADER_H))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(overlay),
            );
        }

        root
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
    display_name: String,
    processor: DAUx::Vst3RuntimeProcessor,
    cx: &mut App,
) -> Result<WindowHandle<PluginEditorWindow>, String> {
    if plugin_host_bridge_enabled() {
        eprintln!(
            "[plugin-view] editor_backend=external_bridge reason=FUTUREBOARD_PLUGIN_HOST_BRIDGE=1 \
             instance={track_id}::{insert_id}"
        );
    } else {
        eprintln!(
            "[plugin-view] editor_backend=in_process reason=env_disabled instance={track_id}::{insert_id}"
        );
    }
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
    // Opaque shell: Transparent uses ACCENT_ENABLE_TRANSPARENTGRADIENT and shows
    // whatever window is *behind* this floating editor (timeline bleed-through).
    // The VST3 UI is a WS_CHILD under this HWND; with DirectComposition disabled
    // at app boot it composites above the swap chain in the content region.
    options.window_background = WindowBackgroundAppearance::Opaque;
    options.window_min_size = Some(size(
        px(EDITOR_WINDOW_MIN_WIDTH),
        px(EDITOR_WINDOW_MIN_HEIGHT),
    ));

    let editor_id = format!("{track_id}::{insert_id}");
    let result = cx.open_window(options, |_window, cx| {
        cx.new(|cx| PluginEditorWindow::new(track_id, insert_id, display_name, processor, cx))
    });
    if plugin_view_debug() {
        match &result {
            Ok(_) => eprintln!("[plugin-view] gpui window created id={editor_id}"),
            Err(e) => eprintln!("[plugin-view] gpui window create FAILED id={editor_id} err={e}"),
        }
    }
    result.map_err(|e| e.to_string())
}
