//! StudioLayout integration for the Export Arrangement window.
//!
//! Builds a plain engine snapshot + defaults from current project state inside a
//! short UI borrow, then hands them to the external export window. The window
//! owns the background export job — StudioLayout holds only the window handle.

use gpui::{Bounds, Context};

use super::engine_snapshot::{build_engine_project_snapshot_for_export, volume_norm_to_linear};
use super::StudioLayout;
use crate::export::{open_export_arrangement_window, ExportProjectDefaults};

impl StudioLayout {
    pub(super) fn open_export_arrangement_external_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        // Focus an already-open export window instead of spawning a second one.
        if let Some(handle) = self.export_arrangement_window.clone() {
            if handle
                .update(cx, |_w, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.export_arrangement_window = None;
        }

        // Dismiss menus/popovers like the other external-window commands.
        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.open_popover = None;
        self.text_context_menu = None;

        // Capture a plain snapshot of project state under a short borrow — the
        // export job receives only this owned data, never a live entity.
        self.refresh_bridge_plugin_states(cx);
        let tl_state = self.timeline.read(cx).state.clone();
        let sample_rate = self.current_audio_sample_rate();
        let project_root = self
            .project_session
            .folder_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        // Export renders plugins in-process from the saved VST3 state captured by
        // refresh_bridge_plugin_states above — the isolated offline graph has no
        // out-of-process bridge host attached.
        let snapshot = build_engine_project_snapshot_for_export(
            &tl_state,
            sample_rate,
            project_root.as_deref(),
            None,
        );
        let master_volume = volume_norm_to_linear(tl_state.master.volume);
        let content_end_beat = snapshot
            .clips
            .iter()
            .map(|c| c.start_beat + c.duration_beats)
            .chain(
                snapshot
                    .midi_clips
                    .iter()
                    .map(|c| c.start_beat + c.length_beats),
            )
            .fold(0.0_f64, f64::max);
        let project_name = self.project_session.name.clone();

        let defaults = ExportProjectDefaults {
            project_sample_rate: sample_rate,
            master_volume,
            content_end_beat,
            time_selection: None,
            loop_range: None,
            mp3_available: sphere_encoder::mp3_available(),
        };

        // Default output: <project folder>/Exports/<Name>.wav when the project
        // is saved on disk; otherwise the window falls back to the temp dir.
        let default_output = project_root.as_ref().map(|root| {
            let exports_dir = std::path::Path::new(root).join("Exports");
            // Best-effort: ensure the folder exists so the default path validates.
            let _ = std::fs::create_dir_all(&exports_dir);
            exports_dir.join(format!("{}.wav", sanitize_file_stem(&project_name)))
        });

        let owner_bounds = crate::window_position::resolve_owner_bounds_with_preferred(
            owner_bounds,
            self.studio_window_bounds(cx),
            cx,
        );

        match open_export_arrangement_window(
            owner_bounds,
            project_name,
            snapshot,
            defaults,
            default_output,
            cx,
        ) {
            Ok(handle) => self.export_arrangement_window = Some(handle),
            Err(err) => eprintln!("[export] failed to open export window: {err}"),
        }
    }
}

/// Strip characters that are illegal in file names so the default export path is
/// always valid.
fn sanitize_file_stem(name: &str) -> String {
    let trimmed = name.trim();
    let stem: String = trimmed
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if stem.is_empty() {
        "Export".to_string()
    } else {
        stem
    }
}
