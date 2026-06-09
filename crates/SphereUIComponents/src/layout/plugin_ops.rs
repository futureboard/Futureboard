use gpui::{Bounds, Context, Window};

use crate::components::native_editor_shell::{shell_defaults, NativeEditorShell};
use crate::components::plugin_manager::open_plugin_manager_window;
use crate::components::plugin_picker::{
    ensure_default_highlight, PickerFilter, PluginInsertKind, PluginPickerState, STUB_PLUGIN_ID,
};
use sphere_plugin_host::{load_au_cache_state, CatalogLoad};

use super::{PluginCatalogStatus, PluginSearchIndex, StudioLayout};

/// Lifecycle state of a native main-owned bridge editor session.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BridgeEditorState {
    /// `PrepareEditorView` sent; awaiting `EditorPreferredSize`.
    Preparing,
    /// Shell resized to preferred size; `ConfirmEditorContentReady` sent.
    AwaitingAttach,
    /// `IPlugView` attached into the native content HWND.
    Attached,
    /// Attach failed / host disconnected.
    Failed(String),
}

/// One open native main-owned plugin editor (external-bridge path). The window
/// is a real Win32 top-level shell (`NativeEditorShell`) owned by the main app;
/// the host process attaches the VST3 view into the shell's content HWND over
/// IPC. No GPUI surface is composited over it, so it actually paints (spec
/// Part 7). NOT `host_detached` — the shell is main-owned.
pub(crate) struct BridgeEditorSession {
    pub(crate) track_id: String,
    pub(crate) instance_id: String,
    pub(crate) display_name: String,
    pub(crate) shell: NativeEditorShell,
    pub(crate) state: BridgeEditorState,
    /// True once the plug-in's preferred size has been applied to the shell.
    pub(crate) preferred_applied: bool,
    /// Last content (client) size pushed to the host as `ResizeEditor`.
    pub(crate) last_content: (i32, i32),
    /// Plugin-host child HWND reported in `EditorAttached` (0 until attached).
    pub(crate) host_hwnd: u64,
}

/// Logical→physical DPI passthrough for `ResizeEditor`. The host sizes the view
/// from the actual child client rect, so this value is a hint only.
const BRIDGE_EDITOR_DPI: u32 = 96;

impl StudioLayout {
    pub(super) fn poll_plugin_bridge_runtime(&mut self, cx: &mut Context<Self>) {
        use crate::components::timeline::timeline_state::{
            PluginRuntimeBackend, PluginRuntimeState,
        };
        use sphere_plugin_host::ipc::HostEvent;
        use sphere_plugin_host::plugin_host_client::ClientEvent;

        let Some(runtime) = self.plugin_bridge_runtime.as_ref().cloned() else {
            return;
        };
        let events = runtime
            .lock()
            .map(|mut runtime| runtime.drain_events())
            .unwrap_or_default();
        if events.is_empty() {
            return;
        }
        let mut changed = false;
        // This poll is the SINGLE drain of the shared runtime queue. Editor-
        // targeted events (EditorAttached / EditorPreferredSize / …) must be
        // forwarded to the owning editor window, or they are lost and the editor
        // stays stuck on "Loading" (spec Part 2/5/6). Collect them while we
        // handle the load-lifecycle events, then dispatch after the loop so we
        // never hold a borrow of `self` across `handle.update`.
        let mut editor_routes: Vec<(String, ClientEvent)> = Vec::new();
        let mut disconnect_all = false;
        for event in events {
            if let Some(instance) =
                crate::components::plugin_editor_window::PluginEditorWindow::editor_event_instance_id(
                    &event,
                )
            {
                editor_routes.push((instance.to_string(), event.clone()));
            } else if matches!(event, ClientEvent::Disconnected) {
                disconnect_all = true;
            }
            match event {
                ClientEvent::Host(HostEvent::PluginLoading { plugin_instance_id }) => {
                    eprintln!("[plugin-bridge] event PluginLoading instance={plugin_instance_id}");
                    let host_pid = runtime.lock().ok().and_then(|r| r.host_pid());
                    changed |= self.timeline.update(cx, |timeline, _cx| {
                        let track_ids: Vec<String> = timeline
                            .state
                            .tracks
                            .iter()
                            .filter(|track| {
                                track
                                    .inserts
                                    .iter()
                                    .any(|slot| slot.id == plugin_instance_id)
                            })
                            .map(|track| track.id.clone())
                            .collect();
                        track_ids.into_iter().any(|track_id| {
                            timeline.state.set_insert_runtime(
                                &track_id,
                                &plugin_instance_id,
                                PluginRuntimeBackend::ExternalBridge,
                                PluginRuntimeState::Loading,
                                host_pid,
                            )
                        })
                    });
                }
                ClientEvent::Host(HostEvent::PluginAlreadyLoaded {
                    plugin_instance_id,
                    name,
                }) => {
                    eprintln!(
                        "[plugin-bridge] event PluginAlreadyLoaded instance={plugin_instance_id} name={name}"
                    );
                }
                ClientEvent::Host(HostEvent::PluginLoaded {
                    plugin_instance_id,
                    name,
                }) => {
                    eprintln!("[plugin-bridge] event PluginLoaded instance={plugin_instance_id} name={name}");
                    let host_pid = runtime.lock().ok().and_then(|r| r.host_pid());
                    changed |= self.timeline.update(cx, |timeline, _cx| {
                        let track_ids: Vec<String> = timeline
                            .state
                            .tracks
                            .iter()
                            .filter(|track| {
                                track
                                    .inserts
                                    .iter()
                                    .any(|slot| slot.id == plugin_instance_id)
                            })
                            .map(|track| track.id.clone())
                            .collect();
                        track_ids.into_iter().any(|track_id| {
                            timeline.state.set_insert_runtime(
                                &track_id,
                                &plugin_instance_id,
                                PluginRuntimeBackend::ExternalBridge,
                                PluginRuntimeState::Ready,
                                host_pid,
                            )
                        })
                    });
                    eprintln!("[plugin-runtime] state Loading -> Ready");
                }
                ClientEvent::Host(HostEvent::PluginLoadFailed {
                    plugin_instance_id,
                    error,
                }) => {
                    eprintln!("[plugin-bridge] event PluginLoadFailed instance={plugin_instance_id} error={error}");
                    let host_pid = runtime.lock().ok().and_then(|r| r.host_pid());
                    changed |= self.timeline.update(cx, |timeline, _cx| {
                        let track_ids: Vec<String> = timeline
                            .state
                            .tracks
                            .iter()
                            .filter(|track| {
                                track
                                    .inserts
                                    .iter()
                                    .any(|slot| slot.id == plugin_instance_id)
                            })
                            .map(|track| track.id.clone())
                            .collect();
                        track_ids.into_iter().any(|track_id| {
                            timeline.state.set_insert_runtime(
                                &track_id,
                                &plugin_instance_id,
                                PluginRuntimeBackend::ExternalBridge,
                                PluginRuntimeState::Failed(error.clone()),
                                host_pid,
                            )
                        })
                    });
                }
                ClientEvent::Disconnected => {
                    eprintln!("[plugin-runtime] external bridge host disconnected");
                }
                ClientEvent::Host(HostEvent::AudioBridgeConfigured {
                    sample_rate,
                    max_block_size,
                    follows_engine,
                }) => {
                    eprintln!(
                        "[plugin-bridge] event AudioBridgeConfigured sample_rate={sample_rate} max_block_size={max_block_size} follows_engine={follows_engine}"
                    );
                }
                ClientEvent::Host(HostEvent::AudioBridgeStatus {
                    block_id,
                    dsp_output,
                    latency_samples,
                }) => {
                    eprintln!(
                        "[plugin-bridge] event AudioBridgeStatus block_id={block_id} dsp_output={dsp_output} latency_samples={latency_samples}"
                    );
                }
                ClientEvent::Host(HostEvent::SharedAudioAttached {
                    attached,
                    name,
                    bytes,
                }) => {
                    eprintln!(
                        "[plugin-bridge] event SharedAudioAttached attached={attached} name={name} bytes={bytes}"
                    );
                }
                ClientEvent::Host(HostEvent::ProcessingPrepared {
                    plugin_instance_id,
                    sample_rate,
                    max_block_size,
                    output_channels,
                }) => {
                    eprintln!(
                        "[plugin-bridge] event ProcessingPrepared instance={plugin_instance_id} sr={sample_rate} block={max_block_size} outputs={output_channels}"
                    );
                    eprintln!("[plugin-runtime] dsp_output=ready");
                }
                _ => {}
            }
        }
        // Forward editor-targeted events to the owning editor window(s).
        for (instance, event) in editor_routes {
            self.dispatch_editor_event(&instance, event, cx);
        }
        if disconnect_all {
            self.broadcast_editor_disconnect(cx);
        }
        if changed {
            cx.notify();
        }
    }

