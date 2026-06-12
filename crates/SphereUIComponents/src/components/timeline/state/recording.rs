use super::*;

impl TimelineState {
    pub fn insert_recorded_clip(
        &mut self,
        track_id: &str,
        source_path: String,
        clip_name: String,
        start_beat: f32,
        duration_seconds: f64,
        bpm: f32,
    ) -> String {
        let duration_beats = (duration_seconds.max(0.0) * bpm.max(1.0) as f64 / 60.0) as f32;
        self.insert_audio_clip_with_duration(
            track_id.to_string(),
            source_path,
            clip_name,
            start_beat,
            duration_beats.max(0.01),
            Some(duration_seconds),
        )
    }

    // ── Realtime recording preview clip (Part 1) ─────────────────────────
    //
    // A temporary, UI-only clip drawn while a take is recording. It has no
    // source path so it is never sent to the engine or persisted; the
    // arrangement renderer lays it out like any clip, and `waveform_canvas`
    // draws its streamed peaks from the recording-preview registry.

    /// Create (or replace) the live recording preview clip on `track_id`.
    pub fn begin_recording_preview_clip(&mut self, clip_id: &str, track_id: &str, start_beat: f32) {
        self.remove_recording_preview_clip(clip_id);
        let clip = ClipState {
            id: clip_id.to_string(),
            name: "Recording…".to_string(),
            start_beat: start_beat.max(0.0),
            duration_beats: 0.01,
            source_duration_seconds: None,
            offset_beats: 0.0,
            gain: 1.0,
            clip_type: ClipType::Audio {
                file_id: String::new(),
                source_path: None,
            },
            muted: false,
            audio_import: AudioImportState::Pending,
        };
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.clips.push(clip);
        }
    }

    /// Grow the preview clip as recording proceeds. Returns `true` if changed.
    pub fn set_recording_preview_clip_length(
        &mut self,
        clip_id: &str,
        duration_beats: f32,
    ) -> bool {
        let next = duration_beats.max(0.01);
        for track in &mut self.tracks {
            if let Some(c) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                if (c.duration_beats - next).abs() > f32::EPSILON {
                    c.duration_beats = next;
                    return true;
                }
                return false;
            }
        }
        false
    }

    /// Remove the preview clip (take finished / cancelled). Returns `true` if
    /// a clip was removed.
    pub fn remove_recording_preview_clip(&mut self, clip_id: &str) -> bool {
        let mut removed = false;
        for track in &mut self.tracks {
            let before = track.clips.len();
            track.clips.retain(|c| c.id != clip_id);
            removed |= track.clips.len() != before;
        }
        if removed {
            self.selection.selected_clip_ids.retain(|id| id != clip_id);
        }
        removed
    }
}
