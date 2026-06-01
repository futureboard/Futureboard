use gpui::{px, size, App, Bounds, Context, Point, Window};

use std::path::PathBuf;
use std::sync::Arc;

use crate::components::message_box_dialog::{
    open_message_box_window, MessageBoxKind, MessageBoxOptions, MessageBoxResult,
};
use crate::components::project_switcher::ProjectSwitcherState;
use crate::components::timeline::timeline_state::{
    self, CreateTrackOptions, TimelineState, TrackType,
};
use crate::project::{
    apply_to_timeline, io::load_project, io::save_project, now_secs, FutureboardProject,
    ProjectTemplate,
};

use super::StudioLayout;

/// A project-lifecycle action that must be guarded by the unsaved-changes
/// prompt. All four entry points (New / Open / Close / Quit) funnel through
/// [`StudioLayout::guard_dirty_then`] so they share one dirty-project guard.
#[derive(Debug, Clone, Copy)]
pub(super) enum LifecycleAction {
    /// Replace the current project with a fresh empty workspace.
    NewProject,
    /// Show the Open Project file picker (replaces the current project).
    OpenProject,
    /// Unload the session and return to the Welcome window.
    CloseProject,
    /// Quit the whole application.
    Quit,
}

fn default_owner_bounds() -> Bounds<gpui::Pixels> {
    Bounds {
        origin: Point::default(),
        size: size(px(1400.0), px(900.0)),
    }
}
impl StudioLayout {
    pub(super) fn reset_project(&mut self, cx: &mut Context<Self>) {
        self.project_path = None;
        self.project_folder = None;
        self.file_browser.set_project_folder(None);
        self.project_switcher = ProjectSwitcherState::default();
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state = TimelineState::default();
            cx.notify();
        });
    }

    // ── New project (no wizard) ────────────────────────────────────────────────

    /// Enter a fresh, empty, *unsaved* workspace. Replaces the old Project
    /// Wizard modal: there is no dialog, no folder is created, and nothing is
    /// written to disk until the user saves. The studio simply resets to a
    /// blank arrangement that is marked dirty/unsaved.
    pub fn new_empty_project(&mut self, cx: &mut Context<Self>) {
        self.reset_project(cx);
        self.project_switcher.current_project.name = "Untitled Project".to_string();
        self.project_switcher.current_project.path = None;
        self.project_switcher.current_project.is_dirty = false;
        self.project_switcher.current_project.subtitle = "New project".to_string();
        self.mark_engine_media_dirty();
        self.schedule_audio_project_sync(cx, true, "new_empty_project");
        cx.notify();
    }

    /// Create a new unsaved workspace pre-populated from a `ProjectTemplate`.
    /// Like `new_empty_project`, this stays entirely in memory — the user saves
    /// when ready. Sample rate follows the current app defaults.
    ///
    /// TODO: richer template presets (default inserts, sends, routing, master
    /// chain) once the template/preset system lands. For now templates only set
    /// tempo, time signature, and an initial track layout.
    pub fn new_project_from_template(&mut self, template: ProjectTemplate, cx: &mut Context<Self>) {
        self.reset_project(cx);

        let (ts_num, ts_den) = template.time_signature();
        let audio_count = template.audio_tracks();
        let midi_count = template.midi_tracks();
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.bpm = template.default_bpm();
            timeline.state.time_signature_num = ts_num;
            timeline.state.time_signature_den = ts_den;
            for i in 0..audio_count {
                let color = timeline.state.track_color_for_index(i as usize);
                timeline.state.create_track(CreateTrackOptions {
                    track_type: TrackType::Audio,
                    name: format!("Audio {}", i + 1),
                    color,
                    volume: timeline_state::volume::db_to_norm(0.0),
                    pan: 0.0,
                    armed: false,
                    input_monitor: false,
                });
            }
            for i in 0..midi_count {
                let color = timeline
                    .state
                    .track_color_for_index((audio_count + i) as usize);
                timeline.state.create_track(CreateTrackOptions {
                    track_type: TrackType::Midi,
                    name: format!("MIDI {}", i + 1),
                    color,
                    volume: timeline_state::volume::db_to_norm(0.0),
                    pan: 0.0,
                    armed: false,
                    input_monitor: false,
                });
            }
            cx.notify();
        });

        self.project_switcher.current_project.name =
            format!("Untitled {} Project", template.label());
        self.project_switcher.current_project.path = None;
        self.project_switcher.current_project.is_dirty = true;
        self.project_switcher.current_project.subtitle = "Unsaved changes".to_string();
        self.mark_engine_media_dirty();
        self.schedule_audio_project_sync(cx, true, "new_template_project");
        cx.notify();
    }

    // ── Unsaved-changes guard (shared by New / Open / Close / Quit) ─────────────

    /// Request to quit the whole application, going through the unsaved-changes
    /// guard first. Used by the WCO / OS window close button.
    pub fn request_quit(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        self.guard_dirty_then(LifecycleAction::Quit, owner_bounds, cx);
    }

    /// Shared dirty-project guard. If the current project has no unsaved
    /// changes, runs `action` immediately. Otherwise shows the Save / Don't
    /// Save / Cancel message box and only runs `action` once the user confirms
    /// (Save must also succeed). The project is never unloaded — and transport
    /// is never stopped — before the user answers.
    pub(super) fn guard_dirty_then(
        &mut self,
        action: LifecycleAction,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if !self.project_switcher.current_project.is_dirty {
            self.run_lifecycle_action(action, cx);
            return;
        }

        // A guard is already on screen — focus it instead of stacking another.
        if let Some(handle) = self.unsaved_guard_window.clone() {
            if handle
                .update(cx, |_mb, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.unsaved_guard_window = None;
        }

        let owner_bounds = owner_bounds.unwrap_or_else(default_owner_bounds);
        let options = MessageBoxOptions {
            kind: MessageBoxKind::Warning,
            title: "Save Changes?".to_string(),
            message: "This project has unsaved changes. Do you want to save before closing it?"
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
                let _ = owner.update(cx, |this, cx| {
                    this.unsaved_guard_window = None;
                    match result.response {
                        0 => this.save_project_then(action, cx),    // Save
                        1 => this.run_lifecycle_action(action, cx), // Don't Save
                        _ => {}                                     // Cancel / Esc / close
                    }
                });
            });

        match open_message_box_window(owner_bounds, options, on_response, cx) {
            Ok(handle) => self.unsaved_guard_window = Some(handle),
            Err(err) => {
                // The native message box is Windows-only. Elsewhere (or on
                // failure) fall back to proceeding without saving rather than
                // trapping the user — Windows is the supported target.
                eprintln!("[project] unsaved-changes dialog unavailable: {err}");
                self.run_lifecycle_action(action, cx);
            }
        }
    }

    /// Run a guarded lifecycle action *after* the dirty-project guard has been
    /// satisfied (not dirty, Don't Save, or a successful Save).
    fn run_lifecycle_action(&mut self, action: LifecycleAction, cx: &mut Context<Self>) {
        match action {
            LifecycleAction::NewProject => self.new_empty_project(cx),
            LifecycleAction::OpenProject => self.cmd_open_project(cx),
            LifecycleAction::CloseProject => self.do_close_project(cx),
            LifecycleAction::Quit => self.do_quit(cx),
        }
    }

    /// Save the current project, then run `action` only if the save succeeds.
    /// A project that has never been saved routes through Save As; cancelling
    /// the Save As dialog aborts the action (the project stays open).
    fn save_project_then(&mut self, action: LifecycleAction, cx: &mut Context<Self>) {
        if let Some(path) = self.project_path.clone() {
            if self.do_save_project(&path, cx) {
                self.run_lifecycle_action(action, cx);
            }
            // Save failed → keep the project open; the status bar shows the error.
            return;
        }

        // Never-saved project → Save As. Continue only on a successful save.
        let default_dir = crate::project::io::default_projects_dir();
        let name = self.project_switcher.current_project.name.clone();
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Save Project As")
                .set_directory(&default_dir)
                .set_file_name(&format!(
                    "{}.{}",
                    crate::project::io::sanitize_project_name(&name),
                    crate::project::io::PROJECT_FILE_EXT
                ))
                .add_filter(
                    "Futureboard Project",
                    &[crate::project::io::PROJECT_FILE_EXT],
                )
                .save_file()
                .await;
            if let Some(handle) = result {
                let path = handle.path().to_path_buf();
                let _ = entity.update(cx, |this, cx| {
                    if this.do_save_project(&path, cx) {
                        this.project_path = Some(path);
                        this.run_lifecycle_action(action, cx);
                    }
                });
            }
            // None → user cancelled Save As → abort the action (project stays open).
        })
        .detach();
    }

    // ── Close / quit (post-confirmation) ────────────────────────────────────────

    /// Quit the whole application. Runs only after the unsaved-changes guard is
    /// satisfied. Transport is stopped here so it stops *after* confirmation.
    fn do_quit(&mut self, cx: &mut Context<Self>) {
        self.stop_native_playback(cx);
        cx.quit();
    }

    /// Unload the current project/session and return the app to the Welcome
    /// screen, keeping the application running. Runs only after the
    /// unsaved-changes guard is satisfied. This is *not* an app quit — the
    /// WCO / OS window close button handles quitting via [`Self::request_quit`].
    fn do_close_project(&mut self, cx: &mut Context<Self>) {
        // 1. Stop transport (safe even when idle — engine pauses). Only reached
        //    after the user has confirmed the close.
        self.stop_native_playback(cx);

        // 2. Clear project-specific editor/timeline/mixer state.
        self.reset_project(cx);
        self.mark_engine_media_dirty();
        self.schedule_audio_project_sync(cx, true, "close_project");

        // 3. Return to Welcome by opening a fresh welcome window via the
        //    app-level hook. Opening a new window from inside this update is
        //    safe; it also guarantees a window is always present so the app
        //    never quits during the handoff.
        if let Some(request_welcome) = self.on_request_welcome.clone() {
            request_welcome(cx);
        }

        // 4. Close this workspace window. `do_close_project` runs inside this
        //    window's own entity update, so removing it synchronously would
        //    re-enter the active lease. Defer to the next cycle. `remove_window`
        //    destroys the window directly (no WM_CLOSE), so the WCO
        //    `on_window_should_close` guard does not re-fire here.
        if let Some(handle) = self.self_window.take() {
            cx.spawn(async move |_this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(0))
                    .await;
                let _ = handle.update(cx, |_studio, window, _cx| window.remove_window());
            })
            .detach();
        }
        cx.notify();
    }

    // ── Save / load ───────────────────────────────────────────────────────────

    pub(super) fn mark_dirty(&mut self) {
        self.project_switcher.current_project.is_dirty = true;
        self.project_switcher.current_project.subtitle = "Unsaved changes".to_string();
        self.mark_engine_project_dirty();
    }

    pub(super) fn cmd_save_project(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.project_path.clone() {
            self.do_save_project(&path, cx);
        } else {
            self.cmd_save_project_as(cx);
        }
    }

    pub(super) fn cmd_save_project_as(&mut self, cx: &mut Context<Self>) {
        let default_dir = self
            .project_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(crate::project::io::default_projects_dir);
        let name = self.project_switcher.current_project.name.clone();
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Save Project As")
                .set_directory(&default_dir)
                .set_file_name(&format!(
                    "{}.{}",
                    crate::project::io::sanitize_project_name(&name),
                    crate::project::io::PROJECT_FILE_EXT
                ))
                .add_filter(
                    "Futureboard Project",
                    &[crate::project::io::PROJECT_FILE_EXT],
                )
                .save_file()
                .await;
            if let Some(handle) = result {
                let path = handle.path().to_path_buf();
                let _ = entity.update(cx, |this, cx| {
                    this.do_save_project(&path, cx);
                    this.project_path = Some(path);
                });
            }
        })
        .detach();
    }

    pub(super) fn cmd_save_project_copy(&mut self, cx: &mut Context<Self>) {
        let default_dir = self
            .project_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(crate::project::io::default_projects_dir);
        let name = self.project_switcher.current_project.name.clone();
        let entity = cx.entity().clone();
        let tl_state = self.timeline.read(cx).state.clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Save Copy")
                .set_directory(&default_dir)
                .set_file_name(&format!(
                    "{} Copy.{}",
                    crate::project::io::sanitize_project_name(&name),
                    crate::project::io::PROJECT_FILE_EXT
                ))
                .add_filter(
                    "Futureboard Project",
                    &[crate::project::io::PROJECT_FILE_EXT],
                )
                .save_file()
                .await;
            if let Some(handle) = result {
                let path = handle.path().to_path_buf();
                let mut project = FutureboardProject::from(&tl_state);
                let _ = entity.update(cx, |_this, _cx| {
                    if let Err(e) = save_project(&mut project, &path) {
                        eprintln!("[project] save copy failed: {e}");
                    }
                });
            }
        })
        .detach();
    }

    /// Persist the project to `path`. Returns `true` on success so callers
    /// (notably the unsaved-changes guard) can decide whether to continue.
    pub(super) fn do_save_project(&mut self, path: &PathBuf, cx: &mut Context<Self>) -> bool {
        let tl_state = self.timeline.read(cx).state.clone();
        let mut project = FutureboardProject::from(&tl_state);
        project.name = self.project_switcher.current_project.name.clone();
        match save_project(&mut project, path) {
            Ok(()) => {
                self.project_switcher.current_project.is_dirty = false;
                self.project_switcher.current_project.subtitle = "Saved".to_string();
                self.project_switcher.current_project.path = Some(path.clone());
                self.recent_projects
                    .push(&project.name, path.clone(), now_secs());
                self.sync_recent_to_switcher();
                true
            }
            Err(e) => {
                eprintln!("[project] save failed: {e}");
                self.project_switcher.current_project.subtitle = format!("Save failed: {e}");
                false
            }
        }
    }

    pub(super) fn cmd_open_project(&mut self, cx: &mut Context<Self>) {
        let default_dir = self
            .project_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(crate::project::io::default_projects_dir);
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Open Project")
                .set_directory(&default_dir)
                .add_filter(
                    "Futureboard Project",
                    &[crate::project::io::PROJECT_FILE_EXT],
                )
                .pick_file()
                .await;
            if let Some(handle) = result {
                let path = handle.path().to_path_buf();
                let _ = entity.update(cx, |this, cx| {
                    this.load_project_from_path(path, cx);
                });
            }
        })
        .detach();
    }

    pub fn load_project_from_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match load_project(&path) {
            Ok(project) => {
                let _ = self.timeline.update(cx, |timeline, _cx| {
                    apply_to_timeline(&project, &mut timeline.state);
                });
                self.project_path = Some(path.clone());
                self.project_folder = path.parent().map(|p| p.to_path_buf());
                self.file_browser
                    .set_project_folder(self.project_folder.clone());
                self.project_switcher.current_project.name = project.name.clone();
                self.project_switcher.current_project.path = Some(path.clone());
                self.project_switcher.current_project.is_dirty = false;
                self.project_switcher.current_project.subtitle = "Opened".to_string();
                self.recent_projects.push(&project.name, path, now_secs());
                self.sync_recent_to_switcher();
                self.mark_engine_media_dirty();
                self.schedule_audio_project_sync(cx, true, "project_loaded");
                cx.notify();
            }
            Err(e) => {
                eprintln!("[project] load failed: {e}");
            }
        }
    }

    pub(super) fn cmd_open_recent_project(&mut self, cx: &mut Context<Self>) {
        self.recent_projects.refresh_missing();
        let idx = self.project_switcher.selected_index;
        if idx == 0 {
            return;
        }
        let path = self
            .recent_projects
            .entries()
            .get(idx.saturating_sub(1))
            .map(|e| e.path.clone());
        if let Some(path) = path {
            self.load_project_from_path(path, cx);
        }
    }

    pub(super) fn cmd_reveal_project_folder(&self, _cx: &mut Context<Self>) {
        #[cfg(target_os = "windows")]
        if let Some(folder) = &self.project_folder {
            let _ = std::process::Command::new("explorer").arg(folder).spawn();
        }
        #[cfg(target_os = "macos")]
        if let Some(folder) = &self.project_folder {
            let _ = std::process::Command::new("open").arg(folder).spawn();
        }
        #[cfg(target_os = "linux")]
        if let Some(folder) = &self.project_folder {
            let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
        }
    }

    pub(super) fn sync_recent_to_switcher(&mut self) {
        self.recent_projects.refresh_missing();
        self.project_switcher.recent_projects = self
            .recent_projects
            .entries()
            .iter()
            .map(|e| crate::components::project_switcher::ProjectSummary {
                name: e.name.clone(),
                path: Some(e.path.clone()),
                is_current: self.project_path.as_ref() == Some(&e.path),
                is_dirty: false,
                subtitle: if e.missing {
                    "Missing".to_string()
                } else {
                    String::new()
                },
            })
            .collect();
    }
}
