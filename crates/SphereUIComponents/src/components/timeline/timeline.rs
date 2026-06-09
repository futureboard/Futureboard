use crate::assets;
use crate::components::edit::{normalize_range, ClipSnapshot, EditCommand, EditHistory};
use crate::components::sidebar::{BrowserDragItem, SIDEBAR_WIDTH};
use crate::components::timeline::floating_tools_bar::floating_tools_bar;
use crate::components::timeline::tempo_track::tempo_track_lane;
use crate::components::timeline::time_signature_track::time_signature_track_lane;
use crate::components::timeline::timeline_ruler::timeline_ruler;
use crate::components::timeline::timeline_state::{
    ClipDragItem, ClipResizeDrag, SnapDivision, TempoPointDrag, TimeSignaturePointDrag,
    TimelineRangeSelection,
    TimelineState, TimelineTool, TrackDragItem, TrackType, HEADER_WIDTH, RULER_HEIGHT,
    TEMPO_LANE_PAD, TRACK_HEIGHT,
};
use crate::components::timeline::track_list::track_list;
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
use gpui::{
    div, pulsating_between, px, svg, Animation, AnimationExt, AppContext, Context, Empty,
    ExternalPaths, InteractiveElement, IntoElement, ParentElement, Render, ScrollDelta,
    StatefulInteractiveElement, Styled, Subscription, Window,
};
use std::time::Duration;

/// App chrome (top titlebar/menu strip) — used to convert window-space y into
/// the timeline track area. Mirrors the value used by app_chrome.
const APP_CHROME_HEIGHT: f32 = 36.0;
const MARQUEE_DRAG_THRESHOLD: f32 = 4.0;

/// Sizes of the surrounding chrome panels that the timeline's scroll/grid
/// math has to subtract from the window to know the actual timeline body
/// rect. Pushed by `StudioLayout` each render so resizing the bottom
/// panel, toggling browser/inspector, and maximizing the window all stay
/// in sync — no hardcoded constants.
#[derive(Clone, Copy, Debug, Default)]
pub struct TimelineChromeMetrics {
    pub browser_width: f32,
    pub inspector_width: f32,
    pub bottom_panel_height: f32,
    pub status_bar_height: f32,
}

/// Live pen-tool MIDI clip draw. Held only while the gesture is in flight
/// (mouse-down → mouse-up); the real clip is created once on release. `start_beat`
/// is snapped at mouse-down; `current_beat` tracks the snapped cursor while
/// dragging so the ghost preview and the committed clip share one set of bounds.
#[derive(Clone, Debug)]
struct ClipDrawPreview {
    track_id: String,
    start_beat: f32,
    current_beat: f32,
    /// `true` once the cursor has moved past the start — distinguishes a plain
    /// click (default-length clip) from a drag (sized clip).
    dragging: bool,
}

#[derive(Clone, Debug)]
struct RangeSelectDrag {
    start_beat: f32,
    current_beat: f32,
    start_track_id: String,
    additive: bool,
    dragging: bool,
}

fn is_supported_audio_ext(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("wav") | Some("mp3") | Some("flac") | Some("ogg")
    )
}

use std::collections::HashSet;

pub struct Timeline {
    pub state: TimelineState,
    edit_history: EditHistory,
    on_seek_beats: Option<std::sync::Arc<dyn Fn(f32, f32) + Send + Sync + 'static>>,
    on_track_param_change:
        Option<std::sync::Arc<dyn Fn(String, String, f32) + Send + Sync + 'static>>,
    on_project_changed: Option<TimelineProjectChangedCb>,
    on_tempo_map_changed: Option<TimelineProjectChangedCb>,
    on_time_signature_map_changed: Option<TimelineProjectChangedCb>,
    on_media_changed: Option<TimelineProjectChangedCb>,
    on_add_track: Option<TimelineAddTrackCb>,
    /// Window-space position of the last drag-move event while files are
    /// being dragged. We need this because `on_drop::<ExternalPaths>` does
    /// not carry the drop position itself — gpui translates the submit into
    /// a synthetic MouseUp, so we have to remember the last cursor position
    /// observed during the drag.
    last_drag_position: Option<gpui::Point<gpui::Pixels>>,
    clip_drag_origin: Option<gpui::Point<gpui::Pixels>>,
    clip_drag_target_track_index: Option<usize>,
    /// Pen-tool click-drag MIDI clip preview, live until mouse-up creates the clip.
    pen_clip_draw: Option<ClipDrawPreview>,
    /// Pointer-tool empty-lane marquee. Rule: Pointer + empty lane drag starts
    /// replace-marquee; Ctrl/Cmd + Pointer + empty lane drag starts additive
    /// marquee. Clips, rulers, toolbar controls, and non-pointer tools never
    /// start this gesture.
    range_select_drag: Option<RangeSelectDrag>,
    /// Right-drag erase: clip ids already queued for deletion this gesture.
    erase_clip_drag: Option<HashSet<String>>,
    /// Live preview of clip ids marked for erase (mirrors `erase_clip_drag`).
    erase_preview_ids: HashSet<String>,
    /// In-flight automation point move. Mutated live; committed once on release.
    automation_drag: Option<crate::components::timeline::timeline_state::AutomationPointDrag>,
    /// In-flight automation marquee (rubber-band) selection. UI-only.
    automation_marquee: Option<crate::components::timeline::timeline_state::AutomationMarquee>,
    /// In-flight tempo-point drag on the global Tempo Track lane.
    tempo_drag: Option<TempoPointDrag>,
    /// In-flight time-signature marker drag on the global Time Signature lane.
    ts_drag: Option<TimeSignaturePointDrag>,
    pan_last_position: Option<gpui::Point<gpui::Pixels>>,
    on_context_menu: Option<TimelineContextMenuCb>,
    /// Invoked when the user double-clicks a MIDI clip — `StudioLayout` uses it
    /// to switch the bottom panel to the piano-roll Editor tab.
    on_open_editor: Option<TimelineOpenEditorCb>,
    chrome_metrics: TimelineChromeMetrics,
    focus_lost_subscription: Option<Subscription>,
}

pub type TimelineOpenEditorCb = std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + 'static>;

#[derive(Clone, Debug)]
pub enum TimelineContextTarget {
    TimelineEmpty,
    TrackHeader(String),
    Clip(String),
    /// Right-click on the arrangement ruler. Carries the beat under the cursor.
    Ruler(f64),
    /// Right-click on the global Tempo Track lane.
    TempoTrack {
        beat: f64,
        bpm: f64,
        point_id: Option<String>,
    },
    /// Right-click on the global Time Signature Track lane.
    TimeSignatureTrack {
        beat: f64,
        point_id: Option<String>,
    },
    /// Lane header menu button on the Tempo track.
    TempoLaneHeader,
    /// Lane header menu button on the Time Signature track.
    TimeSignatureLaneHeader,
}