    /// Route a single editor-targeted host event to the native main-owned editor
    /// shell that owns `plugin_instance_id` (IPC uses `plugin_instance_id`,
    /// Part 4). Editor host events only ever target bridge sessions — the legacy
    /// in-process editor path never produces them.
    fn dispatch_editor_event(
        &mut self,
        plugin_instance_id: &str,
        event: sphere_plugin_host::plugin_host_client::ClientEvent,
        cx: &mut Context<Self>,
    ) {
        use sphere_plugin_host::ipc::HostEvent;
        use sphere_plugin_host::plugin_host_client::ClientEvent;

        // Clone the shared-runtime Arc up front so we can send ResizeEditor while
        // holding a `&mut` borrow of the matched session.
        let runtime = self.plugin_bridge_runtime.as_ref().cloned();
        let Some((_, session)) = self
            .bridge_editors
            .iter_mut()
            .find(|((_, id), _)| id == plugin_instance_id)
        else {
            eprintln!(
                "[plugin-bridge] editor event for instance={plugin_instance_id} dropped (no native editor shell)"
            );
            return;
        };

        match event {
            ClientEvent::Host(HostEvent::EditorAttached {
                result,
                preferred_width,
                preferred_height,
                host_hwnd,
                ..
            }) => {
                let was = session.state.clone();
                session.state = BridgeEditorState::Attached;
                session.host_hwnd = host_hwnd;
                session.shell.mark_attached();
                if was != BridgeEditorState::Attached {
                    eprintln!(
                        "[plugin-editor-window] plugin_instance_id={plugin_instance_id} editor_window_id=0x{:x}",
                        session.shell.top_hwnd()
                    );
                    eprintln!("[plugin-editor-window] state {was:?} -> Attached");
                    eprintln!("[plugin-editor-window] loading_overlay_visible=false");
                    eprintln!(
                        "[plugin-editor-window] native_content_region_reserved=true gpui_paints_over_content=false"
                    );
                }
                eprintln!(
                    "[plugin-view][host] EditorAttached instance={plugin_instance_id} \
                     attached_result={result} preferred={preferred_width}x{preferred_height} host_hwnd=0x{host_hwnd:x}"
                );
                if !session.preferred_applied {
                    apply_bridge_preferred(
                        session,
                        runtime.as_ref(),
                        preferred_width,
                        preferred_height,
                    );
                }
                let plugin_path = runtime
                    .as_ref()
                    .and_then(|rt| rt.lock().ok())
                    .and_then(|r| r.loaded_descriptor(plugin_instance_id))
                    .map(|p| p.descriptor.plugin_path)
                    .unwrap_or_else(|| "<unknown>".to_string());
                session.shell.apply_content_layout();
                if host_hwnd != 0 {
                    session.shell.log_black_gap_check(host_hwnd);
                }
                log_bridge_gpu_diagnostics(session, plugin_instance_id, &plugin_path);
                log_bridge_paint_stats(session);
            }
            ClientEvent::Host(HostEvent::EditorPreferredSize { width, height, .. }) => {
                eprintln!(
                    "[plugin-bridge] event EditorPreferredSize instance={plugin_instance_id} width={width} height={height}"
                );
                if session.state == BridgeEditorState::Preparing {
                    if width > 0 && height > 0 {
                        resize_shell_before_attach(session, width, height);
                    } else {
                        eprintln!(
                            "[plugin-editor-window] preferred_size_missing using_shell_default instance={plugin_instance_id}"
                        );
                    }
                    let content_hwnd = session.shell.content_hwnd();
                    let (cw, ch) = session.shell.content_size();
                    if let Some(rt) = runtime.as_ref() {
                        if let Ok(mut r) = rt.lock() {
                            let confirm = r.confirm_editor_content_ready(
                                session.instance_id.clone(),
                                content_hwnd,
                                cw as u32,
                                ch as u32,
                                BRIDGE_EDITOR_DPI,
                            );
                            if let Err(e) = confirm {
                                eprintln!(
                                    "[plugin-bridge] ConfirmEditorContentReady FAILED instance={plugin_instance_id} err={e}"
                                );
                                session
                                    .shell
                                    .set_status(&format!("Editor failed: {e}"), true);
                                session.state = BridgeEditorState::Failed(e.to_string());
                            } else {
                                session.state = BridgeEditorState::AwaitingAttach;
                            }
                        }
                    }
                } else if session.state != BridgeEditorState::Attached {
                    apply_bridge_preferred(session, runtime.as_ref(), width, height);
                }
            }
            ClientEvent::Host(HostEvent::EditorAttachFailed { error, .. }) => {
                eprintln!(
                    "[plugin-view][host] EditorAttachFailed instance={plugin_instance_id} error={error}"
                );
                session
                    .shell
                    .set_status(&format!("Editor failed: {error}"), true);
                session.state = BridgeEditorState::Failed(error);
            }
            ClientEvent::Host(HostEvent::EditorClosed { .. }) => {
                eprintln!("[plugin-view][host] EditorClosed instance={plugin_instance_id}");
            }
            _ => {}
        }
        cx.notify();
    }

