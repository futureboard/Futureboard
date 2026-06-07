//! `FutureboardPluginHost-x64.exe` — the separated VST3 plugin/editor host
//! process (IPC *server*).
//!
//! VST3 editor hosting follows public.sdk/samples/vst-hosting/editorhost
//! lifecycle: the host owns the COM STA thread and the editor message pump, and
//! drives `createView`/`attached`/`onSize`/`removed` via the proven C++ backend
//! (`sphere_plugin_host::native_editor`). What is new here is *where* it runs:
//! out-of-process, so a crashing plugin editor cannot take down the GPUI main
//! app.
//!
//! In `main_owned_window` mode (Slice 1 default) the **visible editor window is
//! owned by the main app** — this process only receives an HWND over IPC and
//! attaches the VST3 view to it. The host therefore never creates a top-level
//! editor window; it only pumps messages so the attached `IPlugView` repaints.
//!
//! Protocol: [`HostCommand`] frames arrive on **stdin**, [`HostEvent`] frames
//! are written to **stdout**, human logs go to **stderr** behind
//! `FUTUREBOARD_PLUGIN_VIEW_DEBUG`. See [`sphere_plugin_host::ipc`].

use std::collections::HashMap;
use std::io::{self, BufReader};
use std::time::Duration;

use sphere_plugin_host::ipc::{self, HostCommand, HostEvent, PROTOCOL_VERSION};
use sphere_plugin_host::native_editor::{
    self, EmbedRegion,
};

fn debug_enabled() -> bool {
    std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some()
}

macro_rules! hlog {
    ($($arg:tt)*) => {{
        if debug_enabled() {
            eprintln!($($arg)*);
        }
    }};
}

fn main() {
    let selftest = std::env::args().any(|a| a == "--selftest");

    platform::com_init();
    let pid = std::process::id();
    let thread_id = platform::current_thread_id();
    hlog!("[PluginHostEditor] start pid={pid} thread_id={thread_id} selftest={selftest}");

    if selftest {
        let code = run_selftest();
        platform::com_uninit();
        std::process::exit(code);
    }

    let mut out = io::stdout();
    let _ = ipc::write_frame(
        &mut out,
        &HostEvent::Ready {
            protocol_version: PROTOCOL_VERSION,
            pid,
        },
    );

    run_ipc_loop(out);
    platform::com_uninit();
}

/// Editor handles keyed by `plugin_instance_id` — the in-process
/// `PluginEditorRegistry` role, living inside the host process.
type Registry = HashMap<String, u64>;

fn run_ipc_loop(mut out: io::Stdout) {
    // Commands are read on a dedicated thread so the STA/message-pump thread
    // never blocks on stdin (spec Part 9).
    let (tx, rx) = crossbeam_channel::unbounded::<HostCommand>();
    std::thread::Builder::new()
        .name("plugin-host-stdin".into())
        .spawn(move || {
            let mut reader = BufReader::new(io::stdin());
            loop {
                match ipc::read_frame::<HostCommand, _>(&mut reader) {
                    Ok(Some(cmd)) => {
                        if tx.send(cmd).is_err() {
                            break;
                        }
                    }
                    // EOF (main app gone) or malformed frame → stop reading; the
                    // main loop will see the channel disconnect and exit.
                    Ok(None) | Err(_) => break,
                }
            }
        })
        .expect("spawn plugin-host stdin reader");

    let mut registry = Registry::new();

    loop {
        // 1. Drain and dispatch every queued command.
        loop {
            match rx.try_recv() {
                Ok(cmd) => {
                    if matches!(cmd, HostCommand::Shutdown) {
                        hlog!("[PluginHostEditor] shutdown requested");
                        native_editor::detach_all_embedded_editors();
                        return;
                    }
                    dispatch(cmd, &mut registry, &mut out);
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    hlog!("[PluginHostEditor] stdin closed → detaching all and exiting");
                    native_editor::detach_all_embedded_editors();
                    return;
                }
            }
        }

        // 2. Keep attached editors painting / geometry in sync, and pump our own
        //    message queue so the foreign-parented IPlugView gets messages.
        for handle in registry.values() {
            native_editor::refresh_editor_host(*handle);
        }
        platform::pump_messages();

        // 3. Idle a touch to avoid a busy spin (~120 Hz).
        std::thread::sleep(Duration::from_millis(8));
    }
}

