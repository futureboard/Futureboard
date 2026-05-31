use gpui::Context;

use crate::components::timeline::timeline_state::{self, TrackType};

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
                        input_monitor: false,
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
