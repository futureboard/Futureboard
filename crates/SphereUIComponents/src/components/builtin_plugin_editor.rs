//! CEF host for built-in plugin editors.
//!
//! Built-in plugins ship their editor as a compiled React app embedded in the
//! plugin library. This module owns the browser-process side: the CEF runtime,
//! the `mikoplugin://` asset registry, and the child views parented into the
//! editor window shell.
//!
//! ## Availability
//!
//! Everything here is behind the `builtin-plugin-editor` feature. Without it,
//! [`availability`] reports why the editor cannot open, and the caller surfaces
//! that instead of showing an empty window. That is deliberate: a checkout with
//! no CEF SDK must still build and run.
//!
//! ## Threading
//!
//! CEF is initialized lazily on the GPUI UI thread and must only be driven from
//! there ([`CefRuntime`] enforces this). [`pump`] has to be called from the UI
//! loop or the browser never paints.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Why a built-in editor can or cannot be hosted right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAvailability {
    /// CEF is compiled in and the plugin has embedded UI assets.
    Ready,
    /// The binary was built without the `builtin-plugin-editor` feature.
    NotCompiledIn,
    /// The plugin id is not a built-in that ships an editor.
    NoEditorForPlugin(String),
    /// The plugin's `editorui/dist` was not built when the library was compiled.
    UiNotEmbedded(String),
    /// CEF failed to start.
    RuntimeFailed(String),
}

impl fmt::Display for HostAvailability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready => write!(f, "ready"),
            Self::NotCompiledIn => write!(
                f,
                "built-in plugin editors are not available in this build \
                 (rebuild with --features builtin-plugin-editor)"
            ),
            Self::NoEditorForPlugin(id) => write!(f, "{id} does not ship an editor UI"),
            Self::UiNotEmbedded(id) => write!(
                f,
                "{id} was compiled without its editor UI (run `bun run build` in its editorui/)"
            ),
            Self::RuntimeFailed(err) => write!(f, "CEF failed to start: {err}"),
        }
    }
}

/// The `mikoplugin://` origin for a built-in plugin.
///
/// Accepts both identifier forms a built-in travels under: the catalog id
/// (`builtin:rodharerist`) and the class id (`rodharerist`) that insert slots
/// actually store. Resolution is validated against the built-in catalog, so an
/// unprefixed external class id is never treated as a built-in.
pub fn origin_for_plugin_id(plugin_id: &str) -> Option<&'static str> {
    SpherePluginHost::resolve_builtin_stem(plugin_id)
}

/// Physical-pixel rect the editor view occupies inside its parent window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Process-unique identity for one concrete editor window.
///
/// The logical editor id (`track::insert`) can be reused after a window closes;
/// native host commands must not be, otherwise a delayed close from the old
/// window can tear down the newly opened browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewId(u64);

pub fn allocate_view_id() -> ViewId {
    static NEXT_VIEW_ID: AtomicU64 = AtomicU64::new(1);
    ViewId(NEXT_VIEW_ID.fetch_add(1, Ordering::Relaxed))
}

/// Completion events produced by the serialized CEF command processor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewEvent {
    Opened,
    OpenFailed(String),
    Closed,
    /// The renderer process for this browser terminated (crash, OOM-kill,
    /// `chrome://kill` in dev). The browser object and native DSP state on
    /// the Rust side are untouched — only the page's JS state is gone, so
    /// the window stays open and the browser reloads its own URL; once
    /// `bridgeReady` arrives again the current selection is re-sent (see
    /// `BuiltinPluginEditorWindow::push_selected_instance`, gated on
    /// `browser_ready`).
    RendererCrashed,
}

#[cfg(not(feature = "builtin-plugin-editor"))]
mod imp {
    use super::{HostAvailability, ViewEvent, ViewId, ViewRect};

    pub fn availability(_plugin_id: &str) -> HostAvailability {
        HostAvailability::NotCompiledIn
    }

    pub fn pump() {}

    pub fn open_view(
        _view_id: ViewId,
        _editor_id: &str,
        _plugin_id: &str,
        _parent_hwnd: u64,
        _rect: ViewRect,
    ) -> Result<(), HostAvailability> {
        Err(HostAvailability::NotCompiledIn)
    }

