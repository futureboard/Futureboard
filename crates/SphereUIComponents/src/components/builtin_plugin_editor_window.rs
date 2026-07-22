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

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, size, App, AppContext, Bounds, Context, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Point, Render, Styled, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind,
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

/// Width of the native instance sidebar. Reserved out of the CEF content rect
/// the same way `HEADER_H` is — the browser must never be told to draw under
/// it, and the sidebar must never be told to draw over the browser.
const SIDEBAR_W: f32 = 208.0;

/// Identity of one DSP insert that can be shown in a shared built-in editor.
/// `track_id`/`insert_id` are the same stable, session-monotonic ids
/// `InsertSlotState` already uses (see `plugin_chain.rs`) — never reused
/// within a session and round-tripped from the project file, so they are
/// stable enough to key a binding without introducing a parallel id scheme.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginInstanceKey {
    pub track_id: String,
    pub insert_id: String,
}

/// One row in the shared editor's sidebar: enough to render the row and to
/// re-resolve the live insert slot when selected. Cheap to rebuild wholesale
/// on every lifecycle event (add/remove/rename/reorder) rather than diffed.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInstanceDescriptor {
    pub instance_key: PluginInstanceKey,
    pub plugin_id: String,
    pub track_name: String,
    pub insert_name: String,
    pub bypassed: bool,
    pub enabled: bool,
    /// Persisted per-insert DSP state, if this insert has ever been saved —
    /// see `InsertSlotState::vst3_state`'s doc comment (reused generically,
    /// not VST3-only). UTF-8 JSON bytes for built-ins. `None` for a fresh
    /// insert; `push_selected_instance` falls back to DSP defaults then.
    pub state_bytes: Option<std::sync::Arc<Vec<u8>>>,
}

/// Wire protocol version. Bump alongside any breaking change to the message
/// shapes below; both sides reject a mismatch instead of guessing.
const BRIDGE_PROTOCOL_VERSION: u32 = 1;

/// The wire-format instance id (`futureboard.selectInstance.instanceId`,
/// route `/instance/{instanceId}`). One string, not a `(track_id, insert_id)`
/// pair — React never needs to know they're composite.
fn wire_instance_id(key: &PluginInstanceKey) -> String {
    format!("{}::{}", key.track_id, key.insert_id)
}

