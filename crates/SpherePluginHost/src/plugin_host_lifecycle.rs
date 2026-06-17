//! Windows Job Object and coordinated shutdown for `FutureboardPluginHostX64.exe`
//! child processes. Keeps plugin hosts owned by the main app lifecycle so they
//! cannot outlive `FutureboardNative.exe`.

use std::collections::HashMap;
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::plugin_host_client::PluginHostClient;

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
    pub host_id: String,
    pub pid: u32,
    pub session_id: String,
    pub instance_ids: Vec<String>,
    pub state: HostLifecycleState,
}

/// Tracks every spawned plugin-host pid and owns the Windows job object that
/// terminates children when the main process exits.
pub struct BridgeHostManager {
    #[cfg(windows)]
    job: PluginHostJob,
    hosts: Mutex<HashMap<u32, BridgeHostRecord>>,
}

static MANAGER: OnceLock<BridgeHostManager> = OnceLock::new();

/// Per-host graceful shutdown wait before force-terminate.
pub const HOST_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(4);

impl BridgeHostManager {
    pub fn global() -> &'static Self {
        MANAGER.get_or_init(|| {
            #[cfg(windows)]
            {
                BridgeHostManager {
                    job: PluginHostJob::new(),
                    hosts: Mutex::new(HashMap::new()),
                }
            }
            #[cfg(not(windows))]
            {
                BridgeHostManager {
                    hosts: Mutex::new(HashMap::new()),
                }
            }
        })
    }

    /// Register a newly spawned host child and assign it to the job object.
    pub fn on_host_spawned(&self, child: &Child) {
        let pid = child.id();
        let host_id = format!("host-{pid}");
        self.hosts
            .lock()
            .expect("bridge host lock")
            .insert(
                pid,
                BridgeHostRecord {
                    host_id: host_id.clone(),
                    pid,
                    session_id: "studio".to_string(),
                    instance_ids: Vec::new(),
                    state: HostLifecycleState::Running,
                },
            );
        eprintln!("[PluginHost] registered host_id={host_id} pid={pid}");
        #[cfg(windows)]
        self.job.assign_child(child);
    }

    pub fn set_host_instances(&self, pid: u32, instance_ids: Vec<String>) {
        if let Some(record) = self.hosts.lock().expect("bridge host lock").get_mut(&pid) {
            record.instance_ids = instance_ids;
        }
    }

    pub fn mark_shutdown_requested(&self, pid: u32) {
        if let Some(record) = self.hosts.lock().expect("bridge host lock").get_mut(&pid) {
            record.state = HostLifecycleState::ShutdownRequested;
            eprintln!(
                "[PluginHost] shutdown requested host_id={} pid={pid}",
                record.host_id
            );
        }
    }

    pub fn mark_exited(&self, pid: u32, code: Option<i32>) {
        if let Some(record) = self.hosts.lock().expect("bridge host lock").get_mut(&pid) {
            record.state = HostLifecycleState::Exited;
            eprintln!(
                "[PluginHost] exited host_id={} pid={pid} code={}",
                record.host_id,
                code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
            );
        }
    }

    pub fn mark_kill_required(&self, pid: u32) {
        if let Some(record) = self.hosts.lock().expect("bridge host lock").get_mut(&pid) {
            record.state = HostLifecycleState::KillRequired;
            eprintln!(
                "[PluginHost] timeout host_id={} pid={pid}",
                record.host_id
            );
        }
    }

    pub fn mark_killed(&self, pid: u32) {
        if let Some(record) = self.hosts.lock().expect("bridge host lock").get_mut(&pid) {
            eprintln!(
                "[PluginHost] killed host_id={} pid={pid}",
                record.host_id
            );
        }
        self.hosts.lock().expect("bridge host lock").remove(&pid);
    }

    pub fn host_count(&self) -> usize {
        self.hosts.lock().expect("bridge host lock").len()
    }

    pub fn host_records(&self) -> Vec<BridgeHostRecord> {
        self.hosts
            .lock()
            .expect("bridge host lock")
            .values()
            .cloned()
            .collect()
    }

    /// Clear tracked host pids after coordinated shutdown.
    pub fn clear_hosts(&self) {
        self.hosts.lock().expect("bridge host lock").clear();
    }

    /// Best-effort cleanup hook for panic/crash paths. The job object still
    /// terminates assigned children when the main process exits.
    pub fn shutdown_all(&self) {
        self.clear_hosts();
    }
}

/// Initialize the global job object early in main-app startup so crash/abnormal
/// exit still closes the handle and kills assigned children.
pub fn init_plugin_host_job() {
    let _ = BridgeHostManager::global();
}

/// Set the process-wide explicit AppUserModelID so the OS never groups a stray
/// plugin-host or plugin-editor window under its own taskbar identity.
///
/// **Both** `FutureboardNative.exe` (the DAW) and `FutureboardPluginHostX64.exe`
/// (the bridge) must call this early in startup with the *same* id. It is not the
/// primary taskbar-hiding mechanism — owned `WS_EX_TOOLWINDOW` popups already stay
/// out of the taskbar/Alt-Tab — but it prevents bad grouping if any window ever
/// becomes app-visible, and keeps the bridge's editor windows associated with the
/// DAW's shell identity. Failures are logged, never fatal.
pub const APP_USER_MODEL_ID: &str = "com.futureboard.studio";