pub type TimelineContextMenuCb = std::sync::Arc<
    dyn Fn(&(TimelineContextTarget, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
>;

#[derive(Clone, Copy, Debug)]
pub struct TimelineAddTrackRequest {
    pub track_count: usize,
    pub has_master_track: bool,
}

pub type TimelineAddTrackCb =
    std::sync::Arc<dyn Fn(&TimelineAddTrackRequest, &mut gpui::Window, &mut gpui::App) + 'static>;

pub type TimelineProjectChangedCb = std::sync::Arc<dyn Fn(&mut gpui::App) + 'static>;

#[derive(Clone, Debug)]
struct ScrollbarDrag {
    axis: ScrollAxis,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ScrollAxis {
    Horizontal,
    Vertical,
}

impl Render for ScrollbarDrag {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
}

// Clip edge-resize uses GPUI's drag system with no visible drag image, so the
// payload renders as `Empty` (same as the scrollbar thumb drag).
impl Render for ClipResizeDrag {
    fn render(&mut self, _w: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Timeline {
    fn input_debug_enabled() -> bool {
        static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FLAG.get_or_init(|| {
            std::env::var_os("FUTUREBOARD_TIMELINE_INPUT_DEBUG").is_some()
                || std::env::var_os("FUTUREBOARD_SELECTION_DEBUG").is_some()
        })
    }

    fn log_input_state(&self, label: &str) {
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

    pub fn reset_input_state(&mut self) {
        self.log_input_state("reset-before");
        self.clip_drag_origin = None;
        self.clip_drag_target_track_index = None;
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
        self.log_input_state("reset-after");
    }

    fn cancel_active_gesture(&mut self, cx: &mut gpui::Context<Self>) {
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
            on_tempo_map_changed: None,
            on_time_signature_map_changed: None,
            on_media_changed: None,
            on_add_track: None,
            last_drag_position: None,
            clip_drag_origin: None,
            clip_drag_target_track_index: None,
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
            on_open_editor: None,
            chrome_metrics: TimelineChromeMetrics::default(),
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
            on_tempo_map_changed: None,
            on_time_signature_map_changed: None,
            on_media_changed: None,
            on_add_track: None,
            last_drag_position: None,
            clip_drag_origin: None,
            clip_drag_target_track_index: None,
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
            on_open_editor: None,
            chrome_metrics: TimelineChromeMetrics::default(),
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

    fn beat_from_window_x(&self, x: f32) -> f32 {
        let click_x = x - SIDEBAR_WIDTH - HEADER_WIDTH;
        self.state.x_to_beats(click_x)
    }

    fn snap_beat(&self, beat: f32) -> f32 {
        let snapped_sec = self.state.snap_time(beat * self.state.seconds_per_beat());
        snapped_sec / self.state.seconds_per_beat()
    }

    /// Push the measured chrome panel sizes that surround the timeline so
    /// `scroll_geometry` can compute the real available body rect. Called
    /// by `StudioLayout` each render — cheap, no notify.
    pub fn set_chrome_metrics(&mut self, metrics: TimelineChromeMetrics) {
        self.chrome_metrics = metrics;
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

    pub(crate) fn mark_project_changed(&self, cx: &mut gpui::App) {
        if let Some(callback) = self.on_project_changed.as_ref() {
            callback(cx);
        }
    }

    fn mark_media_changed(&self, cx: &mut gpui::App) {
        if let Some(callback) = self.on_media_changed.as_ref() {
            callback(cx);
        }
    }

    fn finish_pen_midi_clip(&mut self, end_beat: f32, cx: &mut gpui::Context<Self>) {
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

    fn finish_range_select(&mut self, end_beat: f32, cx: &mut gpui::Context<Self>) {
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

    fn finish_erase_clip_drag(&mut self, cx: &mut gpui::Context<Self>) {
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

    fn update_erase_clip_drag(&mut self, beat: f32, cx: &mut gpui::Context<Self>) {
        let ids = self.state.clips_intersecting_beats(beat, beat);
        let set = self.erase_clip_drag.get_or_insert_with(HashSet::new);
        for id in ids {
            set.insert(id);
        }
        self.erase_preview_ids = set.clone();
        cx.notify();
    }

    fn begin_erase_at(&mut self, beat: f32, clip_id: Option<String>, cx: &mut gpui::Context<Self>) {
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
    fn automation_value_from_window_y(&self, track_id: &str, window_y: f32) -> f32 {
        use crate::components::timeline::timeline_state::automation_y_to_value;
        let index = self
            .state
            .tracks
            .iter()
            .position(|t| t.id == track_id)
            .unwrap_or(0);
        let local_y = (window_y - APP_CHROME_HEIGHT - self.state.arrangement_content_top()
            + self.state.viewport.scroll_y)
            - index as f32 * TRACK_HEIGHT;
        automation_y_to_value(local_y, TRACK_HEIGHT)
    }

    fn tempo_bpm_from_window_y(&self, window_y: f32) -> f64 {
        use crate::components::timeline::timeline_state::y_to_bpm;
        let lane_h = self.state.tempo_track_height();
        let local_y = (window_y - APP_CHROME_HEIGHT - RULER_HEIGHT - TEMPO_LANE_PAD).max(0.0);
        let (min_bpm, max_bpm) = self.state.tempo_lane_bpm_range();
        y_to_bpm(local_y, lane_h, min_bpm, max_bpm)
    }

    fn begin_tempo_track_interaction(
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

    fn update_tempo_track_interaction(
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

    fn finish_tempo_track_interaction(&mut self, cx: &mut Context<Self>) -> bool {
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

    fn begin_time_signature_track_interaction(
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
                if let Some(id) = self.state.add_time_signature_point(
                    beat,
                    pt.numerator,
                    pt.denominator,
                ) {
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

    fn update_time_signature_track_interaction(
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

    fn add_tempo_point_at_playhead_from_header(&mut self, cx: &mut Context<Self>) {
        let beat = self.state.transport.playhead_beats as f64;
        let bpm = self.state.effective_bpm_at_beat(beat);
        if let Some(id) = self.state.add_tempo_point(beat, bpm) {
            self.state.select_tempo_point(&id);
            self.mark_tempo_map_changed(cx);
            cx.notify();
        }
    }

    fn add_time_signature_marker_at_playhead_from_header(&mut self, cx: &mut Context<Self>) {
        let beat = self.state.transport.playhead_beats as f64;
        let pt = self
            .state
            .time_signature_map
            .time_signature_at_beat(beat);
        if let Some(id) = self.state.add_time_signature_point(
            beat,
            pt.numerator,
            pt.denominator,
        ) {
            self.state.select_time_signature_point(&id);
            self.mark_time_signature_map_changed(cx);
            cx.notify();
        }
    }

    fn finish_time_signature_track_interaction(&mut self, cx: &mut Context<Self>) -> bool {
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
    fn begin_automation_interaction(
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
        let usable = (TRACK_HEIGHT - 2.0 * AUTOMATION_LANE_PAD).max(1.0);
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
    fn update_automation_interaction(
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
    fn finish_automation_interaction(&mut self, cx: &mut Context<Self>) -> bool {
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

    fn timeline_content_width(&self) -> f32 {
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
        on_seek_beats: Option<std::sync::Arc<dyn Fn(f32, f32) + Send + Sync + 'static>>,
        on_track_param_change: Option<
            std::sync::Arc<dyn Fn(String, String, f32) + Send + Sync + 'static>,
        >,
    ) {
        self.on_seek_beats = on_seek_beats;
        self.on_track_param_change = on_track_param_change;
    }

    fn max_scroll_offsets(&self, window: &Window) -> (f32, f32) {
        self.scroll_geometry(window).2
    }

    fn scroll_geometry(&self, window: &Window) -> (f32, f32, (f32, f32)) {
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
        let track_view_h = (window_h - used_v).max(TRACK_HEIGHT);
        let content_w = self.timeline_content_width();
        let content_h = self.state.tracks.len() as f32 * TRACK_HEIGHT;

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

    fn move_dragged_clip_to_position(
        &mut self,
        drag: &ClipDragItem,
        position: gpui::Point<gpui::Pixels>,
        window: &Window,
    ) {
        let origin = *self.clip_drag_origin.get_or_insert(position);
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
        let slot = (dy / TRACK_HEIGHT).round() as isize;
        let max_index = self.state.tracks.len().saturating_sub(1) as isize;
        let target_index = (source_index as isize + slot).clamp(0, max_index) as usize;
        self.clip_drag_target_track_index = Some(target_index);

        let current_track_id = self
            .state
            .find_clip(&drag.clip_id)
            .map(|(track, _)| track.id.clone())
            .unwrap_or_else(|| drag.source_track_id.clone());
        self.state
            .move_clip_to_track(&drag.clip_id, &current_track_id, snapped);

        let (max_x, max_y) = self.max_scroll_offsets(window);
        self.state.viewport.scroll_x = self.state.viewport.scroll_x.clamp(0.0, max_x);
        self.state.viewport.scroll_y = self.state.viewport.scroll_y.clamp(0.0, max_y);
    }

    fn track_area_y_from_window(&self, position: gpui::Point<gpui::Pixels>) -> f32 {
        let y: f32 = position.y.into();
        (y - APP_CHROME_HEIGHT - self.state.arrangement_content_top()).max(0.0)
    }

    fn import_audio_path_at_last_drag(
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

        let (drop_x, drop_y) = match self.last_drag_position {
            Some(p) if !force_new_track => {
                let x: f32 = p.x.into();
                let y: f32 = p.y.into();
                let lane_x = (x - SIDEBAR_WIDTH - HEADER_WIDTH).max(0.0);
                let lane_y =
                    (y - APP_CHROME_HEIGHT - self.state.arrangement_content_top()).max(0.0);
                (lane_x, lane_y)
            }
            _ => (0.0, 1.0e9_f32),
        };

        self.state
            .import_audio_at(path_key.clone(), clip_name, drop_x, drop_y);
        self.mark_project_changed(cx);
        self.mark_media_changed(cx);
        super::audio_import::spawn_timeline_import(
            path.to_path_buf(),
            cx.entity().clone(),
            None,
            cx,
        );
        true
    }
}

impl Render for Timeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _tl_scope = crate::perf::PerfScope::enter("Timeline");
        let (viewport_w, viewport_h, (scroll_max_x, scroll_max_y)) = self.scroll_geometry(window);
        self.state.update_viewport_size(viewport_w, viewport_h);
        self.state.clamp_scroll(scroll_max_x, scroll_max_y);
        let scrolling = self.state.smooth_scroll_towards_target();
        if scrolling {
            cx.notify();
        }
        if self.focus_lost_subscription.is_none() {
            self.focus_lost_subscription = Some(cx.on_focus_lost(window, |this, _window, cx| {
                if this.range_select_drag.is_some()
                    || this.pen_clip_draw.is_some()
                    || this.erase_clip_drag.is_some()
                    || this.automation_drag.is_some()
                    || this.automation_marquee.is_some()
                    || this.tempo_drag.is_some()
                    || this.pan_last_position.is_some()
                {
                    if Self::input_debug_enabled() {
                        eprintln!("[selection] focus_lost_cancel");
                    }
                    this.cancel_active_gesture(cx);
                }
            }));
        }

        crate::perf::count(
            "clips",
            self.state
                .tracks
                .iter()
                .map(|t| t.clips.len() as u64)
                .sum::<u64>(),
        );

        let on_select_track = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.select_track(track_id);
            cx.notify();
        });

        let on_select_clip =
            cx.listener(|this, (clip_id, additive): &(String, bool), _window, cx| {
                if *additive {
                    this.state.select_clip_additive(clip_id);
                } else {
                    this.state.select_clip(clip_id);
                }
                cx.notify();
            });

        let on_toggle_mute = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_mute(track_id);
            this.mark_project_changed(cx);
            if let Some(track) = this.state.find_track(track_id) {
                if let Some(cb) = this.on_track_param_change.as_ref() {
                    cb(
                        track_id.clone(),
                        "mute".to_string(),
                        if track.muted { 1.0 } else { 0.0 },
                    );
                }
            }
            cx.notify();
        });

        let on_toggle_solo = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_solo(track_id);
            this.mark_project_changed(cx);
            if let Some(track) = this.state.find_track(track_id) {
                if let Some(cb) = this.on_track_param_change.as_ref() {
                    cb(
                        track_id.clone(),
                        "solo".to_string(),
                        if track.solo { 1.0 } else { 0.0 },
                    );
                }
            }
            cx.notify();
        });

        let on_toggle_arm = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.toggle_track_arm(track_id);
            this.mark_project_changed(cx);
            cx.notify();
        });

        let on_toggle_input = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.cycle_track_input_monitor(track_id);
            this.mark_project_changed(cx);
            cx.notify();
        });

        // Automation mode toggle is UI-only: it selects the track and flips the
        // lane edit mode but never marks the project/engine dirty on its own.
        let on_toggle_automation = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.select_track(track_id);
            this.state.toggle_track_lane_mode(track_id);
            cx.notify();
        });

        let on_delete_track = cx.listener(|this, track_id: &String, _window, cx| {
            this.state.tracks.retain(|t| t.id != *track_id);
            if this.state.selection.selected_track_id.as_ref() == Some(track_id) {
                this.state.selection.selected_track_id = None;
            }
            this.mark_project_changed(cx);
            cx.notify();
        });

        let on_volume_change =
            cx.listener(|this, (track_id, volume): &(String, f32), _window, cx| {
                this.state.set_track_volume(track_id, *volume);
                this.mark_project_changed(cx);
                if let Some(cb) = this.on_track_param_change.as_ref() {
                    cb(track_id.clone(), "volume".to_string(), *volume);
                }
                cx.notify();
            });

        let on_pan_change = cx.listener(|this, (track_id, pan): &(String, f32), _window, cx| {
            this.state.set_track_pan(track_id, *pan);
            this.mark_project_changed(cx);
            if let Some(cb) = this.on_track_param_change.as_ref() {
                cb(track_id.clone(), "pan".to_string(), *pan);
            }
            cx.notify();
        });

        let on_add_clip = cx.listener(|this, (track_id, beat): &(String, f32), _window, cx| {
            let track_type = this
                .state
                .tracks
                .iter()
                .find(|t| t.id == *track_id)
                .map(|t| t.track_type);
            match track_type {
                Some(TrackType::Audio) => {
                    // Audio clips require a real file import — pen draw does not
                    // create placeholder clips.
                }
                Some(TrackType::Midi | TrackType::Instrument) => {
                    if this.state.active_tool == TimelineTool::Pen {
                        let start = this.snap_beat(*beat);
                        this.pen_clip_draw = Some(ClipDrawPreview {
                            track_id: track_id.clone(),
                            start_beat: start,
                            current_beat: start,
                            dragging: false,
                        });
                    }
                }
                Some(TrackType::Bus | TrackType::Return | TrackType::Master) | None => {}
            }
            cx.notify();
        });

        let on_range_start = cx.listener(
            |this, (track_id, beat, additive): &(String, f32, bool), _window, cx| {
                if this.state.active_tool == TimelineTool::Pointer {
                    if Self::input_debug_enabled() {
                        eprintln!(
                            "[selection] marquee_start_pending track={} beat={:.3} additive={}",
                            track_id, beat, additive
                        );
                    }
                    this.range_select_drag = Some(RangeSelectDrag {
                        start_beat: *beat,
                        current_beat: *beat,
                        start_track_id: track_id.clone(),
                        additive: *additive,
                        dragging: false,
                    });
                    this.state.arrangement_range = None;
                    cx.notify();
                }
            },
        );

        let on_erase_start = cx.listener(|this, beat: &f32, _window, cx| {
            this.begin_erase_at(*beat, None, cx);
        });

        let on_erase_clip = cx.listener(|this, clip_id: &String, _window, cx| {
            let beat = this
                .state
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter())
                .find(|c| c.id == *clip_id)
                .map(|c| c.start_beat)
                .unwrap_or(0.0);
            this.begin_erase_at(beat, Some(clip_id.clone()), cx);
        });

        let on_edit_mouse_move = cx.listener(|this, event: &gpui::MouseMoveEvent, _window, cx| {
            if Self::input_debug_enabled() {
                eprintln!(
                    "[timeline-input] mouse-move pressed={:?} range_drag={} ctrl={} platform={} shift={}",
                    event.pressed_button,
                    this.range_select_drag.is_some(),
                    event.modifiers.control,
                    event.modifiers.platform,
                    event.modifiers.shift,
                );
            }
            if event.pressed_button.is_none()
                && (this.pen_clip_draw.is_some()
                    || this.range_select_drag.is_some()
                    || this.erase_clip_drag.is_some()
                    || this.automation_drag.is_some()
                    || this.automation_marquee.is_some()
                    || this.tempo_drag.is_some()
                    || this.ts_drag.is_some()
                    || this.pan_last_position.is_some())
            {
                this.reset_input_state();
                cx.notify();
                return;
            }
            if event.pressed_button == Some(gpui::MouseButton::Left)
                && (this.automation_drag.is_some()
                    || this.automation_marquee.is_some()
                    || this.tempo_drag.is_some()
                    || this.ts_drag.is_some())
            {
                if this.tempo_drag.is_some() {
                    this.update_tempo_track_interaction(
                        event.position.x.into(),
                        event.position.y.into(),
                        cx,
                    );
                } else if this.ts_drag.is_some() {
                    this.update_time_signature_track_interaction(
                        event.position.x.into(),
                        event.position.y.into(),
                        cx,
                    );
                } else {
                    this.update_automation_interaction(
                        event.position.x.into(),
                        event.position.y.into(),
                        cx,
                    );
                }
                return;
            }
            if event.pressed_button == Some(gpui::MouseButton::Right)
                && this.erase_clip_drag.is_some()
            {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                this.update_erase_clip_drag(beat, cx);
            } else if event.pressed_button == Some(gpui::MouseButton::Left)
                && this.range_select_drag.is_some()
            {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                let lane_y = this.track_area_y_from_window(event.position);
                let current_track_id = this.state.lane_y_to_track_id(lane_y);
                let mut overlay: Option<TimelineRangeSelection> = None;
                if let Some(drag) = this.range_select_drag.as_mut() {
                    drag.current_beat = beat;
                    let dx = this.state.beats_to_x(beat) - this.state.beats_to_x(drag.start_beat);
                    let dy_tracks = current_track_id
                        .as_ref()
                        .and_then(|id| {
                            let start_idx = this
                                .state
                                .tracks
                                .iter()
                                .position(|track| track.id == drag.start_track_id)?;
                            let current_idx = this
                                .state
                                .tracks
                                .iter()
                                .position(|track| track.id == *id)?;
                            Some(((current_idx as isize - start_idx as isize).abs() as f32) * TRACK_HEIGHT)
                        })
                        .unwrap_or(0.0);
                    if !drag.dragging && (dx * dx + dy_tracks * dy_tracks).sqrt() >= MARQUEE_DRAG_THRESHOLD {
                        drag.dragging = true;
                        if Self::input_debug_enabled() {
                            eprintln!("[selection] marquee_start additive={}", drag.additive);
                        }
                    }
                    if drag.dragging {
                        let (lo, hi) = normalize_range(drag.start_beat, beat);
                        let end_track_id = current_track_id.unwrap_or_else(|| drag.start_track_id.clone());
                        overlay = Some(TimelineRangeSelection::new(
                            lo as f64,
                            hi as f64,
                            this.state.track_ids_between(&drag.start_track_id, &end_track_id),
                        ));
                    }
                }
                this.state.arrangement_range = overlay;
                if Self::input_debug_enabled() {
                    if let Some(drag) = this.range_select_drag.as_ref() {
                        eprintln!(
                            "[selection] marquee_update dragging={} beat={:.3}",
                            drag.dragging, drag.current_beat
                        );
                    }
                }
                cx.notify();
            } else if event.pressed_button == Some(gpui::MouseButton::Left)
                && this.state.active_tool == TimelineTool::Pen
                && this.pen_clip_draw.is_some()
            {
                // Live MIDI clip draw: track the snapped cursor beat so the ghost
                // preview expands/shrinks in real time. No project mutation —
                // the real clip is created once on release.
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                if let Some(preview) = this.pen_clip_draw.as_mut() {
                    if (beat - preview.current_beat).abs() > f32::EPSILON {
                        preview.current_beat = beat;
                        if (beat - preview.start_beat).abs() > f32::EPSILON {
                            preview.dragging = true;
                        }
                        cx.notify();
                    }
                }
            }
        });

        let on_pen_mouse_up = cx.listener(|this, event: &gpui::MouseUpEvent, _window, cx| {
            this.log_input_state("mouse-up-left");
            let finished_tempo = this.finish_tempo_track_interaction(cx);
            let finished_ts = this.finish_time_signature_track_interaction(cx);
            let finished_automation = this.finish_automation_interaction(cx);
            if !finished_tempo && !finished_ts && !finished_automation {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                if this.state.active_tool == TimelineTool::Pen && this.pen_clip_draw.is_some() {
                    this.finish_pen_midi_clip(beat, cx);
                } else if this.state.active_tool == TimelineTool::Pointer
                    && this.range_select_drag.is_some()
                {
                    this.finish_range_select(beat, cx);
                } else if this.erase_clip_drag.is_some() {
                    this.finish_erase_clip_drag(cx);
                }
            }
            this.reset_input_state();
            debug_assert!(this.range_select_drag.is_none());
            cx.notify();
        });
        let on_pen_mouse_up_out = cx.listener(|this, event: &gpui::MouseUpEvent, _window, cx| {
            this.log_input_state("mouse-up-left-out");
            let finished_tempo = this.finish_tempo_track_interaction(cx);
            let finished_ts = this.finish_time_signature_track_interaction(cx);
            let finished_automation = this.finish_automation_interaction(cx);
            if !finished_tempo && !finished_ts && !finished_automation {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                if this.state.active_tool == TimelineTool::Pen && this.pen_clip_draw.is_some() {
                    this.finish_pen_midi_clip(beat, cx);
                } else if this.state.active_tool == TimelineTool::Pointer
                    && this.range_select_drag.is_some()
                {
                    this.finish_range_select(beat, cx);
                } else if this.erase_clip_drag.is_some() {
                    this.finish_erase_clip_drag(cx);
                }
            }
            this.reset_input_state();
            debug_assert!(this.range_select_drag.is_none());
            cx.notify();
        });

        let on_add_track = cx.listener(|this, _: &(), window, cx| {
            if let Some(callback) = this.on_add_track.as_ref() {
                callback(
                    &TimelineAddTrackRequest {
                        track_count: this.state.tracks.len(),
                        has_master_track: this
                            .state
                            .tracks
                            .iter()
                            .any(|track| track.track_type == TrackType::Master),
                    },
                    window,
                    cx,
                );
            } else {
                let id = this.state.create_audio_track();
                this.state.select_track(&id);
                cx.notify();
            }
        });

        let on_toggle_snap = cx.listener(|this, _: &(), _window, cx| {
            this.state.snap_to_grid = !this.state.snap_to_grid;
            cx.notify();
        });

        let on_cycle_grid = cx.listener(|this, _: &(), _window, cx| {
            this.state.grid_division = match this.state.grid_division {
                SnapDivision::Auto => SnapDivision::Off,
                SnapDivision::Off => SnapDivision::Bar1,
                SnapDivision::Bar1 => SnapDivision::Div1_1,
                SnapDivision::Div1_1 => SnapDivision::Div1_2,
                SnapDivision::Div1_2 => SnapDivision::Div1_4,
                SnapDivision::Div1_4 => SnapDivision::Div1_8,
                SnapDivision::Div1_8 => SnapDivision::Div1_16,
                SnapDivision::Div1_16 => SnapDivision::Div1_32,
                SnapDivision::Div1_32 => SnapDivision::Div1_64,
                SnapDivision::Div1_64 => SnapDivision::Auto,
            };
            cx.notify();
        });

        let on_seek = cx.listener(|this, click_x: &f32, _window, cx| {
            let beats = this.state.x_to_beats(*click_x);
            let snapped_sec = this.state.snap_time(beats * this.state.seconds_per_beat());
            this.state.transport.playhead_beats = snapped_sec / this.state.seconds_per_beat();
            // Preview Track Volume automation at the clicked beat immediately so
            // the fader/inspector update even before the engine seek round-trips.
            let beat = this.state.transport.playhead_beats;
            this.state.recompute_effective_volumes(beat, "seek");
            if let Some(cb) = this.on_seek_beats.as_ref() {
                cb(this.state.transport.playhead_beats, this.state.bpm);
            }
            cx.notify();
        });

        let on_select_tool = cx.listener(|this, tool: &TimelineTool, _window, cx| {
            this.log_input_state("tool-change");
            this.reset_input_state();
            this.state.active_tool = *tool;
            cx.notify();
        });

        // Smooth, continuous zoom factor — small per-click multiplier so the
        // px/bt label changes feel like a real ramp rather than a jump.
        // Anchor at the viewport center (no cursor info here) so zoom stays
        // visually stable when driven from the buttons.
        let on_zoom_in = cx.listener(|this, _: &(), window, cx| {
            let viewport_w: f32 = window.bounds().size.width.into();
            let anchor = ((viewport_w - SIDEBAR_WIDTH - HEADER_WIDTH) * 0.5).max(0.0);
            this.state.zoom_by(1.10, anchor);
            cx.notify();
        });

        let on_zoom_out = cx.listener(|this, _: &(), window, cx| {
            let viewport_w: f32 = window.bounds().size.width.into();
            let anchor = ((viewport_w - SIDEBAR_WIDTH - HEADER_WIDTH) * 0.5).max(0.0);
            this.state.zoom_by(1.0 / 1.10, anchor);
            cx.notify();
        });

        // Wrap callbacks in std::sync::Arc to allow easy cloning when passing down to sub-elements
        let on_select_track: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_select_track);
        let on_select_clip: std::sync::Arc<
            dyn Fn(&(String, bool), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_select_clip);
        let on_toggle_mute: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_mute);
        let on_toggle_solo: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_solo);
        let on_toggle_arm: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_arm);
        let on_toggle_input: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_input);
        let on_toggle_automation: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_automation);
        let on_delete_track: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_delete_track);
        let on_volume_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_volume_change);
        let _on_pan_change: std::sync::Arc<
            dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_pan_change);
        let on_add_clip: std::sync::Arc<
            dyn Fn(&(String, f32), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_add_clip);
        let on_add_track: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(on_add_track);
        let on_toggle_snap: std::sync::Arc<
            dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_toggle_snap);
        let on_cycle_grid: std::sync::Arc<
            dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_cycle_grid);
        let on_seek: std::sync::Arc<dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(on_seek);

        // Right-click on the ruler → position-aware tempo menu. Converts the
        // markings-area-local x to a beat and forwards the screen position so
        // the overlay anchors under the cursor.
        let on_ruler_context = cx.listener(
            |this, payload: &(f32, f32, f32), window: &mut gpui::Window, cx| {
                let (click_x, sx, sy) = *payload;
                let beat = this.state.x_to_beats(click_x).max(0.0) as f64;
                if let Some(cb) = this.on_context_menu.clone() {
                    cb(&(TimelineContextTarget::Ruler(beat), sx, sy), window, cx);
                }
            },
        );
        let on_ruler_context: std::sync::Arc<
            dyn Fn(&(f32, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_ruler_context);
        let on_select_tool: std::sync::Arc<
            dyn Fn(&TimelineTool, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_select_tool);
        let on_range_start: std::sync::Arc<
            dyn Fn(&(String, f32, bool), &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_range_start);
        let on_erase_start: std::sync::Arc<
            dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_erase_start);
        let on_erase_clip: std::sync::Arc<
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_erase_clip);
        let on_zoom_in: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(on_zoom_in);
        let on_zoom_out: std::sync::Arc<dyn Fn(&(), &mut gpui::Window, &mut gpui::App) + 'static> =
            std::sync::Arc::new(on_zoom_out);
        let on_timeline_context = self.on_context_menu.clone();
        let on_track_context_menu = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |(track_id, x, y): &(String, f32, f32),
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    cb(
                        &(TimelineContextTarget::TrackHeader(track_id.clone()), *x, *y),
                        window,
                        cx,
                    );
                },
            )
                as std::sync::Arc<
                    dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
                >
        });
        let on_clip_context_menu = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |(clip_id, x, y): &(String, f32, f32),
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    cb(
                        &(TimelineContextTarget::Clip(clip_id.clone()), *x, *y),
                        window,
                        cx,
                    );
                },
            )
                as std::sync::Arc<
                    dyn Fn(&(String, f32, f32), &mut gpui::Window, &mut gpui::App) + 'static,
                >
        });

        let on_automation_down =
            cx.listener(|this, payload: &(String, f32, f32, bool), _window, cx| {
                let (track_id, beat, value, additive) =
                    (payload.0.clone(), payload.1, payload.2, payload.3);
                this.begin_automation_interaction(&track_id, beat, value, additive, cx);
            });
        let on_automation_down: crate::components::timeline::track_lane::AutomationDownCallback =
            std::sync::Arc::new(on_automation_down);

        // Cycle the automation target (in-lane target chip). Committed edit —
        // changes the focused lane and which lane persists.
        let on_automation_cycle = cx.listener(|this, track_id: &String, _window, cx| {
            if this.state.cycle_automation_target(track_id).is_some() {
                this.mark_project_changed(cx);
                cx.notify();
            }
        });
        let on_automation_cycle: crate::components::timeline::track_lane::AutomationCycleCallback =
            std::sync::Arc::new(on_automation_cycle);

        let on_tempo_down = cx.listener(
            |this, payload: &(f64, f64, Option<String>, bool, u32), _window, cx| {
                let (beat, bpm, point_id, _additive, click_count) = (
                    payload.0,
                    payload.1,
                    payload.2.clone(),
                    payload.3,
                    payload.4,
                );
                this.begin_tempo_track_interaction(beat, bpm, point_id, click_count, cx);
            },
        );
        let on_tempo_down: crate::components::timeline::tempo_track::TempoTrackDownCallback =
            std::sync::Arc::new(on_tempo_down);

        let on_tempo_context = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |(beat, bpm, point_id, x, y): &(f64, f64, Option<String>, f32, f32),
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    cb(
                        &(
                            TimelineContextTarget::TempoTrack {
                                beat: *beat,
                                bpm: *bpm,
                                point_id: point_id.clone(),
                            },
                            *x,
                            *y,
                        ),
                        window,
                        cx,
                    );
                },
            ) as crate::components::timeline::tempo_track::TempoTrackContextCallback
        });

        let on_tempo_add = cx.listener(|this, _: &(), _window, cx| {
            this.add_tempo_point_at_playhead_from_header(cx);
        });
        let on_tempo_add: crate::components::timeline::tempo_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_tempo_add);

        let on_tempo_header_menu = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |pos: &(f32, f32), window: &mut gpui::Window, cx: &mut gpui::App| {
                    cb(
                        &(TimelineContextTarget::TempoLaneHeader, pos.0, pos.1),
                        window,
                        cx,
                    );
                },
            ) as crate::components::timeline::tempo_track::GlobalLaneMenuCallback
        });

        let on_tempo_hide = cx.listener(|this, _: &(), _window, cx| {
            this.state.hide_tempo_track_lane();
            cx.notify();
        });
        let on_tempo_hide: crate::components::timeline::tempo_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_tempo_hide);

        let on_tempo_toggle_collapsed = cx.listener(|this, _: &(), _window, cx| {
            this.state.tempo_track_collapsed = !this.state.tempo_track_collapsed;
            cx.notify();
        });
        let on_tempo_toggle_collapsed: crate::components::timeline::tempo_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_tempo_toggle_collapsed);

        let on_ts_down = cx.listener(
            |this, payload: &(f64, Option<String>, bool, u32), _window, cx| {
                let (beat, point_id, _additive, click_count) =
                    (payload.0, payload.1.clone(), payload.2, payload.3);
                this.begin_time_signature_track_interaction(beat, point_id, click_count, cx);
            },
        );
        let on_ts_down: crate::components::timeline::time_signature_track::TimeSignatureTrackDownCallback =
            std::sync::Arc::new(on_ts_down);

        let on_ts_context = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |(beat, point_id, x, y): &(f64, Option<String>, f32, f32),
                      window: &mut gpui::Window,
                      cx: &mut gpui::App| {
                    cb(
                        &(
                            TimelineContextTarget::TimeSignatureTrack {
                                beat: *beat,
                                point_id: point_id.clone(),
                            },
                            *x,
                            *y,
                        ),
                        window,
                        cx,
                    );
                },
            ) as crate::components::timeline::time_signature_track::TimeSignatureTrackContextCallback
        });

        let on_ts_add = cx.listener(|this, _: &(), _window, cx| {
            this.add_time_signature_marker_at_playhead_from_header(cx);
        });
        let on_ts_add: crate::components::timeline::time_signature_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_ts_add);

        let on_ts_header_menu = self.on_context_menu.clone().map(|cb| {
            std::sync::Arc::new(
                move |pos: &(f32, f32), window: &mut gpui::Window, cx: &mut gpui::App| {
                    cb(
                        &(
                            TimelineContextTarget::TimeSignatureLaneHeader,
                            pos.0,
                            pos.1,
                        ),
                        window,
                        cx,
                    );
                },
            ) as crate::components::timeline::time_signature_track::GlobalLaneMenuCallback
        });

        let on_ts_hide = cx.listener(|this, _: &(), _window, cx| {
            this.state.hide_time_signature_track_lane();
            cx.notify();
        });
        let on_ts_hide: crate::components::timeline::time_signature_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_ts_hide);

        let on_ts_toggle_collapsed = cx.listener(|this, _: &(), _window, cx| {
            this.state.time_signature_track_collapsed =
                !this.state.time_signature_track_collapsed;
            cx.notify();
        });
        let on_ts_toggle_collapsed: crate::components::timeline::time_signature_track::GlobalLaneVoidCallback =
            std::sync::Arc::new(on_ts_toggle_collapsed);

        let header_callbacks = crate::components::timeline::track_header::TrackHeaderCallbacks {
            on_select_track: on_select_track.clone(),
            on_toggle_mute: on_toggle_mute.clone(),
            on_toggle_solo: on_toggle_solo.clone(),
            on_toggle_arm: on_toggle_arm.clone(),
            on_toggle_input: on_toggle_input.clone(),
            on_toggle_automation: on_toggle_automation.clone(),
            on_delete_track: on_delete_track.clone(),
            on_volume_change: on_volume_change.clone(),
            on_context_menu: on_track_context_menu.clone(),
        };

        let state = &self.state;
        let tempo_h = state.tempo_track_height();
        let ts_h = state.time_signature_track_height();
        let content_top = state.arrangement_content_top();
        // Live pen-draw ghost clip (built before the chain to keep the borrow of
        // `self.pen_clip_draw` separate from the render closures).
        let pen_preview_overlay = self
            .pen_clip_draw
            .as_ref()
            .and_then(|preview| pen_clip_draw_overlay(preview, state));
        let on_zoom_in_btn = on_zoom_in.clone();
        let on_zoom_out_btn = on_zoom_out.clone();

        // ── Scrollbar geometry ──────────────────────────────────────────
        // Computed once per render against the live window size. Both
        // tracks (visible bar) are 8 px wide and sit at the right/bottom
        // edges of the lane area. Clicking the track jumps the scroll
        // position to that point — gives a functional scrollbar without
        // needing a stateful drag.
        let content_w = self.timeline_content_width();
        let content_h = (self.state.tracks.len() as f32 * TRACK_HEIGHT).max(1.0);
        let lane_view_h = viewport_h.max(TRACK_HEIGHT);
        let lane_view_w = viewport_w.max(1.0);

        // ── Drag/drop import wiring ─────────────────────────────────────
        // Track the mouse position throughout an external file drag so that
        // when `on_drop` fires we can resolve the drop coordinates.
        let on_drag_track = cx.listener(
            |this, event: &gpui::DragMoveEvent<ExternalPaths>, _window, _cx| {
                this.last_drag_position = Some(event.event.position);
            },
        );

        let on_files_dropped = cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            let mut any_imported = false;
            // Multi-file drops: the first file lands at the cursor; subsequent
            // files always land on a brand-new track (forced via y past the end).
            let mut force_new_track = false;
            for path in paths.paths().iter() {
                let imported =
                    this.import_audio_path_at_last_drag(path, force_new_track, _window, cx);
                any_imported |= imported;
                force_new_track |= imported;
            }
            if any_imported {
                this.last_drag_position = None;
                cx.notify();
            }
        });

        let on_browser_drag_track = cx.listener(
            |this, event: &gpui::DragMoveEvent<BrowserDragItem>, _window, _cx| {
                this.last_drag_position = Some(event.event.position);
            },
        );

        let on_browser_file_dropped = cx.listener(|this, item: &BrowserDragItem, window, cx| {
            if this.import_audio_path_at_last_drag(&item.path, false, window, cx) {
                this.last_drag_position = None;
                cx.notify();
            }
        });

        let on_clip_drag_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<ClipDragItem>, window, cx| {
                let drag = event.drag(cx).clone();
                this.last_drag_position = Some(event.event.position);
                this.move_dragged_clip_to_position(&drag, event.event.position, window);
                cx.notify();
            },
        );

        let on_clip_dropped = cx.listener(|this, drag: &ClipDragItem, _window, cx| {
            let target_index = this.clip_drag_target_track_index;
            if let Some(target_track_id) = target_index
                .and_then(|index| this.state.tracks.get(index))
                .map(|track| track.id.clone())
            {
                let current_start = this
                    .state
                    .find_clip(&drag.clip_id)
                    .map(|(_, clip)| clip.start_beat)
                    .unwrap_or(drag.start_beat);
                this.state
                    .move_clip_to_track(&drag.clip_id, &target_track_id, current_start);
                this.mark_project_changed(cx);
            }
            this.clip_drag_origin = None;
            this.clip_drag_target_track_index = None;
            this.last_drag_position = None;
            cx.notify();
        });

        // Clip edge-resize: live-mutate the clip bounds on every drag move (no
        // dirty), then commit once on drop. `resize_clip` snaps internally.
        let on_clip_resize_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<ClipResizeDrag>, _window, cx| {
                let drag = event.drag(cx).clone();
                let beat = this.beat_from_window_x(event.event.position.x.into());
                this.state.resize_clip(&drag.clip_id, drag.edge, beat);
                cx.notify();
            },
        );
        let on_clip_resize_drop = cx.listener(|this, _drag: &ClipResizeDrag, _window, cx| {
            this.mark_project_changed(cx);
            cx.notify();
        });

        let on_track_drag_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<TrackDragItem>, _window, cx| {
                let drag = event.drag(cx).clone();
                let y = this.track_area_y_from_window(event.event.position);
                if this.state.dragging_track_id.as_deref() != Some(drag.track_id.as_str()) {
                    this.state
                        .begin_track_drag(&drag.track_id, drag.origin_index, y);
                }
                this.state.update_track_drag(y);
                cx.notify();
            },
        );

        let on_track_dropped = cx.listener(|this, drag: &TrackDragItem, _window, cx| {
            let target_index = this
                .state
                .drag_target_index
                .unwrap_or(drag.origin_index)
                .clamp(0, this.state.tracks.len());
            this.state.reorder_track(&drag.track_id, target_index);
            this.mark_project_changed(cx);
            cx.notify();
        });

        let on_middle_pan_start = cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
            this.pan_last_position = Some(event.position);
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        });

        let on_middle_pan_move = cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
            if event.pressed_button != Some(gpui::MouseButton::Middle) {
                this.pan_last_position = None;
                return;
            }

            let Some(previous) = this.pan_last_position else {
                this.pan_last_position = Some(event.position);
                return;
            };

            let dx: f32 = (event.position.x - previous.x).into();
            let dy: f32 = (event.position.y - previous.y).into();
            let (max_x, max_y) = this.max_scroll_offsets(window);
            let next_x = this.state.viewport.scroll_x - dx;
            let next_y = this.state.viewport.scroll_y - dy;
            this.state
                .set_scroll_immediate(next_x, next_y, max_x, max_y);
            this.pan_last_position = Some(event.position);
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        });

        let on_middle_pan_end = cx.listener(|this, _event: &gpui::MouseUpEvent, _window, cx| {
            this.pan_last_position = None;
            cx.notify();
        });

        let on_middle_pan_end_out =
            cx.listener(|this, _event: &gpui::MouseUpEvent, _window, cx| {
                this.pan_last_position = None;
                cx.notify();
            });

        let on_ctrl_wheel_zoom = cx.listener(|this, event: &gpui::ScrollWheelEvent, window, cx| {
            let delta = match event.delta {
                ScrollDelta::Pixels(p) => {
                    let x: f32 = p.x.into();
                    let y: f32 = p.y.into();
                    (x, y)
                }
                ScrollDelta::Lines(p) => (p.x * 36.0, p.y * 36.0),
            };

            if !event.modifiers.control {
                let (max_x, max_y) = this.max_scroll_offsets(window);
                let (scroll_x, scroll_y) = if event.modifiers.shift {
                    let horizontal = if delta.1.abs() > 0.01 {
                        delta.1
                    } else {
                        delta.0
                    };
                    (horizontal, 0.0)
                } else {
                    (delta.0, delta.1)
                };
                this.state.scroll_by(scroll_x, scroll_y, max_x, max_y);
                if scroll_x.abs() > 0.5 {
                    this.state.note_user_scrolled();
                }
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
                return;
            }

            window.prevent_default();
            cx.stop_propagation();

            if delta.1.abs() < 0.01 {
                return;
            }

            let x: f32 = event.position.x.into();
            let anchor = (x - SIDEBAR_WIDTH - HEADER_WIDTH).max(0.0);
            let factor = (1.0018_f32).powf(-delta.1);
            this.state.zoom_by(factor, anchor);
            let (max_x, max_y) = this.max_scroll_offsets(window);
            this.state.clamp_scroll(max_x, max_y);
            cx.notify();
        });

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(Colors::surface_base())
            .border_l(px(1.0))
            .border_r(px(1.0))
            .border_color(Colors::border_subtle())
            .relative()
            .capture_key_down(
                cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                    if event.keystroke.key.as_str() == "escape"
                        && (this.range_select_drag.is_some()
                            || this.pen_clip_draw.is_some()
                            || this.erase_clip_drag.is_some()
                            || this.automation_drag.is_some()
                            || this.automation_marquee.is_some()
                            || this.pan_last_position.is_some())
                    {
                        cx.stop_propagation();
                        this.cancel_active_gesture(cx);
                    }
                }),
            )
            .on_drag_move::<ExternalPaths>(on_drag_track)
            .on_drop::<ExternalPaths>(on_files_dropped)
            .on_drag_move::<BrowserDragItem>(on_browser_drag_track)
            .on_drop::<BrowserDragItem>(on_browser_file_dropped)
            .on_drag_move::<ClipDragItem>(on_clip_drag_move)
            .on_drop::<ClipDragItem>(on_clip_dropped)
            .on_drag_move::<ClipResizeDrag>(on_clip_resize_move)
            .on_drop::<ClipResizeDrag>(on_clip_resize_drop)
            .on_drag_move::<TrackDragItem>(on_track_drag_move)
            .on_drop::<TrackDragItem>(on_track_dropped)
            .on_mouse_down(gpui::MouseButton::Middle, on_middle_pan_start)
            .when_some(on_timeline_context, |this, cb| {
                this.on_mouse_down(gpui::MouseButton::Right, move |event, window, cx| {
                    let x: f32 = event.position.x.into();
                    let y: f32 = event.position.y.into();
                    cb(&(TimelineContextTarget::TimelineEmpty, x, y), window, cx);
                })
            })
            .on_mouse_move(on_middle_pan_move)
            .on_mouse_move(on_edit_mouse_move)
            .on_mouse_up(gpui::MouseButton::Middle, on_middle_pan_end)
            .on_mouse_up_out(gpui::MouseButton::Middle, on_middle_pan_end_out)
            .on_mouse_up(gpui::MouseButton::Left, on_pen_mouse_up)
            .on_mouse_up_out(gpui::MouseButton::Left, on_pen_mouse_up_out)
            .on_mouse_up(
                gpui::MouseButton::Right,
                cx.listener(|this, _ev, _w, cx| {
                    this.log_input_state("mouse-up-right");
                    if this.erase_clip_drag.is_some() {
                        this.finish_erase_clip_drag(cx);
                    }
                    this.reset_input_state();
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Right,
                cx.listener(|this, _ev, _w, cx| {
                    this.log_input_state("mouse-up-right-out");
                    if this.erase_clip_drag.is_some() {
                        this.finish_erase_clip_drag(cx);
                    }
                    this.reset_input_state();
                    cx.notify();
                }),
            )
            .on_scroll_wheel(on_ctrl_wheel_zoom)
            // 1. Timeline Ruler
            .child(timeline_ruler(
                state,
                on_add_track.clone(),
                on_toggle_snap.clone(),
                on_cycle_grid.clone(),
                on_seek.clone(),
                on_ruler_context.clone(),
            ))
            // 1b. Global Tempo Track lane (below ruler, above tracks)
            .when(state.show_tempo_track, |this| {
                this.child(tempo_track_lane(
                    state,
                    tempo_h,
                    Some(on_tempo_down.clone()),
                    on_tempo_context.clone(),
                    Some(on_tempo_add.clone()),
                    on_tempo_header_menu.clone(),
                    Some(on_tempo_hide.clone()),
                    Some(on_tempo_toggle_collapsed.clone()),
                ))
            })
            .when(state.show_time_signature_track, |this| {
                this.child(time_signature_track_lane(
                    state,
                    ts_h,
                    Some(on_ts_down.clone()),
                    on_ts_context.clone(),
                    Some(on_ts_add.clone()),
                    on_ts_header_menu.clone(),
                    Some(on_ts_hide.clone()),
                    Some(on_ts_toggle_collapsed.clone()),
                ))
            })
            // 2. Track List Scroll Area
            .child(div().flex_1().min_h_0().relative().child(track_list(
                state,
                header_callbacks.clone(),
                on_select_track.clone(),
                on_select_clip.clone(),
                on_add_clip.clone(),
                on_track_context_menu.clone(),
                on_clip_context_menu.clone(),
                self.on_open_editor.clone(),
                Some(on_range_start.clone()),
                Some(on_erase_start.clone()),
                Some(on_erase_clip.clone()),
                Some(&self.erase_preview_ids),
                Some(on_automation_down.clone()),
                Some(on_automation_cycle.clone()),
                self.automation_marquee.as_ref(),
            )))
            // 3. Playhead Overlay (frontmost timeline pass)
            // Render after ruler + content so grid/ruler/content never cover it.
            // Split into:
            // - head overlay (ruler strip only)
            // - body overlay (content strip only)
            // 2b. Arrangement range-selection overlay (UI-only). Drawn above the
            // lane content but below the playhead/tools so it never hides the
            // playhead. Follows zoom/scroll via the same lane coordinate space.
            .children(arrangement_range_overlay(state).map(|overlay| {
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top(px(content_top))
                    .bottom_0()
                    .overflow_hidden()
                    .child(overlay)
            }))
            // Live pen-draw MIDI clip ghost preview (same lane coordinate space
            // as the arrangement overlay; above content, below the playhead).
            .children(pen_preview_overlay.map(|overlay| {
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top(px(content_top))
                    .bottom_0()
                    .overflow_hidden()
                    .child(overlay)
            }))
            .child(
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top_0()
                    .h(px(RULER_HEIGHT))
                    .overflow_hidden()
                    .child({
                        let playhead_x = state.beats_to_x(state.transport.playhead_beats);
                        if std::env::var_os("FUTUREBOARD_PLAYHEAD_DEBUG").is_some() {
                            eprintln!(
                                "[playhead x] beat={:.3} scroll_x={:.1} px_per_beat={:.3} x={:.1}",
                                state.transport.playhead_beats,
                                state.viewport.scroll_x,
                                state.viewport.pixels_per_beat,
                                playhead_x
                            );
                        }
                        crate::components::timeline::playhead::playhead_head_overlay_at(playhead_x)
                    }),
            )
            .child(
                div()
                    .absolute()
                    .left(px(HEADER_WIDTH))
                    .right_0()
                    .top(px(content_top))
                    .bottom_0()
                    .overflow_hidden()
                    .child({
                        let playhead_x = state.beats_to_x(state.transport.playhead_beats);
                        crate::components::timeline::playhead::playhead_body_overlay_at(playhead_x)
                    }),
            )
            // 4. Floating Tools Bar (above playhead)
            .child(
                div()
                    .absolute()
                    .bottom(px(16.0))
                    .left(px(16.0))
                    .child(floating_tools_bar(
                        state.active_tool,
                        on_select_tool.clone(),
                    )),
            )
            // 5. Vertical scrollbar (right edge, over the lane area)
            .child(vertical_scrollbar(
                cx,
                state.viewport.scroll_y,
                content_h,
                lane_view_h,
                scroll_max_y,
                content_top,
            ))
            // 6. Horizontal scrollbar (bottom edge, over the lane area)
            .child(horizontal_scrollbar(
                cx,
                state.viewport.scroll_x,
                content_w,
                lane_view_w,
                scroll_max_x,
            ))
            // 7. Zoom Controls
            .child(
                div()
                    .absolute()
                    .bottom(px(16.0))
                    .right(px(16.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded_full()
                    .border(px(1.0))
                    .border_color(Colors::border_default())
                    .bg(Colors::surface_panel_alt())
                    .shadow_xl()
                    // Zoom Out Button
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(24.0))
                            .h(px(24.0))
                            .rounded_md()
                            .cursor(gpui::CursorStyle::PointingHand)
                            .text_color(Colors::text_secondary())
                            .id("zoom-out-btn")
                            .hover(|style| style.bg(Colors::surface_hover()))
                            .on_click(move |_, window, cx| {
                                on_zoom_out_btn(&(), window, cx);
                            })
                            .child(
                                svg()
                                    .path(assets::ICON_MINUS_PATH)
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .text_color(Colors::text_secondary()),
                            ),
                    )
                    // Zoom readout label
                    .child(
                        div()
                            .px(px(4.0))
                            .text_size(px(9.0))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(Colors::text_muted())
                            .child({
                                let ppb =
                                    state.viewport.pixels_per_second * state.seconds_per_beat();
                                if ppb >= 100.0 {
                                    format!("{:.0} px/bt", ppb)
                                } else {
                                    format!("{:.1} px/bt", ppb)
                                }
                            }),
                    )
                    // Zoom In Button
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(24.0))
                            .h(px(24.0))
                            .rounded_md()
                            .cursor(gpui::CursorStyle::PointingHand)
                            .text_color(Colors::text_secondary())
                            .id("zoom-in-btn")
                            .hover(|style| style.bg(Colors::surface_hover()))
                            .on_click(move |_, window, cx| {
                                on_zoom_in_btn(&(), window, cx);
                            })
                            .child(
                                svg()
                                    .path(assets::ICON_PLUS_PATH)
                                    .w(px(12.0))
                                    .h(px(12.0))
                                    .text_color(Colors::text_secondary()),
                            ),
                    ),
            )
    }
}

