use gpui::{px, size, Bounds, Context, Point};

use std::path::PathBuf;

use crate::components::project_switcher::ProjectSwitcherState;
use crate::components::timeline::timeline_state::{
    self, CreateTrackOptions, InputMonitorMode, TimelineState, TrackType,
};
use crate::project::{
    apply_to_timeline, io::create_project_folder, io::load_project, io::save_project, now_secs,
    ClipSource, FutureboardProject, ProjectCreateOptions, ProjectTemplate,
};

use super::StudioLayout;

/// A project-lifecycle action that must be guarded by the unsaved-changes
/// prompt (New / Open). Close / Quit use [`super::close_ops::PendingCloseAction`].
#[derive(Debug, Clone, Copy)]
pub(super) enum LifecycleAction {
    /// Replace the current project with a fresh empty workspace.
    NewProject,
    /// Show the Open Project file picker (replaces the current project).
    OpenProject,
}

#[derive(Debug, Clone, Copy)]
enum SaveThenAction {
    PendingClose,
    Lifecycle(LifecycleAction),
}

fn default_owner_bounds() -> Bounds<gpui::Pixels> {
    Bounds {
        origin: Point::default(),
        size: size(px(1400.0), px(900.0)),
    }
}

fn project_save_path_from_picker(path: PathBuf) -> PathBuf {
    let Some(parent) = path.parent() else {
        return path;
    };
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(crate::project::io::sanitize_project_name)
        .unwrap_or_else(|| "Untitled Project".to_string());
    let already_project_folder = parent
        .file_name()
        .and_then(|name| name.to_str())
        .map(|folder| folder == stem)
        .unwrap_or(false);
    if already_project_folder {
        path
    } else {
        parent
            .join(&stem)
            .join(format!("{stem}.{}", crate::project::io::PROJECT_FILE_EXT))
    }
}

