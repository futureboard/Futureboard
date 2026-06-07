//! Main-app side of the separated plugin-host IPC: spawns
//! `FutureboardPluginHost-x64.exe`, sends [`HostCommand`]s on its stdin, and
//! delivers [`HostEvent`]s (plus a synthetic disconnect signal) over a
//! `crossbeam-channel` so the GPUI UI thread can poll without ever blocking
//! (spec Part 9).
//!
//! Slice 1: this client is only constructed when
//! `FUTUREBOARD_PLUGIN_EDITOR_OWNERSHIP=host_process`. The default
//! (`main_owned`, the current in-process path) does not touch this module, so
//! existing behavior is unchanged. Wiring the GPUI editor window's content
//! child HWND through `open_editor` is Slice 2.

use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, TryRecvError};

use crate::ipc::{self, HostCommand, HostEvent, PROTOCOL_VERSION};

/// What the UI thread receives from [`PluginHostClient::try_recv_event`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    /// A typed event from the host process.
    Host(HostEvent),
    /// The host's stdout pipe closed — the process exited or crashed. The main
    /// app should mark any open editors on this host as offline (spec Part 7).
    Disconnected,
}

/// Errors spawning / locating the host binary.
#[derive(Debug)]
pub enum PluginHostClientError {
    BinaryMissing(String),
    Spawn(std::io::Error),
}

impl std::fmt::Display for PluginHostClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryMissing(p) => write!(f, "plugin host binary not found: {p}"),
            Self::Spawn(e) => write!(f, "failed to spawn plugin host: {e}"),
        }
    }
}

impl std::error::Error for PluginHostClientError {}

const BINARY_STEM: &str = "FutureboardPluginHost-x64";

fn binary_name(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        dir.join(format!("{BINARY_STEM}.exe"))
    }
    #[cfg(not(windows))]
    {
        dir.join(BINARY_STEM)
    }
}

/// Truthy env values accepted for boolean flags: `1`, `true`, `yes`, `on`
/// (case-insensitive).
fn env_is_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

/// Whether the external plugin-host **bridge** is enabled
/// (`FUTUREBOARD_PLUGIN_HOST_BRIDGE` truthy). When enabled, the editor flow MUST
/// route through `FutureboardPluginHost-x64.exe` and MUST NOT fall back to the
/// in-process VST3 editor path.
pub fn plugin_host_bridge_enabled() -> bool {
    env_is_truthy("FUTUREBOARD_PLUGIN_HOST_BRIDGE")
        // Back-compat alias from the earlier slice.
        || std::env::var("FUTUREBOARD_PLUGIN_EDITOR_OWNERSHIP")
            .map(|v| v.eq_ignore_ascii_case("host_process"))
            .unwrap_or(false)
}

/// One-time boot diagnostics for the bridge flag. Call early in app startup.
pub fn log_bridge_env() {
    let raw = std::env::var("FUTUREBOARD_PLUGIN_HOST_BRIDGE").unwrap_or_else(|_| "<unset>".into());
    eprintln!("[plugin-bridge] env FUTUREBOARD_PLUGIN_HOST_BRIDGE={raw}");
    eprintln!("[plugin-bridge] enabled={}", plugin_host_bridge_enabled());
}

/// Resolve the host executable and whether it exists on disk. Resolution order
/// (spec): `FUTUREBOARD_PLUGIN_HOST_EXE` → next to the running exe →
/// `target/{debug,release}`. `FUTUREBOARD_PLUGIN_HOST` is a legacy alias for the
/// explicit override. The returned path is logged by [`PluginHostClient::spawn_bridge`].
pub fn resolve_host_exe() -> (PathBuf, bool) {
    for var in ["FUTUREBOARD_PLUGIN_HOST_EXE", "FUTUREBOARD_PLUGIN_HOST"] {
        if let Ok(path) = std::env::var(var) {
            let path = PathBuf::from(path);
            let exists = path.is_file();
            return (path, exists);
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = binary_name(dir);
            if candidate.is_file() {
                return (candidate, true);
            }
        }
    }

    for profile in ["debug", "release"] {
        let candidate = binary_name(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../target/{profile}")),
        );
        if candidate.is_file() {
            return (candidate, true);
        }
    }

    (PathBuf::from(BINARY_STEM), false)
}

/// Resolve the host executable, erroring if it cannot be found. Used by tests.
pub fn locate_plugin_host_binary() -> Result<PathBuf, PluginHostClientError> {
    let (path, exists) = resolve_host_exe();
    if exists {
        Ok(path)
    } else {
        Err(PluginHostClientError::BinaryMissing(
            path.display().to_string(),
        ))
    }
}

/// A live connection to one `FutureboardPluginHost-x64.exe` process.
///
/// Drop sends `Shutdown` (best-effort) and then kills the child, so a dropped
/// client never leaves an orphan host process.
pub struct PluginHostClient {
    child: Child,
    stdin: ChildStdin,
    events: Receiver<ClientEvent>,
    reader: Option<JoinHandle<()>>,
}

impl PluginHostClient {
    /// Spawn the host process and start the background event reader.
    pub fn spawn() -> Result<Self, PluginHostClientError> {
        let binary = locate_plugin_host_binary()?;
        Self::spawn_from(&binary)
    }

