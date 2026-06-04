//! Optional tracing for transport Play freeze investigations.
//! Enable with `FUTUREBOARD_TRANSPORT_FREEZE_DEBUG=1`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static ENABLED: OnceLock<bool> = OnceLock::new();
static SEQ: AtomicU32 = AtomicU32::new(0);

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_TRANSPORT_FREEZE_DEBUG").is_some())
}

pub fn log(step: &str) {
    if !enabled() {
        return;
    }
    let n = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("[play-debug #{n:03}] {step}");
}

/// Reset the sequence counter at the start of each Play attempt.
pub fn reset_sequence() {
    if enabled() {
        SEQ.store(0, Ordering::Relaxed);
    }
}

pub struct PlayWatchdog {
    started: Instant,
}

impl PlayWatchdog {
    pub fn start() -> Option<Self> {
        enabled().then(|| {
            log("watchdog armed (500ms)");
            Self {
                started: Instant::now(),
            }
        })
    }

    pub fn check(&self, transport_playing: bool) {
        let elapsed_ms = self.started.elapsed().as_millis();
        log(&format!(
            "watchdog check elapsed_ms={elapsed_ms} transport_playing={transport_playing}"
        ));
        if !transport_playing {
            eprintln!(
                "[play-debug] WARNING: transport UI/engine still not playing {elapsed_ms}ms after Play handler returned"
            );
        }
    }
}