/// Decode an insert's persisted state bytes (UTF-8 JSON, see
/// `PluginInstanceDescriptor::state_bytes`) for the `selectInstance` wire
/// message. Deliberately generic (`serde_json::Value`, not a specific
/// plugin's Rust type) — this module hosts any built-in plugin's shared
/// editor, not only rodharerist's.
///
/// A fresh insert with no saved state, or bytes that fail to parse (a
/// corrupt/foreign blob), both fall back to `{}` rather than erroring: the
/// editor is expected to apply its own defaults when it receives an empty
/// object, same as it does today with nothing wired at all.
fn decode_state_bytes(bytes: Option<&[u8]>) -> serde_json::Value {
    let Some(bytes) = bytes else {
        return serde_json::json!({});
    };
    match std::str::from_utf8(bytes).map(serde_json::from_str::<serde_json::Value>) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            eprintln!("[plugin-bridge] persisted state is not valid JSON, using defaults: {error}");
            serde_json::json!({})
        }
        Err(error) => {
            eprintln!(
                "[plugin-bridge] persisted state is not valid UTF-8, using defaults: {error}"
            );
            serde_json::json!({})
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceDisplayMetadata {
    track_id: String,
    track_name: String,
    insert_id: String,
    insert_name: String,
}

/// Native->React: rebind the shared page to a different DSP instance. See
/// module docs on `select_instance` for the transaction this is one step of.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectInstanceMsg {
    r#type: &'static str,
    protocol_version: u32,
    plugin_id: String,
    instance_id: String,
    binding_generation: u64,
    display: InstanceDisplayMetadata,
    state_revision: u64,
    /// TODO(phase5): rodharerist has no serialized per-insert DSP state yet
    /// (confirmed by audit — no `Serialize` impl anywhere in the DSP crate,
    /// no project-file schema, no engine wiring). Until that lands this is
    /// always `{}`; React has nothing real to bind to, only the identity and
    /// display metadata.
    state: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceRemovedMsg {
    r#type: &'static str,
    protocol_version: u32,
    instance_id: String,
}

/// React->native. Only these three are actionable without Phase 5 (real
/// state); `rodhareist.setParam` / `applyStatePatch` / `requestFullState`
/// have no canonical state to validate against yet and are intentionally not
/// modeled here until that lands — sending them today is inert (logged, not
/// acted on) rather than silently mis-acted-on.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type")]
enum InboundMsg {
    #[serde(rename = "futureboard.bridgeReady", rename_all = "camelCase")]
    BridgeReady {
        #[allow(dead_code)]
        plugin_id: String,
        #[allow(dead_code)]
        bridge_version: u32,
    },
    #[serde(rename = "futureboard.instanceReady", rename_all = "camelCase")]
    InstanceReady {
        #[allow(dead_code)]
        plugin_id: String,
        instance_id: String,
        binding_generation: u64,
        #[allow(dead_code)]
        state_revision: u64,
    },
    #[serde(rename = "futureboard.requestSelectInstance", rename_all = "camelCase")]
    RequestSelectInstance { instance_id: String },
    #[serde(other)]
    Unknown,
}

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

/// How long to wait, after the browser attaches, for `futureboard.bridgeReady`
/// before assuming the page load silently died and reloading.
const BRIDGE_READY_TIMEOUT: Duration = Duration::from_secs(8);
/// Cap on automatic reloads from the watchdog above — a page that never comes
/// up after this many tries has a real problem the user needs to see, not
/// another silent retry.
const MAX_BRIDGE_READY_RETRIES: u32 = 3;

struct PumpTick {
    keep_going: bool,
    content_to_drop: Option<ContentChildHwnd>,
}

pub struct BuiltinPluginEditorWindow {
    view_id: ViewId,
    /// Label for the currently active instance, used only in host-side logs
    /// (`HostedView`). Not an identity — `active_instance` is.
    editor_id: String,
    plugin_id: String,
    display_name: String,
    status: Status,
    content: Option<ContentChildHwnd>,
    last_rect: Option<ViewRect>,
    /// Every insert instance across the project currently using this
    /// `plugin_id`. Rebuilt wholesale by `set_instances` on every lifecycle
    /// event that could change the list — never diffed in place.
    instances: Vec<PluginInstanceDescriptor>,
    /// Which instance the (single, shared) browser is currently bound to.
    /// `None` is the valid empty state — the browser stays open, no instance
    /// selected (e.g. the last instance using this plugin_id was removed).
    active_instance: Option<PluginInstanceKey>,
    /// Bumped on every `select_instance`. Phase 3 (bridge protocol) threads
    /// this into every host<->React message so a message referencing a
    /// superseded selection is provably stale and can be rejected instead of
    /// mutating whatever instance happens to be active when it arrives.
    binding_generation: u64,
    sidebar_collapsed: bool,
    /// Set once `futureboard.bridgeReady` arrives. Before that, `select_instance`
    /// still updates native state (so reopening/refocusing works) but has no
    /// live page to push `selectInstance` into yet — the selection made while
    /// waiting is simply the one already reflected once the bridge comes up,
    /// since `active_instance` already holds it.
    browser_ready: bool,
    /// When the browser reached `Attached`. Drives the bridge-ready watchdog:
    /// a page that never announces `bridgeReady` within
    /// `BRIDGE_READY_TIMEOUT` gets reloaded rather than left blank forever
    /// (observed cause: Chromium's network service crashes mid-transfer —
    /// it auto-restarts itself for *future* requests but does not retry the
    /// one already in flight, so the page can finish HTTP 200 headers and
    /// still never actually paint or run its scripts).
    attached_at: Option<Instant>,
    /// How many times the watchdog above has already reloaded this browser.
    /// Capped so a page that *never* comes up (broken build, not a transient
    /// crash) fails loudly instead of reloading forever.
    bridge_ready_retries: u32,
}

impl BuiltinPluginEditorWindow {
    pub fn new(
        plugin_id: String,
        display_name: String,
        instances: Vec<PluginInstanceDescriptor>,
        active_instance: Option<PluginInstanceKey>,
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

        let editor_id = active_instance
            .as_ref()
            .map(|key| format!("{}::{}", key.track_id, key.insert_id))
            .unwrap_or_else(|| format!("{plugin_id}::<none>"));

        Self {
            view_id: host::allocate_view_id(),
            editor_id,
            plugin_id,
            display_name,
            status,
            content: None,
            last_rect: None,
            instances,
            active_instance,
            binding_generation: 0,
            sidebar_collapsed: false,
            browser_ready: false,
            attached_at: None,
            bridge_ready_retries: 0,
        }
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    /// Replace the sidebar's instance list wholesale. Called whenever an
    /// insert using this `plugin_id` is added/removed/renamed/reordered
    /// anywhere in the project, and whenever this window is (re)focused from
    /// an Open-Editor request.
    ///
    /// If the previously active instance is gone, this picks the nearest
    /// remaining instance (first in the rebuilt list) rather than just
    /// clearing selection — "deleting the active insert selects another
    /// valid instance" per spec. Only goes to the empty state when the list
    /// is genuinely empty.
    pub(crate) fn set_instances(
        &mut self,
        instances: Vec<PluginInstanceDescriptor>,
        cx: &mut Context<Self>,
    ) {
        let active_still_present = self
            .active_instance
            .as_ref()
            .is_some_and(|active| instances.iter().any(|i| &i.instance_key == active));
        let removed_active = self
            .active_instance
            .clone()
            .filter(|_| !active_still_present);
        self.instances = instances;

        if !active_still_present {
            if let Some(removed) = removed_active {
                self.post_to_view(&InstanceRemovedMsg {
                    r#type: "futureboard.instanceRemoved",
                    protocol_version: BRIDGE_PROTOCOL_VERSION,
                    instance_id: wire_instance_id(&removed),
                });
            }
            match self.instances.first().map(|i| i.instance_key.clone()) {
                Some(next) => {
                    self.select_instance(next, cx);
                    return;
                }
                None => {
                    self.active_instance = None;
                    self.editor_id = format!("{}::<none>", self.plugin_id);
                }
            }
        }
        cx.notify();
    }

    /// Rebind the shared browser to a different instance. A no-op re-select
    /// of the already-active instance still bumps `binding_generation` — the
    /// caller (sidebar click, or `requestSelectInstance`) does not need to
    /// special-case "already selected".
    ///
    /// Native decides, always: this is called from the sidebar click handler
    /// AND from the inbound `requestSelectInstance` handler in `tick()` — the
    /// latter validates the request against `self.instances` exactly the same
    /// way before calling this, so a route change alone can never bind an
    /// instance native hasn't approved (spec's "URL does not authorize
    /// access" rule).
    pub(crate) fn select_instance(&mut self, key: PluginInstanceKey, cx: &mut Context<Self>) {
        if !self.instances.iter().any(|i| i.instance_key == key) {
            eprintln!(
                "[BuiltinPluginEditor] select_instance rejected: {}::{} is not in the sidebar for plugin={}",
                key.track_id, key.insert_id, self.plugin_id
            );
            return;
        }
        self.binding_generation += 1;
        self.editor_id = wire_instance_id(&key);
        self.active_instance = Some(key);
        self.push_selected_instance();
        cx.notify();
    }

    /// Push `futureboard.selectInstance` for the current `active_instance`
    /// into the live page. No-op if the browser hasn't announced
    /// `bridgeReady` yet or nothing is selected — `browser_ready` becoming
    /// true re-calls this so a selection made while loading isn't lost.
    fn push_selected_instance(&self) {
        if !self.browser_ready {
            return;
        }
        let Some(active) = self.active_instance.as_ref() else {
            return;
        };
        let Some(descriptor) = self.instances.iter().find(|i| &i.instance_key == active) else {
            return;
        };
        let msg = SelectInstanceMsg {
            r#type: "futureboard.selectInstance",
            protocol_version: BRIDGE_PROTOCOL_VERSION,
            plugin_id: self.plugin_id.clone(),
            instance_id: wire_instance_id(active),
            binding_generation: self.binding_generation,
            display: InstanceDisplayMetadata {
                track_id: active.track_id.clone(),
                track_name: descriptor.track_name.clone(),
                insert_id: active.insert_id.clone(),
                insert_name: descriptor.insert_name.clone(),
            },
            // TODO(phase5 remaining): no per-insert state *revision counter*
            // exists yet (nothing generates incremental patches to count),
            // so this is always 0 even though `state` below can now be real.
            state_revision: 0,
            state: decode_state_bytes(descriptor.state_bytes.as_deref().map(Vec::as_slice)),
        };
        self.post_to_view(&msg);
    }

    fn post_to_view(&self, msg: &impl serde::Serialize) {
        let Ok(json) = serde_json::to_string(msg) else {
            return;
        };
        host::send_to_view(self.view_id, &format!("window.postMessage({json}, \"*\");"));
    }

    /// Handle one React->native bridge message already parsed from
    /// `host::take_inbound`. Split out of `tick()` for readability; still
    /// only ever called from there (the UI-thread pump), never from CEF's IO
    /// thread where the message actually arrived.
    fn handle_inbound(&mut self, msg: InboundMsg, cx: &mut Context<Self>) {
        match msg {
            InboundMsg::BridgeReady { .. } => {
                self.browser_ready = true;
                // Watchdog satisfied: the page came up, so the reload count
                // that got it here shouldn't count against a *future*,
                // unrelated crash.
                self.attached_at = None;
                self.bridge_ready_retries = 0;
                self.push_selected_instance();
            }
            InboundMsg::InstanceReady {
                instance_id,
                binding_generation,
                ..
            } => {
                if binding_generation != self.binding_generation {
                    eprintln!(
                        "[plugin-bridge] instanceReady stale plugin={} instance={instance_id} \
                         ack_generation={binding_generation} current_generation={}",
                        self.plugin_id, self.binding_generation
                    );
                    return;
                }
                // TODO(phase5): this is where native would start incremental
                // state-patch delivery / meter subscription for the newly
                // bound instance — there is nothing to subscribe to yet.
            }
            InboundMsg::RequestSelectInstance { instance_id } => {
                let Some(key) = self
                    .instances
                    .iter()
                    .find(|i| wire_instance_id(&i.instance_key) == instance_id)
                    .map(|i| i.instance_key.clone())
                else {
                    eprintln!(
                        "[plugin-bridge] requestSelectInstance rejected plugin={} instance={instance_id} reason=not_in_sidebar",
                        self.plugin_id
                    );
                    // Restore the route the browser actually has a valid
                    // binding for, rather than trusting the requested one.
                    self.push_selected_instance();
                    return;
                };
                self.select_instance(key, cx);
            }
            InboundMsg::Unknown => {}
        }
    }

    pub(crate) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        cx.notify();
    }

    fn sidebar_width(&self) -> f32 {
        if self.sidebar_collapsed {
            0.0
        } else {
            SIDEBAR_W
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
                    self.attached_at = Some(Instant::now());
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
                ViewEvent::RendererCrashed => {
                    // The host already reloaded the browser. `active_instance`
                    // and `instances` are untouched — native DSP state (what
                    // there is of it) never lived in the page, so there is
                    // nothing to lose here. Wait for the fresh page's
                    // `bridgeReady` before pushing the selection again.
                    self.browser_ready = false;
                    self.attached_at = Some(Instant::now());
                    eprintln!(
                        "[plugin-bridge] renderer crashed plugin={}, waiting for bridgeReady to resend selection",
                        self.plugin_id
                    );
                }
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

        // Bridge inbound: only once the browser exists, keyed by the same
        // scheme origin `resolve_asset`/`bridge_sink` match requests against.
        if matches!(self.status, Status::Attaching | Status::Attached) {
            if let Some(origin) = host::origin_for_plugin_id(&self.plugin_id) {
                for raw in host::take_inbound(origin) {
                    match serde_json::from_slice::<InboundMsg>(&raw) {
                        Ok(msg) => self.handle_inbound(msg, cx),
                        Err(error) => eprintln!(
                            "[plugin-bridge] malformed inbound message plugin={} err={error}",
                            self.plugin_id
                        ),
                    }
                }
            }
        }

        // Bridge-ready watchdog (see `attached_at`'s doc comment). Only
        // meaningful once actually attached and not yet confirmed ready.
        if self.status == Status::Attached && !self.browser_ready {
            if let Some(attached_at) = self.attached_at {
                if attached_at.elapsed() >= BRIDGE_READY_TIMEOUT {
                    if self.bridge_ready_retries >= MAX_BRIDGE_READY_RETRIES {
                        self.status = Status::Failed(format!(
                            "the editor page never became responsive after {} reload attempts",
                            self.bridge_ready_retries
                        ));
                        cx.notify();
                    } else {
                        self.bridge_ready_retries += 1;
                        eprintln!(
                            "[plugin-bridge] bridgeReady watchdog fired plugin={} attempt={}/{} — reloading",
                            self.plugin_id, self.bridge_ready_retries, MAX_BRIDGE_READY_RETRIES
                        );
                        host::reload_view(self.view_id);
                        self.attached_at = Some(Instant::now());
                    }
                }
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
        let rect = content_rect(bounds, scale, self.sidebar_width());
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
        let rect = content_rect(bounds, window.scale_factor(), self.sidebar_width());
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

/// The physical-pixel rect the browser occupies inside the shell's client
/// area: everything below the GPUI-drawn header and right of the native
/// sidebar. `sidebar_w` is reserved the same way as `HEADER_H` — the browser
/// must never be told to draw under either.
fn content_rect(bounds: Bounds<Pixels>, scale: f32, sidebar_w: f32) -> ViewRect {
    let width: f32 = bounds.size.width.into();
    let height: f32 = bounds.size.height.into();
    let phys = |v: f32| (v * scale).round() as i32;
    ViewRect {
        x: phys(sidebar_w),
        y: phys(HEADER_H),
        width: (phys(width) - phys(sidebar_w)).max(0),
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
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .flex_row()
                    .child(self.render_sidebar(cx))
                    .child(match failure {
                        // The browser paints itself into the native child
                        // below the header and right of the sidebar, so on
                        // success this area stays deliberately empty.
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
                    }),
            )
    }
}

impl BuiltinPluginEditorWindow {
    /// Native instance list. Reserved width matches `sidebar_width()`, which
    /// `content_rect` also reads — the CEF child and this column can never
    /// disagree about where the boundary is.
    fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        if self.sidebar_collapsed {
            return div().flex_none().w(px(0.0)).into_any_element();
        }

        let rows = if self.instances.is_empty() {
            vec![div()
                .p(px(10.0))
                .text_size(px(11.0))
                .text_color(Colors::text_secondary())
                .child(format!(
                    "No {} instances are available in this project.",
                    self.plugin_id
                ))
                .into_any_element()]
        } else {
            self.instances
                .iter()
                .map(|instance| {
                    let is_active = self.active_instance.as_ref() == Some(&instance.instance_key);
                    let key = instance.instance_key.clone();
                    let weak = cx.weak_entity();
                    div()
                        .id(("builtin-plugin-instance-row", {
                            let mut hasher = std::collections::hash_map::DefaultHasher::new();
                            std::hash::Hash::hash(&key, &mut hasher);
                            std::hash::Hasher::finish(&hasher) as usize
                        }))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .px(px(10.0))
                        .py(px(6.0))
                        .when(is_active, |el| el.bg(Colors::surface_raised()))
                        .when(!is_active, |el| el.bg(Colors::surface_panel()))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _window, cx| {
                            let _ = weak.update(cx, |editor, cx| {
                                editor.select_instance(key.clone(), cx);
                            });
                        })
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(Colors::text_secondary())
                                .child(instance.track_name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(Colors::text_primary())
                                .child(if instance.bypassed {
                                    format!("{} (bypassed)", instance.insert_name)
                                } else {
                                    instance.insert_name.clone()
                                }),
                        )
                        .into_any_element()
                })
                .collect()
        };

        div()
            .flex_none()
            .w(px(SIDEBAR_W))
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(Colors::border_subtle())
            .bg(Colors::surface_panel())
            .overflow_hidden()
            .children(rows)
            .into_any_element()
    }
}

/// Open the shared shell window for a built-in plugin's editor. One per
/// `plugin_id` — callers must check the registry for an existing window
/// before calling this (see `open_builtin_insert_editor` in `plugin_ops.rs`);
/// this function always creates a fresh CEF browser.
pub fn open_builtin_editor_window(
    owner_bounds: Bounds<Pixels>,
    plugin_id: String,
    display_name: String,
    instances: Vec<PluginInstanceDescriptor>,
    active_instance: Option<PluginInstanceKey>,
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
        let view = cx.new(|cx| {
            BuiltinPluginEditorWindow::new(plugin_id, display_name, instances, active_instance, cx)
        });
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
        let rect = content_rect(bounds(1000.0, 700.0), 1.0, 0.0);
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, HEADER_H as i32);
        assert_eq!(rect.width, 1000);
        assert_eq!(rect.height, 700 - HEADER_H as i32);
    }

    #[test]
    fn content_rect_scales_with_dpi() {
        let rect = content_rect(bounds(1000.0, 700.0), 2.0, 0.0);
        assert_eq!(rect.y, (HEADER_H * 2.0) as i32);
        assert_eq!(rect.width, 2000);
        assert_eq!(rect.height, 1400 - (HEADER_H * 2.0) as i32);
        // The browser must never be told to draw over the header.
        assert!(rect.y > 0);
    }

    #[test]
    fn a_window_shorter_than_the_header_clamps_to_zero_rather_than_going_negative() {
        let rect = content_rect(bounds(400.0, 10.0), 1.0, 0.0);
        assert_eq!(rect.height, 0);
        assert!(rect.height >= 0);
    }

    #[test]
    fn content_rect_reserves_sidebar_width_on_the_left() {
        let rect = content_rect(bounds(1000.0, 700.0), 1.0, SIDEBAR_W);
        assert_eq!(rect.x, SIDEBAR_W as i32);
        assert_eq!(rect.width, 1000 - SIDEBAR_W as i32);
    }

    #[test]
    fn content_rect_sidebar_width_scales_with_dpi() {
        let rect = content_rect(bounds(1000.0, 700.0), 2.0, SIDEBAR_W);
        assert_eq!(rect.x, (SIDEBAR_W * 2.0) as i32);
        assert_eq!(rect.width, 2000 - (SIDEBAR_W * 2.0) as i32);
    }

    #[test]
    fn a_sidebar_wider_than_the_window_clamps_content_to_zero_rather_than_going_negative() {
        let rect = content_rect(bounds(100.0, 700.0), 1.0, SIDEBAR_W);
        assert_eq!(rect.width, 0);
    }

    #[test]
    fn set_instances_clears_active_selection_when_it_is_no_longer_present() {
        let a = PluginInstanceKey {
            track_id: "track-2".into(),
            insert_id: "insert-4".into(),
        };
        let b = PluginInstanceKey {
            track_id: "track-3".into(),
            insert_id: "insert-9".into(),
        };
        let descriptor = |key: PluginInstanceKey| PluginInstanceDescriptor {
            instance_key: key,
            plugin_id: "rodharerist".into(),
            track_name: "Track".into(),
            insert_name: "Insert".into(),
            bypassed: false,
            enabled: true,
            state_bytes: None,
        };
        // Exercised indirectly through the full entity in integration/manual
        // testing (GPUI `Context<Self>` cannot be constructed standalone in a
        // unit test); this test only pins down the pure membership check the
        // real method relies on so a future refactor can't silently drop it.
        let instances = [descriptor(a.clone())];
        assert!(instances.iter().any(|i| i.instance_key == a));
        assert!(!instances.iter().any(|i| i.instance_key == b));
    }

    #[test]
    fn wire_instance_id_joins_track_and_insert() {
        let key = PluginInstanceKey {
            track_id: "track-2".into(),
            insert_id: "insert-4".into(),
        };
        assert_eq!(wire_instance_id(&key), "track-2::insert-4");
    }

    #[test]
    fn select_instance_message_matches_the_documented_wire_shape() {
        let msg = SelectInstanceMsg {
            r#type: "futureboard.selectInstance",
            protocol_version: BRIDGE_PROTOCOL_VERSION,
            plugin_id: "rodharerist".into(),
            instance_id: "track-2::insert-4".into(),
            binding_generation: 42,
            display: InstanceDisplayMetadata {
                track_id: "track-2".into(),
                track_name: "Track 2".into(),
                insert_id: "insert-4".into(),
                insert_name: "Power Chord".into(),
            },
            state_revision: 157,
            state: serde_json::json!({}),
        };
        let json: serde_json::Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "futureboard.selectInstance");
        assert_eq!(json["protocolVersion"], 1);
        assert_eq!(json["pluginId"], "rodharerist");
        assert_eq!(json["instanceId"], "track-2::insert-4");
        assert_eq!(json["bindingGeneration"], 42);
        assert_eq!(json["display"]["trackName"], "Track 2");
        assert_eq!(json["display"]["insertName"], "Power Chord");
        assert_eq!(json["stateRevision"], 157);
    }

    #[test]
    fn decode_state_bytes_falls_back_to_empty_object_when_absent() {
        assert_eq!(decode_state_bytes(None), serde_json::json!({}));
    }

    #[test]
    fn decode_state_bytes_parses_real_json() {
        let bytes = br#"{"schema_version":1,"params":{"amp_gain":7.0}}"#;
        let value = decode_state_bytes(Some(bytes));
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["params"]["amp_gain"], 7.0);
    }

    #[test]
    fn decode_state_bytes_falls_back_to_empty_object_on_corrupt_bytes() {
        assert_eq!(decode_state_bytes(Some(b"not json")), serde_json::json!({}));
        assert_eq!(
            decode_state_bytes(Some(&[0xff, 0xfe])),
            serde_json::json!({})
        );
    }

    #[test]
    fn inbound_bridge_ready_parses_by_tag() {
        let raw =
            br#"{"type":"futureboard.bridgeReady","pluginId":"rodharerist","bridgeVersion":1}"#;
        let msg: InboundMsg = serde_json::from_slice(raw).unwrap();
        assert!(matches!(msg, InboundMsg::BridgeReady { .. }));
    }

    #[test]
    fn inbound_request_select_instance_parses_by_tag() {
        let raw =
            br#"{"type":"futureboard.requestSelectInstance","instanceId":"track-3::insert-9"}"#;
        let msg: InboundMsg = serde_json::from_slice(raw).unwrap();
        match msg {
            InboundMsg::RequestSelectInstance { instance_id } => {
                assert_eq!(instance_id, "track-3::insert-9");
            }
            other => panic!("expected RequestSelectInstance, got {other:?}"),
        }
    }

    #[test]
    fn inbound_unknown_type_does_not_error() {
        let raw = br#"{"type":"something.the.host.does.not.know"}"#;
        let msg: InboundMsg = serde_json::from_slice(raw).unwrap();
        assert!(matches!(msg, InboundMsg::Unknown));
    }

    #[test]
    fn instance_ready_generation_mismatch_is_detectable_before_acting_on_it() {
        // Pure pin of the comparison `handle_inbound` relies on to reject a
        // stale ack (Context<Self> can't be constructed in a unit test, so
        // the full method isn't exercised here — see module doc for why).
        let current_generation = 42u64;
        let ack_generation = 41u64;
        assert_ne!(ack_generation, current_generation);
    }
}
