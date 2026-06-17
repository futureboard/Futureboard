//! Split out of `timeline.rs` (god-file decomposition): inherent `impl Timeline`.

use super::*;

impl Timeline {
    pub(super) fn hit_test_debug_enabled() -> bool {
        static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_HITTEST_DEBUG").is_some())
    }

    pub(super) fn input_debug_enabled() -> bool {
        static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FLAG.get_or_init(|| {
            std::env::var_os("FUTUREBOARD_TIMELINE_INPUT_DEBUG").is_some()
                || std::env::var_os("FUTUREBOARD_SELECTION_DEBUG").is_some()
        })
    }

    pub(super) fn log_input_state(&self, label: &str) {
        if Self::input_debug_enabled() {
            eprintln!(
                "[timeline-input] {label} pen_drag={} range_drag={} erase_drag={} automation_drag={} automation_marquee={} tempo_drag={} clip_drag_origin={} pan_drag={}",
                self.pen_clip_draw.is_some(),
                self.range_select_drag.is_some(),
                self.erase_clip_drag.is_some(),
                self.automation_drag.is_some(),
                self.automation_marquee.is_some(),
                self.tempo_drag.is_some(),
                self.clip_drag_origin.is_some(),
                self.pan_last_position.is_some(),
            );
        }
    }

    pub(super) fn arrangement_coordinate_context(&self) -> ArrangementCoordinateContext {
        let panel_origin_px = gpui::point(px(SIDEBAR_WIDTH), px(APP_CHROME_HEIGHT));
        let viewport_origin_px = gpui::point(
            px(SIDEBAR_WIDTH + HEADER_WIDTH),
            px(APP_CHROME_HEIGHT + self.state.arrangement_content_top()),
        );
        ArrangementCoordinateContext {
            panel_origin_px,
            viewport_origin_px,
            scroll_x_px: self.state.viewport.scroll_x,
            scroll_y_px: self.state.viewport.scroll_y,
            zoom_px_per_beat: self.state.viewport.pixels_per_beat.max(0.0001),
            ruler_height_px: RULER_HEIGHT,
            track_header_width_px: HEADER_WIDTH,
        }
    }

    pub(super) fn resolve_context_target_from_window_point(
        &self,
        position: gpui::Point<gpui::Pixels>,
    ) -> TimelineContextTarget {
        let ctx = self.arrangement_coordinate_context();
        let result = hit_test_arrangement(&self.state, position, &ctx);
        if Self::hit_test_debug_enabled() {
            let screen_x: f32 = position.x.into();
            let screen_y: f32 = position.y.into();
            eprintln!(
                "Arrangement hit-test:\nscreen=({screen_x:.1},{screen_y:.1})\nlocal=({:.1},{:.1})\ntarget={}\n{}\nz_priority={}",
                result.local.viewport_x,
                result.local.viewport_y,
                result.target.kind(),
                format_arrangement_target_debug(&result.target),
                result.z_priority,
            );
        }
        match result.target {
            ArrangementHitTarget::EmptyArrangement { .. } => TimelineContextTarget::TimelineEmpty,
            ArrangementHitTarget::TrackHeader { track_id } => {
                TimelineContextTarget::TrackHeader(track_id)
            }
            ArrangementHitTarget::TrackLane {
                track_id,
                timeline_beat,
            } => TimelineContextTarget::TrackLane {
                track_id,
                beat: timeline_beat,
            },
            ArrangementHitTarget::AudioClip {
                track_id,
                clip_id,
                timeline_beat,
                local_beat,
            } => TimelineContextTarget::AudioClip {
                track_id,
                clip_id,
                beat: timeline_beat,
                local_beat,
            },
            ArrangementHitTarget::MidiClip {
                track_id,
                clip_id,
                timeline_beat,
                local_beat,
            } => TimelineContextTarget::MidiClip {
                track_id,
                clip_id,
                beat: timeline_beat,
                local_beat,
            },
            ArrangementHitTarget::Ruler { timeline_beat } => {
                TimelineContextTarget::Ruler(timeline_beat)
            }
            ArrangementHitTarget::Marker {
                marker_id,
                timeline_beat,
            } => TimelineContextTarget::Marker {
                marker_id,
                beat: timeline_beat,
            },
            ArrangementHitTarget::AutomationLane {
                track_id,
                lane_id,
                timeline_beat,
            } => TimelineContextTarget::AutomationLane {
                track_id,
                lane_id,
                beat: timeline_beat,
            },
        }
    }

    pub fn reset_input_state(&mut self) {
        self.log_input_state("reset-before");
        self.clip_drag_origin = None;
        self.clip_drag_target_track_index = None;
        self.clip_clone_drag_id = None;
        self.pen_clip_draw = None;
        self.range_select_drag = None;
        self.state.arrangement_range = None;
        self.erase_clip_drag = None;
        self.erase_preview_ids.clear();
        self.automation_drag = None;
        self.automation_marquee = None;
        self.tempo_drag = None;
        self.ts_drag = None;
        self.pan_last_position = None;
        self.state.clear_track_drag();
        self.state.cancel_track_height_resize();
        self.log_input_state("reset-after");
    }

