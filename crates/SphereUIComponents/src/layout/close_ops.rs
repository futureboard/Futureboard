//! Window / project close confirmation and ordered studio teardown.

use gpui::{App, Bounds, Context, Pixels, Window, WindowId};

use std::sync::Arc;

use crate::components::message_box_dialog::{
    open_message_box_window, MessageBoxKind, MessageBoxOptions, MessageBoxResult,
};
use crate::shutdown::{self, ShutdownState};

use super::project_ops::LifecycleAction;
use super::StudioLayout;

/// User-initiated close that may require an unsaved-changes prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingCloseAction {
    /// Unload the session and return to Welcome (File → Close Project).
    CloseProject,
    /// Exit the application (File → Quit, window X, Alt+F4).
    QuitApp,
    /// Platform window close for a specific handle (studio main window).
    CloseWindow(WindowId),
}

impl PendingCloseAction {
    fn label(self) -> &'static str {
        match self {
            Self::CloseProject => "close_project",
            Self::QuitApp => "quit_app",
            Self::CloseWindow(_) => "close_window",
        }
    }
}

impl StudioLayout {
    /// Entry point for OS window close (X), mapped to app quit on the studio window.
    pub fn request_close(
        &mut self,
        action: PendingCloseAction,
        owner_bounds: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if ShutdownState::global().is_shutting_down() {
            shutdown::log("request_close ignored — already shutting down");
            return;
        }
        let dirty = self.project_switcher.current_project.is_dirty;
        shutdown::log(&format!(
            "close requested action={} dirty={dirty}",
            action.label()
        ));
        self.pending_close_action = Some(action);
        if !dirty {
            self.perform_pending_close(cx);
            return;
        }
        self.show_unsaved_changes_dialog(owner_bounds, cx);
    }

    /// Legacy alias used by the native shell WCO hook.
    pub fn request_quit(&mut self, owner_bounds: Option<Bounds<Pixels>>, cx: &mut Context<Self>) {
        self.request_close(PendingCloseAction::QuitApp, owner_bounds, cx);
    }

    pub(super) fn guard_dirty_then_lifecycle(
        &mut self,
        action: LifecycleAction,
        owner_bounds: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if ShutdownState::global().is_shutting_down() {
            return;
        }
        let dirty = self.project_switcher.current_project.is_dirty;
        shutdown::log(&format!("lifecycle guard action={action:?} dirty={dirty}"));
        if !dirty {
            self.run_lifecycle_action(action, cx);
            return;
        }
        self.pending_lifecycle_action = Some(action);
        self.show_unsaved_changes_dialog(owner_bounds, cx);
    }

    fn show_unsaved_changes_dialog(
        &mut self,
        owner_bounds: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.unsaved_guard_window.clone() {
            if handle
                .update(cx, |_mb, window, _cx| window.activate_window())
                .is_ok()
            {
                shutdown::log("unsaved dialog already open — focused");
                return;
            }
            self.unsaved_guard_window = None;
        }

        let owner_bounds = crate::window_position::resolve_owner_bounds_with_preferred(
            owner_bounds,
            self.studio_window_bounds(cx),
            cx,
        );
        let options = MessageBoxOptions {
            kind: MessageBoxKind::Warning,
            title: "Save Changes?".to_string(),
            message: "This project has unsaved changes. Do you want to save before closing?"
                .to_string(),
            detail: None,
            buttons: vec![
                "Save".to_string(),
                "Don't Save".to_string(),
                "Cancel".to_string(),
            ],
            default_id: 0,
            cancel_id: Some(2),
        };

        let owner = cx.entity().clone();
        let on_response: Arc<dyn Fn(MessageBoxResult, &mut Window, &mut App) + Send + Sync> =
            Arc::new(move |result, _window, cx| {
                if ShutdownState::global().is_shutting_down() {
                    shutdown::log("unsaved dialog response ignored — shutting down");
                    return;
                }
                let _ = owner.update(cx, |this, cx| {
                    this.unsaved_guard_window = None;
                    match result.response {
                        0 => {
                            shutdown::log("unsaved dialog: Save");
                            this.save_then_pending(cx);
                        }
                        1 => {
                            shutdown::log("unsaved dialog: Don't Save");
                            this.perform_pending_after_guard(cx);
                        }
                        _ => {
                            shutdown::log("unsaved dialog: Cancel");
                            this.pending_close_action = None;
                            this.pending_lifecycle_action = None;
                        }
                    }
                });
            });

        match open_message_box_window(owner_bounds, options, on_response, cx) {
            Ok(handle) => {
                self.unsaved_guard_window = Some(handle);
                shutdown::log("unsaved dialog shown");
            }
            Err(err) => {
                eprintln!("[project] unsaved-changes dialog unavailable: {err}");
                self.pending_close_action = None;
                self.pending_lifecycle_action = None;
                shutdown::log("unsaved dialog failed — close aborted (stay open)");
            }
        }
    }

    fn save_then_pending(&mut self, cx: &mut Context<Self>) {
        if let Some(action) = self.pending_lifecycle_action.take() {
            self.save_lifecycle_then(action, cx);
            return;
        }
        if self.pending_close_action.is_some() {
            self.save_close_then(cx);
        }
    }

    fn perform_pending_after_guard(&mut self, cx: &mut Context<Self>) {
        if self.pending_lifecycle_action.is_some() {
            let action = self.pending_lifecycle_action.take().expect("checked");
            self.run_lifecycle_action(action, cx);
            return;
        }
        self.perform_pending_close(cx);
    }

    pub(super) fn perform_pending_close(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.pending_close_action.take() else {
            return;
        };
        shutdown::log(&format!("perform_pending_close action={}", action.label()));
        match action {
            PendingCloseAction::CloseProject => self.do_close_project(cx),
            PendingCloseAction::QuitApp | PendingCloseAction::CloseWindow(_) => {
                self.do_quit(cx);
            }
        }
    }

    /// Ordered teardown before GPUI / TLS destruction. Idempotent.
    pub(super) fn shutdown_studio(&mut self, cx: &mut Context<Self>) {
        if !ShutdownState::global().begin() {
            shutdown::log("shutdown_studio skipped — already began");
            return;
        }
        shutdown::log("shutdown_studio begin");

        shutdown::log("phase: stop transport");
        self.stop_native_playback(cx);

        shutdown::log("phase: audio engine shutdown");
        if let Some(engine) = self.audio_engine.as_mut() {
            engine.shutdown();
        }

        shutdown::log("phase: plugin editors");
        self.shutdown_plugin_editors(cx);

        shutdown::log("shutdown_studio end");
    }

    pub(super) fn do_quit(&mut self, cx: &mut Context<Self>) {
        shutdown::log("do_quit");
        self.shutdown_studio(cx);
        shutdown::log("phase: cx.quit");
        cx.quit();
    }
}
