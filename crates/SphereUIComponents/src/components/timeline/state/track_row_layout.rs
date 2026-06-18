use std::collections::HashMap;

use super::*;

/// Default arrangement row height (px).
pub const DEFAULT_TRACK_HEIGHT: f32 = 72.0;

/// Legacy alias — prefer [`DEFAULT_TRACK_HEIGHT`] or per-track layout.
pub const TRACK_HEIGHT: f32 = DEFAULT_TRACK_HEIGHT;

pub const MAX_TRACK_HEIGHT: f32 = 320.0;
pub const MIN_AUDIO_TRACK_HEIGHT: f32 = 36.0;
pub const MIN_MIDI_TRACK_HEIGHT: f32 = 44.0;
pub const MIN_BUS_TRACK_HEIGHT: f32 = 32.0;

pub const TRACK_HEIGHT_SMALL: f32 = 44.0;
pub const TRACK_HEIGHT_NORMAL: f32 = 72.0;
pub const TRACK_HEIGHT_LARGE: f32 = 120.0;
pub const TRACK_HEIGHT_HUGE: f32 = 180.0;

pub const TRACK_RESIZE_HANDLE_HITBOX: f32 = 5.0;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackViewLayout {
    /// Per-track row height overrides (px). Missing ids use [`DEFAULT_TRACK_HEIGHT`].
    heights: HashMap<TrackId, f32>,
}

impl TrackViewLayout {
    pub fn height_for(&self, track_id: &str) -> Option<f32> {
        self.heights.get(track_id).copied()
    }

    pub fn set_height(&mut self, track_id: impl Into<TrackId>, height: f32) {
        self.heights.insert(track_id.into(), height);
    }

    pub fn remove_track(&mut self, track_id: &str) {
        self.heights.remove(track_id);
    }

    pub fn clear(&mut self) {
        self.heights.clear();
    }

