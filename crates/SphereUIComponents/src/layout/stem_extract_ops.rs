//! StudioLayout integration for the Stem Extractor dialog.
//!
//! Captures audio clips currently on arrangement tracks (plain owned data),
//! then opens the external Stem Extractor window. The window owns the
//! background MDX-NET job — StudioLayout holds only the window handle.

use gpui::{Bounds, Context};

use super::StudioLayout;
use crate::components::timeline::timeline_state::ClipType;
use crate::components::{
    open_stem_extractor_window, StemExtractorDialogDefaults, StemSourceClip,
};

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

        let audio_clips = collect_audio_source_clips(&tl_state);
        let selected_clip_id = tl_state
            .selection
            .selected_clip_ids
            .iter()
            .find(|id| audio_clips.iter().any(|clip| clip.clip_id == **id))
            .cloned()
            .or_else(|| audio_clips.first().map(|clip| clip.clip_id.clone()));

        let suggested_output_dir = project_root.as_ref().map(|root| {
            let dir = root.join("Rendered").join("Stems");
            let _ = std::fs::create_dir_all(&dir);
            dir
        });

        let defaults = StemExtractorDialogDefaults {
            project_name,
            audio_clips,
            selected_clip_id,
            suggested_output_dir,
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

fn collect_audio_source_clips(
    tl_state: &crate::components::timeline::timeline_state::TimelineState,
) -> Vec<StemSourceClip> {
    let mut clips = Vec::new();
    for track in &tl_state.tracks {
        for clip in &track.clips {
            let ClipType::Audio {
                source_path: Some(path),
                ..
            } = &clip.clip_type
            else {
                continue;
            };
            let path = std::path::PathBuf::from(path);
            if !path.exists() {
                continue;
            }
            clips.push(StemSourceClip {
                clip_id: clip.id.clone(),
                track_id: track.id.clone(),
                track_name: track.name.clone(),
                clip_name: clip.name.clone(),
                source_path: path,
            });
        }
    }
    clips
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::timeline::timeline_state::{ClipState, ClipType, TimelineState};

    #[test]
    fn collect_only_resolvable_audio_clips() {
        let mut state = TimelineState::default();
        let track_id = state.create_audio_track();
        {
            let track = state
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .expect("audio track");
            track.name = "Drums".into();
            track.clips.push(ClipState {
                id: "clip-a".into(),
                name: "Loop A".into(),
                start_beat: 0.0,
                duration_beats: 4.0,
                source_duration_seconds: Some(2.0),
                offset_beats: 0.0,
                gain: 1.0,
                clip_type: ClipType::Audio {
                    file_id: "a".into(),
                    source_path: Some("/tmp/does-not-exist-stem.wav".into()),
                },
                muted: false,
                audio_import: Default::default(),
                stretch: Default::default(),
            });
            let existing = std::env::temp_dir().join("fb-stem-source-test.wav");
            std::fs::write(&existing, b"RIFF").unwrap();
            track.clips.push(ClipState {
                id: "clip-b".into(),
                name: "Loop B".into(),
                start_beat: 4.0,
                duration_beats: 4.0,
                source_duration_seconds: Some(2.0),
                offset_beats: 0.0,
                gain: 1.0,
                clip_type: ClipType::Audio {
                    file_id: "b".into(),
                    source_path: Some(existing.display().to_string()),
                },
                muted: false,
                audio_import: Default::default(),
                stretch: Default::default(),
            });
        }

        let clips = collect_audio_source_clips(&state);
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].clip_id, "clip-b");
        assert_eq!(clips[0].label(), "Drums · Loop B");
        let _ = std::fs::remove_file(std::env::temp_dir().join("fb-stem-source-test.wav"));
    }
}