    pub(super) fn cancel_active_gesture(&mut self, cx: &mut gpui::Context<Self>) {
        if Self::input_debug_enabled() {
            eprintln!("[selection] marquee_cancel");
        }
        self.reset_input_state();
        cx.notify();
    }

    /// Clean empty-project Timeline — the real runtime entry point.
    pub fn new() -> Self {
        Self {
            state: TimelineState::default(),
            edit_history: EditHistory::new(100),
            on_seek_beats: None,
            on_track_param_change: None,
            on_project_changed: None,
            on_loop_changed: None,
            on_tempo_map_changed: None,
            on_time_signature_map_changed: None,
            on_media_changed: None,
            on_add_track: None,
            last_drag_position: None,
            clip_drag_origin: None,
            clip_drag_target_track_index: None,
            clip_clone_drag_id: None,
            pen_clip_draw: None,
            range_select_drag: None,
            erase_clip_drag: None,
            erase_preview_ids: HashSet::new(),
            automation_drag: None,
            automation_marquee: None,
            tempo_drag: None,
            ts_drag: None,
            pan_last_position: None,
            on_context_menu: None,
            on_playhead_scrub_begin: None,
            on_playhead_scrub_end: None,
            on_open_editor: None,
            chrome_metrics: TimelineChromeMetrics::default(),
            project_root: None,
            focus_lost_subscription: None,
        }
    }

    /// Seeded demo Timeline. Use only from explicit dev/demo entry points;
    /// production startup should always call [`Timeline::new`].
    pub fn with_demo_content() -> Self {
        Self {
            state: TimelineState::demo_project(),
            edit_history: EditHistory::new(100),
            on_seek_beats: None,
            on_track_param_change: None,
            on_project_changed: None,
            on_loop_changed: None,
            on_tempo_map_changed: None,
            on_time_signature_map_changed: None,
            on_media_changed: None,
            on_add_track: None,
            last_drag_position: None,
            clip_drag_origin: None,
            clip_drag_target_track_index: None,
            clip_clone_drag_id: None,
            pen_clip_draw: None,
            range_select_drag: None,
            erase_clip_drag: None,
            erase_preview_ids: HashSet::new(),
            automation_drag: None,
            automation_marquee: None,
            tempo_drag: None,
            ts_drag: None,
            pan_last_position: None,
            on_context_menu: None,
            on_playhead_scrub_begin: None,
            on_playhead_scrub_end: None,
            on_open_editor: None,
            chrome_metrics: TimelineChromeMetrics::default(),
            project_root: None,
            focus_lost_subscription: None,
        }
    }

    pub fn run_edit_command(&mut self, cmd: EditCommand, cx: &mut gpui::Context<Self>) {
        cmd.execute(&mut self.state);
        self.edit_history.push(cmd);
        self.mark_project_changed(cx);
        cx.notify();
    }

    /// Record a command whose effect has already been applied to the state
    /// (e.g. a gesture that mutated `state` live). Pushes it onto the undo
    /// stack without re-executing, then marks the project changed.
    pub fn record_executed_command(&mut self, cmd: EditCommand, cx: &mut gpui::Context<Self>) {
        self.edit_history.push(cmd);
        self.mark_project_changed(cx);
        cx.notify();
    }