// ── Timeline scrollbars ─────────────────────────────────────────────────
//
// Both scrollbars are rendered as absolute overlays on top of the
// arrangement area. The thumb is sized by `viewport / content` and
// positioned by `scroll / max_scroll`. Mouse-down on the track jumps
// the scroll position so the click point becomes the new thumb top
// (vertical) or thumb left (horizontal). The wheel handler on the
// Timeline div continues to handle smooth scrolling and zoom; the
// scrollbar is the visible indicator + a coarse jump target.

const SCROLLBAR_THICKNESS: f32 = 8.0;
const SCROLLBAR_MIN_THUMB: f32 = 24.0;

fn vertical_scrollbar(
    cx: &mut Context<Timeline>,
    scroll_y: f32,
    content_h: f32,
    view_h: f32,
    max_scroll: f32,
    content_top: f32,
) -> gpui::AnyElement {
    if max_scroll <= 0.5 || view_h <= 0.0 {
        return Empty.into_any_element();
    }
    let track_h = view_h;
    let thumb_h = ((view_h / content_h) * track_h).max(SCROLLBAR_MIN_THUMB);
    let progress = (scroll_y / max_scroll).clamp(0.0, 1.0);
    let thumb_top = progress * (track_h - thumb_h).max(0.0);

    let on_track_click = cx.listener(move |this, event: &gpui::MouseDownEvent, _w, cx| {
        // Position is in window space; convert to a fraction of the
        // scrollbar track. We approximate the track top as the click
        // y minus the thumb half-height when clicking above the thumb,
        // and snap the thumb center to the click otherwise.
        let click_y: f32 = event.position.y.into();
        // The scrollbar sits at top=RULER_HEIGHT inside the timeline.
        // Re-derive the local y by subtracting an estimated chrome
        // height; clamp with `max_scroll` so any over/under-estimate
        // still yields a valid scroll position.
        let local = (click_y - 36.0 - content_top).max(0.0);
        let frac = (local / track_h.max(1.0)).clamp(0.0, 1.0);
        this.state.set_scroll_immediate(
            this.state.viewport.scroll_x,
            (frac * max_scroll).clamp(0.0, max_scroll),
            f32::MAX,
            max_scroll,
        );
        cx.notify();
    });

    let on_thumb_drag = cx.listener(
        move |this, event: &gpui::DragMoveEvent<ScrollbarDrag>, _w, cx| {
            if event.drag(cx).axis != ScrollAxis::Vertical {
                return;
            }
            let y: f32 = event.event.position.y.into();
            let oy: f32 = event.bounds.origin.y.into();
            let track_range = (track_h - thumb_h).max(1.0);
            let local = (y - oy - thumb_h * 0.5).clamp(0.0, track_range);
            let frac = local / track_range;
            this.state.set_scroll_immediate(
                this.state.viewport.scroll_x,
                frac * max_scroll,
                f32::MAX,
                max_scroll,
            );
            cx.notify();
        },
    );

    div()
        .absolute()
        .top(px(content_top))
        .right(px(0.0))
        .bottom(px(0.0))
        .w(px(SCROLLBAR_THICKNESS))
        .id("timeline-vscroll")
        .on_mouse_down(gpui::MouseButton::Left, on_track_click)
        .on_drag(
            ScrollbarDrag {
                axis: ScrollAxis::Vertical,
            },
            |drag, _offset, _window, cx| cx.new(|_| drag.clone()),
        )
        .on_drag_move::<ScrollbarDrag>(on_thumb_drag)
        .child(
            div()
                .absolute()
                .top(px(thumb_top))
                .left(px(2.0))
                .right(px(2.0))
                .h(px(thumb_h))
                .rounded_full()
                .bg(Colors::with_alpha(Colors::text_primary(), 0.2)),
        )
        .into_any_element()
}