    /// Host process disconnected (crash/exit): mark every open native editor
    /// session failed so none waits forever (spec Part 9 — surface, no fallback).
    fn broadcast_editor_disconnect(&mut self, cx: &mut Context<Self>) {
        if self.bridge_editors.is_empty() {
            return;
        }
        for session in self.bridge_editors.values_mut() {
            session
                .shell
                .set_status("Plugin host disconnected (crashed or exited).", true);
            session.state = BridgeEditorState::Failed(
                "Plugin host process disconnected (crashed or exited).".to_string(),
            );
        }
        cx.notify();
    }

    /// Open a native main-owned editor shell for a bridged insert (spec Part 7).
    /// Creates a real Win32 top-level window + content HWND and asks the host to
    /// attach the VST3 view into it. No GPUI surface is composited over the
    /// content, so the plugin actually paints. Re-open focuses the existing shell.
    pub(super) fn open_bridge_editor(
        &mut self,
        track_id: &str,
        instance_id: &str,
        display_name: String,
        owner_hwnd: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        eprintln!("[plugin-editor-window] ownership=main_owned forced=true");
        crate::forensic_trace::log_trace_plugin(track_id, instance_id);
        let key = (track_id.to_string(), instance_id.to_string());
        if let Some(session) = self.bridge_editors.get(&key) {
            session.shell.focus();
            eprintln!("[plugin-editor-window] existing native editor focus instance={instance_id}");
            return;
        }
        let Some(runtime) = self.plugin_bridge_runtime.as_ref().cloned() else {
            eprintln!(
                "[plugin-runtime] external bridge mandatory but no runtime for editor instance={instance_id}"
            );
            return;
        };

        let defaults = shell_defaults();
        let content_w = defaults.default_content_width;
        let content_h = defaults.default_content_height;
        let Some(shell) =
            NativeEditorShell::create(&display_name, content_w, content_h, owner_hwnd)
        else {
            eprintln!("[plugin-editor-window] native shell create FAILED instance={instance_id}");
            return;
        };
        let content_hwnd = shell.content_hwnd();
        let (cw, ch) = shell.content_size();
        eprintln!(
            "[plugin-editor-crossprocess] shell_pid={} content_hwnd=0x{content_hwnd:x} owner=main_process",
            std::process::id()
        );
        crate::components::gpu_editor_diagnostics::log_window_style_audit(
            shell.top_hwnd(),
            content_hwnd,
            0,
        );
        eprintln!(
            "[plugin-bridge] sending PrepareEditorView instance={instance_id} shell_content=0x{content_hwnd:x} size={cw}x{ch}"
        );
        let open_result = runtime
            .lock()
            .map_err(|_| "bridge runtime lock poisoned".to_string())
            .and_then(|mut r| {
                r.prepare_editor_view(instance_id.to_string())
                    .map_err(|e| e.to_string())
            });
        match open_result {
            Ok(()) => {
                self.bridge_editors.insert(
                    key,
                    BridgeEditorSession {
                        track_id: track_id.to_string(),
                        instance_id: instance_id.to_string(),
                        display_name,
                        shell,
                        state: BridgeEditorState::Preparing,
                        preferred_applied: false,
                        last_content: (cw, ch),
                        host_hwnd: 0,
                    },
                );
                if let Some(engine) = self.audio_engine.as_ref() {
                    let _ = engine.set_bridge_editor_active(track_id.to_string(), true);
                }
                cx.notify();
            }
            Err(e) => {
                eprintln!(
                    "[plugin-editor-window] open bridge editor FAILED instance={instance_id} err={e}"
                );
            }
        }
    }