    pub fn init_at_boot() -> Result<(), HostAvailability> {
        Err(HostAvailability::NotCompiledIn)
    }

    pub fn set_view_bounds(_view_id: ViewId, _rect: ViewRect) {}

    pub fn close_view(_view_id: ViewId) {}

    pub fn take_view_events(_view_id: ViewId) -> Vec<ViewEvent> {
        Vec::new()
    }

    pub fn is_view_open(_view_id: ViewId) -> bool {
        false
    }

    pub fn take_inbound(_origin: &str) -> Vec<Vec<u8>> {
        Vec::new()
    }

    pub fn send_to_view(_view_id: ViewId, _code: &str) {}

    pub fn reload_view(_view_id: ViewId) {}
}

#[cfg(feature = "builtin-plugin-editor")]
mod imp {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    use sphere_webview::runtime::{
        CefRuntime, CefRuntimeConfig, NativeParent, WebView, WebViewConfig, WindowBounds,
    };
    use sphere_webview::client::{plugin_browser_client, CrashFlag};
    use sphere_webview::scheme::{register_plugin_scheme_factory, BridgeSink, SchemeAsset};

    use super::{origin_for_plugin_id, HostAvailability, ViewEvent, ViewId, ViewRect};

    struct HostedView {
        _editor_id: String,
        view: WebView<'static>,
        crash_flag: CrashFlag,
    }

    struct ClosingView {
        hosted: HostedView,
        pump_ticks: u16,
    }

    const MAX_CLOSE_PUMP_TICKS: u16 = 250;

    /// One CEF runtime plus every open editor view.
    ///
    /// Field order is load-bearing: Rust drops fields in declaration order, and
    /// the detached views must be released before the runtime that created them
    /// (see `CefRuntime::create_webview_detached`).
    struct Host {
        views: HashMap<ViewId, HostedView>,
        closing_views: HashMap<ViewId, ClosingView>,
        runtime: CefRuntime,
    }

    struct PendingOpen {
        editor_id: String,
        origin: &'static str,
        parent_hwnd: u64,
        rect: ViewRect,
    }

    #[derive(Default)]
    struct PendingViewCommands {
        open: Option<PendingOpen>,
        bounds: Option<ViewRect>,
        close: bool,
    }

    thread_local! {
        /// Browser-process CEF state, owned by the UI thread because
        /// `CefRuntime` is `!Send`.
        static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
        /// Native operations are queued separately from `HOST`. GPUI/Win32 can
        /// re-enter while CEF is working; nested callbacks may enqueue commands,
        /// but must never try to borrow or call CEF synchronously.
        static COMMANDS: RefCell<HashMap<ViewId, PendingViewCommands>> = RefCell::new(HashMap::new());
        static EVENTS: RefCell<HashMap<ViewId, Vec<ViewEvent>>> = RefCell::new(HashMap::new());
        static PUMPING: Cell<bool> = const { Cell::new(false) };
    }

    struct PumpGuard;

    impl PumpGuard {
        fn enter() -> Option<Self> {
            PUMPING.with(|pumping| {
                if pumping.replace(true) {
                    None
                } else {
                    Some(Self)
                }
            })
        }
    }

    impl Drop for PumpGuard {
        fn drop(&mut self) {
            PUMPING.with(|pumping| pumping.set(false));
        }
    }

    /// Resolve `mikoplugin://<origin>/<path>` to embedded bytes.
    ///
    /// This is the isolation boundary: an origin that is not a known built-in
    /// returns `None`, so one plugin's editor can never read another's assets.
    /// Runs on CEF's IO thread — it only indexes static tables.
    fn resolve_asset(origin: &str, path: &str) -> Option<SchemeAsset> {
        let asset = match origin {
            rodharerist::ui::UI_ORIGIN => {
                use builtin_ui_embed::EmbeddedPluginUi;
                rodharerist::ui::RodhareistUi::resolve_ui_asset(path)?
            }
            _ => return None,
        };
        Some(SchemeAsset {
            bytes: asset.bytes,
            mime_type: asset.mime_type,
        })
    }