    /// Resolve + spawn the bridge host with full `[plugin-bridge]` diagnostics
    /// (spec: current_exe / resolved_host_exe / exists / spawning / spawned).
    /// Does NOT fall back silently — the caller decides what to do on `Err`.
    pub fn spawn_bridge() -> Result<Self, PluginHostClientError> {
        let current_exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "<unknown>".into());
        let (exe, exists) = resolve_host_exe();
        eprintln!("[plugin-bridge] current_exe={current_exe}");
        eprintln!("[plugin-bridge] resolved_host_exe={}", exe.display());
        eprintln!("[plugin-bridge] exists={exists}");
        if !exists {
            let err = PluginHostClientError::BinaryMissing(exe.display().to_string());
            eprintln!("[plugin-bridge] spawn_failed error={err}");
            return Err(err);
        }
        eprintln!("[plugin-bridge] spawning {}", exe.display());
        match Self::spawn_from(&exe) {
            Ok(client) => {
                eprintln!("[plugin-bridge] spawned pid={}", client.pid());
                Ok(client)
            }
            Err(e) => {
                eprintln!("[plugin-bridge] spawn_failed error={e}");
                Err(e)
            }
        }
    }

    /// Spawn a specific host binary (used by tests).
    pub fn spawn_from(binary: &Path) -> Result<Self, PluginHostClientError> {
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(PluginHostClientError::Spawn)?;

        let stdin = child
            .stdin
            .take()
            .expect("child configured with piped stdin");
        let stdout = child
            .stdout
            .take()
            .expect("child configured with piped stdout");

        let (tx, rx) = crossbeam_channel::unbounded::<ClientEvent>();
        let reader = std::thread::Builder::new()
            .name("plugin-host-events".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    match ipc::read_frame::<HostEvent, _>(&mut reader) {
                        Ok(Some(event)) => {
                            if tx.send(ClientEvent::Host(event)).is_err() {
                                break; // client dropped
                            }
                        }
                        // EOF or malformed frame → the host is gone/unusable.
                        Ok(None) | Err(_) => {
                            let _ = tx.send(ClientEvent::Disconnected);
                            break;
                        }
                    }
                }
            })
            .expect("spawn plugin-host event reader");

        let mut client = Self {
            child,
            stdin,
            events: rx,
            reader: Some(reader),
        };
        client.send(&HostCommand::Hello {
            protocol_version: PROTOCOL_VERSION,
        })?;
        Ok(client)
    }

    /// OS process id of the spawned host (for diagnostics / Task Manager).
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Send a `Ping`; the host replies with [`HostEvent::Pong`].
    pub fn ping(&mut self) -> Result<(), PluginHostClientError> {
        self.send(&HostCommand::Ping)
    }

    /// Send one command to the host. Maps to `EditorAttached` etc. on the event
    /// channel; this call itself only writes the frame.
    pub fn send(&mut self, cmd: &HostCommand) -> Result<(), PluginHostClientError> {
        ipc::write_frame(&mut self.stdin, cmd).map_err(PluginHostClientError::Spawn)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open_editor(
        &mut self,
        plugin_instance_id: impl Into<String>,
        plugin_path: impl Into<String>,
        class_id: impl Into<String>,
        parent_hwnd: u64,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> Result<(), PluginHostClientError> {
        self.send(&HostCommand::OpenEditorWithParentHwnd {
            plugin_instance_id: plugin_instance_id.into(),
            plugin_path: plugin_path.into(),
            class_id: class_id.into(),
            parent_hwnd,
            width,
            height,
            dpi,
        })
    }

    pub fn resize_editor(
        &mut self,
        plugin_instance_id: impl Into<String>,
        width: u32,
        height: u32,
        dpi: u32,
    ) -> Result<(), PluginHostClientError> {
        self.send(&HostCommand::ResizeEditor {
            plugin_instance_id: plugin_instance_id.into(),
            width,
            height,
            dpi,
        })
    }

    pub fn close_editor(
        &mut self,
        plugin_instance_id: impl Into<String>,
    ) -> Result<(), PluginHostClientError> {
        self.send(&HostCommand::CloseEditor {
            plugin_instance_id: plugin_instance_id.into(),
        })
    }

    pub fn unload_plugin(
        &mut self,
        plugin_instance_id: impl Into<String>,
    ) -> Result<(), PluginHostClientError> {
        self.send(&HostCommand::UnloadPlugin {
            plugin_instance_id: plugin_instance_id.into(),
        })
    }

    /// Ask the host to shut down gracefully. Best-effort; `Drop` still enforces
    /// teardown.
    pub fn shutdown(&mut self) -> Result<(), PluginHostClientError> {
        self.send(&HostCommand::Shutdown)
    }

    /// Non-blocking poll for the next event. `None` when nothing is queued.
    pub fn try_recv_event(&self) -> Option<ClientEvent> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            // The reader thread already pushed `Disconnected` before exiting,
            // so a closed channel just means nothing more is coming.
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// `Some(true)` if the host has exited, `Some(false)` if still running,
    /// `None` if the status could not be queried.
    pub fn has_exited(&mut self) -> Option<bool> {
        match self.child.try_wait() {
            Ok(Some(_)) => Some(true),
            Ok(None) => Some(false),
            Err(_) => None,
        }
    }
}

impl Drop for PluginHostClient {
    fn drop(&mut self) {
        // Best-effort graceful shutdown, then ensure no orphan process.
        let _ = ipc::write_frame(&mut self.stdin, &HostCommand::Shutdown);
        let _ = self.stdin.flush();
        // Closing stdin gives the host its EOF; give the reader a beat to drain,
        // then force-kill if it is still alive.
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