    /// Per-tick driver for native editor shells: honor OS close requests and
    /// forward window resizes to the host as `ResizeEditor` (spec Part 4/8). The
    /// content child is resized synchronously in the shell `WndProc`; this only
    /// pushes the matching `onSize` to the plugin.
    pub(super) fn drive_bridge_editors(&mut self, cx: &mut Context<Self>) {
        if self.bridge_editors.is_empty() {
            return;
        }
        let runtime = self.plugin_bridge_runtime.as_ref().cloned();
        let mut to_close: Vec<(String, String)> = Vec::new();
        let mut changed = false;
        for (key, session) in self.bridge_editors.iter_mut() {
            let poll = session.shell.poll();
            if poll.close_requested {
                eprintln!(
                    "[plugin-editor-window] user close requested instance={}",
                    session.instance_id
                );
                to_close.push(key.clone());
                continue;
            }
            if let Some((w, h)) = poll.resized {
                if w > 0 && h > 0 && (w, h) != session.last_content {
                    session.last_content = (w, h);
                    if session.state == BridgeEditorState::Attached {
                        if let Some(rt) = runtime.as_ref() {
                            if let Ok(mut r) = rt.lock() {
                                r.resize_editor(
                                    session.instance_id.clone(),
                                    w as u32,
                                    h as u32,
                                    BRIDGE_EDITOR_DPI,
                                );
                            }
                        }
                        if session.host_hwnd != 0 {
                            session.shell.log_black_gap_check(session.host_hwnd);
                        }
                    }
                    session.shell.ensure_visible_zorder();
                    eprintln!(
                        "[plugin-bridge] ResizeEditor instance={} width={w} height={h}",
                        session.instance_id
                    );
                    changed = true;
                }
            }
        }
        for key in to_close {
            self.close_bridge_editor(&key.0, &key.1);
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }

    /// Close a native editor session: send `CloseEditor` to the host (view
    /// `removed()`), then drop the session so the shell window is destroyed.
    /// Only called on genuine close (user / replace / track-delete / shutdown).
    pub(super) fn close_bridge_editor(&mut self, track_id: &str, instance_id: &str) {
        let key = (track_id.to_string(), instance_id.to_string());
        if let Some(session) = self.bridge_editors.remove(&key) {
            if let Some(runtime) = self.plugin_bridge_runtime.as_ref() {
                if let Ok(mut r) = runtime.lock() {
                    r.close_editor(session.instance_id.clone());
                }
            }
            if let Some(engine) = self.audio_engine.as_ref() {
                let _ = engine.set_bridge_editor_active(track_id.to_string(), false);
            }
            eprintln!(
                "[plugin-editor-window] close native editor instance={instance_id} (CloseEditor sent, shell destroyed)"
            );
        }
    }

    /// Lazily populated cache of registered audio plugins. First call
    /// runs `PluginRegistry::scan(None)` synchronously — the SQLite
    /// cache backing the registry makes subsequent scans fast. The UI
    /// thread blocks here on purpose; the audio thread is untouched.
    /// `None` return = registry has zero insert-capable plugins.
    /// Open the GPUI-hosted native editor window for an insert slot (Phase 4).
    /// GPUI owns a borderless shell; the C++ backend embeds the VST3 IPlugView
    /// in a native child region under it. If already open, this is a no-op (the
    /// window stays up). UI thread only; bad plugin → the editor window shows a
    /// fallback panel, never a crash.
    pub(super) fn open_insert_editor(
        &mut self,
        track_id: &str,
        insert_index: usize,
        plugin_instance_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::components::timeline::timeline_state::InsertPluginFormat;
        let debug = std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some();

        let resolved = {
            let timeline = self.timeline.read(cx);
            timeline.state.find_track(track_id).and_then(|track| {
                track.inserts.get(insert_index).map(|slot| {
                    let insert_found = slot.id == plugin_instance_id;
                    (
                        insert_found,
                        slot.id.clone(),
                        slot.plugin_id.clone(),
                        slot.plugin_path
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned()),
                        slot.plugin_format,
                        slot.display_name.clone(),
                    )
                })
            })
        };
        let Some((
            insert_found,
            resolved_plugin_instance_id,
            plugin_id,
            plugin_path,
            plugin_format,
            display_name,
        )) = resolved
        else {
            eprintln!(
                "[plugin-view] open_plugin_editor requested_track_id={} requested_insert_index={} resolved_plugin_instance_id=<none> insert_found=false",
                track_id, insert_index
            );
            return;
        };

        eprintln!(
            "[plugin-view] open_plugin_editor requested_track_id={} requested_insert_index={} resolved_plugin_instance_id={} insert_found={}",
            track_id, insert_index, resolved_plugin_instance_id, insert_found
        );

        if !insert_found {
            return;
        }

        let insert_id = resolved_plugin_instance_id.as_str();
        let key = (track_id.to_string(), resolved_plugin_instance_id.clone());

        // One editor window per insert. If a live editor already exists for this
        // slot, focus/raise it instead of opening (or instantiating) a second
        // one. Only drop the handle when its window is actually gone.
        if let Some(handle) = self.open_plugin_editors.get(&key) {
            if handle
                .update(cx, |_, window, _| {
                    window.activate_window();
                })
                .is_ok()
            {
                if debug {
                    eprintln!(
                        "[plugin-view] existing editor found track={track_id} slot={insert_id} \
                         → focus (no new instance)"
                    );
                }
                return;
            }
            if debug {
                eprintln!("[plugin-view] stale editor handle track={track_id} slot={insert_id} → recreating");
            }
            self.open_plugin_editors.remove(&key);
        }

        let path = plugin_path.filter(|p| !p.trim().is_empty());
        let editable = plugin_format == Some(InsertPluginFormat::Vst3)
            && path.is_some()
            && plugin_id.is_some();
        if !editable {
            if debug {
                eprintln!(
                    "[plugin-view] not editable track={track_id} slot={insert_id} fmt={plugin_format:?}"
                );
            }
            return;
        }

        if super::plugin_bridge_runtime::bridge_enabled() {
            // External-bridge path: the editor lives in a real native main-owned
            // Win32 shell (no GPUI flip swap chain over the content), so the
            // host's IPlugView child actually paints (spec Part 7). The legacy
            // GPUI-window path below is reachable only under
            // FUTUREBOARD_PLUGIN_LEGACY_IN_PROCESS.
            let owner_hwnd = studio_native_hwnd(window);
            self.open_bridge_editor(track_id, insert_id, display_name, owner_hwnd, cx);
            return;
        }

        // The editor attaches to the EXISTING runtime VST3 instance for this
        // insert — never a new component/controller. Look it up from the engine;
        // if the insert has no ready native processor, there is nothing to edit.
        let Some(engine) = self.audio_engine.as_ref() else {
            if debug {
                eprintln!("[plugin-view] no audio engine track={track_id} slot={insert_id}");
            }
            return;
        };
        let Some(processor) = engine.insert_processor(track_id, insert_id) else {
            if debug {
                eprintln!(
                    "[plugin-view] no ready runtime VST3 instance track={track_id} slot={insert_id} \
                     (insert not loaded / not native)"
                );
            }
            return;
        };

        let owner_bounds = window.bounds();
        match crate::components::plugin_editor_window::open_plugin_editor_window(
            owner_bounds,
            track_id.to_string(),
            insert_id.to_string(),
            display_name,
            Some(processor),
            None,
            cx,
        ) {
            Ok(handle) => {
                self.open_plugin_editors.insert(key, handle);
                if debug {
                    eprintln!("[plugin-view] open track={track_id} slot={insert_id}");
                }
            }
            Err(err) => {
                if debug {
                    eprintln!(
                        "[plugin-view] open FAILED track={track_id} slot={insert_id} err={err}"
                    );
                }
            }
        }
    }

