//! Plugin host process logging — file redirect when console is hidden.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static LOG_FILE: OnceLock<Mutex<Option<File>>> = OnceLock::new();

/// Whether the host should show a visible console (debug escape hatch).
pub fn host_console_enabled() -> bool {
    std::env::var_os("FUTUREBOARD_PLUGIN_HOST_CONSOLE").is_some()
        || std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some()
}

/// Default directory for plugin-host log files.
pub fn default_log_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("logs").join("plugin-host")))
        .unwrap_or_else(|| PathBuf::from("logs").join("plugin-host"))
}

/// Open `logs/plugin-host/<pid>.log` and redirect stderr when console is hidden.
pub fn init_host_logging() -> Option<PathBuf> {
    if host_console_enabled() {
        eprintln!("[PluginHost] logging=console reason=FUTUREBOARD_PLUGIN_HOST_CONSOLE");
        return None;
    }

    let pid = std::process::id();
    let log_dir = default_log_dir();

    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("[PluginHost] log_dir_create_failed path={log_dir:?} err={e}");
        return None;
    }

    let log_path = log_dir.join(format!("{pid}.log"));
    let file = OpenOptions::new().create(true).append(true).open(&log_path);

    match file {
        Ok(mut f) => {
            let _ = writeln!(f, "--- plugin host session pid={pid} ---");
            LOG_FILE.get_or_init(|| Mutex::new(Some(f)));
            eprintln!("[PluginHost] logging=file path={}", log_path.display());
            Some(log_path)
        }
        Err(e) => {
            eprintln!(
                "[PluginHost] log_open_failed path={} err={e}",
                log_path.display()
            );
            None
        }
    }
}

/// Write a lifecycle line to the host log file (and stderr when console enabled).
pub fn host_log(msg: &str) {
    if host_console_enabled() {
        eprintln!("{msg}");
        return;
    }
    if let Some(mutex) = LOG_FILE.get() {
        if let Ok(mut guard) = mutex.lock() {
            if let Some(ref mut f) = *guard {
                let _ = writeln!(f, "{msg}");
                let _ = f.flush();
            }
        }
    }
}

/// Log CPU/OS diagnostics at host startup.
pub fn log_startup_environment() {
    host_log(&format!(
        "[PluginHost] arch={} pid={}",
        std::env::consts::ARCH,
        std::process::id()
    ));
    if let Ok(exe) = std::env::current_exe() {
        host_log(&format!("[PluginHost] exe={}", exe.display()));
    }
    host_log(&format!(
        "[PluginHost] build={}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    ));

    #[cfg(target_arch = "x86_64")]
    {
        host_log(&format!(
            "[PluginHost] cpu_sse2={}",
            std::arch::is_x86_feature_detected!("sse2")
        ));
        host_log(&format!(
            "[PluginHost] cpu_sse4.1={}",
            std::arch::is_x86_feature_detected!("sse4.1")
        ));
        host_log(&format!(
            "[PluginHost] cpu_avx={}",
            std::arch::is_x86_feature_detected!("avx")
        ));
        host_log(&format!(
            "[PluginHost] cpu_avx2={}",
            std::arch::is_x86_feature_detected!("avx2")
        ));
        host_log(&format!(
            "[PluginHost] cpu_fma={}",
            std::arch::is_x86_feature_detected!("fma")
        ));
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(v) = std::env::var("OS") {
            host_log(&format!("[PluginHost] os={v}"));
        }
    }
}
