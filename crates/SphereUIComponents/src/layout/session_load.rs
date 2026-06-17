//! Phased, crash-safe project-open transaction (the "Loading Session..." flow).
//!
//! Pre-studio decode/validate runs in [`crate::loading_session`] before
//! [`super::StudioLayout`] is mounted. Once a [`crate::loading_session::LoadedSessionPackage`]
//! is ready, this module installs it into a fresh studio workspace in one
//! synchronous pass (tracks, plugins, validation, waveforms) before the first
//! studio frame renders.
//!
//! In-studio project replacement keeps the root [`super::StudioLayout`] window
//! alive: quiesce the live session, show the in-window loading gate, decode on a
//! background thread, then install into the existing layout (rollback on failure).

use std::path::PathBuf;

use gpui::{BorrowAppContext, Context};

use crate::app_state::{AppMode, AppSessionGate, ProjectState, SessionInstallStatus};
use crate::loading_session::{LoadedSessionPackage, SessionInstallHandoff, SessionRollbackSnapshot};
use crate::session_shutdown::{
    PluginUnloadTarget, SessionShutdownReason, SessionShutdownSnapshot,
};
use crate::project::io::{load_project, validate_project_file};
use crate::project::{apply_to_timeline, now_secs};

use super::project_ops::ProjectOpenOptions;
use super::{RecordingUiState, StudioLayout};

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
        mut package: LoadedSessionPackage,
        cx: &mut Context<Self>,
    ) {
        session_log!(
            "install loaded session: {} ({})",
            package.project.name,
            package.path.display()
        );

        if let Some(handoff) = package.install_handoff.take() {
            self.install_loaded_session_with_handoff(package, handoff, cx);
            return;
        }

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

        self.begin_async_plugin_restore_and_finalize(package, cx);
    }

    fn install_loaded_session_with_handoff(
        &mut self,
        package: LoadedSessionPackage,
        handoff: SessionInstallHandoff,
        cx: &mut Context<Self>,
    ) {
        session_log!("install with pre-studio handoff");
        self.project_state = crate::app_state::ProjectState::Loading;

        self.adopt_session_install_handoff(handoff, cx);

        if !self.bind_loaded_project_session(&package, cx) {
            self.session_install_status = crate::app_state::SessionInstallStatus::Failed;
            self.project_state = crate::app_state::ProjectState::Error(
                "The restored arrangement did not match the project file.".to_string(),
            );
            session_log!("install failed: track integrity check");
            cx.notify();
            return;
        }

        self.validate_session_references(cx);
        self.update_virtual_keyboard_target_status(cx);
        self.schedule_loaded_project_waveforms(&package, cx);

        self.session_install_warnings = package.restore_warnings.clone();
        self.session_install_status = crate::app_state::SessionInstallStatus::Ready;
        self.project_state = crate::app_state::ProjectState::SavedProject {
            path: package.path,
        };
        self.session_install_detail.clear();
        self.session_install_progress =
            crate::components::progress_dialog::ProgressBarValue::value(1.0);

        session_log!(
            "install complete (pre-studio) warnings={}",
            self.session_install_warnings.len()
        );

        if !self.session_install_warnings.is_empty() {
            for warning in &self.session_install_warnings {
                eprintln!("[PluginRestore] warning: {warning}");
            }
            self.queue_session_load_warning_dialog(self.session_install_warnings.clone(), cx);
        }

        cx.notify();
    }

    fn bind_loaded_project_session(
        &mut self,
        package: &LoadedSessionPackage,
        cx: &mut Context<Self>,
    ) -> bool {
        let project = &package.project;
        let path = &package.path;
        let expected_tracks = project.tracks.len();

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

        let restored_tracks = self.timeline.read(cx).state.tracks.len();
        if restored_tracks != expected_tracks {
            session_log!(
                "integrity check failed: expected {expected_tracks} tracks, restored {restored_tracks}"
            );
            return false;
        }
        true
    }

    pub(super) fn adopt_session_install_handoff(
        &mut self,
        handoff: SessionInstallHandoff,
        cx: &mut Context<Self>,
    ) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.reset_input_state();
            timeline.state = handoff.timeline_state;
            cx.notify();
        });

        if let Some(runtime) = handoff.bridge_runtime {
            self.plugin_editors.bridge_runtime = Some(runtime);
        }

        self.install_audio_callbacks(&handoff.engine, cx);
        self.audio_bridge.running = handoff.engine_stats.running;
        self.audio_bridge.stats = Some(handoff.engine_stats);
        self.audio_bridge.last_error = None;
        self.audio_bridge.engine = Some(handoff.engine);
        self.sync_plugin_bridge_sinks_to_engine(cx, "pre_studio_handoff");
        self.mark_engine_media_dirty();
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

    /// Quiesce the live session for an in-window project switch. Does not close
    /// or unhook the root studio window.
    pub fn prepare_for_in_studio_project_switch(&mut self, cx: &mut Context<Self>) -> usize {
        session_log!("prepare for in-studio project switch");
        let plugin_editors = self.plugin_editors.open.len() + self.plugin_editors.bridge.len();
        let midi_editor = usize::from(self.midi_editor.window.is_some());

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.project_switcher.is_open = false;
        self.command_palette.close();
        self.overlay.text_context_menu = None;
        self.overlay.open_popover = None;

        if matches!(
            self.recording.ui_state,
            RecordingUiState::Recording | RecordingUiState::Preparing | RecordingUiState::Finalizing
        ) {
            self.stop_native_recording(cx);
        }
        self.stop_native_playback(cx);
        self.defer_panic_virtual_keyboard(cx);
        self.shutdown_plugin_editors(cx);
        if self.midi_editor.window.is_some() {
            self.close_midi_editor_window(cx);
        }

        plugin_editors + midi_editor
    }

    /// Replace the current project inside the existing studio window.
    pub fn begin_in_studio_project_switch(
        &mut self,
        path: PathBuf,
        open_options: ProjectOpenOptions,
        cx: &mut Context<Self>,
    ) {
        if let Some(request_load) = self.window_hooks.on_request_project_load.clone() {
            request_load(path, open_options, cx);
            return;
        }

        let root_alive = self.window_hooks.self_window.is_some();
        eprintln!("[ProjectSwitch] root window alive before switch={root_alive}");
        eprintln!("[ProjectSwitch] begin switch");
        let rollback = self.capture_session_rollback_snapshot(cx);
        let transient_count = self.prepare_for_in_studio_project_switch(cx);
        eprintln!(
            "[ProjectSwitch] closing transient windows count={transient_count}"
        );
        eprintln!("[ProjectSwitch] old session quiesced");

        self.session_install_status = SessionInstallStatus::Loading;
        self.project_state = ProjectState::Loading;
        cx.update_global::<AppSessionGate, _>(|gate, _| {
            eprintln!("[ProjectSwitch] entering LoadingSession mode");
            gate.mode = AppMode::LoadingSession;
        });
        cx.notify();

        let path_for_job = path.clone();
        let path_for_error = path.clone();
        let entity = cx.entity().clone();
        cx.spawn(async move |_this, cx| {
            eprintln!("[ProjectSwitch] loading target project");
            let decoded = cx
                .background_executor()
                .spawn(async move {
                    if !path_for_job.exists() {
                        return Err(LoadSwitchError::NotFound(path_for_job));
                    }
                    validate_project_file(&path_for_job).map_err(LoadSwitchError::Project)?;
                    load_project(&path_for_job)
                        .map_err(LoadSwitchError::Project)
                        .map(|project| (project, path_for_job))
                })
                .await;

            let _ = entity.update(cx, |this, cx| match decoded {
                Ok((project, path)) => {
                    eprintln!("[ProjectSwitch] loaded target project");
                    eprintln!("[ProjectSwitch] installing session");
                    let failed_path = path.clone();
                    let package = LoadedSessionPackage {
                        project,
                        path,
                        open_options,
                        install_handoff: None,
                        restore_warnings: Vec::new(),
                    };
                    this.install_loaded_session(package, cx);
                    if this.session_install_status.is_failed() {
                        eprintln!("[ProjectSwitch] install failed — restoring rollback");
                        this.restore_session_rollback_snapshot(rollback, cx);
                        cx.update_global::<AppSessionGate, _>(|gate, _| gate.mode = AppMode::Studio);
                        this.show_project_open_failed_dialog(
                            "Open Project Failed",
                            "The project file could not be restored into the session.",
                            Some(
                                "The restored arrangement did not match the project file."
                                    .to_string(),
                            ),
                            Some(failed_path),
                            open_options,
                            cx,
                        );
                    } else {
                        eprintln!(
                            "[ProjectSwitch] session install started — awaiting plugin restore"
                        );
                    }
                }
                Err(LoadSwitchError::NotFound(path)) => {
                    eprintln!(
                        "[ProjectSwitch] switch failed error=project not found: {}",
                        path.display()
                    );
                    this.finish_in_studio_switch_failure(
                        rollback,
                        "Open Project Failed",
                        "The project file could not be found at the saved location.",
                        Some(format!("Details: {}", path.display())),
                        Some(path),
                        open_options,
                        cx,
                    );
                }
                Err(LoadSwitchError::Project(e)) => {
                    eprintln!(
                        "[ProjectSwitch] switch failed error={}",
                        e.technical_detail()
                    );
                    this.finish_in_studio_switch_failure(
                        rollback,
                        "Open Project Failed",
                        &e.user_message(),
                        Some(format!("Details: {}", e.technical_detail())),
                        Some(path_for_error),
                        open_options,
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    fn finish_in_studio_switch_failure(
        &mut self,
        rollback: SessionRollbackSnapshot,
        title: &str,
        message: &str,
        detail: Option<String>,
        path: Option<PathBuf>,
        open_options: ProjectOpenOptions,
        cx: &mut Context<Self>,
    ) {
        self.restore_session_rollback_snapshot(rollback, cx);
        cx.update_global::<AppSessionGate, _>(|gate, _| gate.mode = AppMode::Studio);
        let root_alive = self.window_hooks.self_window.is_some();
        eprintln!("[ProjectSwitch] root window alive after switch={root_alive}");
        eprintln!("[ProjectSwitch] notifying root window");
        self.show_project_open_failed_dialog(title, message, detail, path, open_options, cx);
        cx.notify();
    }

    /// Prepare rollback + shutdown snapshot for an in-studio project switch.
    /// Does not close or unhook the root studio window.
    pub fn prepare_for_in_studio_project_switch_transaction(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (
        SessionRollbackSnapshot,
        SessionShutdownSnapshot,
        Option<gpui::Bounds<gpui::Pixels>>,
    ) {
        session_log!("prepare for in-studio project switch transaction");
        eprintln!("[ProjectSwitch] close project switcher popover");
        let transient_count = self.prepare_for_in_studio_project_switch(cx);
        eprintln!(
            "[SessionShutdown] closing transient windows count={transient_count}"
        );
        let rollback = self.capture_session_rollback_snapshot(cx);
        let shutdown = self.capture_session_shutdown_snapshot(
            SessionShutdownReason::ProjectSwitch,
            cx,
        );
        let owner_bounds = self.studio_window_bounds(cx);
        (rollback, shutdown, owner_bounds)
    }

    /// Tear down the live studio surface before a welcome-path project reload
    /// that closes and remounts the studio window.
    pub fn prepare_for_app_level_project_reload(
        &mut self,
        cx: &mut Context<Self>,
    ) -> (
        SessionRollbackSnapshot,
        Option<gpui::Bounds<gpui::Pixels>>,
        Option<gpui::WindowHandle<Self>>,
        SessionShutdownSnapshot,
    ) {
        session_log!("prepare for app-level project reload");
        self.prepare_immediate_session_shutdown(cx);
        let rollback = self.capture_session_rollback_snapshot(cx);
        let shutdown =
            self.capture_session_shutdown_snapshot(SessionShutdownReason::ProjectSwitch, cx);
        let owner_bounds = self.studio_window_bounds(cx);
        let self_window = self.window_hooks.self_window.take();
        (rollback, owner_bounds, self_window, shutdown)
    }

    /// Stop transport/recording and close transient UI before session shutdown.
    pub fn prepare_immediate_session_shutdown(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.recording.ui_state,
            super::RecordingUiState::Recording
                | super::RecordingUiState::Preparing
                | super::RecordingUiState::Finalizing
        ) {
            self.stop_native_recording(cx);
        }
        self.stop_native_playback(cx);
        self.defer_release_virtual_keyboard_notes(cx);
        self.shutdown_plugin_editors(cx);
        if self.midi_editor.window.is_some() {
            self.close_midi_editor_window(cx);
        }
    }

    /// Capture plugin/host state needed to shut down the session off the UI thread.
    pub fn capture_session_shutdown_snapshot(
        &mut self,
        reason: SessionShutdownReason,
        cx: &Context<Self>,
    ) -> SessionShutdownSnapshot {
        use crate::components::plugin_picker::STUB_PLUGIN_ID;
        use crate::components::timeline::timeline_state::{InsertPluginFormat, TrackType};

        let state = &self.timeline.read(cx).state;
        let mut plugin_targets = Vec::new();
        let mut instrument_track_ids = Vec::new();

        for track in &state.tracks {
            let is_instrument_track =
                matches!(track.track_type, TrackType::Instrument | TrackType::Midi);
            if is_instrument_track {
                instrument_track_ids.push(track.id.clone());
            }
            for (index, slot) in track.inserts.iter().enumerate() {
                if slot.plugin_id.as_deref() == Some(STUB_PLUGIN_ID) {
                    continue;
                }
                if slot.plugin_format != Some(InsertPluginFormat::Vst3) {
                    continue;
                }
                let is_instrument = is_instrument_track
                    && (track.instrument_plugin_instance_id.as_deref() == Some(slot.id.as_str())
                        || index == 0);
                plugin_targets.push(PluginUnloadTarget {
                    track_id: track.id.clone(),
                    insert_id: slot.id.clone(),
                    display_name: slot.display_name.clone(),
                    track_name: track.name.clone(),
                    is_instrument,
                });
            }
        }

        for slot in &state.master.inserts {
            if slot.plugin_id.as_deref() == Some(STUB_PLUGIN_ID) {
                continue;
            }
            if slot.plugin_format != Some(InsertPluginFormat::Vst3) {
                continue;
            }
            plugin_targets.push(PluginUnloadTarget {
                track_id: crate::components::timeline::timeline_state::MASTER_TRACK_ID.to_string(),
                insert_id: slot.id.clone(),
                display_name: slot.display_name.clone(),
                track_name: "Master".to_string(),
                is_instrument: false,
            });
        }

        SessionShutdownSnapshot {
            reason,
            plugin_targets,
            bridge_runtime: self.plugin_editors.bridge_runtime.take(),
            instrument_track_ids,
        }
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

    pub(super) fn validate_session_references(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn schedule_loaded_project_waveforms(
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

enum LoadSwitchError {
    NotFound(PathBuf),
    Project(crate::project::ProjectError),
}
