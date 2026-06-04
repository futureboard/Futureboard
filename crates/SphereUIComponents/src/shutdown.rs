//! Process-wide shutdown gate and optional shutdown tracing.
//!
//! Set `FUTUREBOARD_SHUTDOWN_DEBUG=1` to log close/shutdown phases.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static GLOBAL: OnceLock<ShutdownState> = OnceLock::new();

/// Shared flag: once true, UI callbacks must not `notify` or touch GPUI state.
pub struct ShutdownState {
    shutting_down: AtomicBool,
}

impl ShutdownState {
    pub fn global() -> &'static Self {
        GLOBAL.get_or_init(|| Self {
            shutting_down: AtomicBool::new(false),
        })
    }

    /// Returns `true` only on the first transition to shutting down.
    pub fn begin(&self) -> bool {
        !self.shutting_down.swap(true, Ordering::SeqCst)
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }
}

pub fn shutdown_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_SHUTDOWN_DEBUG").is_some())
}

pub fn log(msg: &str) {
    if shutdown_debug_enabled() {
        eprintln!("[shutdown] {msg}");
    }
}