    pub fn retain_tracks<'a, I: Iterator<Item = &'a str>>(&mut self, live: I) {
        let live: std::collections::HashSet<&str> = live.collect();
        self.heights.retain(|id, _| live.contains(id.as_str()));
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TrackId, &f32)> {
        self.heights.iter()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackRowLayoutEntry {
    pub track_id: TrackId,
    pub index: usize,
    pub y: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackRowLayout {
    pub rows: Vec<TrackRowLayoutEntry>,
    pub total_height: f32,
    pub scroll_y: f32,
}

impl TrackRowLayout {
    pub fn build(state: &TimelineState) -> Self {
        let scroll_y = state.viewport.scroll_y;
        let mut y = 0.0_f32;
        let mut rows = Vec::with_capacity(state.tracks.len());
        for (index, track) in state.tracks.iter().enumerate() {
            let height = state.track_row_height(track);
            rows.push(TrackRowLayoutEntry {
                track_id: track.id.clone(),
                index,
                y,
                height,
            });
            y += height;
        }
        Self {
            rows,
            total_height: y,
            scroll_y,
        }
    }

    pub fn row_for_index(&self, index: usize) -> Option<&TrackRowLayoutEntry> {
        self.rows.get(index)
    }

    pub fn row_for_track(&self, track_id: &str) -> Option<&TrackRowLayoutEntry> {
        self.rows.iter().find(|row| row.track_id == track_id)
    }

    pub fn track_at_content_y(&self, content_y: f32) -> Option<&TrackRowLayoutEntry> {
        if content_y < 0.0 {
            return None;
        }
        self.rows.iter().find(|row| {
            let bottom = row.y + row.height;
            content_y >= row.y && content_y < bottom
        })
    }

    pub fn insert_index_at_content_y(&self, content_y: f32) -> usize {
        if self.rows.is_empty() {
            return 0;
        }
        let content_y = content_y.max(0.0);
        for (i, row) in self.rows.iter().enumerate() {
            let mid = row.y + row.height * 0.5;
            if content_y < mid {
                return i;
            }
        }
        self.rows.len()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackHeightResizeSession {
    pub anchor_track_id: String,
    pub track_ids: Vec<String>,
    pub start_heights: Vec<(String, f32)>,
    pub start_mouse_y: f32,
}

pub fn min_track_row_height(track_type: TrackType) -> f32 {
    match track_type {
        TrackType::Audio | TrackType::Master => MIN_AUDIO_TRACK_HEIGHT,
        TrackType::Midi | TrackType::Instrument => MIN_MIDI_TRACK_HEIGHT,
        TrackType::Bus | TrackType::Return => MIN_BUS_TRACK_HEIGHT,
    }
}

pub fn clamp_track_row_height(track_type: TrackType, height: f32) -> f32 {
    height.clamp(min_track_row_height(track_type), MAX_TRACK_HEIGHT)
}

pub fn preset_track_row_height(preset: TrackHeightPreset) -> f32 {
    match preset {
        TrackHeightPreset::Small => TRACK_HEIGHT_SMALL,
        TrackHeightPreset::Normal => TRACK_HEIGHT_NORMAL,
        TrackHeightPreset::Large => TRACK_HEIGHT_LARGE,
        TrackHeightPreset::Huge => TRACK_HEIGHT_HUGE,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackHeightPreset {
    Small,
    Normal,
    Large,
    Huge,
}

impl TimelineState {
    pub fn track_row_height(&self, track: &TrackState) -> f32 {
        let raw = self
            .track_view_layout
            .height_for(&track.id)
            .unwrap_or(DEFAULT_TRACK_HEIGHT);
        clamp_track_row_height(track.track_type, raw)
    }

    pub fn track_row_height_for_id(&self, track_id: &str) -> f32 {
        self.tracks
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| self.track_row_height(t))
            .unwrap_or(DEFAULT_TRACK_HEIGHT)
    }

    pub fn set_track_row_height(&mut self, track_id: &str, height: f32) -> bool {
        let Some(track) = self.tracks.iter().find(|t| t.id == track_id) else {
            return false;
        };
        let clamped = clamp_track_row_height(track.track_type, height);
        if (self.track_row_height(track) - clamped).abs() < 0.01 {
            return false;
        }
        self.track_view_layout.set_height(track_id, clamped);
        true
    }

    pub fn reset_track_row_height(&mut self, track_id: &str) -> bool {
        if !self.track_view_layout.height_for(track_id).is_some() {
            return false;
        }
        self.track_view_layout.remove_track(track_id);
        true
    }

    pub fn reset_all_track_row_heights(&mut self) {
        self.track_view_layout.clear();
    }

    pub fn apply_track_row_heights(&mut self, heights: &[(String, f32)]) {
        for (track_id, height) in heights {
            let _ = self.set_track_row_height(track_id, *height);
        }
    }

    pub fn track_row_layout(&self) -> TrackRowLayout {
        TrackRowLayout::build(self)
    }

    pub fn total_track_rows_height(&self) -> f32 {
        self.track_row_layout().total_height
    }

    pub fn track_index_at_content_y(&self, content_y: f32) -> Option<usize> {
        self.track_row_layout()
            .track_at_content_y(content_y)
            .map(|row| row.index)
    }

    pub fn track_insert_index_at_content_y(&self, content_y: f32) -> usize {
        self.track_row_layout().insert_index_at_content_y(content_y)
    }

    pub fn track_index_at_y(&self, viewport_y: f32) -> Option<usize> {
        let content_y = viewport_y + self.viewport.scroll_y;
        self.track_index_at_content_y(content_y)
    }

    pub fn track_insert_index_at_y(&self, viewport_y: f32) -> usize {
        let content_y = (viewport_y + self.viewport.scroll_y).max(0.0);
        self.track_insert_index_at_content_y(content_y)
    }

    pub fn lane_y_to_track_id(&self, viewport_y: f32) -> Option<TrackId> {
        let content_y = viewport_y + self.viewport.scroll_y;
        self.track_row_layout()
            .track_at_content_y(content_y)
            .map(|row| row.track_id.clone())
    }

    pub fn track_height_resize_targets(
        &self,
        anchor_track_id: &str,
        shift: bool,
        alt: bool,
    ) -> Vec<String> {
        if alt {
            return self.tracks.iter().map(|t| t.id.clone()).collect();
        }
        if shift {
            let mut ids = self
                .arrangement_range
                .as_ref()
                .map(|range| range.track_ids.clone())
                .unwrap_or_default();
            if ids.is_empty() {
                if let Some(id) = &self.selection.selected_track_id {
                    ids.push(id.clone());
                }
            }
            if ids.is_empty() {
                ids.push(anchor_track_id.to_string());
            }
            return ids;
        }
        vec![anchor_track_id.to_string()]
    }

    pub fn arm_track_height_resize(
        &mut self,
        anchor_track_id: &str,
        start_mouse_y: f32,
        shift: bool,
        alt: bool,
    ) {
        self.track_height_resize_arm =
            Some((anchor_track_id.to_string(), start_mouse_y, shift, alt));
    }

    pub fn clear_track_height_resize_arm(&mut self) {
        self.track_height_resize_arm = None;
    }

    pub fn ensure_track_height_resize_from_arm(&mut self, mouse_y: f32) -> bool {
        if self.track_height_resize.is_some() {
            return true;
        }
        let Some((anchor, start_y, shift, alt)) = self.track_height_resize_arm.clone() else {
            return false;
        };
        self.track_height_resize_arm = None;
        self.begin_track_height_resize(&anchor, start_y, shift, alt);
        self.update_track_height_resize(mouse_y)
    }

    pub fn begin_track_height_resize(
        &mut self,
        anchor_track_id: &str,
        start_mouse_y: f32,
        shift: bool,
        alt: bool,
    ) -> bool {
        let track_ids = self.track_height_resize_targets(anchor_track_id, shift, alt);
        if track_ids.is_empty() {
            return false;
        }
        let start_heights = track_ids
            .iter()
            .filter_map(|id| {
                self.tracks
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| (id.clone(), self.track_row_height(t)))
            })
            .collect::<Vec<_>>();
        if start_heights.is_empty() {
            return false;
        }
        self.track_height_resize = Some(TrackHeightResizeSession {
            anchor_track_id: anchor_track_id.to_string(),
            track_ids,
            start_heights,
            start_mouse_y,
        });
        true
    }

    pub fn update_track_height_resize(&mut self, mouse_y: f32) -> bool {
        let Some(session) = self.track_height_resize.clone() else {
            return false;
        };
        let delta = mouse_y - session.start_mouse_y;
        let mut changed = false;
        for (track_id, start_h) in &session.start_heights {
            if self.set_track_row_height(track_id, start_h + delta) {
                changed = true;
            }
        }
        changed
    }

    pub fn cancel_track_height_resize(&mut self) -> bool {
        self.track_height_resize_arm = None;
        let Some(session) = self.track_height_resize.take() else {
            return false;
        };
        for (track_id, height) in session.start_heights {
            if (height - DEFAULT_TRACK_HEIGHT).abs() < 0.01 {
                self.track_view_layout.remove_track(&track_id);
            } else {
                self.track_view_layout.set_height(track_id, height);
            }
        }
        true
    }

    pub fn finish_track_height_resize(
        &mut self,
    ) -> Option<(Vec<(String, f32)>, Vec<(String, f32)>)> {
        let session = self.track_height_resize.take()?;
        let prev = session.start_heights;
        let next = session
            .track_ids
            .iter()
            .filter_map(|id| {
                self.tracks
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| (id.clone(), self.track_row_height(t)))
            })
            .collect::<Vec<_>>();
        let changed = prev
            .iter()
            .zip(next.iter())
            .any(|((id_a, h_a), (id_b, h_b))| id_a == id_b && (h_a - h_b).abs() >= 0.01);
        if changed {
            Some((prev, next))
        } else {
            None
        }
    }

    pub fn prune_track_view_layout(&mut self) {
        let ids = self.tracks.iter().map(|t| t.id.as_str());
        self.track_view_layout.retain_tracks(ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::timeline::timeline_state::{CreateTrackOptions, TrackType};

    fn sample_state(track_types: &[TrackType]) -> TimelineState {
        let mut state = TimelineState::default();
        for (i, ty) in track_types.iter().enumerate() {
            state.create_track(CreateTrackOptions {
                name: format!("Track {i}"),
                track_type: *ty,
                color: crate::theme::Colors::track_color_for_index(i),
                volume: 1.0,
                pan: 0.0,
                armed: false,
                input_monitor: InputMonitorMode::Off,
            });
        }
        state
    }

    #[test]
    fn track_row_layout_uses_default_height() {
        let state = sample_state(&[TrackType::Audio, TrackType::Midi]);
        let layout = state.track_row_layout();
        assert_eq!(layout.rows.len(), 2);
        assert_eq!(layout.rows[0].height, DEFAULT_TRACK_HEIGHT);
        assert_eq!(layout.rows[1].y, DEFAULT_TRACK_HEIGHT);
        assert_eq!(layout.total_height, DEFAULT_TRACK_HEIGHT * 2.0);
    }

    #[test]
    fn track_row_layout_respects_per_track_override() {
        let mut state = sample_state(&[TrackType::Audio, TrackType::Midi]);
        let track_id = state.tracks[0].id.clone();
        state.track_view_layout.set_height(&track_id, 120.0);
        let layout = state.track_row_layout();
        assert_eq!(layout.rows[0].height, 120.0);
        assert_eq!(layout.rows[1].y, 120.0);
    }

    #[test]
    fn clamp_track_row_height_enforces_type_minimum() {
        assert_eq!(
            clamp_track_row_height(TrackType::Audio, 10.0),
            MIN_AUDIO_TRACK_HEIGHT
        );
        assert_eq!(
            clamp_track_row_height(TrackType::Midi, 10.0),
            MIN_MIDI_TRACK_HEIGHT
        );
        assert_eq!(
            clamp_track_row_height(TrackType::Audio, 500.0),
            MAX_TRACK_HEIGHT
        );
    }

    #[test]
    fn resize_session_restores_on_cancel() {
        let mut state = sample_state(&[TrackType::Audio]);
        let track_id = state.tracks[0].id.clone();
        state.begin_track_height_resize(&track_id, 100.0, false, false);
        state.update_track_height_resize(130.0);
        assert!(state.track_row_height(&state.tracks[0]) > DEFAULT_TRACK_HEIGHT);
        state.cancel_track_height_resize();
        assert_eq!(
            state.track_row_height(&state.tracks[0]),
            DEFAULT_TRACK_HEIGHT
        );
    }

    #[test]
    fn alt_resize_targets_all_tracks() {
        let state = sample_state(&[TrackType::Audio, TrackType::Midi, TrackType::Bus]);
        let anchor = state.tracks[0].id.clone();
        let ids = state.track_height_resize_targets(&anchor, false, true);
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn shift_resize_uses_selection_when_no_range() {
        let mut state = sample_state(&[TrackType::Audio, TrackType::Midi]);
        state.selection.selected_track_id = Some(state.tracks[1].id.clone());
        let anchor = state.tracks[0].id.clone();
        let ids = state.track_height_resize_targets(&anchor, true, false);
        assert_eq!(ids, vec![state.tracks[1].id.clone()]);
    }
}
