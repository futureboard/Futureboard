use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineSelection {
    pub selected_track_id: Option<String>,
    pub selected_clip_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRangeSelection {
    pub start_beat: f64,
    pub end_beat: f64,
    pub track_ids: Vec<String>,
}

impl TimelineRangeSelection {
    pub fn new(start_beat: f64, end_beat: f64, track_ids: Vec<String>) -> Self {
        let (start_beat, end_beat) = if start_beat <= end_beat {
            (start_beat, end_beat)
        } else {
            (end_beat, start_beat)
        };
        Self {
            start_beat,
            end_beat,
            track_ids,
        }
    }

    pub fn as_f32_range(&self) -> (f32, f32) {
        (self.start_beat as f32, self.end_beat as f32)
    }
}

impl TimelineState {
    pub fn selected_range_track_ids(&self) -> Vec<String> {
        self.selection
            .selected_track_id
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn track_ids_between(&self, a: &str, b: &str) -> Vec<String> {
        let Some(a_index) = self.tracks.iter().position(|track| track.id == a) else {
            return Vec::new();
        };
        let Some(b_index) = self.tracks.iter().position(|track| track.id == b) else {
            return vec![a.to_string()];
        };
        let (lo, hi) = if a_index <= b_index {
            (a_index, b_index)
        } else {
            (b_index, a_index)
        };
        self.tracks[lo..=hi]
            .iter()
            .map(|track| track.id.clone())
            .collect()
    }

    pub fn select_track(&mut self, track_id: &str) {
        self.selection.selected_track_id = Some(track_id.to_string());
        self.selection.selected_clip_ids.clear();
        self.arrangement_range = None;
    }

    pub fn select_clip(&mut self, clip_id: &str) {
        self.selection.selected_clip_ids = vec![clip_id.to_string()];
        self.arrangement_range = None;
        if let Some(track) = self
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
        {
            self.selection.selected_track_id = Some(track.id.clone());
        }
    }

    pub fn select_clip_additive(&mut self, clip_id: &str) {
        self.arrangement_range = None;
        if let Some(pos) = self
            .selection
            .selected_clip_ids
            .iter()
            .position(|id| id == clip_id)
        {
            self.selection.selected_clip_ids.remove(pos);
        } else {
            self.selection.selected_clip_ids.push(clip_id.to_string());
        }
        if let Some(track) = self
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
        {
            self.selection.selected_track_id = Some(track.id.clone());
        }
    }
}