#[cfg(windows)]
pub fn set_app_user_model_id() {
    use windows::core::w;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    // SAFETY: `w!` is a 'static NUL-terminated UTF-16 literal; the call only reads it.
    let result = unsafe { SetCurrentProcessExplicitAppUserModelID(w!("com.futureboard.studio")) };
    match result {
        Ok(()) => eprintln!("[app-id] explicit_app_user_model_id={APP_USER_MODEL_ID} ok=true"),
        Err(error) => {
            eprintln!(
                "[app-id] SetCurrentProcessExplicitAppUserModelID failed ok=false error={error}"
            )
        }
    }
}

#[cfg(not(windows))]
pub fn set_app_user_model_id() {
    // AppUserModelID is a Windows shell concept; no-op elsewhere.
}

/// Gracefully shut down one live [`PluginHostClient`], waiting briefly before
/// force-terminating stragglers. Idempotent.
pub fn shutdown_host_client(client: &mut PluginHostClient) {
    shutdown_host_client_with_timeout(client, HOST_SHUTDOWN_TIMEOUT);
}

/// Gracefully shut down one live [`PluginHostClient`] with an explicit timeout.
pub fn shutdown_host_client_with_timeout(
    client: &mut PluginHostClient,
    timeout: Duration,
) {
    if client.shutdown_started {
        return;
    }
    client.shutdown_started = true;
    let pid = client.pid();
    BridgeHostManager::global().mark_shutdown_requested(pid);
    eprintln!("[PluginHost] shutdown requested host_id=host-{pid} pid={pid}");
    let _ = client.shutdown();

    let deadline = Instant::now() + timeout;
    loop {
        match client.has_exited() {
            Some(true) => {
                let code = client
                    .wait_for_exit()
                    .ok()
                    .and_then(|status| status.code());
                BridgeHostManager::global().mark_exited(pid, code);
                eprintln!("[PluginHost] exited host_id=host-{pid} pid={pid} code={}", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()));
                break;
            }
            Some(false) if Instant::now() >= deadline => {
                BridgeHostManager::global().mark_kill_required(pid);
                eprintln!("[PluginHost] terminate requested host_id=host-{pid} pid={pid}");
                let _ = client.force_kill();
                let _ = client.wait_for_exit();
                BridgeHostManager::global().mark_killed(pid);
                eprintln!("[PluginHost] killed host_id=host-{pid} pid={pid}");
                break;
            }
            Some(false) => std::thread::sleep(Duration::from_millis(50)),
            None => break,
        }
    }
}

#[cfg(windows)]
mod job {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct PluginHostJob {
        /// `None` when the job object could not be created (rare: low resources,
        /// or a sandbox that forbids it). The bridge still works — assignment is
        /// skipped and the per-host parent watchdog handles cleanup — so a job
        /// failure must never crash the DAW.
        handle: Option<HANDLE>,
    }

    // Job object handles are process-wide kernel objects; safe to share across
    // threads for assign-on-spawn / close-on-exit.
    unsafe impl Send for PluginHostJob {}
    unsafe impl Sync for PluginHostJob {}

    impl PluginHostJob {
        pub fn new() -> Self {
            unsafe {
                let job = match CreateJobObjectW(None, PCWSTR::null()) {
                    Ok(job) => job,
                    Err(error) => {
                        eprintln!(
                            "[plugin-bridge] job_object create failed ok=false error={error}; \
                             bridge hosts fall back to parent-watchdog cleanup"
                        );
                        return Self { handle: None };
                    }
                };
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if let Err(error) = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) {
                    eprintln!(
                        "[plugin-bridge] job_object set_information failed ok=false error={error}; \
                         closing handle, bridge hosts fall back to parent-watchdog cleanup"
                    );
                    let _ = CloseHandle(job);
                    return Self { handle: None };
                }
                eprintln!("[plugin-bridge] job_object created kill_on_close=true");
                Self { handle: Some(job) }
            }
        }

        pub fn assign_child(&self, child: &Child) {
            let pid = child.id();
            let Some(handle) = self.handle else {
                eprintln!("[plugin-bridge] assign host pid={pid} skipped reason=no_job_object");
                return;
            };
            unsafe {
                let raw = child.as_raw_handle();
                let process = HANDLE(raw as *mut _);
                // A process already inside a non-nestable job (e.g. launched under
                // some sandboxes / debuggers) makes this fail with
                // ERROR_ACCESS_DENIED — log it and keep running; the parent
                // watchdog still terminates the host on DAW exit.
                match AssignProcessToJobObject(handle, process) {
                    Ok(()) => {
                        eprintln!("[plugin-bridge] assigned host pid={pid} to job_object ok=true")
                    }
                    Err(error) => eprintln!(
                        "[plugin-bridge] assigned host pid={pid} to job_object ok=false \
                         error={error} (process may already be in another job)"
                    ),
                }
            }
        }
    }

    impl Drop for PluginHostJob {
        fn drop(&mut self) {
            if let Some(handle) = self.handle {
                unsafe {
                    let _ = CloseHandle(handle);
                }
            }
        }
    }
}

#[cfg(windows)]
use job::PluginHostJob;