fn horizontal_scrollbar(
    cx: &mut Context<Timeline>,
    scroll_x: f32,
    content_w: f32,
    view_w: f32,
    max_scroll: f32,
) -> gpui::AnyElement {
    if max_scroll <= 0.5 || view_w <= 0.0 {
        return Empty.into_any_element();
    }
    let track_w = view_w;
    let thumb_w = ((view_w / content_w) * track_w).max(SCROLLBAR_MIN_THUMB);
    let progress = (scroll_x / max_scroll).clamp(0.0, 1.0);
    let thumb_left = progress * (track_w - thumb_w).max(0.0);

    let on_track_click = cx.listener(move |this, event: &gpui::MouseDownEvent, _w, cx| {
        let click_x: f32 = event.position.x.into();
        let local = (click_x - SIDEBAR_WIDTH - HEADER_WIDTH).max(0.0);
        let frac = (local / track_w.max(1.0)).clamp(0.0, 1.0);
        this.state.set_scroll_immediate(
            (frac * max_scroll).clamp(0.0, max_scroll),
            this.state.viewport.scroll_y,
            max_scroll,
            f32::MAX,
        );
        cx.notify();
    });

    let on_thumb_drag = cx.listener(
        move |this, event: &gpui::DragMoveEvent<ScrollbarDrag>, _w, cx| {
            if event.drag(cx).axis != ScrollAxis::Horizontal {
                return;
            }
            let x: f32 = event.event.position.x.into();
            let ox: f32 = event.bounds.origin.x.into();
            let track_range = (track_w - thumb_w).max(1.0);
            let local = (x - ox - thumb_w * 0.5).clamp(0.0, track_range);
            let frac = local / track_range;
            this.state.set_scroll_immediate(
                frac * max_scroll,
                this.state.viewport.scroll_y,
                max_scroll,
                f32::MAX,
            );
            cx.notify();
        },
    );

    div()
        .absolute()
        .bottom(px(0.0))
        .left(px(HEADER_WIDTH))
        .right(px(SCROLLBAR_THICKNESS))
        .h(px(SCROLLBAR_THICKNESS))
        .id("timeline-hscroll")
        .on_mouse_down(gpui::MouseButton::Left, on_track_click)
        .on_drag(
            ScrollbarDrag {
                axis: ScrollAxis::Horizontal,
            },
            |drag, _offset, _window, cx| cx.new(|_| drag.clone()),
        )
        .on_drag_move::<ScrollbarDrag>(on_thumb_drag)
        .child(
            div()
                .absolute()
                .left(px(thumb_left))
                .top(px(2.0))
                .bottom(px(2.0))
                .w(px(thumb_w))
                .rounded_full()
                .bg(Colors::with_alpha(Colors::text_primary(), 0.2)),
        )
        .into_any_element()
}

