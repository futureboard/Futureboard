use gpui::Context;

use crate::components::edit::{ClipSnapshot, EditCommand, TrackSnapshot};
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

    /// Release everything a track owns at the plugin-host level *before* the
    /// track is removed from the project model.
    ///
    /// Ordering matters. An open plugin editor holds its own clone of the
    /// insert's `Vst3RuntimeProcessor` (an `Arc`); the engine drops its clone
    /// when the next project sync reconciles the now-absent track, but the C++
    /// VST3 instance is only destroyed once the *last* clone drops. So unless we
    /// close the editor windows here, deleting the track leaks the plugin
    /// instance and leaves an orphan editor window pointing at a disconnected
    /// processor. We also MIDI-panic the track up front so a note that is
    /// sounding (or stuck) when the track is deleted is silenced immediately,
    /// without waiting for the async engine reload. UI thread only.
    pub(super) fn cleanup_track_plugins_before_delete(
        &mut self,
        track_id: &str,
        cx: &mut Context<Self>,
    ) {
        // Count owned plugin inserts/editors for diagnostics before we start closing.
        let plugin_ids: Vec<String> = self
            .timeline
            .read(cx)
            .state
            .find_track(track_id)
            .map(|track| {
                track
                    .inserts
                    .iter()
                    .map(|insert| insert.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let insert_ids: Vec<String> = self
            .plugin_editors
            .open
            .keys()
            .filter(|(tid, _)| tid == track_id)
            .map(|(_, insert_id)| insert_id.clone())
            .collect();

        eprintln!(
            "[TrackDelete] track={} plugins_count={} plugin_editors={} reason=track_delete",
            track_id,
            plugin_ids.len(),
            insert_ids.len()
        );

        // 1. Silence the track's instrument now (Part 13: delete while sounding).
        //    The engine reload also panics on project_load, but doing it here
        //    stops audio without waiting for the background sync.
        if let Some(engine) = self.audio_bridge.engine.as_ref() {
            if let Err(error) = engine.midi_preview_all_notes_off(track_id.to_string()) {
                eprintln!("[TrackDelete] midi panic failed track_id={track_id} err={error}");
            }
        }

        // 2. Tear down every insert on the track (editor window + external
        //    bridge-host instance + engine bridge sink). `teardown_insert_instance`
        //    closes the editor, so the `insert_ids` set above is only used for the
        //    diagnostic count. The in-process VST3 graph nodes are released by the
        //    engine reconcile once `delete_track` removes the track and the next
        //    project sync runs, after which the C++ instances are destroyed.
        for plugin_id in &plugin_ids {
            self.teardown_insert_instance(track_id, plugin_id, cx, "track_delete");
            eprintln!("[GraphUpdate] remove_plugin_node={plugin_id}");
            eprintln!("[PluginUnload] plugin={plugin_id} released=pending_runtime_reconcile");
        }
    }

    pub(super) fn delete_selected_track(&mut self, cx: &mut Context<Self>) {
        let Some(track_id) = self
            .timeline
            .read(cx)
            .state
            .selection
            .selected_track_id
            .clone()
        else {
            return;
        };
        // Close editors + MIDI panic BEFORE the model mutation so the engine
        // reload triggered by `mark_dirty` can actually release the instances.
        self.cleanup_track_plugins_before_delete(&track_id, cx);
        let _ = self.timeline.update(cx, |timeline, cx| {
            let Some(snapshot) = TrackSnapshot::capture(&timeline.state, &track_id) else {
                return;
            };
            timeline.run_edit_command(EditCommand::DeleteTrack { snapshot }, cx);
        });
        self.mark_dirty();
    }

    pub(super) fn delete_selected_clip_or_track(&mut self, cx: &mut Context<Self>) {
        // Decide up front whether this gesture resolves to a *track* delete, so
        // plugin cleanup (close editors, MIDI panic) runs before the model
        // mutation. Mirrors the branch order inside the update below: automation
        // points win, then a selected clip, then the track.
        let track_to_delete: Option<String> = {
            use crate::components::timeline::timeline_state::TrackLaneMode;
            let state = &self.timeline.read(cx).state;
            let sel_track = state.selection.selected_track_id.clone();
            let is_automation_delete = sel_track
                .as_deref()
                .map(|tid| {
                    state.track_lane_mode(tid) == TrackLaneMode::Automation
                        && state.selected_automation_point_count(tid) > 0
                })
                .unwrap_or(false);
            let has_clip = !state.selection.selected_clip_ids.is_empty();
            if is_automation_delete || has_clip {
                None
            } else {
                sel_track
            }
        };
        if let Some(track_id) = track_to_delete.as_deref() {
            self.cleanup_track_plugins_before_delete(track_id, cx);
        }

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
                if let Some(snapshot) = TrackSnapshot::capture(&timeline.state, &id) {
                    timeline.run_edit_command(EditCommand::DeleteTrack { snapshot }, cx);
                }
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
                let clip = timeline.state.clone_clip_for_insert(
                    &snapshot.clip,
                    timeline.state.next_clip_id(),
                    snapshot.clip.name.clone(),
                    (snapshot.clip.start_beat + offset).max(0.0),
                );
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
        let _ = self.timeline.update(cx, |timeline, cx| {
            let selected = timeline.state.selection.selected_clip_ids.clone();
            if selected.is_empty() {
                return;
            }
            let mut clips = Vec::new();
            for id in selected {
                if let Some((track_id, clip)) = timeline.state.build_clip_duplicate_after(&id) {
                    clips.push((track_id, clip));
                }
            }
            if clips.is_empty() {
                return;
            }
            timeline.run_edit_command(EditCommand::BatchCreateClips { clips }, cx);
        });
        self.mark_dirty();
    }

    pub(super) fn split_selected_audio_clip_at_playhead(&mut self, cx: &mut Context<Self>) {
        let mut did_split = false;
        let _ = self.timeline.update(cx, |timeline, cx| {
            let Some(clip_id) = timeline.state.selection.selected_clip_ids.first().cloned() else {
                return;
            };
            let Some(snapshot) = ClipSnapshot::capture(&timeline.state, &clip_id) else {
                return;
            };
            if !matches!(
                snapshot.clip.clip_type,
                crate::components::timeline::timeline_state::ClipType::Audio { .. }
            ) {
                return;
            }
            let split_beat = timeline.state.transport.playhead_beats;
            let clip_start = snapshot.clip.start_beat;
            let clip_end = snapshot.clip.start_beat + snapshot.clip.duration_beats;
            let min_len = 0.25_f32;
            if split_beat <= clip_start + min_len || split_beat >= clip_end - min_len {
                return;
            }

            let left_len = split_beat - clip_start;
            let right_len = clip_end - split_beat;
            let right_offset = snapshot.clip.offset_beats + left_len;
            let next_id = timeline.state.next_clip_id();
            let next_next_id = next_clip_id_after(&next_id);

            let mut left = timeline.state.clone_clip_for_insert(
                &snapshot.clip,
                next_id,
                snapshot.clip.name.clone(),
                clip_start,
            );
            left.duration_beats = left_len;

            let mut right = timeline.state.clone_clip_for_insert(
                &snapshot.clip,
                next_next_id,
                format!("{} Split", snapshot.clip.name),
                split_beat,
            );
            right.duration_beats = right_len;
            right.offset_beats = right_offset;

            timeline.run_edit_command(
                EditCommand::ReplaceClipWithClips {
                    clips: vec![
                        (snapshot.track_id.clone(), left),
                        (snapshot.track_id.clone(), right),
                    ],
                    snapshot,
                },
                cx,
            );
            did_split = true;
        });
        if did_split {
            self.mark_dirty();
        }
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

fn next_clip_id_after(id: &str) -> String {
    id.strip_prefix("clip-")
        .and_then(|rest| rest.parse::<u32>().ok())
        .map(|n| format!("clip-{}", n + 1))
        .unwrap_or_else(|| format!("{id}-split"))
}