    pub fn undo_edit(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        if self.edit_history.undo(&mut self.state) {
            self.mark_project_changed(cx);
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn redo_edit(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        if self.edit_history.redo(&mut self.state) {
            self.mark_project_changed(cx);
            cx.notify();
            true
        } else {
            false
        }
    }

    pub fn delete_clip_command(&mut self, clip_id: &str, cx: &mut gpui::Context<Self>) {
        let Some(snapshot) = ClipSnapshot::capture(&self.state, clip_id) else {
            return;
        };
        self.run_edit_command(EditCommand::DeleteClip { snapshot }, cx);
    }

    pub(super) fn beat_from_window_x(&self, x: f32) -> f32 {
        let click_x = x - SIDEBAR_WIDTH - HEADER_WIDTH;
        self.state.x_to_beats(click_x)
    }

    pub(super) fn snap_beat(&self, beat: f32) -> f32 {
        let snapped_sec = self.state.snap_time(beat * self.state.seconds_per_beat());
        snapped_sec / self.state.seconds_per_beat()
    }

    /// Push the measured chrome panel sizes that surround the timeline so
    /// `scroll_geometry` can compute the real available body rect. Called
    /// by `StudioLayout` each render — cheap, no notify.
    pub fn set_chrome_metrics(&mut self, metrics: TimelineChromeMetrics) {
        self.chrome_metrics = metrics;
    }

    /// Push the current project's root folder (or `None` when Untitled). Called
    /// by `StudioLayout` each render — cheap, no notify. Drives eager
    /// copy-into-project for dropped audio.
    pub fn set_project_root(&mut self, root: Option<std::path::PathBuf>) {
        self.project_root = root;
    }

    pub fn set_context_menu_callback(&mut self, callback: Option<TimelineContextMenuCb>) {
        self.on_context_menu = callback;
    }

    pub fn set_open_editor_callback(&mut self, callback: Option<TimelineOpenEditorCb>) {
        self.on_open_editor = callback;
    }

    pub fn set_add_track_callback(&mut self, callback: Option<TimelineAddTrackCb>) {
        self.on_add_track = callback;
    }

    pub fn set_project_changed_callback(&mut self, callback: Option<TimelineProjectChangedCb>) {
        self.on_project_changed = callback;
    }

    pub fn set_loop_changed_callback(&mut self, callback: Option<TimelineProjectChangedCb>) {
        self.on_loop_changed = callback;
    }

    pub fn set_tempo_map_changed_callback(&mut self, callback: Option<TimelineProjectChangedCb>) {
        self.on_tempo_map_changed = callback;
    }

    pub fn set_time_signature_map_changed_callback(
        &mut self,
        callback: Option<TimelineProjectChangedCb>,
    ) {
        self.on_time_signature_map_changed = callback;
    }

    pub fn set_media_changed_callback(&mut self, callback: Option<TimelineProjectChangedCb>) {
        self.on_media_changed = callback;
    }

    pub(crate) fn mark_tempo_map_changed(&self, cx: &mut gpui::App) {
        if let Some(callback) = self.on_tempo_map_changed.as_ref() {
            callback(cx);
        } else {
            self.mark_project_changed(cx);
        }
    }

    pub(crate) fn mark_time_signature_map_changed(&self, cx: &mut gpui::App) {
        if let Some(callback) = self.on_time_signature_map_changed.as_ref() {
            callback(cx);
        } else {
            self.mark_project_changed(cx);
        }
    }

    pub(crate) fn mark_loop_changed(&self, cx: &mut gpui::App) {
        if let Some(callback) = self.on_loop_changed.as_ref() {
            callback(cx);
        } else {
            self.mark_project_changed(cx);
        }
    }

    pub(crate) fn mark_project_changed(&self, cx: &mut gpui::App) {
        if let Some(callback) = self.on_project_changed.as_ref() {
            callback(cx);
        }
    }

    pub(crate) fn mark_media_changed(&self, cx: &mut gpui::App) {
        if let Some(callback) = self.on_media_changed.as_ref() {
            callback(cx);
        }
    }

    pub(super) fn finish_pen_midi_clip(&mut self, end_beat: f32, cx: &mut gpui::Context<Self>) {
        use crate::components::timeline::timeline_state::{TrackType, MIN_MIDI_CLIP_BEATS};
        let Some(preview) = self.pen_clip_draw.take() else {
            return;
        };
        let track_id = preview.track_id;
        let track_type = self
            .state
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| t.track_type);
        if !matches!(track_type, Some(TrackType::Midi | TrackType::Instrument)) {
            return;
        }

        let (clip_start, length) = if let Some(range) = self.state.arrangement_range.as_ref() {
            let (range_start, range_end) = range.as_f32_range();
            let (lo, hi) = normalize_range(range_start, range_end);
            (lo, (hi - lo).max(MIN_MIDI_CLIP_BEATS))
        } else {
            // Commit exactly what the ghost preview showed: same start + snapped
            // length helper, fed the live end beat from release.
            compute_pen_clip_span(&self.state, preview.start_beat, end_beat)
        };

        if let Some(clip) = self.state.build_midi_clip(&track_id, clip_start, length) {
            let clip_id = clip.id.clone();
            self.run_edit_command(
                EditCommand::CreateClip {
                    track_id: track_id.clone(),
                    clip,
                },
                cx,
            );
            if crate::components::timeline::timeline_state::midi_debug_enabled() {
                eprintln!(
                    "[midi] clip created track={} clip={} start={:.3} len={:.3}",
                    track_id, clip_id, clip_start, length
                );
            }
        }
    }

    pub(super) fn finish_range_select(&mut self, end_beat: f32, cx: &mut gpui::Context<Self>) {
        let Some(drag) = self.range_select_drag.take() else {
            return;
        };
        let (lo, hi) = normalize_range(drag.start_beat, end_beat);
        let track_ids = self
            .state
            .arrangement_range
            .as_ref()
            .map(|range| range.track_ids.clone())
            .filter(|ids| !ids.is_empty())
            .unwrap_or_else(|| {
                self.state
                    .track_ids_between(&drag.start_track_id, &drag.start_track_id)
            });

        let mut hit_clip_ids = Vec::new();
        if drag.dragging && (hi - lo).abs() > f32::EPSILON {
            for track in &self.state.tracks {
                if !track_ids.iter().any(|id| id == &track.id) {
                    continue;
                }
                for clip in &track.clips {
                    let clip_start = clip.start_beat;
                    let clip_end = clip.start_beat + clip.duration_beats;
                    if clip_start < hi && clip_end > lo {
                        hit_clip_ids.push(clip.id.clone());
                    }
                }
            }
        }

        if drag.additive {
            for clip_id in hit_clip_ids {
                if !self.state.selection.selected_clip_ids.contains(&clip_id) {
                    self.state.selection.selected_clip_ids.push(clip_id);
                }
            }
        } else if drag.dragging {
            self.state.selection.selected_clip_ids = hit_clip_ids;
            self.state.selection.selected_track_id = track_ids.first().cloned();
        }

        if Self::input_debug_enabled() {
            eprintln!(
                "[selection] marquee_commit additive={} dragging={} selected={}",
                drag.additive,
                drag.dragging,
                self.state.selection.selected_clip_ids.len()
            );
        }

        // The marquee rectangle is a transient drag affordance only. Commit the
        // selected clip ids, then clear the overlay immediately on mouse-up.
        self.state.arrangement_range = None;
        cx.notify();
    }

    pub(super) fn finish_erase_clip_drag(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(erased) = self.erase_clip_drag.take() else {
            return;
        };
        self.erase_preview_ids.clear();
        if erased.is_empty() {
            return;
        }
        let snapshots: Vec<ClipSnapshot> = erased
            .iter()
            .filter_map(|id| ClipSnapshot::capture(&self.state, id))
            .collect();
        if snapshots.is_empty() {
            return;
        }
        self.erase_preview_ids.clear();
        self.run_edit_command(EditCommand::BatchDeleteClips { snapshots }, cx);
    }

    pub(super) fn update_erase_clip_drag(&mut self, beat: f32, cx: &mut gpui::Context<Self>) {
        let ids = self.state.clips_intersecting_beats(beat, beat);
        let set = self.erase_clip_drag.get_or_insert_with(HashSet::new);
        for id in ids {
            set.insert(id);
        }
        self.erase_preview_ids = set.clone();
        cx.notify();
    }

    pub(super) fn begin_erase_at(
        &mut self,
        beat: f32,
        clip_id: Option<String>,
        cx: &mut gpui::Context<Self>,
    ) {
        self.erase_clip_drag = Some(HashSet::new());
        if let Some(id) = clip_id {
            self.erase_clip_drag.as_mut().unwrap().insert(id);
        }
        self.update_erase_clip_drag(beat, cx);
    }

    // ── Automation lane interaction ──────────────────────────────────────────
    // Add/select/move/marquee/delete of automation points. Selection + marquee
    // are UI-only; point add/move commit dirty exactly once on mouse release.

    /// Map a window-space y to a lane-local automation value for `track_id`.
    pub(super) fn automation_value_from_window_y(&self, track_id: &str, window_y: f32) -> f32 {
        use crate::components::timeline::timeline_state::{
            automation_y_to_value, DEFAULT_TRACK_HEIGHT,
        };
        let row_layout = self.state.track_row_layout();
        let row = row_layout.row_for_track(track_id);
        let row_y = row.map(|r| r.y).unwrap_or(0.0);
        let row_h = row
            .map(|r| r.height)
            .unwrap_or(DEFAULT_TRACK_HEIGHT);
        let local_y = (window_y - APP_CHROME_HEIGHT - self.state.arrangement_content_top()
            + self.state.viewport.scroll_y)
            - row_y;
        automation_y_to_value(local_y, row_h)
    }

    pub(super) fn tempo_bpm_from_window_y(&self, window_y: f32) -> f64 {
        use crate::components::timeline::timeline_state::y_to_bpm;
        let lane_h = self.state.tempo_track_height();
        let local_y = (window_y - APP_CHROME_HEIGHT - RULER_HEIGHT - TEMPO_LANE_PAD).max(0.0);
        let (min_bpm, max_bpm) = self.state.tempo_lane_bpm_range();
        y_to_bpm(local_y, lane_h, min_bpm, max_bpm)
    }

    pub(super) fn begin_tempo_track_interaction(
        &mut self,
        beat: f64,
        bpm: f64,
        point_id: Option<String>,
        click_count: u32,
        cx: &mut Context<Self>,
    ) {
        if click_count >= 2 {
            if point_id.is_none() {
                if let Some(id) = self.state.add_tempo_point(beat, bpm) {
                    self.state.select_tempo_point(&id);
                    self.tempo_drag = Some(TempoPointDrag {
                        point_id: id,
                        moved: true,
                    });
                    self.mark_tempo_map_changed(cx);
                }
            }
            cx.notify();
            return;
        }

        if let Some(id) = point_id {
            self.state.select_tempo_point(&id);
            self.tempo_drag = Some(TempoPointDrag {
                point_id: id,
                moved: false,
            });
            cx.notify();
            return;
        }

        self.state.clear_tempo_point_selection();
        cx.notify();
    }

    pub(super) fn update_tempo_track_interaction(
        &mut self,
        window_x: f32,
        window_y: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(drag) = self.tempo_drag.clone() else {
            return false;
        };
        let beat = self.snap_beat(self.beat_from_window_x(window_x)).max(0.0) as f64;
        let bpm = self.tempo_bpm_from_window_y(window_y);
        if self.state.move_tempo_point(&drag.point_id, beat, bpm) {
            if let Some(d) = self.tempo_drag.as_mut() {
                d.moved = true;
            }
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(super) fn finish_tempo_track_interaction(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(drag) = self.tempo_drag.take() {
            if drag.moved {
                self.mark_tempo_map_changed(cx);
            }
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(super) fn begin_time_signature_track_interaction(
        &mut self,
        beat: f64,
        point_id: Option<String>,
        click_count: u32,
        cx: &mut Context<Self>,
    ) {
        if click_count >= 2 {
            if let Some(id) = point_id {
                self.state.select_time_signature_point(&id);
            } else {
                let pt = self.state.time_signature_map.time_signature_at_beat(beat);
                if let Some(id) =
                    self.state
                        .add_time_signature_point(beat, pt.numerator, pt.denominator)
                {
                    self.state.select_time_signature_point(&id);
                    self.ts_drag = Some(TimeSignaturePointDrag {
                        point_id: id,
                        moved: true,
                    });
                    self.mark_time_signature_map_changed(cx);
                }
            }
            cx.notify();
            return;
        }

        if let Some(id) = point_id {
            self.state.select_time_signature_point(&id);
            self.ts_drag = Some(TimeSignaturePointDrag {
                point_id: id,
                moved: false,
            });
            cx.notify();
            return;
        }

        self.state.clear_time_signature_point_selection();
        cx.notify();
    }

    pub(super) fn update_time_signature_track_interaction(
        &mut self,
        window_x: f32,
        _window_y: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(drag) = self.ts_drag.clone() else {
            return false;
        };
        let beat = self.snap_beat(self.beat_from_window_x(window_x)).max(0.0) as f64;
        if self.state.move_time_signature_point(&drag.point_id, beat) {
            if let Some(d) = self.ts_drag.as_mut() {
                d.moved = true;
            }
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(super) fn add_tempo_point_at_playhead_from_header(&mut self, cx: &mut Context<Self>) {
        let beat = self.state.transport.playhead_beats as f64;
        let bpm = self.state.effective_bpm_at_beat(beat);
        if let Some(id) = self.state.add_tempo_point(beat, bpm) {
            self.state.select_tempo_point(&id);
            self.mark_tempo_map_changed(cx);
            cx.notify();
        }
    }

    pub(super) fn add_time_signature_marker_at_playhead_from_header(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        let beat = self.state.transport.playhead_beats as f64;
        let pt = self.state.time_signature_map.time_signature_at_beat(beat);
        if let Some(id) = self
            .state
            .add_time_signature_point(beat, pt.numerator, pt.denominator)
        {
            self.state.select_time_signature_point(&id);
            self.mark_time_signature_map_changed(cx);
            cx.notify();
        }
    }

    pub(super) fn finish_time_signature_track_interaction(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(drag) = self.ts_drag.take() {
            if drag.moved {
                self.mark_time_signature_map_changed(cx);
            }
            cx.notify();
            true
        } else {
            false
        }
    }

    /// Mouse-down inside an automation lane: hit-test a point (select + begin
    /// move), else add a point (Pen) or start a marquee (Pointer).
    pub(super) fn begin_automation_interaction(
        &mut self,
        track_id: &str,
        beat: f32,
        value: f32,
        additive: bool,
        cx: &mut Context<Self>,
    ) {
        use crate::components::timeline::timeline_state::{
            AutomationMarquee, AutomationPointDrag, TrackLaneMode, AUTOMATION_LANE_PAD,
        };
        self.state.select_track(track_id);
        if self.state.track_lane_mode(track_id) != TrackLaneMode::Automation {
            return;
        }
        let target = self.state.active_automation_target(track_id);
        let Some(lane_id) = self.state.ensure_automation_lane(track_id, target) else {
            return;
        };

        let ppb = self.state.viewport.pixels_per_beat.max(1.0);
        let row_h = self.state.track_row_height_for_id(track_id);
        let usable = (row_h - 2.0 * AUTOMATION_LANE_PAD).max(1.0);
        let beat_tol = 8.0 / ppb;
        let value_tol = 8.0 / usable;

        if let Some(point_id) = self
            .state
            .automation_point_at(track_id, &lane_id, beat, value, beat_tol, value_tol)
        {
            // Select (UI-only) and begin a move drag.
            self.state
                .select_automation_point(track_id, &lane_id, point_id, additive);
            self.automation_drag = Some(AutomationPointDrag {
                track_id: track_id.to_string(),
                lane_id,
                point_id,
                moved: false,
            });
            cx.notify();
            return;
        }

        match self.state.active_tool {
            TimelineTool::Pen | TimelineTool::Automation => {
                // Add a point and begin dragging it. The commit happens once on
                // release (moved=true), so a plain click still persists the add.
                if !additive {
                    self.state.clear_automation_selection(track_id);
                }
                if let Some(point_id) = self
                    .state
                    .add_automation_point(track_id, &lane_id, beat, value)
                {
                    self.state
                        .select_automation_point(track_id, &lane_id, point_id, false);
                    self.automation_drag = Some(AutomationPointDrag {
                        track_id: track_id.to_string(),
                        lane_id,
                        point_id,
                        moved: true,
                    });
                }
                cx.notify();
            }
            _ => {
                // Pointer (and other tools): rubber-band marquee selection.
                if !additive {
                    self.state.clear_automation_selection(track_id);
                }
                self.automation_marquee = Some(AutomationMarquee {
                    track_id: track_id.to_string(),
                    lane_id,
                    start_beat: beat,
                    start_value: value,
                    cur_beat: beat,
                    cur_value: value,
                    additive,
                });
                cx.notify();
            }
        }
    }

    /// Live update during an automation drag or marquee. Returns true if a
    /// gesture was active and consumed the move.
    pub(super) fn update_automation_interaction(
        &mut self,
        window_x: f32,
        window_y: f32,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(drag) = self.automation_drag.clone() {
            let beat = self.snap_beat(self.beat_from_window_x(window_x)).max(0.0);
            let value = self.automation_value_from_window_y(&drag.track_id, window_y);
            self.state.move_automation_point(
                &drag.track_id,
                &drag.lane_id,
                drag.point_id,
                beat,
                value,
            );
            if let Some(d) = self.automation_drag.as_mut() {
                d.moved = true;
            }
            cx.notify();
            return true;
        }
        if let Some(mut m) = self.automation_marquee.clone() {
            let beat = self.beat_from_window_x(window_x).max(0.0);
            let value = self.automation_value_from_window_y(&m.track_id, window_y);
            m.cur_beat = beat;
            m.cur_value = value;
            self.state.marquee_select_automation(
                &m.track_id,
                &m.lane_id,
                m.start_beat,
                beat,
                m.start_value,
                value,
                m.additive,
            );
            self.automation_marquee = Some(m);
            cx.notify();
            return true;
        }
        false
    }

    /// Commit an automation gesture on mouse release. Point moves/adds dirty the
    /// project exactly once; marquee selection is UI-only. Returns true if a
    /// gesture was active.
    pub(super) fn finish_automation_interaction(&mut self, cx: &mut Context<Self>) -> bool {
        let mut handled = false;
        if let Some(drag) = self.automation_drag.take() {
            if drag.moved {
                self.mark_project_changed(cx);
            }
            handled = true;
        }
        if self.automation_marquee.take().is_some() {
            handled = true;
        }
        if handled {
            cx.notify();
        }
        handled
    }

    pub(super) fn timeline_content_width(&self) -> f32 {
        let longest_seconds = self
            .state
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| {
                self.state
                    .beats_to_seconds(clip.start_beat + clip.duration_beats)
                    + 4.0
            })
            .fold(16.0_f32, f32::max);
        (longest_seconds * self.state.viewport.pixels_per_second).max(1200.0)
    }

    pub fn set_native_audio_callbacks(
        &mut self,
        on_seek_beats: Option<
            std::sync::Arc<dyn Fn(f32, f32, crate::layout::SeekReason) + Send + Sync + 'static>,
        >,
        on_track_param_change: Option<
            std::sync::Arc<dyn Fn(String, String, f32) + Send + Sync + 'static>,
        >,
    ) {
        self.on_seek_beats = on_seek_beats;
        self.on_track_param_change = on_track_param_change;
    }

    pub fn set_playhead_scrub_callbacks(
        &mut self,
        on_begin: Option<
            std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + Send + Sync + 'static>,
        >,
        on_end: Option<
            std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + Send + Sync + 'static>,
        >,
    ) {
        self.on_playhead_scrub_begin = on_begin;
        self.on_playhead_scrub_end = on_end;
    }

    pub fn seek_to_beat(&mut self, beat: f32, cx: &mut Context<Self>) {
        self.seek_to_beat_with_reason(beat, crate::layout::SeekReason::TimelineClick, cx);
    }

    pub fn seek_to_beat_with_reason(
        &mut self,
        beat: f32,
        reason: crate::layout::SeekReason,
        cx: &mut Context<Self>,
    ) {
        let snapped_sec = self
            .state
            .snap_time(beat.max(0.0) * self.state.seconds_per_beat());
        self.state.transport.playhead_beats = snapped_sec / self.state.seconds_per_beat();
        let beat = self.state.transport.playhead_beats;
        self.state.recompute_effective_volumes(beat, "seek");
        if let Some(cb) = self.on_seek_beats.as_ref() {
            cb(beat, self.state.bpm, reason);
        }
        cx.notify();
    }

    pub(super) fn max_scroll_offsets(&self, window: &Window) -> (f32, f32) {
        self.scroll_geometry(window).2
    }

    pub(super) fn scroll_geometry(&self, window: &Window) -> (f32, f32, (f32, f32)) {
        let window_size = window.bounds().size;
        let window_w: f32 = window_size.width.into();
        let window_h: f32 = window_size.height.into();
        let m = self.chrome_metrics;
        // Width: window minus browser/sidebar (only when actually shown via
        // its measured width), inspector (only when shown), and the
        // timeline's own fixed track-header column.
        let track_view_w = (window_w - m.browser_width - m.inspector_width - HEADER_WIDTH).max(0.0);
        // Height: window minus app chrome, ruler, the actual current
        // bottom panel height (0 when hidden), and status bar. No magic
        // 220 — the previous constant was stale whenever the bottom
        // panel was resized or hidden, which left the timeline either
        // too short (blank bottom area) or too tall (overflowing).
        let used_v = APP_CHROME_HEIGHT
            + self.state.arrangement_content_top()
            + m.bottom_panel_height
            + m.status_bar_height;
        let track_view_h = (window_h - used_v).max(DEFAULT_TRACK_HEIGHT);
        let content_w = self.timeline_content_width();
        let content_h = self.state.total_track_rows_height();

        if std::env::var_os("FUTUREBOARD_TIMELINE_VIEWPORT_DEBUG").is_some() {
            eprintln!(
                "[tl-viewport] window={}x{} body={}x{} browser={} inspector={} bottom={} status={} content={}x{}",
                window_w,
                window_h,
                track_view_w,
                track_view_h,
                m.browser_width,
                m.inspector_width,
                m.bottom_panel_height,
                m.status_bar_height,
                content_w,
                content_h
            );
        }

        (
            track_view_w,
            track_view_h,
            (
                (content_w - track_view_w).max(0.0),
                (content_h - track_view_h).max(0.0),
            ),
        )
    }

    pub(super) fn move_dragged_clip_to_position(
        &mut self,
        drag: &ClipDragItem,
        position: gpui::Point<gpui::Pixels>,
        window: &Window,
    ) {
        let origin = *self.clip_drag_origin.get_or_insert(position);
        let (target_index, snapped) = self.resolve_clip_drag_target(drag, origin, position);
        self.clip_drag_target_track_index = Some(target_index);

        let Some((current_drag_track_id, current_drag_start)) = self
            .state
            .find_clip(&drag.clip_id)
            .map(|(track, clip)| (track.id.clone(), clip.start_beat))
        else {
            return;
        };
        let beat_delta = snapped - current_drag_start;
        let drag_ids = self.clip_drag_selection_ids(&drag.clip_id);

        for clip_id in &drag_ids {
            let Some((track_id, start_beat)) = self
                .state
                .find_clip(clip_id)
                .map(|(track, clip)| (track.id.clone(), clip.start_beat))
            else {
                continue;
            };
            let next_start = if clip_id == &drag.clip_id {
                snapped
            } else {
                (start_beat + beat_delta).max(0.0)
            };
            self.state
                .move_clip_to_track(clip_id, &track_id, next_start);
        }
        self.restore_clip_drag_selection(&drag.clip_id, drag_ids, Some(current_drag_track_id));

        let (max_x, max_y) = self.max_scroll_offsets(window);
        self.state.viewport.scroll_x = self.state.viewport.scroll_x.clamp(0.0, max_x);
        self.state.viewport.scroll_y = self.state.viewport.scroll_y.clamp(0.0, max_y);
    }

    pub(super) fn clip_drag_selection_ids(&self, dragged_clip_id: &str) -> Vec<String> {
        let selected = &self.state.selection.selected_clip_ids;
        if selected.iter().any(|id| id == dragged_clip_id) {
            selected
                .iter()
                .filter(|id| self.state.find_clip(id).is_some())
                .cloned()
                .collect()
        } else {
            vec![dragged_clip_id.to_string()]
        }
    }

    pub(super) fn restore_clip_drag_selection(
        &mut self,
        dragged_clip_id: &str,
        clip_ids: Vec<String>,
        fallback_track_id: Option<String>,
    ) {
        let existing = clip_ids
            .into_iter()
            .filter(|id| self.state.find_clip(id).is_some())
            .collect::<Vec<_>>();
        if existing.is_empty() {
            return;
        }

        let selected_track_id = self
            .state
            .find_clip(dragged_clip_id)
            .map(|(track, _)| track.id.clone())
            .or(fallback_track_id)
            .or_else(|| {
                existing
                    .first()
                    .and_then(|id| self.state.find_clip(id))
                    .map(|(track, _)| track.id.clone())
            });
        self.state.selection.selected_track_id = selected_track_id;
        self.state.selection.selected_clip_ids = existing;
    }

    pub(super) fn resolve_clip_drag_target(
        &self,
        drag: &ClipDragItem,
        origin: gpui::Point<gpui::Pixels>,
        position: gpui::Point<gpui::Pixels>,
    ) -> (usize, f32) {
        let dx: f32 = (position.x - origin.x).into();
        let dy: f32 = (position.y - origin.y).into();
        let ppb = self.state.viewport.pixels_per_second * self.state.seconds_per_beat();
        let new_start = (drag.start_beat + dx / ppb.max(1.0)).max(0.0);
        let snapped = self.state.snap_beats(new_start).max(0.0);

        let source_index = self
            .state
            .tracks
            .iter()
            .position(|track| track.id == drag.source_track_id)
            .unwrap_or(0);
        let viewport_y = self.track_area_y_from_window(position);
        let target_index = self
            .state
            .track_index_at_y(viewport_y)
            .unwrap_or(source_index);
        (target_index, snapped)
    }

    pub(super) fn build_clip_clone_at(
        &self,
        source_clip_id: &str,
        target_track_id: &str,
        start_beat: f32,
    ) -> Option<(String, ClipState)> {
        let (_, source) = self.state.find_clip(source_clip_id)?;
        let clip = self.state.clone_clip_for_insert(
            source,
            self.state.next_clip_id(),
            format!("{} Copy", source.name),
            start_beat,
        );
        Some((target_track_id.to_string(), clip))
    }

    pub(super) fn create_clip_clone_at(
        &mut self,
        source_clip_id: &str,
        target_track_id: &str,
        start_beat: f32,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some((track_id, clip)) =
            self.build_clip_clone_at(source_clip_id, target_track_id, start_beat)
        else {
            return false;
        };
        self.run_edit_command(EditCommand::CreateClip { track_id, clip }, cx);
        true
    }

    pub(super) fn track_area_y_from_window(&self, position: gpui::Point<gpui::Pixels>) -> f32 {
        let y: f32 = position.y.into();
        (y - APP_CHROME_HEIGHT - self.state.arrangement_content_top()).max(0.0)
    }

    pub(super) fn import_audio_path_at_last_drag(
        &mut self,
        path: &std::path::Path,
        force_new_track: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !is_supported_audio_ext(path) {
            return false;
        }

        let path_key = path.to_string_lossy().to_string();
        let clip_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Imported Audio".to_string());

        let (drop_x, drop_y) = self.drop_position_or_new_track(force_new_track);

        self.state
            .import_audio_at(path_key.clone(), clip_name, drop_x, drop_y);
        self.mark_project_changed(cx);
        self.mark_media_changed(cx);
        super::super::audio_import::spawn_timeline_import(
            path.to_path_buf(),
            self.project_root.clone(),
            cx.entity().clone(),
            None,
            cx,
        );
        true
    }

    pub(super) fn import_midi_path_at_last_drag(
        &mut self,
        path: &std::path::Path,
        force_new_track: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !is_supported_midi_ext(path) {
            return false;
        }

        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!(
                    "[MidiImport] read failed path={} err={error}",
                    path.display()
                );
                return false;
            }
        };
        let imported = match super::super::midi_import::parse_smf_notes(&bytes) {
            Ok(imported) => imported,
            Err(error) => {
                eprintln!(
                    "[MidiImport] parse failed path={} err={error}",
                    path.display()
                );
                return false;
            }
        };
        let clip_name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Imported MIDI".to_string());
        let (drop_x, drop_y) = self.drop_position_or_new_track(force_new_track);
        let Some((track_id, clip)) = self
            .state
            .import_midi_at(clip_name, imported, drop_x, drop_y)
        else {
            return false;
        };
        if crate::components::timeline::timeline_state::midi_debug_enabled() {
            eprintln!(
                "[MidiImport] imported path={} track={} clip={} notes={}",
                path.display(),
                track_id,
                clip.id,
                match &clip.clip_type {
                    crate::components::timeline::timeline_state::ClipType::Midi {
                        notes, ..
                    } => notes.len(),
                    _ => 0,
                }
            );
        }
        self.run_edit_command(EditCommand::CreateClip { track_id, clip }, cx);
        true
    }

    pub(super) fn drop_position_or_new_track(&self, force_new_track: bool) -> (f32, f32) {
        match self.last_drag_position {
            Some(p) if !force_new_track => {
                let x: f32 = p.x.into();
                let y: f32 = p.y.into();
                let lane_x = (x - SIDEBAR_WIDTH - HEADER_WIDTH).max(0.0);
                let lane_y =
                    (y - APP_CHROME_HEIGHT - self.state.arrangement_content_top()).max(0.0);
                (lane_x, lane_y)
            }
            _ => (0.0, 1.0e9_f32),
        }
    }
}
