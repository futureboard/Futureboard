//! Central plugin-host process manager for the DAW session.

use std::collections::HashMap;
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::platform::PluginHostJob;
use crate::plugin_host_client::{PluginHostClient, PluginHostClientError};
use crate::plugin_host_spawn_config::PluginHostSpawnConfig;

/// Stable id for a spawned `FutureboardPluginHostX64.exe` process.
pub type PluginHostId = String;

/// Lifecycle state of a tracked plugin-host child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostLifecycleState {
    Running,
    ShutdownRequested,
    Exited,
    KillRequired,
}

/// One `FutureboardPluginHostX64.exe` child owned by the studio session.
#[derive(Debug, Clone)]
pub struct BridgeHostRecord {
    pub host_id: PluginHostId,
    pub pid: u32,
    pub session_id: String,
    pub project_id: String,
    pub instance_ids: Vec<String>,
    pub state: HostLifecycleState,
}

/// Handle returned after a successful host spawn.
pub struct PluginHostHandle {
    pub host_id: PluginHostId,
    pub pid: u32,
    pub client: PluginHostClient,
}

impl std::fmt::Debug for PluginHostHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHostHandle")
            .field("host_id", &self.host_id)
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

/// Tracks every spawned plugin-host pid and owns the Windows job object.
pub struct PluginHostProcessManager {
    #[cfg(windows)]
    job: PluginHostJob,
    hosts: Mutex<HashMap<u32, BridgeHostRecord>>,
}

static MANAGER: OnceLock<PluginHostProcessManager> = OnceLock::new();

/// Per-host graceful shutdown wait before force-terminate.
pub const HOST_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(4);

impl PluginHostProcessManager {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            #[cfg(windows)]
            job: PluginHostJob::new(),
            hosts: Mutex::new(HashMap::new()),
        })
    }

    pub fn global() -> &'static Self {
        MANAGER.get_or_init(|| Self::new().expect("plugin host process manager"))
    }

    /// Spawn a plugin-host process and register it with the session job object.
    pub fn spawn_host(
        &self,
        config: PluginHostSpawnConfig,
    ) -> Result<PluginHostHandle, PluginHostClientError> {
        eprintln!(
            "[PluginHost] spawn begin instance={} project={} main_hwnd={}",
            config.instance_id,
            config.project_id,
            config
                .main_hwnd
                .map(|h| format!("0x{h:x}"))
                .unwrap_or_else(|| "none".into())
        );
        let client = PluginHostClient::spawn_bridge_with_config(&config)?;
        let pid = client.pid();
        let host_id = format!("host-{pid}");
        eprintln!("[PluginHost] spawn complete host_id={host_id} pid={pid}");
        Ok(PluginHostHandle {
            host_id,
            pid,
            client,
        })
    }

    /// Register a child that was spawned through the legacy path.
    pub fn on_host_spawned(&self, child: &Child, config: &PluginHostSpawnConfig) {
        let pid = child.id();
        let host_id = format!("host-{pid}");
        self.hosts.lock().expect("plugin host lock").insert(
            pid,
            BridgeHostRecord {
                host_id: host_id.clone(),
                pid,
                session_id: config.project_id.clone(),
                project_id: config.project_id.clone(),
                instance_ids: vec![config.instance_id.clone()],
                state: HostLifecycleState::Running,
            },
        );
        eprintln!(
            "[PluginHost] registered host_id={host_id} pid={pid} project={}",
            config.project_id
        );
        #[cfg(windows)]
        self.job.assign_child(child);
    }

    pub fn set_host_instances(&self, pid: u32, instance_ids: Vec<String>) {
        if let Some(record) = self.hosts.lock().expect("plugin host lock").get_mut(&pid) {
            record.instance_ids = instance_ids;
        }
    }

    pub fn mark_shutdown_requested(&self, pid: u32) {
        if let Some(record) = self.hosts.lock().expect("plugin host lock").get_mut(&pid) {
            record.state = HostLifecycleState::ShutdownRequested;
            eprintln!(
                "[PluginHost] shutdown requested host_id={} pid={pid} instances={:?}",
                record.host_id, record.instance_ids
            );
        }
    }

    pub fn mark_exited(&self, pid: u32, code: Option<i32>) {
        if let Some(record) = self.hosts.lock().expect("plugin host lock").get_mut(&pid) {
            record.state = HostLifecycleState::Exited;
            eprintln!(
                "[PluginHost] exited host_id={} pid={pid} code={}",
                record.host_id,
                code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
            );
        }
    }

    pub fn mark_kill_required(&self, pid: u32) {
        if let Some(record) = self.hosts.lock().expect("plugin host lock").get_mut(&pid) {
            record.state = HostLifecycleState::KillRequired;
            eprintln!(
                "[PluginHost] timeout host_id={} pid={pid}",
                record.host_id
            );
        }
    }

    pub fn mark_killed(&self, pid: u32) {
        if let Some(record) = self.hosts.lock().expect("plugin host lock").get_mut(&pid) {
            eprintln!(
                "[PluginHost] killed host_id={} pid={pid}",
                record.host_id
            );
        }
        self.hosts.lock().expect("plugin host lock").remove(&pid);
    }

    pub fn host_count(&self) -> usize {
        self.hosts.lock().expect("plugin host lock").len()
    }

    pub fn host_records(&self) -> Vec<BridgeHostRecord> {
        self.hosts
            .lock()
            .expect("plugin host lock")
            .values()
            .cloned()
            .collect()
    }

    pub fn clear_hosts(&self) {
        self.hosts.lock().expect("plugin host lock").clear();
    }

    /// Gracefully shut down one live client, waiting briefly before killing.
    pub fn shutdown_host(
        &self,
        client: &mut PluginHostClient,
        timeout: Duration,
    ) -> Result<(), PluginHostClientError> {
        shutdown_host_client_on(self, client, timeout);
        Ok(())
    }

    /// Terminate every tracked host. Used on panic / app exit fallback.
    pub fn shutdown_all(&self, timeout: Duration) -> Result<(), String> {
        let records = self.host_records();
        eprintln!(
            "[PluginHost] shutdown_all begin tracked_hosts={}",
            records.len()
        );
        for record in records {
            if matches!(
                record.state,
                HostLifecycleState::Exited | HostLifecycleState::KillRequired
            ) {
                continue;
            }
            eprintln!(
                "[PluginHost] shutdown_all terminate host_id={} pid={}",
                record.host_id, record.pid
            );
            #[cfg(windows)]
            {
                let _ = timeout;
                crate::platform::windows_process::terminate_process_pid(record.pid);
            }
            #[cfg(not(windows))]
            {
                let _ = timeout;
            }
            self.mark_killed(record.pid);
        }
        self.clear_hosts();
        eprintln!("[PluginHost] shutdown_all complete");
        Ok(())
    }
}