    /// Whether a built-in plugin has embedded editor assets to serve.
    fn has_embedded_ui(origin: &str) -> bool {
        match origin {
            rodharerist::ui::UI_ORIGIN => rodharerist::ui::RodhareistUi::is_embedded(),
            _ => false,
        }
    }

    /// React->native bridge inbound queue, keyed by scheme origin (the same
    /// stem `resolve_asset` matches on, e.g. `"rodharerist"`). Filled from
    /// CEF's IO thread (`bridge_sink`, below) — process-wide `Mutex`, not a
    /// UI-thread `thread_local!` like `HOST`/`COMMANDS`, since the scheme
    /// factory callback does not run on the UI thread. Drained by
    /// `take_inbound`, called from the GPUI pump tick (non-realtime).
    static INBOUND: OnceLock<Mutex<HashMap<String, Vec<Vec<u8>>>>> = OnceLock::new();

    fn inbound_map() -> &'static Mutex<HashMap<String, Vec<Vec<u8>>>> {
        INBOUND.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn bridge_sink() -> BridgeSink {
        Arc::new(|origin: &str, body: Vec<u8>| {
            if let Ok(mut map) = inbound_map().lock() {
                map.entry(origin.to_string()).or_default().push(body);
            }
        })
    }

    /// Drain every bridge message POSTed by `origin`'s page since the last
    /// call. Never blocks on CEF — just takes whatever `bridge_sink` queued.
    pub fn take_inbound(origin: &str) -> Vec<Vec<u8>> {
        inbound_map()
            .lock()
            .ok()
            .and_then(|mut map| map.remove(origin))
            .unwrap_or_default()
    }

    /// Run `code` in `view_id`'s document. No-op (not an error) if the view
    /// isn't open yet or already closed — callers already gate on
    /// `is_view_open`/`ViewEvent::Opened` where it matters.
    pub fn send_to_view(view_id: ViewId, code: &str) {
        HOST.with(|cell| {
            if let Ok(slot) = cell.try_borrow() {
                if let Some(host) = slot.as_ref() {
                    if let Some(hosted) = host.views.get(&view_id) {
                        if let Err(error) = hosted.view.execute_javascript(code) {
                            eprintln!(
                                "[plugin-bridge] execute_javascript failed view_id={view_id:?} err={error}"
                            );
                        }
                    }
                }
            }
        });
    }

    /// Reload `view_id`'s document. Used by the bridge-ready watchdog to
    /// recover a page whose load silently died (e.g. Chromium's network
    /// service crashed mid-transfer, which it auto-restarts for *future*
    /// requests but does not retry the one already in flight, so a page can
    /// finish HTTP-200 headers and still never actually paint).
    pub fn reload_view(view_id: ViewId) {
        HOST.with(|cell| {
            if let Ok(slot) = cell.try_borrow() {
                if let Some(host) = slot.as_ref() {
                    if let Some(hosted) = host.views.get(&view_id) {
                        if let Err(error) = hosted.view.reload() {
                            eprintln!(
                                "[plugin-bridge] reload failed view_id={view_id:?} err={error}"
                            );
                        }
                    }
                }
            }
        });
    }

    pub fn availability(plugin_id: &str) -> HostAvailability {
        let Some(origin) = origin_for_plugin_id(plugin_id) else {
            return HostAvailability::NoEditorForPlugin(plugin_id.to_string());
        };
        if origin != rodharerist::ui::UI_ORIGIN {
            return HostAvailability::NoEditorForPlugin(plugin_id.to_string());
        }
        if !has_embedded_ui(origin) {
            return HostAvailability::UiNotEmbedded(plugin_id.to_string());
        }
        HostAvailability::Ready
    }

