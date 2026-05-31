use crate::assets;
use crate::components::edit::{normalize_range, ClipSnapshot, EditCommand, EditHistory};
use crate::components::sidebar::{BrowserDragItem, SIDEBAR_WIDTH};
use crate::components::timeline::floating_tools_bar::floating_tools_bar;
use crate::components::timeline::timeline_ruler::timeline_ruler;
use crate::components::timeline::timeline_state::{
    ClipDragItem, SnapDivision, TimelineState, TimelineTool, TrackDragItem, TrackType,
    HEADER_WIDTH, RULER_HEIGHT, TRACK_HEIGHT,
};
use crate::components::timeline::track_list::track_list;
use crate::theme::Colors;
use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, svg, AppContext, Context, Empty, ExternalPaths, InteractiveElement, IntoElement,
    ParentElement, Render, ScrollDelta, StatefulInteractiveElement, Styled, Window,
};

/// App chrome (top titlebar/menu strip) — used to convert window-space y into
/// the timeline track area. Mirrors the value used by app_chrome.
const APP_CHROME_HEIGHT: f32 = 36.0;

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
    /// Pen-tool click-drag: `(track_id, start_beat)` until mouse-up creates the clip.
    pen_clip_draw: Option<(String, f32)>,
    /// Pointer-tool range select drag in beats.
    range_select_drag: Option<(f32, f32)>,
    /// Right-drag erase: clip ids already queued for deletion this gesture.
    erase_clip_drag: Option<HashSet<String>>,
    /// Live preview of clip ids marked for erase (mirrors `erase_clip_drag`).
    erase_preview_ids: HashSet<String>,
    pan_last_position: Option<gpui::Point<gpui::Pixels>>,
    on_context_menu: Option<TimelineContextMenuCb>,
    /// Invoked when the user double-clicks a MIDI clip — `StudioLayout` uses it
    /// to switch the bottom panel to the piano-roll Editor tab.
    on_open_editor: Option<TimelineOpenEditorCb>,
    chrome_metrics: TimelineChromeMetrics,
}

pub type TimelineOpenEditorCb = std::sync::Arc<dyn Fn(&mut gpui::Window, &mut gpui::App) + 'static>;

#[derive(Clone, Debug)]
pub enum TimelineContextTarget {
    TimelineEmpty,
    TrackHeader(String),
    Clip(String),
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

impl Timeline {
    /// Clean empty-project Timeline — the real runtime entry point.
    pub fn new() -> Self {
        Self {
            state: TimelineState::default(),
            edit_history: EditHistory::new(100),
            on_seek_beats: None,
            on_track_param_change: None,
            on_project_changed: None,
            on_media_changed: None,
            on_add_track: None,
            last_drag_position: None,
            clip_drag_origin: None,
            clip_drag_target_track_index: None,
            pen_clip_draw: None,
            range_select_drag: None,
            erase_clip_drag: None,
            erase_preview_ids: HashSet::new(),
            pan_last_position: None,
            on_context_menu: None,
            on_open_editor: None,
            chrome_metrics: TimelineChromeMetrics::default(),
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
            on_media_changed: None,
            on_add_track: None,
            last_drag_position: None,
            clip_drag_origin: None,
            clip_drag_target_track_index: None,
            pen_clip_draw: None,
            range_select_drag: None,
            erase_clip_drag: None,
            erase_preview_ids: HashSet::new(),
            pan_last_position: None,
            on_context_menu: None,
            on_open_editor: None,
            chrome_metrics: TimelineChromeMetrics::default(),
        }
    }

    pub fn run_edit_command(&mut self, cmd: EditCommand, cx: &mut gpui::Context<Self>) {
        cmd.execute(&mut self.state);
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

    pub fn set_media_changed_callback(&mut self, callback: Option<TimelineProjectChangedCb>) {
        self.on_media_changed = callback;
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
        use crate::components::timeline::timeline_state::{
            TrackType, DEFAULT_MIDI_CLIP_BEATS, MIN_MIDI_CLIP_BEATS,
        };
        let Some((track_id, start_beat)) = self.pen_clip_draw.take() else {
            return;
        };
        let track_type = self
            .state
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| t.track_type);
        if !matches!(track_type, Some(TrackType::Midi | TrackType::Instrument)) {
            return;
        }

        let (clip_start, length) =
            if let Some((range_start, range_end)) = self.state.arrangement_range {
                let (lo, hi) = normalize_range(range_start, range_end);
                (lo, (hi - lo).max(MIN_MIDI_CLIP_BEATS))
            } else {
                let (lo, hi) = normalize_range(start_beat, end_beat);
                let mut len = (hi - lo).max(DEFAULT_MIDI_CLIP_BEATS);
                if self.state.snap_to_grid {
                    let step = self.state.midi_snap_step_beats();
                    len = ((len / step).ceil() * step).max(MIN_MIDI_CLIP_BEATS);
                } else {
                    len = len.max(MIN_MIDI_CLIP_BEATS);
                }
                (lo, len)
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
        let Some((start, _)) = self.range_select_drag.take() else {
            return;
        };
        let (lo, hi) = normalize_range(start, end_beat);
        self.state.arrangement_range = Some((lo, hi));
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
        let used_v = APP_CHROME_HEIGHT + RULER_HEIGHT + m.bottom_panel_height + m.status_bar_height;
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

    fn track_area_y_from_window(position: gpui::Point<gpui::Pixels>) -> f32 {
        let y: f32 = position.y.into();
        (y - APP_CHROME_HEIGHT - RULER_HEIGHT).max(0.0)
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
                let lane_y = (y - APP_CHROME_HEIGHT - RULER_HEIGHT).max(0.0);
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

        let on_select_clip = cx.listener(|this, clip_id: &String, _window, cx| {
            this.state.select_clip(clip_id);
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
            this.state.toggle_track_input_monitor(track_id);
            this.mark_project_changed(cx);
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
                        this.pen_clip_draw = Some((track_id.clone(), *beat));
                    }
                }
                Some(TrackType::Bus | TrackType::Return | TrackType::Master) | None => {}
            }
            cx.notify();
        });

