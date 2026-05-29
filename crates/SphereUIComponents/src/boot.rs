//! Startup boot-phase logging.
//!
//! Cheap, opt-in tracing of the critical startup path so we can see *when* each
//! phase completes and confirm the main window is only shown after initial
//! layout is ready. Enable with `FUTUREBOARD_BOOT_DEBUG=1`.
//!
//! Phases (see `apps/native`):
//!   Phase 0 — process setup (env flags, panic hook, logging)
//!   Phase 1 — critical init (settings, audio engine handle, StudioLayout)
//!   Phase 2 — show window (only after the first frame is painted)
//!   Phase 3 — background init (audio refresh, plugin scan, indexer, …)

use std::sync::OnceLock;
use std::time::Instant;

fn boot_debug() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_BOOT_DEBUG").is_some())
}

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// Log a boot milestone with a monotonic `+Nms` offset from the first call.
/// No-op unless `FUTUREBOARD_BOOT_DEBUG=1`.
pub fn log(msg: &str) {
    if !boot_debug() {
        return;
    }
    let elapsed = start().elapsed().as_millis();
    eprintln!("[boot +{elapsed}ms] {msg}");
}
