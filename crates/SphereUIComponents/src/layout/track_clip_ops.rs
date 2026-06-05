use gpui::Context;

use crate::components::edit::{ClipSnapshot, EditCommand};
#[cfg(debug_assertions)]
use crate::components::timeline::timeline_state::TrackType;
use crate::components::timeline::timeline_state::{self, InputMonitorMode};

use super::StudioLayout;
impl StudioLayout {
    /// Dev-only: bulk-create `count` tracks for scalability stress testing.
    /// Tracks cycle through Audio/MIDI/Instrument types. Does not add clips.
    #[cfg(debug_assertions)]
    pub(super) fn stress_add_tracks(&mut self, count: usize, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, _cx| {
            for _ in 0..count {
                let idx = timeline.state.tracks.len();
                let track_type = match idx % 3 {
                    0 => TrackType::Audio,
                    1 => TrackType::Midi,
                    _ => TrackType::Instrument,
                };
                let color = timeline.state.track_color_for_index(idx);
                timeline
                    .state
                    .create_track(timeline_state::CreateTrackOptions {
                        track_type,
                        name: format!("Track {}", idx + 1),
                        color,
                        volume: timeline_state::volume::db_to_norm(0.0),
                        pan: 0.0,
                        armed: false,
                        input_monitor: InputMonitorMode::Off,
                    });
            }
        });
        cx.notify();
    }

    #[cfg(not(debug_assertions))]
    pub(super) fn stress_add_tracks(&mut self, _count: usize, _cx: &mut Context<Self>) {}

    // Add Track is now an external window that owns its own state.

    pub(super) fn delete_selected_track(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.delete_track(&id);
                cx.notify();
            }
        });
    }

    pub(super) fn delete_selected_clip_or_track(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            use crate::components::timeline::timeline_state::TrackLaneMode;
            // Automation mode: Delete removes selected automation points first
            // (committed once), and never falls through to clip/track deletion.
            if let Some(track_id) = timeline.state.selection.selected_track_id.clone() {
                if timeline.state.track_lane_mode(&track_id) == TrackLaneMode::Automation
                    && timeline.state.selected_automation_point_count(&track_id) > 0
                {
                    if timeline.state.delete_selected_automation_points(&track_id) > 0 {
                        timeline.mark_project_changed(cx);
                        cx.notify();
                    }
                    return;
                }
            }
            if let Some(id) = timeline.state.selection.selected_clip_ids.first().cloned() {
                timeline.delete_clip_command(&id, cx);
            } else if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.delete_track(&id);
                timeline.mark_project_changed(cx);
                cx.notify();
            }
        });
        self.mark_dirty();
    }

    pub(super) fn select_all_timeline_items(&mut self, cx: &mut Context<Self>) {
        let mut selected_automation = false;
        let _ = self.timeline.update(cx, |timeline, cx| {
            use crate::components::timeline::timeline_state::TrackLaneMode;
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                if timeline.state.track_lane_mode(&id) == TrackLaneMode::Automation {
                    if let Some(lane_id) = timeline.state.active_automation_lane_id(&id) {
                        timeline.state.select_all_automation_points(&id, &lane_id);
                        selected_automation = true;
                        cx.notify();
                        return;
                    }
                }
            }

            let clip_ids: Vec<String> = timeline
                .state
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter().map(|clip| clip.id.clone()))
                .collect();
            if !clip_ids.is_empty() {
                timeline.state.selection.selected_track_id =
                    timeline.state.tracks.first().map(|t| t.id.clone());
                timeline.state.selection.selected_clip_ids = clip_ids;
                timeline.state.arrangement_range = None;
                cx.notify();
            }
        });
        if selected_automation {
            return;
        }
    }

    pub(super) fn copy_selected_clips(&mut self, cx: &mut Context<Self>) {
        self.clip_clipboard = self
            .timeline
            .read(cx)
            .state
            .selection
            .selected_clip_ids
            .iter()
            .filter_map(|id| ClipSnapshot::capture(&self.timeline.read(cx).state, id))
            .collect();
    }

    pub(super) fn cut_selected_clips(&mut self, cx: &mut Context<Self>) {
        self.copy_selected_clips(cx);
        if self.clip_clipboard.is_empty() {
            return;
        }
        self.mark_dirty();
        let snapshots = self.clip_clipboard.clone();
        let _ = self.timeline.update(cx, |timeline, cx| {
            timeline.run_edit_command(EditCommand::BatchDeleteClips { snapshots }, cx);
        });
    }

    pub(super) fn paste_clips_at_playhead(&mut self, cx: &mut Context<Self>) {
        if self.clip_clipboard.is_empty() {
            return;
        }
        self.mark_dirty();
        let snapshots = self.clip_clipboard.clone();
        let _ = self.timeline.update(cx, |timeline, cx| {
            let min_start = snapshots
                .iter()
                .map(|snapshot| snapshot.clip.start_beat)
                .fold(f32::INFINITY, f32::min);
            let paste_beat = timeline.state.transport.playhead_beats.max(0.0);
            let offset = if min_start.is_finite() {
                paste_beat - min_start
            } else {
                0.0
            };
            let mut pasted_ids = Vec::new();
            for snapshot in snapshots {
                let track_id = if timeline
                    .state
                    .tracks
                    .iter()
                    .any(|track| track.id == snapshot.track_id)
                {
                    snapshot.track_id.clone()
                } else if let Some(track_id) = timeline
                    .state
                    .selection
                    .selected_track_id
                    .clone()
                    .or_else(|| timeline.state.tracks.first().map(|track| track.id.clone()))
                {
                    track_id
                } else {
                    continue;
                };
                let mut clip = snapshot.clip.clone();
                clip.id = timeline.state.next_clip_id();
                clip.start_beat = (clip.start_beat + offset).max(0.0);
                pasted_ids.push(clip.id.clone());
                timeline.run_edit_command(EditCommand::CreateClip { track_id, clip }, cx);
            }
            if !pasted_ids.is_empty() {
                timeline.state.selection.selected_clip_ids = pasted_ids;
                timeline.state.arrangement_range = None;
                cx.notify();
            }
        });
    }

    pub(super) fn toggle_selected_track_automation_mode(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_lane_mode(&id);
                cx.notify();
            }
        });
    }

    pub(super) fn select_all_automation_points(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            use crate::components::timeline::timeline_state::TrackLaneMode;
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                if timeline.state.track_lane_mode(&id) != TrackLaneMode::Automation {
                    return;
                }
                if let Some(lane_id) = timeline.state.active_automation_lane_id(&id) {
                    timeline.state.select_all_automation_points(&id, &lane_id);
                    cx.notify();
                }
            }
        });
    }

    pub(super) fn clear_automation_selection(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                if timeline.state.clear_automation_selection(&id) {
                    cx.notify();
                }
            }
        });
    }

    pub(super) fn cycle_selected_track_automation_target(&mut self, cx: &mut Context<Self>) {
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                if timeline.state.cycle_automation_target(&id).is_some() {
                    timeline.mark_project_changed(cx);
                    cx.notify();
                }
            }
        });
        self.mark_dirty();
    }

    pub(super) fn duplicate_selected_clip(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_clip_ids.first().cloned() {
                timeline.state.duplicate_clip(&id);
                cx.notify();
            }
        });
    }

    pub(super) fn toggle_selected_track_mute(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_mute(&id);
                cx.notify();
            }
        });
    }

    pub(super) fn toggle_selected_track_solo(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_solo(&id);
                cx.notify();
            }
        });
    }

    pub(super) fn toggle_selected_track_arm(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.toggle_track_arm(&id);
                cx.notify();
            }
        });
    }

    pub(super) fn reset_selected_track_volume(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline
                    .state
                    .set_track_volume(&id, timeline_state::volume::db_to_norm(0.0));
                cx.notify();
            }
        });
    }

    pub(super) fn reset_selected_track_pan(&mut self, cx: &mut Context<Self>) {
        self.mark_dirty();
        let _ = self.timeline.update(cx, |timeline, cx| {
            if let Some(id) = timeline.state.selection.selected_track_id.clone() {
                timeline.state.set_track_pan(&id, 0.0);
                cx.notify();
            }
        });
    }
}