        let on_range_start = cx.listener(|this, beat: &f32, _window, cx| {
            if this.state.active_tool == TimelineTool::Pointer {
                this.range_select_drag = Some((*beat, *beat));
                this.state.arrangement_range = Some((*beat, *beat));
                cx.notify();
            }
        });

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
            if event.pressed_button == Some(gpui::MouseButton::Right)
                && this.erase_clip_drag.is_some()
            {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                this.update_erase_clip_drag(beat, cx);
            } else if event.pressed_button == Some(gpui::MouseButton::Left)
                && this.range_select_drag.is_some()
            {
                let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
                if let Some((start, _)) = this.range_select_drag {
                    this.state.arrangement_range = Some(normalize_range(start, beat));
                    cx.notify();
                }
            }
        });

        let on_pen_mouse_up = cx.listener(|this, event: &gpui::MouseUpEvent, _window, cx| {
            let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
            if this.state.active_tool == TimelineTool::Pen && this.pen_clip_draw.is_some() {
                this.finish_pen_midi_clip(beat, cx);
                return;
            }
            if this.state.active_tool == TimelineTool::Pointer && this.range_select_drag.is_some() {
                this.finish_range_select(beat, cx);
                return;
            }
            if this.erase_clip_drag.is_some() {
                this.finish_erase_clip_drag(cx);
            }
        });
        let on_pen_mouse_up_out = cx.listener(|this, event: &gpui::MouseUpEvent, _window, cx| {
            let beat = this.snap_beat(this.beat_from_window_x(event.position.x.into()));
            if this.state.active_tool == TimelineTool::Pen && this.pen_clip_draw.is_some() {
                this.finish_pen_midi_clip(beat, cx);
                return;
            }
            if this.state.active_tool == TimelineTool::Pointer && this.range_select_drag.is_some() {
                this.finish_range_select(beat, cx);
                return;
            }
            if this.erase_clip_drag.is_some() {
                this.finish_erase_clip_drag(cx);
            }
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
            if let Some(cb) = this.on_seek_beats.as_ref() {
                cb(this.state.transport.playhead_beats, this.state.bpm);
            }
            cx.notify();
        });

        let on_select_tool = cx.listener(|this, tool: &TimelineTool, _window, cx| {
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
            dyn Fn(&String, &mut gpui::Window, &mut gpui::App) + 'static,
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
        let on_select_tool: std::sync::Arc<
            dyn Fn(&TimelineTool, &mut gpui::Window, &mut gpui::App) + 'static,
        > = std::sync::Arc::new(on_select_tool);
        let on_range_start: std::sync::Arc<
            dyn Fn(&f32, &mut gpui::Window, &mut gpui::App) + 'static,
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

        let header_callbacks = crate::components::timeline::track_header::TrackHeaderCallbacks {
            on_select_track: on_select_track.clone(),
            on_toggle_mute: on_toggle_mute.clone(),
            on_toggle_solo: on_toggle_solo.clone(),
            on_toggle_arm: on_toggle_arm.clone(),
            on_toggle_input: on_toggle_input.clone(),
            on_delete_track: on_delete_track.clone(),
            on_volume_change: on_volume_change.clone(),
            on_context_menu: on_track_context_menu.clone(),
        };

        let state = &self.state;
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

        let on_track_drag_move = cx.listener(
            |this, event: &gpui::DragMoveEvent<TrackDragItem>, _window, cx| {
                let drag = event.drag(cx).clone();
                let y = Self::track_area_y_from_window(event.event.position);
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
            .on_drag_move::<ExternalPaths>(on_drag_track)
            .on_drop::<ExternalPaths>(on_files_dropped)
            .on_drag_move::<BrowserDragItem>(on_browser_drag_track)
            .on_drop::<BrowserDragItem>(on_browser_file_dropped)
            .on_drag_move::<ClipDragItem>(on_clip_drag_move)
            .on_drop::<ClipDragItem>(on_clip_dropped)
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
                    if this.erase_clip_drag.is_some() {
                        this.finish_erase_clip_drag(cx);
                    }
                }),
            )
            .on_mouse_up_out(
                gpui::MouseButton::Right,
                cx.listener(|this, _ev, _w, cx| {
                    if this.erase_clip_drag.is_some() {
                        this.finish_erase_clip_drag(cx);
                    }
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
            ))
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
            )))
            // 3. Playhead Overlay (frontmost timeline pass)
            // Render after ruler + content so grid/ruler/content never cover it.
            // Split into:
            // - head overlay (ruler strip only)
            // - body overlay (content strip only)
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
                    .top(px(RULER_HEIGHT))
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
        let local = (click_y - 36.0 - RULER_HEIGHT).max(0.0);
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
        .top(px(RULER_HEIGHT))
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