fn dispatch(cmd: HostCommand, registry: &mut Registry, out: &mut io::Stdout) {
    match cmd {
        HostCommand::Hello { protocol_version } => {
            // The startup `Ready` is the handshake response; `Hello` only carries
            // the client's version for a compatibility check.
            if protocol_version != PROTOCOL_VERSION {
                hlog!(
                    "[PluginHostEditor] protocol mismatch client={protocol_version} host={PROTOCOL_VERSION}"
                );
            }
        }
        HostCommand::Ping => {
            hlog!("[PluginHostEditor] Ping → Pong");
            let _ = ipc::write_frame(
                out,
                &HostEvent::Pong {
                    pid: std::process::id(),
                },
            );
        }
        HostCommand::OpenEditorWithParentHwnd {
            plugin_instance_id,
            plugin_path,
            class_id,
            parent_hwnd,
            width,
            height,
            dpi,
        } => {
            let is_window = platform::is_window(parent_hwnd);
            hlog!(
                "[PluginHostEditor] mode=main_owned_window plugin_instance_id={plugin_instance_id} \
                 received_parent_hwnd=0x{parent_hwnd:x} is_window={is_window} \
                 width={width} height={height} dpi={dpi} thread_id={}",
                platform::current_thread_id()
            );

            if !is_window {
                emit_attach_failed(out, &plugin_instance_id, "parent_hwnd is not a valid window");
                return;
            }

            // Fail fast on a missing plugin before handing it to the C++ loader:
            // the path-based backend can otherwise block on bad input, and there
            // is nothing to attach. (The HWND was already validated above, so a
            // path failure here still proves cross-process HWND recognition.)
            if !std::path::Path::new(&plugin_path).exists() {
                emit_attach_failed(
                    out,
                    &plugin_instance_id,
                    &format!("plugin path not found: {plugin_path}"),
                );
                return;
            }

            let region = EmbedRegion {
                x: 0,
                y: 0,
                width: width as i32,
                height: height as i32,
            };
            match native_editor::attach_editor_into_parent(
                parent_hwnd,
                &plugin_path,
                &class_id,
                region,
            ) {
                Ok(handle) => {
                    // Re-issue onSize against the requested content rect.
                    native_editor::set_editor_region_bounds(handle, region);
                    registry.insert(plugin_instance_id.clone(), handle);
                    hlog!(
                        "[PluginHostEditor] attached_result=ok handle=0x{handle:x} onSize=({width}x{height})"
                    );
                    // NOTE: the path-based facade does not yet expose
                    // IPlugView::getSize, so preferred == requested for now.
                    // Exposing getSize (and emitting EditorPreferredSize) is a
                    // later slice.
                    let _ = ipc::write_frame(
                        out,
                        &HostEvent::EditorAttached {
                            plugin_instance_id,
                            result: 0,
                            preferred_width: width,
                            preferred_height: height,
                        },
                    );
                }
                Err(err) => {
                    hlog!("[PluginHostEditor] attached_result=err {err}");
                    emit_attach_failed(out, &plugin_instance_id, &err);
                }
            }
        }
        HostCommand::ResizeEditor {
            plugin_instance_id,
            width,
            height,
            dpi,
        } => {
            if let Some(&handle) = registry.get(&plugin_instance_id) {
                let region = EmbedRegion {
                    x: 0,
                    y: 0,
                    width: width as i32,
                    height: height as i32,
                };
                native_editor::set_editor_region_bounds(handle, region);
                hlog!(
                    "[PluginHostEditor] resize plugin_instance_id={plugin_instance_id} \
                     onSize width={width} height={height} dpi={dpi}"
                );
            } else {
                hlog!("[PluginHostEditor] resize: unknown plugin_instance_id={plugin_instance_id}");
            }
        }
        HostCommand::CloseEditor { plugin_instance_id } => {
            if let Some(handle) = registry.remove(&plugin_instance_id) {
                native_editor::detach_editor(handle);
                hlog!("[PluginHostEditor] removed called plugin_instance_id={plugin_instance_id}");
            }
            let _ = ipc::write_frame(out, &HostEvent::EditorClosed { plugin_instance_id });
        }
        HostCommand::UnloadPlugin { plugin_instance_id } => {
            // Slice 1: the path-based facade ties the loaded instance to the
            // editor session, so unload == detach. A dedicated load/unload
            // registry is a later slice.
            if let Some(handle) = registry.remove(&plugin_instance_id) {
                native_editor::detach_editor(handle);
            }
            hlog!("[PluginHostEditor] unload plugin_instance_id={plugin_instance_id} released=true");
            let _ = ipc::write_frame(out, &HostEvent::PluginUnloaded { plugin_instance_id });
        }
        HostCommand::Shutdown => {
            // Handled in run_ipc_loop before dispatch; unreachable here.
        }
    }
}

fn emit_attach_failed(out: &mut io::Stdout, plugin_instance_id: &str, error: &str) {
    let _ = ipc::write_frame(
        out,
        &HostEvent::EditorAttachFailed {
            plugin_instance_id: plugin_instance_id.to_string(),
            error: error.to_string(),
        },
    );
}

