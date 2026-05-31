use gpui::{px, Bounds, Context};

use std::{path::PathBuf, sync::Arc};

use crate::components::project_switcher::ProjectSwitcherState;
use crate::components::project_wizard::{
    open_project_wizard_window, ProjectCreateCallback, ProjectWizardResult,
};
use crate::components::timeline::timeline_state::{
    self, CreateTrackOptions, TimelineState, TrackType,
};
use crate::project::{
    apply_to_timeline, io::load_project, io::save_project, now_secs, FutureboardProject,
};

use super::StudioLayout;
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

    // ── Project wizard ────────────────────────────────────────────────────────

    pub(super) fn open_project_wizard(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.project_wizard_window.clone() {
            if handle
                .update(cx, |_wizard, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.project_wizard_window = None;
        }

        let owner = cx.entity().clone();
        let on_create: ProjectCreateCallback = Arc::new(move |result, cx| {
            owner
                .update(cx, |this, cx| this.on_project_created(&result, cx))
                .map_err(|error| format!("Unable to update the main studio window: {error}"))
        });
        let bounds = owner_bounds.unwrap_or_else(|| Bounds {
            origin: gpui::Point::default(),
            size: gpui::size(px(1400.0), px(900.0)),
        });

        match open_project_wizard_window(bounds, on_create, cx) {
            Ok(handle) => self.project_wizard_window = Some(handle),
            Err(error) => eprintln!("[project] failed to open project wizard window: {error}"),
        }
    }

    pub(super) fn on_project_created(
        &mut self,
        result: &ProjectWizardResult,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let safe_name = crate::project::io::sanitize_project_name(&result.name);
        let target_folder = result.location.join(&safe_name);
        if target_folder.exists() {
            return Err("A project with this name already exists at that location.".to_string());
        }
        let folder = match crate::project::io::create_project_folder(&result.location, &result.name)
        {
            Ok(f) => f,
            Err(e) => {
                return Err(format!("Failed to create project folder: {e}"));
            }
        };
        let project_file = folder.join(format!(
            "{}.{}",
            crate::project::io::sanitize_project_name(&result.name),
            crate::project::io::PROJECT_FILE_EXT
        ));

        // Reset timeline to match wizard settings
        let _ = self.timeline.update(cx, |timeline, _cx| {
            timeline.state = TimelineState::default();
            timeline.state.bpm = result.bpm as f32;
            timeline.state.time_signature_num = result.time_sig_num;
            timeline.state.time_signature_den = result.time_sig_den;
        });

        // Create tracks from template
        let audio_count = result.template.audio_tracks();
        let midi_count = result.template.midi_tracks();
        if audio_count > 0 || midi_count > 0 {
            let _ = self.timeline.update(cx, |timeline, _cx| {
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
            });
        }

        // Save initial project file
        let tl_state = self.timeline.read(cx).state.clone();
        let mut project = FutureboardProject::from(&tl_state);
        project.name = result.name.clone();
        project.settings.sample_rate = result.sample_rate;

        save_project(&mut project, &project_file)
            .map_err(|e| format!("Failed to save initial project file: {e}"))?;

        self.project_path = Some(project_file.clone());
        self.project_folder = Some(folder.clone());
        self.file_browser.set_project_folder(Some(folder));
        self.project_switcher.current_project.name = result.name.clone();
        self.project_switcher.current_project.path = Some(project_file.clone());
        self.project_switcher.current_project.is_dirty = false;
        self.project_switcher.current_project.subtitle = "Saved".to_string();

        self.recent_projects
            .push(&result.name, project_file, now_secs());
        self.sync_recent_to_switcher();
        cx.notify();
        Ok(())
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

    pub(super) fn do_save_project(&mut self, path: &PathBuf, cx: &mut Context<Self>) {
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
            }
            Err(e) => {
                eprintln!("[project] save failed: {e}");
                self.project_switcher.current_project.subtitle = format!("Save failed: {e}");
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

    pub(super) fn load_project_from_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
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