    /// Start CEF if it is not already running. Idempotent.
    fn ensure_runtime(slot: &mut Option<Host>) -> Result<(), HostAvailability> {
        if slot.is_some() {
            return Ok(());
        }

        // The same scheme declaration the helper processes made in `main`.
        let mut app = sphere_webview::scheme::plugin_scheme_app();
        let config = CefRuntimeConfig {
            cache_path: cef_cache_dir(),
            remote_debugging_port: debug_port(),
            ..Default::default()
        };
        let runtime = CefRuntime::initialize(config, Some(&mut app))
            .map_err(|e| HostAvailability::RuntimeFailed(e.to_string()))?;

        // The factory can only be installed once initialize has succeeded.
        let resolver: sphere_webview::scheme::SchemeResolver =
            Arc::new(|origin: &str, path: &str| resolve_asset(origin, path));
        register_plugin_scheme_factory(resolver, Some(bridge_sink()))
            .map_err(|e| HostAvailability::RuntimeFailed(e.to_string()))?;

        *slot = Some(Host {
            views: HashMap::new(),
            closing_views: HashMap::new(),
            runtime,
        });
        Ok(())
    }

    /// Start CEF during application boot, on the UI thread.
    ///
    /// Initialization spawns Chromium's helper processes and takes on the order
    /// of a few hundred milliseconds. Doing it lazily on first editor open means
    /// paying that cost inside a render pass, which stalls the UI thread and
    /// delays the first paint of the editor window. Doing it at boot moves the
    /// cost into startup, where there is already a loading screen.
    ///
    /// The thread that calls this is the thread that must later drive
    /// [`pump`] and create every view — `CefRuntime` enforces that.
    ///
    /// Failure is not fatal: the editor route falls back to reporting the error
    /// in its window rather than bringing down the app.
    pub fn init_at_boot() -> Result<(), HostAvailability> {
        HOST.with(|cell| {
            let mut slot = cell.borrow_mut();
            ensure_runtime(&mut slot)
        })
    }

    /// Queue creation of the editor browser for `plugin_id` as a child of
    /// `parent_hwnd`.
    ///
    /// No CEF API is called here. This function is intentionally safe to invoke
    /// from a GPUI render/update: [`pump`] executes the native operation later,
    /// after GPUI has released its `AppCell` and entity borrows.
    pub fn open_view(
        view_id: ViewId,
        editor_id: &str,
        plugin_id: &str,
        parent_hwnd: u64,
        rect: ViewRect,
    ) -> Result<(), HostAvailability> {
        match super::availability(plugin_id) {
            HostAvailability::Ready => {}
            other => return Err(other),
        }
        let Some(origin) = origin_for_plugin_id(plugin_id) else {
            return Err(HostAvailability::NoEditorForPlugin(plugin_id.to_string()));
        };
        if parent_hwnd == 0 {
            return Err(HostAvailability::RuntimeFailed(
                "editor window has no native parent handle yet".to_string(),
            ));
        }
        WindowBounds::new(rect.x, rect.y, rect.width, rect.height)
            .map_err(|e| HostAvailability::RuntimeFailed(e.to_string()))?;

        COMMANDS.with(|commands| {
            let mut commands = commands.borrow_mut();
            let pending = commands.entry(view_id).or_default();
            if pending.close {
                return Err(HostAvailability::RuntimeFailed(
                    "editor view is already closing".to_string(),
                ));
            }
            pending.open = Some(PendingOpen {
                editor_id: editor_id.to_string(),
                origin,
                parent_hwnd,
                rect,
            });
            pending.bounds = Some(rect);
            Ok(())
        })
    }

    /// Coalesce a browser resize for the next pump. An unknown id is allowed:
    /// the latest bounds are retained while its open command is still pending.
    pub fn set_view_bounds(view_id: ViewId, rect: ViewRect) {
        if rect.width <= 0 || rect.height <= 0 {
            return;
        }
        COMMANDS.with(|commands| {
            let mut commands = commands.borrow_mut();
            let pending = commands.entry(view_id).or_default();
            if !pending.close {
                pending.bounds = Some(rect);
            }
        });
    }

    /// Queue a close. Close dominates an unprocessed open/resize for this unique
    /// view id, so closing a shell before its first pump never creates a browser
    /// against a dead parent HWND.
    pub fn close_view(view_id: ViewId) {
        COMMANDS.with(|commands| {
            let mut commands = commands.borrow_mut();
            let pending = commands.entry(view_id).or_default();
            pending.open = None;
            pending.bounds = None;
            pending.close = true;
        });
    }

