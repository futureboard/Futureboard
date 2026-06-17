//! Phased, crash-safe project-open transaction (the "Loading Session..." flow).
//!
//! Pre-studio decode/validate runs in [`crate::loading_session`] before
//! [`super::StudioLayout`] is mounted. Once a [`crate::loading_session::LoadedSessionPackage`]
//! is ready, this module installs it into a fresh studio workspace in one
//! synchronous pass (tracks, plugins, validation, waveforms) before the first
//! studio frame renders.
//!
//! In-studio project replacement also routes through the app-level loading gate
//! (snapshot → close studio → load → remount) via
//! [`StudioWindowHooks::on_request_project_load`].

use std::path::PathBuf;

use gpui::Context;

use crate::loading_session::{LoadedSessionPackage, SessionRollbackSnapshot};
use crate::project::{apply_to_timeline, now_secs};

use super::project_ops::ProjectOpenOptions;
use super::StudioLayout;

macro_rules! session_log {
    ($($arg:tt)*) => {
        eprintln!("[SessionLoad] {}", format!($($arg)*))
    };
}

impl StudioLayout {
    /// Capture the live session for rollback before an in-studio project swap.
    pub fn capture_session_rollback_snapshot(&self, cx: &mut Context<Self>) -> SessionRollbackSnapshot {
        SessionRollbackSnapshot {
            timeline_state: self.timeline.read(cx).state.clone(),
            session: self.project_session.clone(),
            project_state: self.project_state.clone(),
        }
    }

