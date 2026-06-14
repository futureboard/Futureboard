//! End-to-end IPC round-trip against a real spawned `FutureboardPluginHostX64`
//! process. Gated behind `plugin-host-bin` so it only runs when the host
//! binary is actually built:
//!
//!   cargo test -p sphere-plugin-host --features plugin-host-bin --test host_ipc_roundtrip
//!
//! No real plugin is required: opening an editor against an invalid HWND (0)
//! deterministically yields `EditorAttachFailed`, which proves the
//! spawn → Hello/Ready → command → event path on every platform.
#![cfg(feature = "plugin-host-bin")]

use std::time::{Duration, Instant};

use sphere_plugin_host::ipc::HostEvent;
use sphere_plugin_host::plugin_host_client::{ClientEvent, PluginHostClient};

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
fn ready_then_attach_failed_for_invalid_hwnd() {
    let mut client = PluginHostClient::spawn().expect("spawn FutureboardPluginHostX64");

    // Host announces itself on startup.
    match wait_event(&client, Duration::from_secs(10)) {
        Some(ClientEvent::Host(HostEvent::Ready {
            protocol_version, ..
        })) => {
            assert_eq!(protocol_version, sphere_plugin_host::ipc::PROTOCOL_VERSION);
        }
        other => panic!("expected Ready, got {other:?}"),
    }

    // Invalid parent HWND (0) → deterministic attach failure, no plugin needed.
    client
        .open_editor(
            "track1:insert1",
            "C:/does/not/exist.vst3",
            "DEADBEEF",
            0,
            800,
            600,
            96,
        )
        .expect("send open_editor");

    match wait_event(&client, Duration::from_secs(10)) {
        Some(ClientEvent::Host(HostEvent::EditorAttachFailed {
            plugin_instance_id, ..
        })) => {
            assert_eq!(plugin_instance_id, "track1:insert1");
        }
        other => panic!("expected EditorAttachFailed, got {other:?}"),
    }

    client.shutdown().expect("send shutdown");
    // Drop force-kills if the host has not already exited.
}

/// State commands round-trip the wire against a real host without a plugin:
/// an unloaded instance deterministically yields `PluginState { ok: false }`
/// and `PluginStateSet { ok: false }`, proving the new frames parse on both
/// sides.
#[test]
fn plugin_state_commands_answer_for_unloaded_instance() {
    let mut client = PluginHostClient::spawn().expect("spawn FutureboardPluginHostX64");

    client
        .get_plugin_state("track1:insert1")
        .expect("send get_plugin_state");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut saw_state = false;
    while Instant::now() < deadline {
        match client.try_recv_event() {
            Some(ClientEvent::Host(HostEvent::PluginState {
                plugin_instance_id,
                ok,
                component_b64,
                controller_b64,
            })) => {
                assert_eq!(plugin_instance_id, "track1:insert1");
                assert!(!ok, "unloaded instance must report ok=false");
                assert!(component_b64.is_empty());
                assert!(controller_b64.is_empty());
                saw_state = true;
                break;
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    assert!(saw_state, "host did not answer GetPluginState");

    client
        .set_plugin_state("track1:insert1", "AAEC".into(), String::new())
        .expect("send set_plugin_state");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut saw_set = false;
    while Instant::now() < deadline {
        match client.try_recv_event() {
            Some(ClientEvent::Host(HostEvent::PluginStateSet {
                plugin_instance_id,
                ok,
            })) => {
                assert_eq!(plugin_instance_id, "track1:insert1");
                assert!(!ok, "unloaded instance must report ok=false");
                saw_set = true;
                break;
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    assert!(saw_set, "host did not answer SetPluginState");
    client.shutdown().ok();
}

#[test]
fn ping_is_answered_with_pong() {
    let mut client = PluginHostClient::spawn().expect("spawn FutureboardPluginHostX64");

    // Drain the startup Ready, then Ping and expect Pong carrying the host pid.
    let mut saw_pong = false;
    client.ping().expect("send ping");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match client.try_recv_event() {
            Some(ClientEvent::Host(HostEvent::Pong { pid })) => {
                assert_eq!(pid, client.pid(), "Pong pid should match the spawned child");
                saw_pong = true;
                break;
            }
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    assert!(saw_pong, "host did not answer Ping with Pong");
    client.shutdown().ok();
}