    /// Close the editor window for a slot if one is open. Idempotent. Removing
    /// the GPUI window drops the entity, which detaches the native view.
    pub(super) fn close_insert_editor(
        &mut self,
        track_id: &str,
        insert_id: &str,
        cx: &mut Context<Self>,
    ) {
        // Native main-owned bridge editor (default path).
        self.close_bridge_editor(track_id, insert_id);
        // Legacy GPUI-window editor (FUTUREBOARD_PLUGIN_LEGACY_IN_PROCESS only).
        let key = (track_id.to_string(), insert_id.to_string());
        if let Some(handle) = self.open_plugin_editors.remove(&key) {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
            eprintln!("[PluginEditorClose] plugin={insert_id} removed_called=true");
            if std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some() {
                eprintln!("[plugin-view] close track={track_id} slot={insert_id}");
            }
        }
    }

    pub(super) fn unload_bridge_plugin(&mut self, insert_id: &str) {
        if let Some(runtime) = self.plugin_bridge_runtime.as_ref() {
            if let Ok(mut runtime) = runtime.lock() {
                runtime.unload_plugin(insert_id.to_string());
            }
        }
    }

    /// Close any open plugin editor whose owning track or insert slot no longer
    /// exists in the project model. This is the catch-all that backstops every
    /// removal path — the track-header delete button mutates the `Timeline`
    /// entity directly (it cannot reach `StudioLayout`'s editor registry), and
    /// undo/redo or programmatic edits can also drop a track/insert without
    /// going through `cleanup_track_plugins_before_delete`. Without this, a
    /// deleted track leaves an orphan editor window holding a live
    /// `Vst3RuntimeProcessor` clone, which keeps the C++ VST3 instance from ever
    /// being destroyed (Part 11). Cheap: only inspects state when an editor is
    /// open, and only closes when its backing slot is genuinely gone.
    pub(super) fn reconcile_open_plugin_editors(&mut self, cx: &mut Context<Self>) {
        if self.open_plugin_editors.is_empty() && self.bridge_editors.is_empty() {
            return;
        }
        let stale: Vec<(String, String)> = {
            let state = &self.timeline.read(cx).state;
            let is_stale =
                |(track_id, insert_id): &&(String, String)| match state.find_track(track_id) {
                    None => true,
                    Some(track) => !track.inserts.iter().any(|insert| &insert.id == insert_id),
                };
            self.open_plugin_editors
                .keys()
                .filter(is_stale)
                .chain(self.bridge_editors.keys().filter(is_stale))
                .cloned()
                .collect()
        };
        for (track_id, insert_id) in stale {
            eprintln!(
                "[PluginUnload] track_id={track_id} insert_id={insert_id} action=close_editor reason=stale_reference"
            );
            self.close_insert_editor(&track_id, &insert_id, cx);
        }
    }

    /// Close every open plugin editor and release native embed sessions before
    /// application exit (avoids HWND/VST3 teardown during TLS destruction).
    pub(super) fn shutdown_plugin_editors(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<(String, String)> = self
            .open_plugin_editors
            .keys()
            .chain(self.bridge_editors.keys())
            .cloned()
            .collect();
        for (track_id, insert_id) in keys {
            self.close_insert_editor(&track_id, &insert_id, cx);
        }
        sphere_plugin_host::native_editor::detach_all_embedded_editors();
    }

