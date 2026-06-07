//! Windows Job Object and coordinated shutdown for `FutureboardPluginHost-x64.exe`
//! child processes. Keeps plugin hosts owned by the main app lifecycle so they
//! cannot outlive `futureboard_native.exe`.

use std::collections::HashMap;
use std::process::Child;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::plugin_host_client::PluginHostClient;

/// Tracks every spawned plugin-host pid and owns the Windows job object that
/// terminates children when the main process exits.
pub struct BridgeHostManager {
    #[cfg(windows)]
    job: PluginHostJob,
    hosts: Mutex<HashMap<u32, ()>>,
}

static MANAGER: OnceLock<BridgeHostManager> = OnceLock::new();

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
        self.hosts.lock().expect("bridge host lock").insert(pid, ());
        #[cfg(windows)]
        self.job.assign_child(child);
    }

    pub fn host_count(&self) -> usize {
        self.hosts.lock().expect("bridge host lock").len()
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

/// Gracefully shut down one live [`PluginHostClient`], waiting briefly before
/// force-terminating stragglers. Idempotent.
pub fn shutdown_host_client(client: &mut PluginHostClient) {
    if client.shutdown_started {
        return;
    }
    client.shutdown_started = true;
    let pid = client.pid();
    eprintln!("[plugin-bridge] sending Shutdown pid={pid}");
    let _ = client.shutdown();

    let deadline = Instant::now() + Duration::from_millis(1500);
    loop {
        match client.has_exited() {
            Some(true) => {
                eprintln!("[plugin-bridge] host exited pid={pid}");
                break;
            }
            Some(false) if Instant::now() >= deadline => {
                eprintln!("[plugin-bridge] force terminate pid={pid} reason=timeout");
                let _ = client.force_kill();
                let _ = client.wait_for_exit();
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
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub struct PluginHostJob {
        handle: HANDLE,
    }

    // Job object handles are process-wide kernel objects; safe to share across
    // threads for assign-on-spawn / close-on-exit.
    unsafe impl Send for PluginHostJob {}
    unsafe impl Sync for PluginHostJob {}

    impl PluginHostJob {
        pub fn new() -> Self {
            unsafe {
                let job = CreateJobObjectW(None, PCWSTR::null())
                    .expect("CreateJobObjectW failed for plugin host job");
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
                .expect("SetInformationJobObject failed for plugin host job");
                eprintln!("[plugin-bridge] job_object created kill_on_close=true");
                Self { handle: job }
            }
        }

        pub fn assign_child(&self, child: &Child) {
            let pid = child.id();
            unsafe {
                let raw = child.as_raw_handle();
                let process = HANDLE(raw as *mut _);
                let ok = AssignProcessToJobObject(self.handle, process).is_ok();
                eprintln!("[plugin-bridge] assigned host pid={pid} to job_object ok={ok}");
            }
        }
    }

    impl Drop for PluginHostJob {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(windows)]
use job::PluginHostJob;