/// Gracefully shut down one live [`PluginHostClient`], waiting briefly before
/// force-terminating stragglers. Idempotent.
pub fn shutdown_host_client(client: &mut PluginHostClient) {
    shutdown_host_client_with_timeout(client, HOST_SHUTDOWN_TIMEOUT);
}

pub fn shutdown_host_client_with_timeout(
    client: &mut PluginHostClient,
    timeout: Duration,
) {
    shutdown_host_client_on(PluginHostProcessManager::global(), client, timeout);
}

fn shutdown_host_client_on(
    manager: &PluginHostProcessManager,
    client: &mut PluginHostClient,
    timeout: Duration,
) {
    if client.shutdown_started {
        return;
    }
    client.shutdown_started = true;
    let pid = client.pid();
    manager.mark_shutdown_requested(pid);
    eprintln!("[PluginHost] shutdown requested host_id=host-{pid} pid={pid}");
    let _ = client.shutdown();

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match client.has_exited() {
            Some(true) => {
                let code = client
                    .wait_for_exit()
                    .ok()
                    .and_then(|status| status.code());
                manager.mark_exited(pid, code);
                eprintln!(
                    "[PluginHost] exited host_id=host-{pid} pid={pid} code={}",
                    code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
                );
                break;
            }
            Some(false) if std::time::Instant::now() >= deadline => {
                manager.mark_kill_required(pid);
                eprintln!("[PluginHost] terminate requested host_id=host-{pid} pid={pid}");
                let _ = client.force_kill();
                let _ = client.wait_for_exit();
                manager.mark_killed(pid);
                eprintln!("[PluginHost] killed host_id=host-{pid} pid={pid}");
                break;
            }
            Some(false) => std::thread::sleep(Duration::from_millis(50)),
            None => break,
        }
    }
}

/// Back-compat alias used across the studio shell.
pub type BridgeHostManager = PluginHostProcessManager;

/// Initialize the global manager early in main-app startup.
pub fn init_plugin_host_job() {
    let _ = PluginHostProcessManager::global();
}