/// Translucent arrangement range-selection rectangle. Pure render of
/// `state.arrangement_range` — UI-only, follows zoom/scroll, and never touches
/// the engine or marks the project dirty. Spans the affected tracks vertically
/// and the selected beat span horizontally. Non-interactive so it does not
/// intercept lane drags. Returns `None` when no range is active.
/// Resolve a pen-draw gesture's `(start_beat, end_beat)` into the final
/// `(clip_start, length_beats)` that will be committed — snapping the length to
/// the MIDI grid when snap is on and clamping to the minimum clip length. Shared
/// by the live ghost preview and the commit so they can never disagree.
fn compute_pen_clip_span(state: &TimelineState, start_beat: f32, end_beat: f32) -> (f32, f32) {
    use crate::components::timeline::timeline_state::{
        DEFAULT_MIDI_CLIP_BEATS, MIN_MIDI_CLIP_BEATS,
    };
    let (lo, hi) = normalize_range(start_beat, end_beat);
    let mut len = (hi - lo).max(DEFAULT_MIDI_CLIP_BEATS);
    if state.snap_to_grid {
        let step = state.midi_snap_step_beats().max(1.0e-3);
        len = ((len / step).ceil() * step).max(MIN_MIDI_CLIP_BEATS);
    } else {
        len = len.max(MIN_MIDI_CLIP_BEATS);
    }
    (lo, len)
}