impl StudioLayout {
    /// Resolve the directory new projects should default to. Reads the
    /// user-configured default project directory from settings (falling back to
    /// the platform default), then best-effort creates it so the save dialog
    /// opens somewhere that exists. Never panics on a bad/missing path.
    pub(super) fn default_projects_dir(&self, cx: &Context<Self>) -> PathBuf {
        let dir = cx
            .try_global::<crate::settings::GlobalSettingsModel>()
            .map(|g| g.0.read(cx).current.general.resolved_default_project_dir())
            .unwrap_or_else(crate::project::io::default_projects_dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// Authoritative project lifecycle state (Part G).
    pub fn project_state(&self) -> &crate::app_state::ProjectState {
        &self.project_state
    }

    /// OS window title derived from the lifecycle state + dirty bit, e.g.
    /// `"Untitled Project — Unsaved"` / `"My Song — Saved"` (Part H).
    pub fn window_title(&self) -> String {
        self.project_state.window_title(
            &self.project_switcher.current_project.name,
            self.project_switcher.current_project.is_dirty,
        )
    }

    pub(super) fn reset_project(&mut self, cx: &mut Context<Self>) {
        self.project_state = crate::app_state::ProjectState::NoProject;
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
        self.project_state = crate::app_state::ProjectState::UnsavedWorkspace;
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
                    input_monitor: InputMonitorMode::Off,
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
                    input_monitor: InputMonitorMode::Off,
                });
            }
            cx.notify();
        });

        self.project_state = crate::app_state::ProjectState::UnsavedWorkspace;
        self.project_switcher.current_project.name =
            format!("Untitled {} Project", template.label());
        self.project_switcher.current_project.path = None;
        self.project_switcher.current_project.is_dirty = true;
        self.project_switcher.current_project.subtitle = "Unsaved changes".to_string();
        self.mark_engine_media_dirty();
        self.schedule_audio_project_sync(cx, true, "new_template_project");
        cx.notify();
    }

    /// Create a named project from the Welcome screen. This is the first point
    /// where disk state is created: it makes the project folder tree, writes the
    /// `.fbproj`, updates recents, and leaves the workspace in `SavedProject`.
    pub fn create_saved_project_from_options(
        &mut self,
        options: ProjectCreateOptions,
        cx: &mut Context<Self>,
    ) {
        let safe_name = crate::project::io::sanitize_project_name(&options.name);
        let folder = match create_project_folder(&options.base_dir, &safe_name) {
            Ok(folder) => folder,
            Err(e) => {
                eprintln!("[project] create project folder failed: {e}");
                self.project_state = crate::app_state::ProjectState::Error(e.to_string());
                self.project_switcher.current_project.subtitle = format!("Create failed: {e}");
                cx.notify();
                return;
            }
        };
        let final_name = folder
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| safe_name.clone());
        let path = folder.join(format!(
            "{}.{}",
            final_name,
            crate::project::io::PROJECT_FILE_EXT
        ));

        if options.template == ProjectTemplate::Empty {
            self.new_empty_project(cx);
        } else {
            self.new_project_from_template(options.template, cx);
        }

        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.state.bpm = options.bpm;
            timeline.state.time_signature_num = options.time_signature_num;
            timeline.state.time_signature_den = options.time_signature_den;
            cx.notify();
        });

        self.project_switcher.current_project.name = final_name;
        self.project_switcher.current_project.path = Some(path.clone());
        self.project_folder = Some(folder.clone());
        self.file_browser.set_project_folder(Some(folder));

        if self.do_save_project(&path, cx) {
            self.project_path = Some(path);
            self.project_switcher.current_project.subtitle = "Saved".to_string();
        }
    }

    /// Run a guarded lifecycle action *after* the dirty-project guard has been
    /// satisfied (not dirty, Don't Save, or a successful Save).
    pub(super) fn run_lifecycle_action(&mut self, action: LifecycleAction, cx: &mut Context<Self>) {
        match action {
            LifecycleAction::NewProject => self.new_empty_project(cx),
            LifecycleAction::OpenProject => self.cmd_open_project(cx),
        }
    }

    /// Save, then run a pending close/quit action if save succeeds.
    pub(super) fn save_close_then(&mut self, cx: &mut Context<Self>) {
        self.save_then(SaveThenAction::PendingClose, cx);
    }

    /// Save, then run a pending New/Open lifecycle action if save succeeds.
    pub(super) fn save_lifecycle_then(&mut self, action: LifecycleAction, cx: &mut Context<Self>) {
        self.save_then(SaveThenAction::Lifecycle(action), cx);
    }

    fn save_then(&mut self, after_save: SaveThenAction, cx: &mut Context<Self>) {
        if let Some(path) = self.project_path.clone() {
            self.save_project_in_background_then(path, Some(after_save), cx);
            return;
        }

        let default_dir = self.default_projects_dir(cx);
        let name = self.project_switcher.current_project.name.clone();
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            if crate::shutdown::ShutdownState::global().is_shutting_down() {
                return;
            }
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
                    crate::project::io::SUPPORTED_PROJECT_FILE_EXTS,
                )
                .save_file()
                .await;
            if let Some(handle) = result {
                let path = project_save_path_from_picker(handle.path().to_path_buf());
                let _ = entity.update(cx, |this, cx| {
                    if crate::shutdown::ShutdownState::global().is_shutting_down() {
                        return;
                    }
                    this.save_project_in_background_then(path, Some(after_save), cx);
                });
            } else {
                let _ = entity.update(cx, |this, _cx| {
                    this.pending_close_action = None;
                    this.pending_lifecycle_action = None;
                });
            }
        })
        .detach();
    }

    fn apply_save_then(&mut self, after_save: SaveThenAction, cx: &mut Context<Self>) {
        match after_save {
            SaveThenAction::PendingClose => self.perform_pending_close(cx),
            SaveThenAction::Lifecycle(action) => self.run_lifecycle_action(action, cx),
        }
    }

    // ── Close project (post-confirmation) ───────────────────────────────────────

    /// Unload the current project/session and return the app to the Welcome
    /// screen, keeping the application running. Runs only after the
    /// unsaved-changes guard is satisfied. This is *not* an app quit — the
    /// WCO / OS window close button handles quitting via [`Self::request_quit`].
    pub(super) fn do_close_project(&mut self, cx: &mut Context<Self>) {
        if crate::shutdown::ShutdownState::global().is_shutting_down() {
            return;
        }
        // 1. Stop transport (safe even when idle — engine pauses). Only reached
        //    after the user has confirmed the close.
        self.stop_native_playback(cx);

        // 2. Clear project-specific editor/timeline/mixer state.
        self.reset_project(cx);
        if !crate::shutdown::ShutdownState::global().is_shutting_down() {
            self.mark_engine_media_dirty();
            self.schedule_audio_project_sync(cx, true, "close_project");
        }

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
                if crate::shutdown::ShutdownState::global().is_shutting_down() {
                    return;
                }
                let _ = handle.update(cx, |_studio, window, cx| {
                    crate::window_position::persist_studio_window_from_window(window, cx);
                    window.remove_window();
                });
            })
            .detach();
        }
        if !crate::shutdown::ShutdownState::global().is_shutting_down() {
            cx.notify();
        }
    }

    // ── Save / load ───────────────────────────────────────────────────────────

    pub(super) fn mark_dirty(&mut self) {
        self.project_switcher.current_project.is_dirty = true;
        self.project_switcher.current_project.subtitle = "Unsaved changes".to_string();
        self.mark_engine_project_dirty();
    }

    pub(super) fn cmd_save_project(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.project_path.clone() {
            self.save_project_in_background(path, cx);
        } else {
            self.cmd_save_project_as(cx);
        }
    }

    pub(super) fn cmd_save_project_as(&mut self, cx: &mut Context<Self>) {
        let default_dir = self
            .project_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| self.default_projects_dir(cx));
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
                    crate::project::io::SUPPORTED_PROJECT_FILE_EXTS,
                )
                .save_file()
                .await;
            if let Some(handle) = result {
                let path = project_save_path_from_picker(handle.path().to_path_buf());
                let _ = entity.update(cx, |this, cx| {
                    this.save_project_in_background(path, cx);
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
            .unwrap_or_else(|| self.default_projects_dir(cx));
        let name = self.project_switcher.current_project.name.clone();
        let entity = cx.entity().clone();
        let tl_state = self.timeline.read(cx).state.clone();
        let sample_rate = self.current_audio_sample_rate();
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
                    crate::project::io::SUPPORTED_PROJECT_FILE_EXTS,
                )
                .save_file()
                .await;
            if let Some(handle) = result {
                let path = handle.path().to_path_buf();
                let mut project = FutureboardProject::from(&tl_state);
                project.settings.sample_rate = sample_rate;
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
        let mut project = self.project_snapshot(cx);
        match save_project(&mut project, path) {
            Ok(()) => {
                self.finish_project_save(project, path.clone(), cx);
                true
            }
            Err(e) => {
                self.handle_project_save_error(e.to_string(), cx);
                false
            }
        }
    }

    fn save_project_in_background(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.save_project_in_background_then(path, None, cx);
    }

    fn save_project_in_background_then(
        &mut self,
        path: PathBuf,
        after_save: Option<SaveThenAction>,
        cx: &mut Context<Self>,
    ) {
        let mut project = self.project_snapshot(cx);
        self.project_switcher.current_project.subtitle = "Saving...".to_string();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let path_for_job = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { save_project(&mut project, &path_for_job).map(|_| project) })
                .await;
            let _ = this.update(cx, move |this, cx| match result {
                Ok(project) => {
                    this.finish_project_save(project, path, cx);
                    if let Some(after_save) = after_save {
                        this.apply_save_then(after_save, cx);
                    }
                }
                Err(e) => this.handle_project_save_error(e.to_string(), cx),
            });
        })
        .detach();
    }

    fn project_snapshot(&self, cx: &mut Context<Self>) -> FutureboardProject {
        let tl_state = self.timeline.read(cx).state.clone();
        let mut project = FutureboardProject::from(&tl_state);
        project.name = self.project_switcher.current_project.name.clone();
        project.settings.sample_rate = self.current_audio_sample_rate();
        project
    }

    fn finish_project_save(
        &mut self,
        project: FutureboardProject,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.sync_timeline_audio_paths_after_save(&project, &path, cx);
        self.project_state = crate::app_state::ProjectState::SavedProject { path: path.clone() };
        self.project_path = Some(path.clone());
        self.project_folder = path.parent().map(PathBuf::from);
        self.file_browser
            .set_project_folder(self.project_folder.clone());
        self.project_switcher.current_project.is_dirty = false;
        self.project_switcher.current_project.subtitle = "Saved".to_string();
        self.project_switcher.current_project.path = Some(path.clone());
        self.recent_projects.push(&project.name, path, now_secs());
        self.sync_recent_to_switcher();
        cx.notify();
    }

    fn handle_project_save_error(&mut self, error: String, cx: &mut Context<Self>) {
        eprintln!("[project] save failed: {error}");
        self.project_switcher.current_project.subtitle = format!("Save failed: {error}");
        cx.notify();
    }

    fn sync_timeline_audio_paths_after_save(
        &mut self,
        project: &FutureboardProject,
        path: &PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some(project_root) = path.parent().map(PathBuf::from) else {
            return;
        };
        let updates: std::collections::HashMap<String, String> = project
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter_map(|clip| {
                let ClipSource::Audio {
                    source_path: Some(source_path),
                    ..
                } = &clip.source
                else {
                    return None;
                };
                let resolved = if source_path.is_absolute() {
                    source_path.clone()
                } else {
                    project_root.join(source_path)
                };
                Some((clip.id.clone(), resolved.to_string_lossy().into_owned()))
            })
            .collect();

        if updates.is_empty() {
            return;
        }

        let changed = self.timeline.update(cx, |timeline, cx| {
            let mut changed = false;
            for track in &mut timeline.state.tracks {
                for clip in &mut track.clips {
                    let Some(new_path) = updates.get(&clip.id) else {
                        continue;
                    };
                    let crate::components::timeline::timeline_state::ClipType::Audio {
                        file_id,
                        source_path,
                    } = &mut clip.clip_type
                    else {
                        continue;
                    };
                    if source_path.as_deref() != Some(new_path.as_str()) {
                        *file_id = new_path.clone();
                        *source_path = Some(new_path.clone());
                        changed = true;
                    }
                }
            }
            if changed {
                cx.notify();
            }
            changed
        });

        if changed {
            self.mark_engine_media_dirty();
            self.schedule_audio_project_sync(cx, true, "project_save_asset_paths");
        }
    }

    pub(super) fn cmd_open_project(&mut self, cx: &mut Context<Self>) {
        let default_dir = self
            .project_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| self.default_projects_dir(cx));
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            let result = rfd::AsyncFileDialog::new()
                .set_title("Open Project")
                .set_directory(&default_dir)
                .add_filter(
                    "Futureboard Project",
                    crate::project::io::SUPPORTED_PROJECT_FILE_EXTS,
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
        self.project_state = crate::app_state::ProjectState::Loading;
        match load_project(&path) {
            Ok(project) => {
                let _ = self.timeline.update(cx, |timeline, _cx| {
                    apply_to_timeline(&project, &mut timeline.state);
                });
                self.project_state =
                    crate::app_state::ProjectState::SavedProject { path: path.clone() };
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
                self.project_state = crate::app_state::ProjectState::Error(e.to_string());
                cx.notify();
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
