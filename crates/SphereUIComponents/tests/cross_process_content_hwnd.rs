//! Cross-process content-HWND validation (spec Parts 2/3/11).
//!
//! Proves the keystone of host-process editor ownership **without** GPUI or a
//! real plugin:
//!   1. the main app (this test process) creates a top HWND + a real `WS_CHILD`
//!      content HWND (`content_hwnd != top_hwnd`, parented correctly);
//!   2. the content HWND's handle is sent to a spawned
//!      `FutureboardPluginHostX64.exe` over IPC;
//!   3. the host process — a *different* process — recognizes the foreign HWND
//!      (`IsWindow == true`) and attempts to attach. With a bogus plugin path it
//!      returns `EditorAttachFailed`, which still proves cross-process HWND
//!      recognition (the Part 11 compatibility question).
//!
//! Windows-only, and skipped (passes) if the host binary has not been built —
//! build it first with:
//!   cargo build -p sphere-plugin-host --features plugin-host-bin --bin FutureboardPluginHostX64
#![cfg(target_os = "windows")]

use std::time::{Duration, Instant};

use SpherePluginHost::ipc::HostEvent;
use SpherePluginHost::plugin_host_client::{
    locate_plugin_host_binary, ClientEvent, PluginHostClient,
};
use sphere_ui_components::components::plugin_content_host::{ContentChildHwnd, ContentRect};

use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, IsChild, WINDOW_EX_STYLE, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WS_OVERLAPPEDWINDOW,
};

/// Create a throwaway top-level window to stand in for the GPUI editor shell.
fn create_top_window() -> HWND {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("FB content-hwnd test"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            0,
            0,
            820,
            640,
            None,
            None,
            None,
            None,
        )
        .expect("create top window")
    }
}

fn wait_event(client: &PluginHostClient, timeout: Duration) -> Option<ClientEvent> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(ev) = client.try_recv_event() {
            return Some(ev);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

#[test]
fn content_child_is_distinct_and_parented() {
    let top = create_top_window();
    let top_u64 = top.0 as u64;

    let content = ContentChildHwnd::create(
        top_u64,
        ContentRect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
    )
    .expect("create content child hwnd");

    assert_ne!(
        content.hwnd(),
        top_u64,
        "content_hwnd must differ from top_hwnd"
    );
    assert_eq!(content.top_hwnd(), top_u64);
    assert!(content.is_valid());
    unsafe {
        assert!(
            IsChild(top, HWND(content.hwnd() as *mut core::ffi::c_void)).as_bool(),
            "content HWND must be a child of the top HWND"
        );
        let _ = DestroyWindow(top); // also destroys the child
    }
}

#[test]
fn host_process_recognizes_main_owned_content_hwnd() {
    // Skip gracefully if the host binary isn't built (e.g. CI without the
    // feature). Not a failure: this test validates the wiring, not the build.
    if locate_plugin_host_binary().is_err() {
        eprintln!(
            "skipping: FutureboardPluginHostX64 not built \
             (cargo build -p sphere-plugin-host --features plugin-host-bin --bin FutureboardPluginHostX64)"
        );
        return;
    }

    let top = create_top_window();
    let top_u64 = top.0 as u64;
    let content = ContentChildHwnd::create(
        top_u64,
        ContentRect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
    )
    .expect("create content child hwnd");
    let content_hwnd = content.hwnd();
    assert_ne!(content_hwnd, top_u64);

    let mut client = PluginHostClient::spawn().expect("spawn host");
    match wait_event(&client, Duration::from_secs(10)) {
        Some(ClientEvent::Host(HostEvent::Ready { .. })) => {}
        other => panic!("expected Ready, got {other:?}"),
    }

    // Hand the main-app-owned content HWND to the host process and ask it to
    // attach. Bogus plugin path → deterministic EditorAttachFailed, but the
    // host first logs `is_window=true` for our cross-process HWND (run with
    // FUTUREBOARD_PLUGIN_VIEW_DEBUG=1 to observe).
    client
        .open_editor(
            "track1:insert1",
            "C:/does/not/exist.vst3",
            "DEADBEEFDEADBEEF",
            content_hwnd,
            800,
            600,
            96,
        )
        .expect("send open_editor");

    match wait_event(&client, Duration::from_secs(10)) {
        Some(ClientEvent::Host(HostEvent::EditorAttachFailed {
            plugin_instance_id,
            error,
        })) => {
            assert_eq!(plugin_instance_id, "track1:insert1");
            // The failure must be about the plugin, NOT the HWND — that is the
            // proof the cross-process HWND was accepted (is_window == true).
            assert!(
                !error.contains("not a valid window"),
                "host rejected the cross-process HWND: {error}"
            );
        }
        // A plugin-attach success is also acceptable (e.g. if a real plugin were
        // wired); only an HWND rejection would be a failure of this test.
        Some(ClientEvent::Host(HostEvent::EditorAttached { .. })) => {}
        other => panic!("expected EditorAttachFailed/EditorAttached, got {other:?}"),
    }

    client.shutdown().ok();
    unsafe {
        let _ = DestroyWindow(top);
    }
}