/// Human-readable musical length, e.g. `1 bar`, `4 bars`, `2.5 bars`, `3.0 bt`.
fn format_clip_length(length_beats: f32, beats_per_bar: f32) -> String {
    let bpb = beats_per_bar.max(1.0);
    let bars = length_beats / bpb;
    if (bars - bars.round()).abs() < 1.0e-3 && bars >= 1.0 {
        let n = bars.round() as i32;
        format!("{} bar{}", n, if n == 1 { "" } else { "s" })
    } else if bars >= 1.0 {
        format!("{:.1} bars", bars)
    } else {
        format!("{:.1} bt", length_beats)
    }
}

/// Live ghost-clip overlay for the in-flight pen MIDI clip draw. Translucent,
/// track-colored, with a pulsing outline and a floating length/range label so
/// the user sees the exact bounds and musical length before releasing.
fn pen_clip_draw_overlay(
    preview: &ClipDrawPreview,
    state: &TimelineState,
) -> Option<gpui::AnyElement> {
    let track_index = state.tracks.iter().position(|t| t.id == preview.track_id)?;
    let track_color = state.tracks[track_index].color;

    let (clip_start, length) =
        compute_pen_clip_span(state, preview.start_beat, preview.current_beat);
    let clip_end = clip_start + length;

    let x_lo = state.beats_to_x(clip_start);
    let width = (state.beats_to_x(clip_end) - x_lo).max(2.0);
    let pad = 7.0;
    let top = track_index as f32 * TRACK_HEIGHT - state.viewport.scroll_y + pad;
    let height = (TRACK_HEIGHT - pad * 2.0).max(1.0);

    let bpb = state.beats_per_bar();
    let length_label = format_clip_length(length, bpb);
    let range_label = format!(
        "{} → {}",
        state.format_bar_beat(clip_start),
        state.format_bar_beat(clip_end)
    );

    let ghost_fill = Colors::with_alpha(track_color, 0.16);
    let label_text = Colors::with_alpha(Colors::text_primary(), 0.92);

    // Ghost clip body — translucent track-colored fill with a pulsing outline so
    // it reads as "in creation". The pulse animates on its own frames, so it
    // stays alive even when the cursor is held still.
    let body = div()
        .absolute()
        .left(px(x_lo))
        .top(px(top))
        .w(px(width))
        .h(px(height))
        .rounded_md()
        .bg(ghost_fill)
        .border(px(1.0))
        .border_color(Colors::with_alpha(track_color, 0.85))
        .overflow_hidden()
        .flex()
        .flex_col()
        .justify_between()
        // Title placeholder.
        .child(
            div()
                .px(px(6.0))
                .pt(px(4.0))
                .text_size(px(9.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(label_text)
                .truncate()
                .child("New MIDI Clip"),
        )
        // Bottom length readout, mirroring the committed clip's label bar.
        .child(
            div()
                .h(px(14.0))
                .w_full()
                .bg(Colors::with_alpha(Colors::surface_panel_alt(), 0.85))
                .border_t(px(1.0))
                .border_color(Colors::divider())
                .px(px(6.0))
                .flex()
                .items_center()
                .justify_end()
                .text_size(px(8.0))
                .text_color(Colors::text_secondary())
                .child(format!("{:.1} bt", length)),
        )
        .with_animation(
            "pen-clip-draw-pulse",
            Animation::new(Duration::from_millis(1100))
                .repeat()
                .with_easing(pulsating_between(0.35, 0.85)),
            move |this, delta| this.border_color(Colors::with_alpha(track_color, delta)),
        );

    // Floating musical-length label, pinned just above the ghost clip (or below
    // it when the clip sits at the very top of the lane area).
    let label_below = top < 26.0;
    let label = div()
        .absolute()
        .left(px(x_lo + 2.0))
        .map(|el| {
            if label_below {
                el.top(px(top + height + 4.0))
            } else {
                el.top(px((top - 22.0).max(0.0)))
            }
        })
        .px(px(6.0))
        .py(px(2.0))
        .rounded_md()
        .bg(Colors::with_alpha(Colors::surface_panel(), 0.96))
        .border(px(1.0))
        .border_color(Colors::with_alpha(track_color, 0.6))
        .shadow_lg()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .text_size(px(9.0))
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(label_text)
                .child(length_label),
        )
        .child(div().text_color(Colors::text_muted()).child(range_label));

    // Subtle full-height guide at the clip end so the snapped end position reads
    // clearly against the grid.
    let end_x = state.beats_to_x(clip_end);
    let end_guide = div()
        .absolute()
        .left(px((end_x - 0.5).max(0.0)))
        .top_0()
        .bottom_0()
        .w(px(1.0))
        .bg(Colors::with_alpha(track_color, 0.45));

    Some(
        div()
            .absolute()
            .inset_0()
            .child(end_guide)
            .child(body)
            .child(label)
            .into_any_element(),
    )
}

fn arrangement_range_overlay(state: &TimelineState) -> Option<gpui::AnyElement> {
    let range = state.arrangement_range.as_ref()?;
    let (start_beat, end_beat) = range.as_f32_range();
    let (lo, hi) = normalize_range(start_beat, end_beat);
    let x_lo = state.beats_to_x(lo);
    let width = (state.beats_to_x(hi) - x_lo).max(1.0);

    // Vertical span follows the affected track ids; an empty set covers the
    // whole lane area (e.g. a horizontal-only time range).
    let (y_top, height) = {
        let mut min_idx = usize::MAX;
        let mut max_idx = 0usize;
        for (idx, track) in state.tracks.iter().enumerate() {
            if range.track_ids.iter().any(|id| id == &track.id) {
                min_idx = min_idx.min(idx);
                max_idx = max_idx.max(idx);
            }
        }
        if min_idx == usize::MAX {
            (0.0_f32, state.viewport.track_area_height.max(0.0))
        } else {
            let top = min_idx as f32 * TRACK_HEIGHT - state.viewport.scroll_y;
            let h = ((max_idx - min_idx + 1) as f32) * TRACK_HEIGHT;
            (top, h)
        }
    };

    Some(
        div()
            .absolute()
            .left(px(x_lo))
            .top(px(y_top))
            .w(px(width))
            .h(px(height))
            .bg(Colors::with_alpha(Colors::accent_primary(), 0.14))
            .border(px(1.0))
            .border_color(Colors::with_alpha(Colors::accent_primary(), 0.7))
            .into_any_element(),
    )
}