/// Self-test path (`--selftest`): prove that the host can create a real
/// content **child** HWND distinct from a top HWND, with the required Win32
/// styles, and (optionally) attach a plugin to it. Drives the acceptance logs
/// without needing the main app or a real plugin.
///
/// Set `FUTUREBOARD_SELFTEST_PLUGIN_PATH` + `FUTUREBOARD_SELFTEST_CLASS_ID` to
/// also exercise a real VST3 attach. Exit code 0 on success.
fn run_selftest() -> i32 {
    match platform::create_selftest_windows() {
        Some((top_hwnd, content_hwnd)) => {
            let content_is_child = content_hwnd != top_hwnd && content_hwnd != 0;
            eprintln!("[plugin-view] selected_host_mode=main_owned_window");
            eprintln!("[plugin-view] top_hwnd=0x{top_hwnd:x}");
            eprintln!("[plugin-view] content_hwnd=0x{content_hwnd:x}");
            eprintln!("[plugin-view] content_is_child={content_is_child}");
            eprintln!("[plugin-view] content_parent=0x{top_hwnd:x}");
            if content_hwnd == top_hwnd {
                eprintln!("[plugin-view] ERROR content_hwnd == top_hwnd — not attaching");
                platform::destroy_selftest_windows(top_hwnd, content_hwnd);
                return 1;
            }
            eprintln!("[plugin-view] content_hwnd != top_hwnd");

            let mut code = 0;
            if let (Ok(path), Ok(class_id)) = (
                std::env::var("FUTUREBOARD_SELFTEST_PLUGIN_PATH"),
                std::env::var("FUTUREBOARD_SELFTEST_CLASS_ID"),
            ) {
                let region = EmbedRegion {
                    x: 0,
                    y: 0,
                    width: 800,
                    height: 600,
                };
                match native_editor::attach_editor_into_parent(content_hwnd, &path, &class_id, region)
                {
                    Ok(handle) => {
                        eprintln!("[vst3-editor] attached begin parent=0x{content_hwnd:x}");
                        eprintln!("[vst3-editor] attached result=ok handle=0x{handle:x}");
                        native_editor::detach_editor(handle);
                    }
                    Err(err) => {
                        eprintln!("[vst3-editor] attached result=err {err}");
                        code = 1;
                    }
                }
            } else {
                eprintln!(
                    "[plugin-view] selftest: no FUTUREBOARD_SELFTEST_PLUGIN_PATH/CLASS_ID — \
                     HWND hierarchy only"
                );
            }

            platform::destroy_selftest_windows(top_hwnd, content_hwnd);
            code
        }
        None => {
            eprintln!("[plugin-view] selftest: window creation unavailable on this platform");
            // Not a failure on non-Windows — there is nothing to host there yet.
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Platform shims. Windows is the real implementation; other targets get no-op
// stubs so the binary still compiles and the IPC loop still runs.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, IsWindow, PeekMessageW, TranslateMessage,
        CW_USEDEFAULT, MSG, PM_REMOVE, WINDOW_EX_STYLE, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    fn hwnd_from(handle: u64) -> HWND {
        HWND(handle as *mut core::ffi::c_void)
    }

    pub fn com_init() {
        // STA: VST3 editors require apartment-threaded COM (spec Part 9).
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
    }

    pub fn com_uninit() {
        unsafe { CoUninitialize() };
    }

    pub fn current_thread_id() -> u64 {
        unsafe { GetCurrentThreadId() as u64 }
    }

    pub fn is_window(handle: u64) -> bool {
        if handle == 0 {
            return false;
        }
        unsafe { IsWindow(Some(hwnd_from(handle))).as_bool() }
    }

    /// Non-blocking drain of this thread's message queue.
    pub fn pump_messages() {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Create a top window + a real WS_CHILD content window using the
    /// predefined `STATIC` class (no RegisterClass/WndProc needed). Returns
    /// `(top_hwnd, content_hwnd)` as `u64`s.
    pub fn create_selftest_windows() -> Option<(u64, u64)> {
        unsafe {
            let top = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Futureboard Plugin Host Selftest"),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                820,
                640,
                None,
                None,
                None,
                None,
            )
            .ok()?;

            let content = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                800,
                600,
                Some(top),
                None,
                None,
                None,
            )
            .ok()?;

            Some((top.0 as u64, content.0 as u64))
        }
    }

    pub fn destroy_selftest_windows(top: u64, content: u64) {
        unsafe {
            if content != 0 {
                let _ = DestroyWindow(hwnd_from(content));
            }
            if top != 0 {
                let _ = DestroyWindow(hwnd_from(top));
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn com_init() {}
    pub fn com_uninit() {}
    pub fn current_thread_id() -> u64 {
        0
    }
    pub fn is_window(handle: u64) -> bool {
        handle != 0
    }
    pub fn pump_messages() {}
    pub fn create_selftest_windows() -> Option<(u64, u64)> {
        None
    }
    pub fn destroy_selftest_windows(_top: u64, _content: u64) {}
}