    pub fn take_view_events(view_id: ViewId) -> Vec<ViewEvent> {
        EVENTS.with(|events| events.borrow_mut().remove(&view_id).unwrap_or_default())
    }

    pub fn is_view_open(view_id: ViewId) -> bool {
        let pending_open = COMMANDS.with(|commands| {
            commands
                .try_borrow()
                .ok()
                .and_then(|commands| {
                    commands
                        .get(&view_id)
                        .map(|pending| pending.open.is_some() && !pending.close)
                })
                .unwrap_or(false)
        });
        pending_open
            || HOST.with(|cell| {
                cell.try_borrow()
                    .ok()
                    .and_then(|slot| slot.as_ref().map(|host| host.views.contains_key(&view_id)))
                    .unwrap_or(false)
            })
    }

    /// Execute queued native operations and advance CEF's message loop.
    ///
    /// Call this only from a GPUI foreground task *outside* `AsyncApp::update`.
    /// CEF may synchronously dispatch Win32 messages; keeping every GPUI borrow
    /// out of this stack is what prevents `AppCell` double-borrow panics.
    pub fn pump() {
        let Some(_guard) = PumpGuard::enter() else {
            return;
        };
        let commands = COMMANDS.with(|commands| std::mem::take(&mut *commands.borrow_mut()));
        let mut completed = Vec::new();

        HOST.with(|cell| {
            let mut slot = cell.borrow_mut();
            if let Err(error) = ensure_runtime(&mut slot) {
                for (view_id, pending) in commands {
                    if pending.close {
                        completed.push((view_id, ViewEvent::Closed));
                    } else if pending.open.is_some() {
                        completed.push((view_id, ViewEvent::OpenFailed(error.to_string())));
                    }
                }
                return;
            }
            let host = slot.as_mut().expect("ensure_runtime installs the host");

            for (view_id, pending) in commands {
                if pending.close {
                    if let Some(hosted) = host.views.remove(&view_id) {
                        let _ = hosted.view.close(true);
                        host.closing_views.insert(
                            view_id,
                            ClosingView {
                                hosted,
                                pump_ticks: 0,
                            },
                        );
                    } else if !host.closing_views.contains_key(&view_id) {
                        // A close that canceled an unprocessed open has no native
                        // browser lifetime to wait for.
                        completed.push((view_id, ViewEvent::Closed));
                    }
                    continue;
                }

                let mut opened_now = false;
                if let Some(open) = pending.open {
                    if host.views.contains_key(&view_id) {
                        completed.push((view_id, ViewEvent::Opened));
                    } else {
                        let rect = pending.bounds.unwrap_or(open.rect);
                        let (mut client, crash_flag) = plugin_browser_client();
                        let result = WindowBounds::new(rect.x, rect.y, rect.width, rect.height)
                            .map_err(|error| error.to_string())
                            .and_then(|bounds| {
                                let url = format!("mikoplugin://{}/index.html", open.origin);
                                // SAFETY: the returned view is stored in
                                // `host.views`, declared before `host.runtime`,
                                // and therefore released first.
                                unsafe {
                                    let parent =
                                        NativeParent::from_raw(hwnd_to_cef(open.parent_hwnd));
                                    host.runtime.create_webview_detached(
                                        parent,
                                        WebViewConfig::new(url, bounds),
                                        Some(&mut client),
                                    )
                                }
                                .map_err(|error| error.to_string())
                            });
                        match result {
                            Ok(view) => {
                                host.views.insert(
                                    view_id,
                                    HostedView {
                                        _editor_id: open.editor_id,
                                        view,
                                        crash_flag,
                                    },
                                );
                                opened_now = true;
                                completed.push((view_id, ViewEvent::Opened));
                            }
                            Err(error) => {
                                completed.push((view_id, ViewEvent::OpenFailed(error)));
                            }
                        }
                    }
                }

                if !opened_now {
                    if let Some(rect) = pending.bounds {
                        if let Some(hosted) = host.views.get(&view_id) {
                            if let Ok(bounds) =
                                WindowBounds::new(rect.x, rect.y, rect.width, rect.height)
                            {
                                let _ = hosted.view.set_bounds(bounds);
                            }
                        }
                    }
                }
            }

            let _ = host.runtime.do_message_loop_work();

            // Renderer crash detection (`client::CrashFlag`, set from
            // `on_render_process_terminated`). The browser object and native
            // DSP state survive a renderer crash — only the page's JS state
            // is gone — so this reloads the same URL rather than tearing the
            // window down; `ViewEvent::RendererCrashed` lets the GPUI window
            // reset `browser_ready` so it re-sends the current selection once
            // the fresh page announces `bridgeReady` again.
            for (view_id, hosted) in &host.views {
                if hosted.crash_flag.take() {
                    eprintln!("[plugin-scheme] reloading crashed renderer view_id={view_id:?}");
                    let _ = hosted.view.reload();
                    completed.push((*view_id, ViewEvent::RendererCrashed));
                }
            }

            // `close_browser(true)` is asynchronous. Keep both the WebView and
            // the GPUI-owned parent shell alive until CEF's native child HWND is
            // actually gone; only then may the window consume `Closed` and tear
            // down its HWND hierarchy. The timeout is a bounded shutdown escape
            // hatch for a wedged renderer process.
            let mut closed = Vec::new();
            for (view_id, closing) in &mut host.closing_views {
                closing.pump_ticks = closing.pump_ticks.saturating_add(1);
                if !webview_window_alive(&closing.hosted.view)
                    || closing.pump_ticks >= MAX_CLOSE_PUMP_TICKS
                {
                    closed.push(*view_id);
                }
            }
            for view_id in closed {
                host.closing_views.remove(&view_id);
                completed.push((view_id, ViewEvent::Closed));
            }
        });

        if !completed.is_empty() {
            EVENTS.with(|events| {
                let mut events = events.borrow_mut();
                for (view_id, event) in completed {
                    events.entry(view_id).or_default().push(event);
                }
            });
        }
    }