    /// Restore a rollback snapshot into a freshly mounted studio workspace.
    pub fn restore_session_rollback_snapshot(
        &mut self,
        snapshot: SessionRollbackSnapshot,
        cx: &mut Context<Self>,
    ) {
        session_log!("restoring rollback session: {}", snapshot.session.name);
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.reset_input_state();
            timeline.state = snapshot.timeline_state;
            cx.notify();
        });
        self.project_session = snapshot.session;
        self.project_state = snapshot.project_state;
        self.sync_project_session_to_workspace(cx);
        self.session_install_status = crate::app_state::SessionInstallStatus::Ready;
        self.mark_engine_media_dirty();
        self.schedule_audio_project_sync(cx, true, "session_rollback_restore");
        cx.notify();
    }

    /// Install a decoded project into a new studio workspace before the first
    /// render. This is the only path that should load a saved project on a
    /// freshly mounted layout.
    pub fn install_loaded_session(
        &mut self,
        package: LoadedSessionPackage,
        cx: &mut Context<Self>,
    ) {
        session_log!(
            "install loaded session: {} ({})",
            package.project.name,
            package.path.display()
        );
        self.session_install_status = crate::app_state::SessionInstallStatus::Loading;
        self.project_state = crate::app_state::ProjectState::Loading;

        if !self.apply_loaded_project_tracks(&package, cx) {
            self.session_install_status = crate::app_state::SessionInstallStatus::Failed;
            self.project_state = crate::app_state::ProjectState::Error(
                "The restored arrangement did not match the project file.".to_string(),
            );
            session_log!("install failed: track integrity check");
            cx.notify();
            return;
        }

        self.restore_plugin_inserts_after_project_load(cx);
        self.validate_session_references(cx);
        self.update_virtual_keyboard_target_status(cx);
        self.schedule_loaded_project_waveforms(&package, cx);
        self.mark_engine_media_dirty();
        self.schedule_audio_project_sync(cx, true, "project_loaded");
        self.session_install_status = crate::app_state::SessionInstallStatus::Ready;
        self.project_state = if self.project_session.project_file_path.is_some() {
            crate::app_state::ProjectState::SavedProject {
                path: package.path,
            }
        } else {
            crate::app_state::ProjectState::UnsavedWorkspace
        };
        session_log!("install complete");
        session_log!("transition: loading -> studio (shell opens studio next)");
        cx.notify();
    }

    pub fn load_project_from_path_with_options(
        &mut self,
        path: PathBuf,
        open_options: ProjectOpenOptions,
        cx: &mut Context<Self>,
    ) {
        if let Some(request_load) = self.window_hooks.on_request_project_load.clone() {
            request_load(path, open_options, cx);
            return;
        }
        session_log!(
            "on_request_project_load hook missing — cannot open {}",
            path.display()
        );
    }

    pub fn load_project_from_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.load_project_from_path_with_options(path, ProjectOpenOptions::default(), cx);
    }

    /// Tear down the live studio surface before an app-level project reload.
    pub fn prepare_for_app_level_project_reload(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (
        SessionRollbackSnapshot,
        Option<gpui::Bounds<gpui::Pixels>>,
        Option<gpui::WindowHandle<Self>>,
    ) {
        session_log!("prepare for app-level project reload");
        let rollback = self.capture_session_rollback_snapshot(cx);
        let owner_bounds = self.studio_window_bounds(cx);
        let self_window = self.window_hooks.self_window.take();
        self.stop_native_playback(cx);
        self.defer_release_virtual_keyboard_notes(cx);
        self.shutdown_plugin_editors(cx);
        if self.midi_editor.window.is_some() {
            self.close_midi_editor_window(cx);
        }
        (rollback, owner_bounds, self_window)
    }

    fn apply_loaded_project_tracks(
        &mut self,
        package: &LoadedSessionPackage,
        cx: &mut Context<Self>,
    ) -> bool {
        let project = &package.project;
        let path = &package.path;
        let expected_tracks = project.tracks.len();

        self.teardown_all_plugin_instances(cx, "project_load_replace");

        let restored_tracks = self.timeline.update(cx, |timeline, cx| {
            timeline.reset_input_state();
            apply_to_timeline(project, &mut timeline.state);
            cx.notify();
            timeline.state.tracks.len()
        });

        if restored_tracks != expected_tracks {
            session_log!(
                "integrity check failed: expected {expected_tracks} tracks, restored {restored_tracks}"
            );
            return false;
        }

        let folder = path.parent().map(PathBuf::from);
        self.project_session.bind_saved(
            project.id.clone(),
            project.name.clone(),
            folder,
            path.clone(),
            project.created_at,
            project.modified_at,
        );
        session_log!(
            "session bound: name={} path={}",
            self.project_session.name,
            path.display()
        );
        self.sync_project_session_to_workspace(cx);
        self.recent_projects
            .push(&project.name, path.clone(), now_secs());
        self.sync_recent_to_switcher();
        true
    }

    fn validate_session_references(&mut self, cx: &mut Context<Self>) {
        let mut dropped = 0usize;
        let _ = self.timeline.update(cx, |timeline, cx| {
            let state = &mut timeline.state;
            if let Some(track_id) = state.selection.selected_track_id.clone() {
                if !state.tracks.iter().any(|track| track.id == track_id) {
                    state.selection.selected_track_id = None;
                    dropped += 1;
                }
            }
            let existing: std::collections::HashSet<String> = state
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter().map(|clip| clip.id.clone()))
                .collect();
            let before = state.selection.selected_clip_ids.len();
            state
                .selection
                .selected_clip_ids
                .retain(|id| existing.contains(id));
            dropped += before - state.selection.selected_clip_ids.len();
            cx.notify();
        });
        if let Some((track_id, insert_id)) = self.selected_insert.clone() {
            let valid = self
                .timeline
                .read(cx)
                .state
                .find_insert_slot(&track_id, &insert_id)
                .is_some();
            if !valid {
                self.selected_insert = None;
                dropped += 1;
            }
        }
        session_log!("validate: invalid references dropped={dropped}");
    }

    fn schedule_loaded_project_waveforms(
        &mut self,
        package: &LoadedSessionPackage,
        cx: &mut Context<Self>,
    ) {
        let project = package.project.clone();
        let Some(root) = package.path.parent().map(PathBuf::from) else {
            return;
        };
        let timeline = self.timeline.clone();
        let layout = cx.entity().clone();
        crate::components::timeline::audio_import::schedule_project_waveform_restore(
            &project, root, timeline, layout, cx,
        );
    }
}
