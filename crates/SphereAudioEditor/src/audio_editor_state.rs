//! Local view state for the audio editor (zoom, scroll, in-editor selection).

#[derive(Debug, Clone)]
pub struct AudioEditorState {
    /// Horizontal zoom — pixels per beat in the editor waveform view.
    pub pixels_per_beat: f32,
    pub scroll_x: f32,
    pub snap_on: bool,
    /// Beat range relative to clip start (for overlay).
    pub selection_range: Option<(f32, f32)>,
    /// Last clip id the editor fitted scroll/zoom for.
    pub fitted_clip_id: Option<String>,
}

impl Default for AudioEditorState {
    fn default() -> Self {
        Self {
            pixels_per_beat: 48.0,
            scroll_x: 0.0,
            snap_on: true,
            selection_range: None,
            fitted_clip_id: None,
        }
    }
}

impl AudioEditorState {
    pub fn fit_clip(&mut self, clip_id: &str, duration_beats: f32, viewport_width: f32) {
        if self.fitted_clip_id.as_deref() == Some(clip_id) {
            return;
        }
        self.fitted_clip_id = Some(clip_id.to_string());
        self.scroll_x = 0.0;
        if duration_beats > 0.0 && viewport_width > 32.0 {
            self.pixels_per_beat = (viewport_width * 0.92 / duration_beats).clamp(8.0, 256.0);
        }
    }

    pub fn reset_for_clip_change(&mut self, clip_id: Option<&str>) {
        if clip_id != self.fitted_clip_id.as_deref() {
            self.fitted_clip_id = None;
            self.selection_range = None;
        }
    }
}