    #[cfg(target_os = "windows")]
    fn webview_window_alive(view: &WebView<'_>) -> bool {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::IsWindow;

        let Ok(handle) = view.native_window_handle() else {
            return false;
        };
        unsafe { IsWindow(Some(HWND(handle.0.cast()))).as_bool() }
    }

    #[cfg(not(target_os = "windows"))]
    fn webview_window_alive(_view: &WebView<'_>) -> bool {
        false
    }

    /// On Windows `cef_window_handle_t` is cef-dll-sys's own `HWND` newtype,
    /// which is distinct from the `windows` crate's `HWND` the rest of the app
    /// passes around as a `u64`.
    #[cfg(target_os = "windows")]
    fn hwnd_to_cef(handle: u64) -> sphere_webview::runtime::cef::sys::cef_window_handle_t {
        sphere_webview::runtime::cef::sys::HWND(handle as *mut _)
    }

    #[cfg(not(target_os = "windows"))]
    fn hwnd_to_cef(handle: u64) -> sphere_webview::runtime::cef::sys::cef_window_handle_t {
        handle as _
    }

    /// Per-user cache directory. CEF requires a writable path; an unwritable or
    /// missing one degrades to in-memory, which is acceptable for an editor UI.
    fn cef_cache_dir() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .or_else(dirs_cache_fallback)?;
        let dir = base.join("Futureboard").join("cef");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    fn dirs_cache_fallback() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .map(|home| home.join(".cache"))
    }

    /// Opens Chromium's remote-debugging endpoint (`http://127.0.0.1:<port>`)
    /// so a real browser's devtools can inspect the editor's console/network
    /// when `FUTUREBOARD_PLUGIN_VIEW_DEBUG=1` — otherwise off, since it is a
    /// local unauthenticated debug surface.
    fn debug_port() -> Option<u16> {
        std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").map(|_| 9222)
    }
}

