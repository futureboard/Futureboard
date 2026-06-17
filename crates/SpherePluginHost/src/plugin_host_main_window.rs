//! Main DAW window HWND published by the studio shell for plugin-host spawn
//! and owned editor popups.

use std::sync::atomic::{AtomicIsize, Ordering};

static MAIN_WINDOW_HWND: AtomicIsize = AtomicIsize::new(0);

/// Publish the native HWND of the primary Futureboard studio window.
pub fn set_main_window_hwnd(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let previous = MAIN_WINDOW_HWND.swap(hwnd, Ordering::SeqCst);
    if previous != hwnd {
        eprintln!("[PluginHost] main_hwnd published hwnd=0x{hwnd:x}");
    }
}

/// Latest published main-window HWND, if any.
pub fn main_window_hwnd() -> Option<isize> {
    let hwnd = MAIN_WINDOW_HWND.load(Ordering::SeqCst);
    if hwnd == 0 { None } else { Some(hwnd) }
}

/// Clear the published HWND when the studio window closes.
pub fn clear_main_window_hwnd() {
    if MAIN_WINDOW_HWND.swap(0, Ordering::SeqCst) != 0 {
        eprintln!("[PluginHost] main_hwnd cleared");
    }
}