    /// Kick off a background SQLite load of the plug-in catalog. The picker
    /// opens instantly with a skeleton; this task replaces the skeleton once
    /// the catalog is read. Re-entrant: a second call while a load is in
    /// flight is a no-op.
    ///
    /// **Never** invokes the VST3/CLAP scanner; **never** touches plug-in
    /// binaries. The picker's open path must stay UI-only.
    pub(super) fn arm_catalog_load(&mut self, cx: &mut Context<Self>) {
        // Already loaded and not stale → nothing to do.
        if matches!(self.plugin_catalog_status, PluginCatalogStatus::Ready)
            && self.available_plugins.is_some()
        {
            return;
        }
        if matches!(self.plugin_catalog_status, PluginCatalogStatus::Loading)
            && self.available_plugins.is_none()
        {
            // Spawn-in-progress (initial boot path also fires this).
        } else {
            self.plugin_catalog_status = PluginCatalogStatus::Loading;
        }

        let debug = std::env::var_os("FUTUREBOARD_PLUGIN_PICKER_DEBUG").is_some()
            || std::env::var_os("FUTUREBOARD_PLUGIN_DB_DEBUG").is_some();
        let shell_started = std::time::Instant::now();

        cx.spawn(async move |this, cx| {
            let load = cx
                .background_executor()
                .spawn(async { sphere_plugin_host::PluginRegistry::load_catalog() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if crate::shutdown::ShutdownState::global().is_shutting_down() {
                    return;
                }
                match load {
                    CatalogLoad::Loaded { catalog, sqlite_ms } => {
                        let count = catalog.plugins.len();
                        let plugins: Vec<sphere_plugin_host::RegistryPlugin> = catalog
                            .plugins
                            .iter()
                            .map(|e| e.to_registry_plugin())
                            .collect();
                        this.available_plugins = Some(plugins.clone());
                        this.plugin_search_index = Some(PluginSearchIndex::from_plugins(plugins));
                        this.plugin_picker_au_error = load_au_cache_state().last_error;
                        this.plugin_cache_present = true;
                        this.plugin_catalog_status = PluginCatalogStatus::Ready;
                        if debug {
                            eprintln!(
                                "[plugin-db] loaded rows={count} sqlite_ms={sqlite_ms} path={} total_ms={}",
                                catalog.source_path.display(),
                                shell_started.elapsed().as_millis(),
                            );
                        }
                    }
                    CatalogLoad::MissingDatabase { path } => {
                        this.available_plugins = Some(Vec::new());
                        this.plugin_cache_present = false;
                        this.plugin_catalog_status = PluginCatalogStatus::MissingDatabase;
                        if debug {
                            eprintln!(
                                "[plugin-db] path={} exists=false",
                                path.display()
                            );
                        }
                    }
                    CatalogLoad::Error { path, message } => {
                        this.available_plugins = Some(Vec::new());
                        this.plugin_cache_present = path.exists();
                        this.plugin_catalog_status =
                            PluginCatalogStatus::Error(message.clone());
                        if debug {
                            eprintln!(
                                "[plugin-db] error path={} message={}",
                                path.display(),
                                message
                            );
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the Phase 2b insert picker for `track_id`. Loads from cached
    /// `.pst` index only (no VST3/CLAP scan, no plug-in binary read) so the
    /// overlay opens instantly even with 1000+ plug-ins. No insert slot is
    /// created until the user picks a plugin.
    pub(super) fn open_insert_picker(
        &mut self,
        track_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_insert_picker_for(track_id, None, PluginInsertKind::Effect, window, cx);
    }

    pub(super) fn open_insert_picker_for(
        &mut self,
        track_id: &str,
        slot_index: Option<usize>,
        desired_kind: PluginInsertKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::components::timeline::timeline_state::TrackType;

        let debug = std::env::var_os("FUTUREBOARD_PLUGIN_PICKER_DEBUG").is_some();
        let started = std::time::Instant::now();
        let track_info = self
            .timeline
            .read(cx)
            .state
            .find_track(track_id)
            .map(|track| (track.name.clone(), track.track_type, track.inserts.len()));
        let (track_name, track_type, next_slot) =
            track_info.unwrap_or((track_id.to_string(), TrackType::Audio, 0));
        let target_slot = slot_index.unwrap_or_else(|| {
            if track_type == TrackType::Instrument && desired_kind == PluginInsertKind::Effect {
                next_slot.max(1)
            } else {
                next_slot
            }
        });
        let filter = match desired_kind {
            PluginInsertKind::Instrument => PickerFilter::Instruments,
            PluginInsertKind::Effect => PickerFilter::Effects,
        };
        self.plugin_picker = PluginPickerState::open_for_with_filter(
            track_id,
            &track_name,
            track_type,
            target_slot,
            self.plugin_picker_prefs.show_details,
            filter,
            desired_kind,
        );
        self.plugin_picker_search_input.set_value("");
        self.plugin_picker.query = String::new();
        self.plugin_picker_search_input
            .focus_handle
            .focus(window, cx);
        if let Some(index) = self.plugin_search_index.as_ref() {
            ensure_default_highlight(&mut self.plugin_picker, index, &self.plugin_picker_prefs);
        }
        // Kick off (or rejoin) the background SQLite load. Picker shell is
        // visible immediately; skeleton rows fill in until the catalog lands.
        if self.available_plugins.is_none()
            || !matches!(self.plugin_catalog_status, PluginCatalogStatus::Ready)
        {
            self.arm_catalog_load(cx);
        }
        if debug {
            let state_label = match &self.plugin_catalog_status {
                PluginCatalogStatus::Loading => "LoadingCatalog",
                PluginCatalogStatus::Ready => "Ready",
                PluginCatalogStatus::MissingDatabase => "MissingDatabase",
                PluginCatalogStatus::Error(_) => "Error",
            };
            eprintln!(
                "[plugin-picker] opened state={state_label} shell_ms={}",
                started.elapsed().as_millis()
            );
        }
        cx.notify();
    }

    /// Apply a picked plugin: append an insert slot to the picker's target
    /// track and bind the chosen descriptor. `plugin_id` is a
    /// `RegistryPlugin.id` or [`STUB_PLUGIN_ID`]. Closes the picker. No audio
    /// thread interaction — the next project sync carries the descriptor down.
    pub(super) fn apply_picked_insert(
        &mut self,
        plugin_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<(String, usize, String)> {
        use crate::components::plugin_picker::validate_insert;
        use crate::components::timeline::timeline_state::InsertPluginFormat;
        use sphere_plugin_host::PluginFormat as RegFmt;

        let track_id = self.plugin_picker.insert_target.track_id.clone();
        let target_slot_index = self.plugin_picker.insert_target.next_slot_index;
        let desired_kind = self.plugin_picker.insert_target.desired_kind;
        if track_id.is_empty() {
            self.plugin_picker = PluginPickerState::closed();
            cx.notify();
            return None;
        }

        if plugin_id != STUB_PLUGIN_ID {
            if let Some(plugins) = self.available_plugins.as_ref() {
                if let Some(reg) = plugins.iter().find(|p| p.id == plugin_id) {
                    if validate_insert(reg, &self.plugin_picker.insert_target)
                        != crate::components::plugin_picker::InsertValidation::Ok
                    {
                        cx.notify();
                        return None;
                    }
                }
            }
        }

        let descriptor = if plugin_id == STUB_PLUGIN_ID {
            None
        } else {
            self.available_plugins
                .as_ref()
                .and_then(|plugins| plugins.iter().find(|p| p.id == plugin_id))
                .map(|reg| {
                    let format = match reg.format {
                        RegFmt::Vst3 => InsertPluginFormat::Vst3,
                        RegFmt::Clap => InsertPluginFormat::Clap,
                        RegFmt::Au => InsertPluginFormat::Au,
                        RegFmt::Lv2 => InsertPluginFormat::Lv2,
                        _ => InsertPluginFormat::Unknown,
                    };
                    let id = reg.class_id.clone().unwrap_or_else(|| reg.id.clone());
                    (id, Some(reg.path.clone()), format, reg.name.clone())
                })
        };
        let (plugin_id_out, plugin_path, plugin_format, display_name) =
            descriptor.unwrap_or_else(|| {
                (
                    STUB_PLUGIN_ID.to_string(),
                    None,
                    InsertPluginFormat::Vst3,
                    "Stub Effect".to_string(),
                )
            });

        let new_slot_id = self.timeline.update(cx, |timeline, _cx| {
            timeline
                .state
                .ensure_insert_slot_at(&track_id, target_slot_index)
        });
        let mut opened_slot = None;
        if let Some(slot_id) = new_slot_id {
            // Replacing the plugin in a slot that already has an editor open: the
            // editor holds a clone of the OLD instance's processor. Close it
            // before rebinding so the old C++ instance is released and we don't
            // orphan a window pointing at a disconnected processor. No-op when
            // the slot is freshly created (no editor open yet).
            self.close_insert_editor(&track_id, &slot_id, cx);
            let log_display_name = display_name.clone();
            let bridge_class_id = plugin_id_out.clone();
            self.timeline.update(cx, |timeline, _cx| {
                timeline.state.set_insert_plugin(
                    &track_id,
                    &slot_id,
                    plugin_id_out,
                    plugin_path,
                    plugin_format,
                    display_name,
                );
            });
            let bridge_enabled = super::plugin_bridge_runtime::bridge_enabled()
                && plugin_format == InsertPluginFormat::Vst3;
            if bridge_enabled {
                use crate::components::timeline::timeline_state::{
                    PluginRuntimeBackend, PluginRuntimeState,
                };
                eprintln!("[plugin-runtime] backend=external_bridge reason=forced_default");
                let path = self
                    .timeline
                    .read(cx)
                    .state
                    .find_track(&track_id)
                    .and_then(|track| track.inserts.iter().find(|slot| slot.id == slot_id))
                    .and_then(|slot| slot.plugin_path.as_ref())
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let sample_rate = self.current_audio_sample_rate();
                let max_block_size = self
                    .audio_engine
                    .as_ref()
                    .map(|engine| engine.config().buffer_size)
                    .unwrap_or(256);
                let descriptor = super::plugin_bridge_runtime::BridgePluginDescriptor {
                    track_id: track_id.clone(),
                    insert_id: slot_id.clone(),
                    plugin_path: path.clone(),
                    class_id: bridge_class_id.clone(),
                    display_name: log_display_name.clone(),
                };
                match super::plugin_bridge_runtime::PluginBridgeRuntime::ensure_shared(
                    &mut self.plugin_bridge_runtime,
                ) {
                    Ok(runtime) => {
                        let host_pid = runtime.lock().ok().and_then(|r| r.host_pid());
                        let _ = self.timeline.update(cx, |timeline, _cx| {
                            timeline.state.set_insert_runtime(
                                &track_id,
                                &slot_id,
                                PluginRuntimeBackend::ExternalBridge,
                                PluginRuntimeState::Loading,
                                host_pid,
                            )
                        });
                        // Stage 3b: after the shared audio region is established
                        // (inside send_load_plugin), install its realtime sink on
                        // the audio engine so plugin DSP output mixes into the
                        // master (gated by FUTUREBOARD_PLUGIN_BRIDGE_AUDIO).
                        let mut bridge_sink = None;
                        match runtime.lock() {
                            Ok(mut runtime) => {
                                if let Err(error) = runtime.send_load_plugin(
                                    descriptor,
                                    sample_rate,
                                    max_block_size,
                                ) {
                                    eprintln!("[plugin-runtime] external bridge LoadPlugin failed: {error}");
                                    let _ = self.timeline.update(cx, |timeline, _cx| {
                                        timeline.state.set_insert_runtime(
                                            &track_id,
                                            &slot_id,
                                            PluginRuntimeBackend::ExternalBridge,
                                            PluginRuntimeState::Failed(error.to_string()),
                                            host_pid,
                                        )
                                    });
                                } else {
                                    bridge_sink = runtime.audio_sink();
                                }
                            }
                            Err(_) => {
                                eprintln!("[plugin-runtime] external bridge runtime lock poisoned");
                            }
                        }
                        crate::forensic_trace::log_trace_plugin(&track_id, &slot_id);
                        let timeline_state = self.timeline.read(cx).state.clone();
                        crate::forensic_trace::log_plugin_main_registry(&timeline_state);
                        #[cfg(feature = "plugin-host-bin")]
                        sphere_plugin_host::plugin_host_preview::PluginHostPreviewEngine::log_unified_runtime(
                            &track_id,
                            &slot_id,
                            &slot_id,
                        );
                        if let (Some(sink), Some(engine)) =
                            (bridge_sink, self.audio_engine.as_ref())
                        {
                            match engine.set_plugin_bridge_sink(track_id.clone(), Some(sink)) {
                                Ok(()) => eprintln!(
                                    "[plugin-bridge] engine plugin_bridge_sink installed track={track_id}"
                                ),
                                Err(error) => eprintln!(
                                    "[plugin-bridge] engine set_plugin_bridge_sink failed: {error}"
                                ),
                            }
                        }
                        self.engine_project_dirty = true;
                        self.schedule_audio_project_sync(cx, true, "bridge_plugin_loaded");
                        self.mark_dirty();
                    }
                    Err(error) => {
                        eprintln!(
                            "[plugin-runtime] refusing in-process fallback while bridge is enabled"
                        );
                        let _ = self.timeline.update(cx, |timeline, _cx| {
                            timeline.state.set_insert_runtime(
                                &track_id,
                                &slot_id,
                                PluginRuntimeBackend::ExternalBridge,
                                PluginRuntimeState::Failed(error.to_string()),
                                None,
                            )
                        });
                    }
                }
            } else {
                eprintln!("[plugin-runtime] backend=in_process reason=FUTUREBOARD_PLUGIN_LEGACY_IN_PROCESS=1");
                eprintln!("[plugin-runtime] WARNING using legacy in-process plugin runtime");
                eprintln!("[plugin-runtime] legacy path may hang GPU/OpenGL/JUCE plugin editors");
                self.mark_dirty();
                self.engine_project_dirty = true;
            }
            if std::env::var_os("FUTUREBOARD_INSPECTOR_DEBUG").is_some()
                || std::env::var_os("FUTUREBOARD_PLUGIN_INSERT_DEBUG").is_some()
            {
                let kind = match desired_kind {
                    PluginInsertKind::Instrument => "Instrument",
                    PluginInsertKind::Effect => "Effect",
                };
                eprintln!(
                    "[inspector] insert apply track={} slot={} kind={} plugin={}",
                    track_id, target_slot_index, kind, log_display_name
                );
            }
            if plugin_id != STUB_PLUGIN_ID {
                self.plugin_picker_prefs.record_recent(plugin_id);
            }
            opened_slot = Some((track_id.clone(), target_slot_index, slot_id));
        }
        self.plugin_picker = PluginPickerState::closed();
        cx.notify();
        opened_slot
    }

    pub(super) fn open_plugin_manager_external_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.plugin_manager_window.clone() {
            if handle
                .update(cx, |_pm, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.plugin_manager_window = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.open_popover = None;
        self.text_context_menu = None;

        let owner_bounds = crate::window_position::resolve_owner_bounds_with_preferred(
            owner_bounds,
            self.studio_window_bounds(cx),
            cx,
        );

        match open_plugin_manager_window(owner_bounds, cx) {
            Ok(handle) => self.plugin_manager_window = Some(handle),
            Err(err) => eprintln!("[plugin-manager] failed to open window: {err}"),
        }
    }
}

/// Resize shell/content to preferred size before attach (no `ResizeEditor` yet).
fn resize_shell_before_attach(session: &mut BridgeEditorSession, width: u32, height: u32) {
    let (req_w, req_h) = (width as i32, height as i32);
    if req_w <= 0 || req_h <= 0 {
        eprintln!(
            "[plugin-editor-window] preferred_size_invalid reason=non_positive ({req_w}x{req_h})"
        );
        return;
    }
    eprintln!(
        "[plugin-editor-window] pre_attach_resize requested_content={req_w}x{req_h} instance={}",
        session.instance_id
    );
    let (cw, ch, clamped) = session.shell.clamp_content_to_work_area(req_w, req_h);
    if clamped {
        eprintln!("[plugin-editor-window] preferred_size_clamped=true");
    }
    let recenter = !session.shell.has_user_moved();
    session.shell.resize_to_content(cw, ch, recenter);
    let (final_cw, final_ch) = session.shell.apply_content_layout();
    session.last_content = (final_cw, final_ch);
    session.preferred_applied = true;
    session.shell.ensure_visible_zorder();
    eprintln!(
        "[plugin-editor-window] pre_attach_resize content={final_cw}x{final_ch} instance={}",
        session.instance_id
    );
}

fn log_bridge_gpu_diagnostics(
    session: &BridgeEditorSession,
    plugin_instance_id: &str,
    plugin_path: &str,
) {
    let stats = session.shell.paint_stats();
    crate::components::gpu_editor_diagnostics::log_window_style_audit(
        session.shell.top_hwnd(),
        session.shell.content_hwnd(),
        session.host_hwnd,
    );
    crate::components::gpu_editor_diagnostics::log_gpu_editor_diagnostics(
        plugin_instance_id,
        plugin_path,
        session.shell.top_hwnd(),
        session.shell.content_hwnd(),
        session.host_hwnd,
        stats.content_paint_count,
        stats.content_erase_count,
        stats.size_count,
    );
}

/// Apply the plug-in's preferred size to a native editor shell exactly once:
/// validate, clamp to monitor work area, resize shell + content HWND, optionally
/// recenter if the user has not moved the window, and push `ResizeEditor` (spec
/// Part 3–6). No-op once already applied so later hints don't fight user resize.
fn apply_bridge_preferred(
    session: &mut BridgeEditorSession,
    runtime: Option<&super::plugin_bridge_runtime::SharedPluginBridgeRuntime>,
    width: u32,
    height: u32,
) {
    if session.preferred_applied {
        return;
    }
    session.preferred_applied = true;

    let (req_w, req_h) = (width as i32, height as i32);
    if req_w <= 0 || req_h <= 0 {
        eprintln!("[plugin-editor-window] preferred_size_valid=false");
        eprintln!(
            "[plugin-editor-window] preferred_size_invalid reason=non_positive ({req_w}x{req_h})"
        );
        return;
    }

    eprintln!("[plugin-editor-window] preferred_size_valid=true");
    eprintln!(
        "[plugin-editor-window] auto_size requested_content={req_w}x{req_h} instance={}",
        session.instance_id
    );

    let (cw, ch, clamped) = session.shell.clamp_content_to_work_area(req_w, req_h);
    if clamped {
        eprintln!("[plugin-editor-window] preferred_size_clamped=true");
    }
    eprintln!(
        "[plugin-editor-window] auto_size clamped_content={cw}x{ch} instance={}",
        session.instance_id
    );

    let recenter = !session.shell.has_user_moved();
    session.shell.resize_to_content(cw, ch, recenter);
    let (final_cw, final_ch) = session.shell.content_size();
    let (shell_w, shell_h) = session.shell.shell_outer_size();
    session.last_content = (final_cw, final_ch);
    if let Some(rt) = runtime {
        if let Ok(mut r) = rt.lock() {
            r.resize_editor(
                session.instance_id.clone(),
                final_cw as u32,
                final_ch as u32,
                BRIDGE_EDITOR_DPI,
            );
        }
    }
    session.shell.ensure_visible_zorder();
    eprintln!(
        "[plugin-editor-window] resize shell={shell_w}x{shell_h} content={final_cw}x{final_ch} instance={}",
        session.instance_id
    );
    eprintln!(
        "[plugin-bridge] sending ResizeEditor instance={} width={final_cw} height={final_ch}",
        session.instance_id
    );
}

#[cfg(target_os = "windows")]
fn studio_native_hwnd(window: &Window) -> Option<u64> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(w) => Some(w.hwnd.get() as u64),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
fn studio_native_hwnd(_window: &Window) -> Option<u64> {
    None
}

/// Emit paint-instrumentation counters for a native editor shell (spec Part 6).
fn log_bridge_paint_stats(session: &BridgeEditorSession) {
    let stats = session.shell.paint_stats();
    eprintln!(
        "[plugin-editor-paint] instance={} content_paint_count={} content_erase_count={} \
         shell_paint_count={} size_count={}",
        session.instance_id,
        stats.content_paint_count,
        stats.content_erase_count,
        stats.shell_paint_count,
        stats.size_count
    );
}