pub use imp::{
    availability, close_view, init_at_boot, is_view_open, open_view, pump, reload_view,
    send_to_view, set_view_bounds, take_inbound, take_view_events,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_ids_map_to_their_url_origin() {
        assert_eq!(
            origin_for_plugin_id("builtin:rodharerist"),
            Some("rodharerist")
        );
        assert_eq!(origin_for_plugin_id("builtin:equz8"), Some("equz8"));
    }

    /// Regression guard for the bug that left the editor unopenable in the real
    /// app: an insert slot stores the registry *class id* (`rodharerist`), not
    /// the catalog id (`builtin:rodharerist`), so both forms must resolve.
    #[test]
    fn the_class_id_form_stored_by_insert_slots_also_resolves() {
        assert_eq!(origin_for_plugin_id("rodharerist"), Some("rodharerist"));
        assert_eq!(origin_for_plugin_id("equz8"), Some("equz8"));
        assert_eq!(
            origin_for_plugin_id("rodharerist"),
            origin_for_plugin_id("builtin:rodharerist"),
            "both identifier forms must resolve to the same origin"
        );
    }

    /// Resolution is catalog-validated, not shape-based: an external plug-in
    /// whose class id merely lacks a prefix must not be mistaken for a built-in.
    #[test]
    fn unknown_unprefixed_ids_are_not_treated_as_built_ins() {
        assert_eq!(origin_for_plugin_id("SomeVst3ControllerClass"), None);
        assert_eq!(origin_for_plugin_id("builtin:not-a-real-plugin"), None);
    }

    /// Regression guard for the routing bug that made built-in editors do
    /// nothing: `open_insert_editor` must dispatch on the plugin id alone.
    ///
    /// Built-ins have no VST3 runtime instance, so their `runtime_state` sits at
    /// `NotLoaded` forever. The editor route therefore cannot depend on runtime
    /// state, load status, plugin path, or plugin format — only on the id. If
    /// this identification ever stops being self-contained, the built-in branch
    /// will fall through into the VST3 gate again and silently return.
    #[test]
    fn built_in_routing_depends_only_on_the_plugin_id() {
        // Both forms, since the editor route is reached from an insert slot.
        for id in [
            "builtin:rodharerist",
            "rodharerist",
            "builtin:equz8",
            "equz8",
        ] {
            assert!(
                SpherePluginHost::is_builtin_ref(id),
                "{id} must be routable without consulting runtime state"
            );
            assert!(origin_for_plugin_id(id).is_some());
        }
        for id in ["vst3:foo", "clap:bar", "", "definitely-not-builtin"] {
            assert!(!SpherePluginHost::is_builtin_ref(id));
        }
    }

    #[test]
    fn external_plugin_ids_have_no_origin() {
        assert_eq!(origin_for_plugin_id("vst3:some-plugin"), None);
        assert_eq!(origin_for_plugin_id("some-vst3-class"), None);
        assert_eq!(origin_for_plugin_id(""), None);
    }

    #[test]
    fn availability_explains_itself_rather_than_returning_a_bare_bool() {
        // Whatever the build config, a non-built-in never reports Ready and the
        // reason is always human-readable.
        let result = availability("vst3:whatever");
        assert_ne!(result, HostAvailability::Ready);
        assert!(!result.to_string().is_empty());
    }

    #[cfg(not(feature = "builtin-plugin-editor"))]
    #[test]
    fn without_the_feature_every_plugin_reports_not_compiled_in() {
        assert_eq!(
            availability("builtin:rodharerist"),
            HostAvailability::NotCompiledIn
        );
        assert!(HostAvailability::NotCompiledIn
            .to_string()
            .contains("builtin-plugin-editor"));
    }

    #[cfg(feature = "builtin-plugin-editor")]
    #[test]
    fn rodhareist_is_hostable_and_unknown_builtins_are_not() {
        // rodharerist embeds a UI in any build that ran its build script with a
        // built dist; either way it must never be `NotCompiledIn` here.
        assert_ne!(
            availability("builtin:rodharerist"),
            HostAvailability::NotCompiledIn
        );
        assert_eq!(
            availability("builtin:equz8"),
            HostAvailability::NoEditorForPlugin("builtin:equz8".to_string())
        );
    }
}
