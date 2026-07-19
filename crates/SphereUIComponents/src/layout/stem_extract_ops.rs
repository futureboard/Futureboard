//! StudioLayout integration for the Stem Extractor dialog.
//!
//! Captures a plain source path / output folder suggestion from the current
//! selection, then opens the external Stem Extractor window. The window owns
//! the background MDX-NET job — StudioLayout holds only the window handle.

use gpui::{Bounds, Context};

use super::StudioLayout;
use crate::components::timeline::timeline_state::ClipType;
use crate::components::{open_stem_extractor_window, StemExtractorDialogDefaults};

impl StudioLayout {
    pub(super) fn open_stem_extractor_external_window(
        &mut self,
        owner_bounds: Option<Bounds<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.external_windows.stem_extractor.clone() {
            if handle
                .update(cx, |_w, window, _cx| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.external_windows.stem_extractor = None;
        }

        self.menu_bar.open_menu_id = None;
        self.menu_bar.submenu_path.clear();
        self.overlay.open_popover = None;
        self.overlay.text_context_menu = None;

        let tl_state = self.timeline.read(cx).state.clone();
        let project_name = self.project_session.name.clone();
        let project_root = self
            .project_session
            .folder_path
            .as_ref()
            .map(|p| p.to_path_buf());

        let mut suggested_source = None;
        let mut selected_clip_label = None;
        if let Some(clip_id) = tl_state.selection.selected_clip_ids.first() {
            if let Some((_track, clip)) = tl_state.find_clip(clip_id) {
                selected_clip_label = Some(clip.name.clone());
                if let ClipType::Audio {
                    source_path: Some(path),
                    ..
                } = &clip.clip_type
                {
                    let path = std::path::PathBuf::from(path);
                    if path.exists() {
                        suggested_source = Some(path);
                    }
                }
            }
        }

        let suggested_output_dir = project_root.as_ref().map(|root| {
            let dir = root.join("Rendered").join("Stems");
            let _ = std::fs::create_dir_all(&dir);
            dir
        });

        let defaults = StemExtractorDialogDefaults {
            project_name,
            suggested_source,
            suggested_output_dir,
            selected_clip_label,
        };

        let owner_bounds = crate::window_position::resolve_owner_bounds_with_preferred(
            owner_bounds,
            self.studio_window_bounds(cx),
            cx,
        );

        match open_stem_extractor_window(owner_bounds, defaults, cx) {
            Ok(handle) => self.external_windows.stem_extractor = Some(handle),
            Err(err) => eprintln!("[stem-extractor] failed to open window: {err}"),
        }
    }
}
