//! Windows Job Object for plugin-host child processes.

use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
};

/// Job object that terminates assigned plugin-host children when the handle closes.
pub struct PluginHostJob {
    /// `None` when creation failed — spawn continues with parent-watchdog fallback.
    handle: Option<HANDLE>,
}

unsafe impl Send for PluginHostJob {}
unsafe impl Sync for PluginHostJob {}

impl PluginHostJob {
    pub fn new() -> Self {
        unsafe {
            let job = match CreateJobObjectW(None, PCWSTR::null()) {
                Ok(job) => job,
                Err(error) => {
                    eprintln!(
                        "[PluginHost] job_object create failed error={error}; \
                         hosts fall back to parent-watchdog cleanup"
                    );
                    return Self { handle: None };
                }
            };
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // KILL_ON_JOB_CLOSE: the host process is terminated when the main app
            // exits (no orphan hosts). SILENT_BREAKAWAY_OK: child processes the
            // host spawns are NOT confined to this job. Browser/Chromium-backed
            // plugin editors (CEF/WebView2) launch sandboxed helper subprocesses
            // that create their OWN nested sandbox job objects; trapping them in
            // a restrictive parent job can make the editor's `attached()` hang
            // forever while it waits for a renderer that never finishes starting.
            // Breakaway lets the plugin's browser subprocesses run normally. The
            // host itself stays in the job (it is assigned directly), so cleanup
            // is unaffected. Generic — no plug-in/vendor branch.
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
            if let Err(error) = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) {
                eprintln!(
                    "[PluginHost] job_object set_information failed error={error}; \
                     closing handle"
                );
                let _ = CloseHandle(job);
                return Self { handle: None };
            }
            eprintln!(
                "[PluginHost] job_object created kill_on_job_close=true silent_breakaway_ok=true"
            );
            Self { handle: Some(job) }
        }
    }

    pub fn assign_child(&self, child: &Child) {
        let pid = child.id();
        let Some(handle) = self.handle else {
            eprintln!("[PluginHost] assign pid={pid} skipped reason=no_job_object");
            return;
        };
        unsafe {
            let process = HANDLE(child.as_raw_handle() as *mut _);
            match AssignProcessToJobObject(handle, process) {
                Ok(()) => {
                    eprintln!("[PluginHost] assigned pid={pid} to job_object ok=true")
                }
                Err(error) => eprintln!(
                    "[PluginHost] assigned pid={pid} to job_object ok=false error={error} \
                     (process may already be in another job)"
                ),
            }
        }
    }
}

impl Drop for PluginHostJob {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            eprintln!("[PluginHost] job_object handle closed");
        }
    }
}

#[cfg(windows)]
pub fn terminate_process_pid(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    unsafe {
        let Ok(process) = OpenProcess(PROCESS_TERMINATE, false, pid) else {
            eprintln!("[PluginHost] terminate pid={pid} failed reason=open_process");
            return false;
        };
        let ok = TerminateProcess(process, 1).is_ok();
        let _ = CloseHandle(process);
        if ok {
            eprintln!("[PluginHost] forced termination pid={pid} ok=true");
        } else {
            eprintln!("[PluginHost] forced termination pid={pid} ok=false");
        }
        ok
    }
}
