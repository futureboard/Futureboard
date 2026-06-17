//! Window / teardown scope helpers for session switch and shutdown logging.

use crate::app_state::AppMode;

/// What a teardown operation is allowed to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeardownScope {
    /// Plugin instances, bridge hosts, audio graph session state.
    SessionRuntimeOnly,
    /// Editors, popouts, menus, switcher popover — not root shells.
    SessionTransientWindows,
    /// Application exit — may close all windows.
    FullAppShutdown,
}

/// Whether `remove_window` is safe during an in-studio project switch.
pub fn can_close_window_for_project_switch(
    is_studio_shell: bool,
    is_loading_shell: bool,
    live_shell_count: u32,
) -> bool {
    if is_studio_shell {
        eprintln!(
            "[WindowLifecycle] refused to close root/last window during project_switch reason=studio_shell"
        );
        return false;
    }
    if is_loading_shell && live_shell_count <= 1 {
        eprintln!(
            "[WindowLifecycle] refused to close root/last window during project_switch reason=loader_would_be_last"
        );
        return false;
    }
    if live_shell_count <= 1 {
        eprintln!(
            "[WindowLifecycle] refused to close root/last window during project_switch reason=last_window"
        );
        return false;
    }
    true
}

pub fn log_app_mode_change(from: AppMode, to: AppMode, reason: &str) {
    eprintln!(
        "[AppMode] {} -> {} reason={reason}",
        from.label(),
        to.label()
    );
}

pub fn log_shell_window_registry(
    stage: &str,
    studio_alive: bool,
    loader_alive: bool,
    app_mode: &str,
) {
    let root_alive = studio_alive;
    eprintln!(
        "[WindowRegistry] {stage} root_alive={root_alive} studio_alive={studio_alive} loader_alive={loader_alive} AppMode={app_mode}"
    );
}

pub fn log_cx_quit(reason: &str) {
    eprintln!("[WindowLifecycle] cx.quit requested reason={reason}");
}

pub fn log_remove_window(kind: &str, reason: &str) {
    eprintln!("[WindowLifecycle] remove_window kind={kind} reason={reason}");
}
