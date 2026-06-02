//! Native GPUI Piano Roll editor.
//!
//! Ported from the WebUI `MidiEditorPanel`. Edits the currently selected MIDI
//! clip in the shared [`Timeline`] entity. All note mutations go through the
//! single-source-of-truth helpers on `TimelineState` (see
//! `timeline_state.rs`) and mark the project dirty so the engine sync /
//! autosave observe the change.
//!
//! Coordinate model (matches WebUI):
//! - notes are stored in beats relative to the clip start
//! - the grid maps beats → x with `ppb` (pixels per beat) and a horizontal
//!   scroll offset; pitch → y with a fixed [`ROW_H`] and a vertical scroll
//!
//! Interaction state (tool, selection, zoom, snap, drag) lives on this entity
//! — never recomputed in render. Note geometry is only built for the visible
//! pitch/beat range each frame.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    canvas, div, px, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Render, ScrollWheelEvent, StatefulInteractiveElement, Styled, Window,
};

use crate::components::edit::{normalize_range, EditCommand};
use crate::components::timeline::timeline::Timeline;
use crate::components::timeline::timeline_state::{
    midi_debug_enabled, MidiControllerKind, MidiControllerPoint, MidiNoteState, TimelineState,
    MIN_NOTE_BEATS,
};
use crate::theme::Colors;

// ── Layout constants (CSS px) ───────────────────────────────────────────────
const ROW_H: f32 = 14.0; // px per semitone
const PITCH_CNT: i32 = 128;
const TOTAL_H: f32 = PITCH_CNT as f32 * ROW_H;
const KEY_W: f32 = 56.0; // piano key lane width
const VEL_H: f32 = 72.0; // velocity lane height
const CC_H: f32 = 80.0; // controller (CC) lane height
const RULER_H: f32 = 18.0; // bar/beat ruler header height
const RESIZE_ZONE: f32 = 6.0; // px on the right edge that starts a resize
/// Pixels of movement before an empty-grid press becomes a marquee drag.
const MARQUEE_DRAG_THRESHOLD: f32 = 4.0;

/// A copied note, stored with timing relative to the earliest note in the
/// selection so paste/duplicate can re-anchor the group at a new beat.
#[derive(Clone)]
struct ClipboardNote {
    pitch: u8,
    rel_start: f32,
    duration: f32,
    velocity: u8,
    muted: bool,
}

thread_local! {
    /// Process-global MIDI note clipboard. Lives outside any single editor so
    /// copy in the docked piano roll can paste in the floating one (both run on
    /// the GPUI main thread). Holds relative timing — not real notes.
    static MIDI_NOTE_CLIPBOARD: std::cell::RefCell<Vec<ClipboardNote>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Strength tier of a vertical timing gridline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GridLineKind {
    Bar,
    Beat,
    Subdivision,
}

/// `true` when `beat` is (within tolerance) an integer multiple of `m`.
#[inline]
fn is_multiple(beat: f32, m: f32) -> bool {
    if m <= 0.0 {
        return false;
    }
    let r = (beat / m).round();
    (beat - r * m).abs() < 1.0e-3
}

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

fn is_black(pitch: i32) -> bool {
    matches!(pitch.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

fn note_name(pitch: i32) -> String {
    format!(
        "{}{}",
        NOTE_NAMES[pitch.rem_euclid(12) as usize],
        pitch / 12 - 1
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PianoTool {
    Draw,
    Select,
}

/// Grid resolution in beats-per-step. Mirrors the WebUI dropdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridRes {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

impl GridRes {
    fn beats(self) -> f32 {
        match self {
            GridRes::Whole => 4.0,
            GridRes::Half => 2.0,
            GridRes::Quarter => 1.0,
            GridRes::Eighth => 0.5,
            GridRes::Sixteenth => 0.25,
            GridRes::ThirtySecond => 0.125,
        }
    }
    fn label(self) -> &'static str {
        match self {
            GridRes::Whole => "1",
            GridRes::Half => "1/2",
            GridRes::Quarter => "1/4",
            GridRes::Eighth => "1/8",
            GridRes::Sixteenth => "1/16",
            GridRes::ThirtySecond => "1/32",
        }
    }
    fn cycle(self) -> Self {
        match self {
            GridRes::Whole => GridRes::Half,
            GridRes::Half => GridRes::Quarter,
            GridRes::Quarter => GridRes::Eighth,
            GridRes::Eighth => GridRes::Sixteenth,
            GridRes::Sixteenth => GridRes::ThirtySecond,
            GridRes::ThirtySecond => GridRes::Whole,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarqueeSelectionMode {
    Replace,
    Add,
    Toggle,
    Subtract,
}

impl MarqueeSelectionMode {
    fn from_modifiers(modifiers: &gpui::Modifiers) -> Self {
        if modifiers.alt {
            MarqueeSelectionMode::Subtract
        } else if modifiers.control || modifiers.platform {
            MarqueeSelectionMode::Toggle
        } else if modifiers.shift {
            MarqueeSelectionMode::Add
        } else {
            MarqueeSelectionMode::Replace
        }
    }

    fn label(self) -> &'static str {
        match self {
            MarqueeSelectionMode::Replace => "Replace",
            MarqueeSelectionMode::Add => "Add",
            MarqueeSelectionMode::Toggle => "Toggle",
            MarqueeSelectionMode::Subtract => "Subtract",
        }
    }
}

#[derive(Debug, Clone)]
enum PianoDrag {
    None,
    /// Move the selected notes. `prev` snapshots each affected note's original
    /// (id, start, pitch). `dx_beats` / `dpitch` are the live, snapped deltas.
    Move {
        start_x: f32,
        start_y: f32,
        prev: Vec<(u64, f32, u8)>,
        dx_beats: f32,
        dpitch: i32,
    },
    /// Resize a single note from its right edge (also used to drag-extend a
    /// freshly drawn note). `new_dur` is the live length.
    Resize {
        id: u64,
        start_x: f32,
        prev_dur: f32,
        new_dur: f32,
    },
    /// Drag a velocity bar.
    Velocity {
        id: u64,
        start_y: f32,
        orig_vel: u8,
    },
    /// Rectangular marquee selection on the note grid (local grid px).
    MarqueeSelect {
        start_x: f32,
        start_y: f32,
        current_x: f32,
        current_y: f32,
        mode: MarqueeSelectionMode,
        /// `true` once the pointer moves past [`MARQUEE_DRAG_THRESHOLD`].
        dragging: bool,
    },
    /// Left-drag note creation preview (committed on mouse-up).
    DrawNote {
        pitch: u8,
        start_beat: f32,
        end_beat: f32,
    },
    /// Right-drag erase — ids collected until mouse-up.
    EraseNotes {
        start_x: f32,
        start_y: f32,
        current_x: f32,
        current_y: f32,
        erased: HashSet<u64>,
    },
    /// Paint (left) or erase (right) on the active CC lane. The lane's pre-drag
    /// points are snapshotted in `cc_edit_prev`; one undo entry on release.
    CcPaint {
        erase: bool,
    },
    /// Drag an existing CC point (by id) to a new beat/value. Pre-drag points
    /// are snapshotted in `cc_edit_prev`; one undo entry on release.
    CcMove {
        id: u64,
    },
}

pub struct PianoRoll {
    timeline: Entity<Timeline>,
    /// When `true`, commit logs use `[MIDI Editor]` (floating window instance).
    pub midi_editor_sink: bool,
    /// Docked editor only: opens the floating MIDI editor window.
    on_pop_out: Option<std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + Send + Sync>>,
    tool: PianoTool,
    ppb: f32,
    snap_on: bool,
    grid_res: GridRes,
    selection: HashSet<u64>,
    scroll_x: f32,
    scroll_y: f32,
    drag: PianoDrag,
    /// Selection snapshot taken when a marquee gesture begins (for modifier modes).
    selection_before_marquee: HashSet<u64>,
    /// Notes highlighted during an erase drag.
    erase_preview_ids: HashSet<u64>,
    /// Last clip id we ran [`Self::fit_piano_roll_to_notes`] for.
    fitted_clip_id: Option<String>,
    grid_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The controller lane currently shown/edited in the CC strip.
    active_cc: MidiControllerKind,
    /// Bounds of the CC strip, captured at paint for cursor → beat/value mapping.
    cc_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Lane points snapshotted when a CC paint/erase gesture begins (undo prev).
    cc_edit_prev: Option<Vec<MidiControllerPoint>>,
    /// Last clip the editor rendered — used to emit the `open_editor` debug log
    /// exactly once when the edited clip changes (not every frame).
    last_editing_clip: Option<String>,
    focus: FocusHandle,
}

impl PianoRoll {
    pub fn new(timeline: Entity<Timeline>, cx: &mut Context<Self>) -> Self {
        Self {
            timeline,
            midi_editor_sink: false,
            on_pop_out: None,
            tool: PianoTool::Draw,
            ppb: 80.0,
            snap_on: true,
            grid_res: GridRes::Sixteenth,
            selection: HashSet::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            drag: PianoDrag::None,
            selection_before_marquee: HashSet::new(),
            erase_preview_ids: HashSet::new(),
            fitted_clip_id: None,
            grid_bounds: Rc::new(Cell::new(None)),
            active_cc: MidiControllerKind::CC(1),
            cc_bounds: Rc::new(Cell::new(None)),
            cc_edit_prev: None,
            last_editing_clip: None,
            focus: cx.focus_handle(),
        }
    }

    pub fn set_pop_out_handler(
        &mut self,
        handler: Option<std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + Send + Sync>>,
    ) {
        self.on_pop_out = handler;
    }

    pub fn selected_note_count(&self) -> usize {
        self.selection.len()
    }

    pub fn grid_label(&self) -> &'static str {
        self.grid_res.label()
    }

    /// Menu / command-bar actions for the MIDI editor (shared menu IDs).
    pub fn run_menu_command(&mut self, command_id: &str, cx: &mut Context<Self>) {
        match command_id {
            "midi:select-all" => {
                let Some(clip_id) = self.editing_clip_id(cx) else {
                    return;
                };
                let all: Vec<u64> = self
                    .timeline
                    .read(cx)
                    .state
                    .midi_clip_notes(&clip_id)
                    .map(|notes| notes.iter().map(|n| n.id).collect())
                    .unwrap_or_default();
                self.selection = all.into_iter().collect();
                cx.notify();
            }
            "midi:delete-selected" => self.delete_selection(cx),
            "midi:quantize" => self.quantize_selection(cx),
            "midi:fit-notes" => {
                if let Some(cid) = self.editing_clip_id(cx) {
                    self.fit_piano_roll_to_notes(cx, &cid);
                    cx.notify();
                }
            }
            "midi:scroll-to-c4" | "midi:reset-pitch-zoom" => {
                self.scroll_to_pitch(60);
                cx.notify();
            }
            _ => {}
        }
    }

    // ── Editing target ───────────────────────────────────────────────────
    /// The selected clip id, but only if it is a MIDI clip.
    fn editing_clip_id(&self, cx: &Context<Self>) -> Option<String> {
        let tl = self.timeline.read(cx);
        let cid = tl.state.selection.selected_clip_ids.first()?.clone();
        tl.state.midi_clip_notes(&cid).map(|_| cid)
    }

    fn step_beats(&self) -> f32 {
        self.grid_res.beats()
    }

    fn snap_beats(&self, beats: f32) -> f32 {
        if !self.snap_on {
            return beats.max(0.0);
        }
        let step = self.step_beats();
        if step <= 0.0 {
            return beats.max(0.0);
        }
        ((beats / step).floor() * step).max(0.0)
    }

    // ── Coordinate helpers (local px → beat / pitch) ──────────────────────
    fn x_to_beat(&self, local_x: f32) -> f32 {
        ((local_x + self.scroll_x) / self.ppb.max(0.0001)).max(0.0)
    }
    fn beat_to_x(&self, beat: f32) -> f32 {
        beat * self.ppb - self.scroll_x
    }
    fn y_to_pitch(&self, local_y: f32) -> u8 {
        let row = ((local_y + self.scroll_y) / ROW_H).floor() as i32;
        (PITCH_CNT - 1 - row).clamp(0, PITCH_CNT - 1) as u8
    }
    fn pitch_to_y(&self, pitch: u8) -> f32 {
        (PITCH_CNT - 1 - pitch as i32) as f32 * ROW_H - self.scroll_y
    }

    fn point_to_beat_pitch(&self, local_x: f32, local_y: f32) -> (f32, u8) {
        (self.x_to_beat(local_x), self.y_to_pitch(local_y))
    }

    fn rects_intersect(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
        a.0 < b.2 && a.2 > b.0 && a.1 < b.3 && a.3 > b.1
    }

    fn normalized_marquee_rect(
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        view_w: f32,
        view_h: f32,
    ) -> (f32, f32, f32, f32) {
        let left = x0.min(x1).max(0.0);
        let top = y0.min(y1).max(0.0);
        let right = x0.max(x1).min(view_w);
        let bottom = y0.max(y1).min(view_h);
        (left, top, right, bottom)
    }

    fn apply_marquee_mode(
        before: &HashSet<u64>,
        hits: &HashSet<u64>,
        mode: MarqueeSelectionMode,
    ) -> HashSet<u64> {
        match mode {
            MarqueeSelectionMode::Replace => hits.clone(),
            MarqueeSelectionMode::Add => before.union(hits).copied().collect(),
            MarqueeSelectionMode::Toggle => before.symmetric_difference(hits).copied().collect(),
            MarqueeSelectionMode::Subtract => before.difference(hits).copied().collect(),
        }
    }

    fn begin_marquee_select(
        &mut self,
        lx: f32,
        ly: f32,
        mode: MarqueeSelectionMode,
        cx: &mut Context<Self>,
    ) {
        self.selection_before_marquee = self.selection.clone();
        self.drag = PianoDrag::MarqueeSelect {
            start_x: lx,
            start_y: ly,
            current_x: lx,
            current_y: ly,
            mode,
            dragging: false,
        };
        cx.notify();
    }

    fn update_marquee_select(&mut self, lx: f32, ly: f32, clip_id: &str, cx: &mut Context<Self>) {
        let (view_w, view_h) = self.grid_view_size();
        let clamped_x = lx.clamp(0.0, view_w);
        let clamped_y = ly.clamp(0.0, view_h);

        if let PianoDrag::MarqueeSelect {
            current_x,
            current_y,
            ..
        } = &mut self.drag
        {
            *current_x = clamped_x;
            *current_y = clamped_y;
        } else {
            return;
        }

        let (start_x, start_y, current_x, current_y, mode, was_dragging) = match &self.drag {
            PianoDrag::MarqueeSelect {
                start_x,
                start_y,
                current_x,
                current_y,
                mode,
                dragging,
            } => (*start_x, *start_y, *current_x, *current_y, *mode, *dragging),
            _ => return,
        };

        if !was_dragging {
            let dx = current_x - start_x;
            let dy = current_y - start_y;
            if (dx * dx + dy * dy).sqrt() < MARQUEE_DRAG_THRESHOLD {
                return;
            }
            if let PianoDrag::MarqueeSelect { dragging, .. } = &mut self.drag {
                *dragging = true;
            }
            if midi_debug_enabled() {
                eprintln!("[midi] marquee_start mode={}", mode.label());
            }
        }

        let marquee =
            Self::normalized_marquee_rect(start_x, start_y, current_x, current_y, view_w, view_h);
        let hits = self.marquee_hits(cx, clip_id, marquee);
        self.selection = Self::apply_marquee_mode(&self.selection_before_marquee, &hits, mode);

        if midi_debug_enabled() {
            let (min_beat, max_pitch) = self.point_to_beat_pitch(marquee.0, marquee.1);
            let (max_beat, min_pitch) = self.point_to_beat_pitch(marquee.2, marquee.3);
            let (min_pitch, max_pitch) = (min_pitch.min(max_pitch), min_pitch.max(max_pitch));
            let (min_beat, max_beat) = (min_beat.min(max_beat), min_beat.max(max_beat));
            eprintln!(
                "[midi] marquee_update beats={:.3}..{:.3} pitch={}..{} hits={}",
                min_beat,
                max_beat,
                min_pitch,
                max_pitch,
                hits.len()
            );
            eprintln!("[midi] marquee_mode mode={}", mode.label());
        }

        cx.notify();
    }

    fn commit_marquee_select(&mut self, cx: &mut Context<Self>) {
        let drag = std::mem::replace(&mut self.drag, PianoDrag::None);
        let PianoDrag::MarqueeSelect { dragging, mode, .. } = drag else {
            return;
        };

        if dragging {
            if midi_debug_enabled() {
                eprintln!("[midi] marquee_commit selected={}", self.selection.len());
            }
        } else if mode == MarqueeSelectionMode::Replace && !self.selection.is_empty() {
            // Click on empty grid without drag — clear selection.
            self.selection.clear();
            cx.notify();
        }

        self.selection_before_marquee.clear();
    }

    fn cancel_marquee_select(&mut self, cx: &mut Context<Self>) {
        if matches!(self.drag, PianoDrag::MarqueeSelect { .. }) {
            self.selection = self.selection_before_marquee.clone();
            self.selection_before_marquee.clear();
            self.drag = PianoDrag::None;
            cx.notify();
        }
    }

    /// Resolve the local (grid-relative) cursor position from a window-space
    /// point using the bounds captured during paint. `None` until the first
    /// frame has laid the grid out.
    fn grid_local(&self, window_pos: gpui::Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.grid_bounds.get()?;
        let ox: f32 = bounds.origin.x.into();
        let oy: f32 = bounds.origin.y.into();
        let x: f32 = window_pos.x.into();
        let y: f32 = window_pos.y.into();
        Some((x - ox, y - oy))
    }

    fn grid_view_size(&self) -> (f32, f32) {
        match self.grid_bounds.get() {
            Some(b) => (
                f32::from(b.size.width).max(1.0),
                f32::from(b.size.height).max(1.0),
            ),
            None => (600.0, 200.0),
        }
    }

    fn max_scroll_y(&self) -> f32 {
        let (_, h) = self.grid_view_size();
        (TOTAL_H - h).max(0.0)
    }

    fn clip_meta(&self, cx: &Context<Self>, clip_id: &str) -> (f32, f32) {
        let tl = self.timeline.read(cx);
        for track in &tl.state.tracks {
            for clip in &track.clips {
                if clip.id == clip_id {
                    return (clip.start_beat, clip.duration_beats);
                }
            }
        }
        (0.0, 4.0)
    }

    /// Scroll the pitch axis so `pitch` is vertically centered in the view.
    fn scroll_to_pitch(&mut self, pitch: u8) {
        let (_, view_h) = self.grid_view_size();
        let target = ((PITCH_CNT - 1) as f32 - pitch as f32) * ROW_H - view_h * 0.5 + ROW_H * 0.5;
        self.scroll_y = target.clamp(0.0, self.max_scroll_y());
    }

    /// Scroll/zoom the grid so selected notes (or all notes) are visible.
    fn fit_piano_roll_to_notes(&mut self, cx: &Context<Self>, clip_id: &str) {
        let (_, view_h) = self.grid_view_size();
        if view_h <= 1.0 {
            return;
        }

        let (notes, selected): (Vec<MidiNoteState>, HashSet<u64>) = {
            let tl = self.timeline.read(cx);
            let notes = tl
                .state
                .midi_clip_notes(clip_id)
                .cloned()
                .unwrap_or_default();
            (notes, self.selection.clone())
        };

        let target_notes: Vec<&MidiNoteState> = if !selected.is_empty() {
            notes.iter().filter(|n| selected.contains(&n.id)).collect()
        } else {
            notes.iter().collect()
        };

        let (min_p, max_p) = if target_notes.is_empty() {
            (60u8, 60u8)
        } else {
            let lo = target_notes.iter().map(|n| n.pitch).min().unwrap_or(60);
            let hi = target_notes.iter().map(|n| n.pitch).max().unwrap_or(60);
            (lo.saturating_sub(6), hi.saturating_add(6))
        };

        let mid = (min_p as f32 + max_p as f32) * 0.5;
        let target_scroll = ((PITCH_CNT - 1) as f32 - mid) * ROW_H - view_h * 0.5 + ROW_H * 0.5;
        self.scroll_y = target_scroll.clamp(0.0, self.max_scroll_y());

        if !target_notes.is_empty() {
            let min_start = target_notes
                .iter()
                .map(|n| n.start)
                .fold(f32::INFINITY, f32::min);
            self.scroll_x = (min_start * self.ppb - 24.0).max(0.0);
        } else {
            self.scroll_x = 0.0;
        }

        if midi_debug_enabled() {
            eprintln!(
                "[midi] piano_roll fit clip={} pitch={}..{} notes={}",
                clip_id,
                min_p,
                max_p,
                target_notes.len()
            );
        }
    }

    // ── Mutations through the timeline ────────────────────────────────────
    /// Full snapshots of the given note ids in a clip (undo prev/next state).
    fn snapshot_notes(&self, cx: &Context<Self>, clip_id: &str, ids: &[u64]) -> Vec<MidiNoteState> {
        self.timeline
            .read(cx)
            .state
            .midi_clip_notes(clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| ids.contains(&n.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Apply an in-place note transform (move / resize / quantize / transpose /
    /// nudge / bulk velocity) and record it as one undoable `EditMidiNotes`
    /// command. Captures full prev/next snapshots of `ids`; a no-op
    /// (`prev == next`) is dropped without dirtying the project.
    fn commit_note_transform<F>(&mut self, cx: &mut Context<Self>, ids: &[u64], mutate: F)
    where
        F: FnOnce(&mut TimelineState, &str),
    {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let prev = self.snapshot_notes(cx, &clip_id, ids);
        self.timeline.update(cx, |tl, tcx| {
            mutate(&mut tl.state, &clip_id);
            tcx.notify();
        });
        let next = self.snapshot_notes(cx, &clip_id, ids);
        self.push_note_edit(cx, clip_id, prev, next);
    }

    /// Record an `EditMidiNotes` command for an already-applied transform,
    /// skipping no-ops. Logs the commit for the floating-editor debug sink.
    fn push_note_edit(
        &mut self,
        cx: &mut Context<Self>,
        clip_id: String,
        prev: Vec<MidiNoteState>,
        next: Vec<MidiNoteState>,
    ) {
        if prev == next {
            return;
        }
        self.timeline.update(cx, |tl, tcx| {
            tl.record_executed_command(
                EditCommand::EditMidiNotes {
                    clip_id,
                    prev,
                    next,
                },
                tcx,
            );
        });
        if self.midi_editor_sink {
            crate::components::midi_editor_window::midi_editor_debug("edit command committed");
        }
    }

    /// Mutate the timeline for a *live* gesture (drag in progress): repaint but
    /// do NOT mark the project dirty, so we don't rebuild the engine snapshot on
    /// every mouse-move. The owning gesture marks dirty once on release.
    fn with_timeline_silent<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Timeline, &mut Context<Timeline>) -> R,
    ) -> R {
        self.timeline.update(cx, |tl, tcx| {
            let r = f(tl, tcx);
            tcx.notify();
            r
        })
    }

    fn run_edit_command(&mut self, cmd: EditCommand, cx: &mut Context<Self>) {
        self.timeline.update(cx, |tl, tcx| {
            tl.run_edit_command(cmd, tcx);
        });
        if self.midi_editor_sink {
            crate::components::midi_editor_window::midi_editor_debug("edit command committed");
        }
    }

    fn note_at_grid(&self, cx: &Context<Self>, clip_id: &str, lx: f32, ly: f32) -> Option<u64> {
        let (view_w, view_h) = self.grid_view_size();
        let rect = (
            (lx - 2.0).max(0.0),
            (ly - 2.0).max(0.0),
            (lx + 2.0).min(view_w),
            (ly + 2.0).min(view_h),
        );
        self.collect_notes_in_rect(cx, clip_id, rect)
            .into_iter()
            .next()
    }

    fn collect_notes_in_rect(
        &self,
        cx: &Context<Self>,
        clip_id: &str,
        rect: (f32, f32, f32, f32),
    ) -> HashSet<u64> {
        let tl = self.timeline.read(cx);
        let Some(notes) = tl.state.midi_clip_notes(clip_id) else {
            return HashSet::new();
        };
        notes
            .iter()
            .filter(|n| Self::rects_intersect(rect, self.note_to_rect(&self.display_note(n))))
            .map(|n| n.id)
            .collect()
    }

    // ── Mouse handlers ─────────────────────────────────────────────────────
    // Notes are interactive elements that handle their own select/move/resize/
    // delete (and stop propagation), so the grid surface only deals with empty
    // space: create a note (Draw tool) or clear the selection (Select tool).
    fn on_grid_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.focus);
        let Some((lx, ly)) = self.grid_local(event.position) else {
            // Bounds not captured yet (first frame) — ignore to avoid creating
            // a note at the wrong coordinate.
            return;
        };
        let Some(_clip_id) = self.editing_clip_id(cx) else {
            return;
        };

        let marquee_modifier = event.modifiers.shift
            || event.modifiers.control
            || event.modifiers.platform
            || event.modifiers.alt;

        if self.tool == PianoTool::Draw && !marquee_modifier {
            let pitch = self.y_to_pitch(ly);
            let start = self.snap_beats(self.x_to_beat(lx));
            self.drag = PianoDrag::DrawNote {
                pitch,
                start_beat: start,
                end_beat: start,
            };
            cx.notify();
        } else if self.tool == PianoTool::Select || marquee_modifier {
            let mode = MarqueeSelectionMode::from_modifiers(&event.modifiers);
            self.begin_marquee_select(lx, ly, mode, cx);
        }
    }

    fn on_grid_right_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.focus);
        let Some((lx, ly)) = self.grid_local(event.position) else {
            return;
        };
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let mut erased = HashSet::new();
        if let Some(id) = self.note_at_grid(cx, &clip_id, lx, ly) {
            erased.insert(id);
        }
        self.erase_preview_ids = erased.clone();
        self.drag = PianoDrag::EraseNotes {
            start_x: lx,
            start_y: ly,
            current_x: lx,
            current_y: ly,
            erased,
        };
        cx.notify();
    }

    fn note_right_down(&mut self, id: u64, lx: f32, ly: f32, cx: &mut Context<Self>) {
        let Some(_clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let erased = HashSet::from([id]);
        self.erase_preview_ids = erased.clone();
        self.drag = PianoDrag::EraseNotes {
            start_x: lx,
            start_y: ly,
            current_x: lx,
            current_y: ly,
            erased,
        };
        cx.notify();
    }
    fn note_mouse_down(
        &mut self,
        id: u64,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus);
        let shift = event.modifiers.shift;
        let ctrl = event.modifiers.control || event.modifiers.platform;
        if shift || ctrl {
            // Toggle this note in/out of the selection — no drag.
            if self.selection.contains(&id) {
                self.selection.remove(&id);
            } else {
                self.selection.insert(id);
            }
            cx.notify();
            return;
        }
        if !self.selection.contains(&id) {
            self.selection = HashSet::from([id]);
        }
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let prev = self.snapshot_selection(cx, &clip_id);
        self.drag = PianoDrag::Move {
            start_x: event.position.x.into(),
            start_y: event.position.y.into(),
            prev,
            dx_beats: 0.0,
            dpitch: 0,
        };
        cx.notify();
    }

    /// Right-edge handle mouse-down: begin a resize drag.
    fn begin_resize_drag(
        &mut self,
        id: u64,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus);
        self.selection = HashSet::from([id]);
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let prev_dur = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .and_then(|notes| notes.iter().find(|n| n.id == id))
            .map(|n| n.duration)
            .unwrap_or_else(|| self.step_beats());
        self.drag = PianoDrag::Resize {
            id,
            start_x: event.position.x.into(),
            prev_dur,
            new_dur: prev_dur,
        };
        cx.notify();
    }

    /// Velocity bar mouse-down: begin a velocity drag.
    fn begin_velocity_drag(
        &mut self,
        id: u64,
        orig_vel: u8,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus);
        self.selection = HashSet::from([id]);
        self.drag = PianoDrag::Velocity {
            id,
            start_y: event.position.y.into(),
            orig_vel,
        };
        cx.notify();
    }

    fn snapshot_selection(&self, cx: &Context<Self>, clip_id: &str) -> Vec<(u64, f32, u8)> {
        let tl = self.timeline.read(cx);
        tl.state
            .midi_clip_notes(clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| self.selection.contains(&n.id))
                    .map(|n| (n.id, n.start, n.pitch))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn on_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        match self.drag {
            PianoDrag::CcPaint { erase } => {
                if let Some((lx, ly)) = self.cc_local(event.position) {
                    self.cc_paint_at(lx, ly, erase, cx);
                }
                return;
            }
            PianoDrag::CcMove { id } => {
                if let Some((lx, ly)) = self.cc_local(event.position) {
                    self.cc_move_to(id, lx, ly, cx);
                }
                return;
            }
            _ => {}
        }
        if event.pressed_button == Some(MouseButton::Right) {
            let Some((lx, ly)) = self.grid_local(event.position) else {
                return;
            };
            let Some(clip_id) = self.editing_clip_id(cx) else {
                return;
            };
            if let PianoDrag::EraseNotes {
                current_x,
                current_y,
                ..
            } = &mut self.drag
            {
                *current_x = lx;
                *current_y = ly;
            }
            let (start_x, start_y, cur_x, cur_y) = match &self.drag {
                PianoDrag::EraseNotes {
                    start_x,
                    start_y,
                    current_x,
                    current_y,
                    ..
                } => (*start_x, *start_y, *current_x, *current_y),
                _ => return,
            };
            let (view_w, view_h) = self.grid_view_size();
            let rect =
                Self::normalized_marquee_rect(start_x, start_y, cur_x, cur_y, view_w, view_h);
            let hits = self.collect_notes_in_rect(cx, &clip_id, rect);
            if let PianoDrag::EraseNotes { erased, .. } = &mut self.drag {
                for id in hits {
                    erased.insert(id);
                }
                self.erase_preview_ids = erased.clone();
                cx.notify();
            }
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        if matches!(self.drag, PianoDrag::DrawNote { .. }) {
            if let Some((lx, _)) = self.grid_local(event.position) {
                let beat = self.snap_beats(self.x_to_beat(lx));
                if let PianoDrag::DrawNote { end_beat, .. } = &mut self.drag {
                    *end_beat = beat;
                    cx.notify();
                }
            }
            return;
        }
        if let PianoDrag::MarqueeSelect { .. } = &self.drag {
            let Some((lx, ly)) = self.grid_local(event.position) else {
                return;
            };
            let Some(clip_id) = self.editing_clip_id(cx) else {
                return;
            };
            self.update_marquee_select(lx, ly, &clip_id, cx);
            return;
        }
        match &mut self.drag {
            PianoDrag::None => {}
            PianoDrag::Move {
                start_x,
                start_y,
                dx_beats,
                dpitch,
                ..
            } => {
                let cur_x: f32 = event.position.x.into();
                let cur_y: f32 = event.position.y.into();
                // Store the raw beat delta; snapping is applied per-note against
                // each note's absolute start in `display_note` / commit.
                *dx_beats = (cur_x - *start_x) / self.ppb.max(0.0001);
                *dpitch = -(((cur_y - *start_y) / ROW_H).round() as i32);
                cx.notify();
            }
            PianoDrag::Resize {
                start_x,
                prev_dur,
                new_dur,
                ..
            } => {
                let cur_x: f32 = event.position.x.into();
                let mut d =
                    (*prev_dur + (cur_x - *start_x) / self.ppb.max(0.0001)).max(MIN_NOTE_BEATS);
                if self.snap_on {
                    let step = self.grid_res.beats();
                    d = ((d / step).round() * step).max(MIN_NOTE_BEATS);
                }
                *new_dur = d;
                cx.notify();
            }
            PianoDrag::Velocity {
                id,
                start_y,
                orig_vel,
            } => {
                let cur_y: f32 = event.position.y.into();
                let delta = (*start_y - cur_y).round() as i32;
                let new_vel = (*orig_vel as i32 + delta).clamp(1, 127) as u8;
                let id = *id;
                if let Some(clip_id) = self.editing_clip_id(cx) {
                    self.with_timeline_silent(cx, |tl, _| {
                        tl.state.set_midi_note_velocity(&clip_id, id, new_vel);
                    });
                }
            }
            PianoDrag::MarqueeSelect { .. } => {}
            PianoDrag::DrawNote { .. } | PianoDrag::EraseNotes { .. } => {}
            PianoDrag::CcPaint { .. } | PianoDrag::CcMove { .. } => {}
        }
    }

    fn commit_draw_note(&mut self, drag: PianoDrag, cx: &mut Context<Self>) {
        let PianoDrag::DrawNote {
            pitch,
            start_beat,
            end_beat,
        } = drag
        else {
            return;
        };
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let (lo, hi) = normalize_range(start_beat, end_beat);
        let step = self.step_beats().max(MIN_NOTE_BEATS);
        let mut duration = (hi - lo).max(step);
        if self.snap_on {
            duration = ((duration / step).ceil() * step).max(MIN_NOTE_BEATS);
        }
        // Do not clamp the note into the current clip length — a note drawn past
        // the clip end auto-expands the clip (see `CreateMidiNote::execute`).
        // `MidiNoteState::new` clamps start >= 0, pitch 0..=127, dur >= MIN.
        let note = MidiNoteState::new(pitch, lo, duration, 100);
        let id = note.id;
        self.run_edit_command(EditCommand::CreateMidiNote { clip_id, note }, cx);
        self.selection = HashSet::from([id]);
        cx.notify();
    }

    fn commit_erase_notes(&mut self, drag: PianoDrag, cx: &mut Context<Self>) {
        let PianoDrag::EraseNotes { erased, .. } = drag else {
            return;
        };
        self.erase_preview_ids.clear();
        if erased.is_empty() {
            return;
        }
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let notes: Vec<MidiNoteState> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| erased.contains(&n.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if notes.is_empty() {
            return;
        }
        self.run_edit_command(EditCommand::DeleteMidiNotes { clip_id, notes }, cx);
        self.selection.retain(|id| !erased.contains(id));
        cx.notify();
    }

    fn on_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if matches!(
            self.drag,
            PianoDrag::CcPaint { .. } | PianoDrag::CcMove { .. }
        ) {
            self.drag = PianoDrag::None;
            self.commit_cc_edit(cx);
            return;
        }
        if matches!(self.drag, PianoDrag::MarqueeSelect { .. }) {
            self.commit_marquee_select(cx);
            return;
        }
        let drag = std::mem::replace(&mut self.drag, PianoDrag::None);
        if matches!(drag, PianoDrag::DrawNote { .. }) {
            self.commit_draw_note(drag, cx);
            return;
        }
        if matches!(drag, PianoDrag::EraseNotes { .. }) {
            self.commit_erase_notes(drag, cx);
            return;
        }
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        match drag {
            PianoDrag::None => return,
            PianoDrag::Move {
                prev,
                dx_beats,
                dpitch,
                ..
            } => {
                if dx_beats.abs() < 0.0001 && dpitch == 0 {
                    return;
                }
                let updates: Vec<(u64, f32, u8)> = prev
                    .iter()
                    .map(|(id, start, pitch)| {
                        let new_start = self.snap_beats(*start + dx_beats);
                        let new_pitch = (*pitch as i32 + dpitch).clamp(0, 127) as u8;
                        (*id, new_start, new_pitch)
                    })
                    .collect();
                // Skip a commit (and the dirty flag) if snapping landed every
                // note back on its original position.
                let changed = updates
                    .iter()
                    .zip(prev.iter())
                    .any(|((_, ns, np), (_, os, op))| (ns - os).abs() > 1e-4 || np != op);
                if !changed {
                    return;
                }
                let ids: Vec<u64> = updates.iter().map(|(id, _, _)| *id).collect();
                self.commit_note_transform(cx, &ids, move |state, cid| {
                    state.move_midi_notes(cid, &updates)
                });
            }
            PianoDrag::Resize {
                id,
                new_dur,
                prev_dur,
                ..
            } => {
                if (new_dur - prev_dur).abs() < 0.0001 {
                    return;
                }
                self.commit_note_transform(cx, &[id], move |state, cid| {
                    state.resize_midi_note(cid, id, new_dur)
                });
            }
            PianoDrag::Velocity { id, orig_vel, .. } => {
                // Velocity was applied live (silent). Reconstruct the pre-drag
                // state from `orig_vel` and record one undoable edit.
                let next = self.snapshot_notes(cx, &clip_id, &[id]);
                let prev: Vec<MidiNoteState> = next
                    .iter()
                    .map(|n| {
                        let mut p = n.clone();
                        p.velocity = orig_vel;
                        p
                    })
                    .collect();
                self.push_note_edit(cx, clip_id, prev, next);
            }
            PianoDrag::MarqueeSelect { .. } => {}
            PianoDrag::DrawNote { .. } | PianoDrag::EraseNotes { .. } => {}
            PianoDrag::CcPaint { .. } | PianoDrag::CcMove { .. } => {}
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let key = event.keystroke.key.as_str();
        let ctrl = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        match key {
            "delete" | "backspace" if !self.selection.is_empty() => {
                cx.stop_propagation();
                self.delete_selection(cx);
            }
            "a" if ctrl => {
                cx.stop_propagation();
                let all: Vec<u64> = self
                    .timeline
                    .read(cx)
                    .state
                    .midi_clip_notes(&clip_id)
                    .map(|notes| notes.iter().map(|n| n.id).collect())
                    .unwrap_or_default();
                self.selection = all.into_iter().collect();
                cx.notify();
            }
            "c" if ctrl => {
                cx.stop_propagation();
                self.copy_selection(cx);
            }
            "v" if ctrl => {
                cx.stop_propagation();
                self.paste_clipboard(cx);
            }
            "d" if ctrl => {
                cx.stop_propagation();
                self.duplicate_selection(cx);
            }
            "m" if !ctrl => {
                cx.stop_propagation();
                self.toggle_mute_selection(cx);
            }
            "escape" => {
                cx.stop_propagation();
                if matches!(self.drag, PianoDrag::MarqueeSelect { .. }) {
                    self.cancel_marquee_select(cx);
                } else {
                    self.drag = PianoDrag::None;
                    self.selection.clear();
                    cx.notify();
                }
            }
            "f" if !ctrl => {
                cx.stop_propagation();
                self.fit_piano_roll_to_notes(cx, &clip_id);
                cx.notify();
            }
            _ => {}
        }
    }

    fn quantize_selection(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        // Empty selection means quantize every note in the clip.
        let mut ids: Vec<u64> = self.selection.iter().copied().collect();
        if ids.is_empty() {
            ids = self
                .timeline
                .read(cx)
                .state
                .midi_clip_notes(&clip_id)
                .map(|notes| notes.iter().map(|n| n.id).collect())
                .unwrap_or_default();
        }
        let step = self.grid_res.beats();
        let target_ids = ids.clone();
        self.commit_note_transform(cx, &ids, move |state, cid| {
            state.quantize_midi_notes(cid, &target_ids, step);
        });
    }

    fn delete_selection(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        if self.selection.is_empty() {
            return;
        }
        let ids: HashSet<u64> = self.selection.clone();
        let notes: Vec<MidiNoteState> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| ids.contains(&n.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if notes.is_empty() {
            return;
        }
        self.run_edit_command(EditCommand::DeleteMidiNotes { clip_id, notes }, cx);
        self.selection.clear();
    }

    /// Toggle mute on the selection. When the selection is mixed, the gesture
    /// mutes everything (the common DAW behaviour); when all are already muted
    /// it unmutes. Routed through an undoable command.
    fn toggle_mute_selection(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        if self.selection.is_empty() {
            return;
        }
        let prev: Vec<(u64, bool)> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| self.selection.contains(&n.id))
                    .map(|n| (n.id, n.muted))
                    .collect()
            })
            .unwrap_or_default();
        if prev.is_empty() {
            return;
        }
        // Unmute only when every selected note is already muted.
        let muted = !prev.iter().all(|(_, m)| *m);
        self.run_edit_command(
            EditCommand::SetMidiNotesMuted {
                clip_id,
                prev,
                muted,
            },
            cx,
        );
    }

    /// Copy the current selection into the process-global note clipboard,
    /// storing timing relative to the earliest selected note.
    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let notes: Vec<MidiNoteState> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| self.selection.contains(&n.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if notes.is_empty() {
            return;
        }
        let min_start = notes.iter().map(|n| n.start).fold(f32::INFINITY, f32::min);
        let clip: Vec<ClipboardNote> = notes
            .iter()
            .map(|n| ClipboardNote {
                pitch: n.pitch,
                rel_start: (n.start - min_start).max(0.0),
                duration: n.duration,
                velocity: n.velocity,
                muted: n.muted,
            })
            .collect();
        MIDI_NOTE_CLIPBOARD.with(|cb| *cb.borrow_mut() = clip);
        if midi_debug_enabled() {
            eprintln!("[midi] copy notes={}", notes.len());
        }
    }

    /// Build notes from the clipboard anchored so the earliest note lands at
    /// `anchor_beat` (clip-local). Returns an empty vec when the clipboard is
    /// empty. New notes get fresh transient ids.
    fn clipboard_notes_at(&self, anchor_beat: f32) -> Vec<MidiNoteState> {
        MIDI_NOTE_CLIPBOARD.with(|cb| {
            cb.borrow()
                .iter()
                .map(|c| {
                    let mut note = MidiNoteState::new(
                        c.pitch,
                        (anchor_beat + c.rel_start).max(0.0),
                        c.duration,
                        c.velocity,
                    );
                    note.muted = c.muted;
                    note
                })
                .collect()
        })
    }

    /// Paste the clipboard at the playhead (clip-local), falling back to clip
    /// beat 0 when the playhead is outside the clip. The pasted notes become the
    /// new selection.
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let (clip_start, _clip_len) = self.clip_meta(cx, &clip_id);
        let playhead = self.timeline.read(cx).state.transport.playhead_beats;
        let anchor = self.snap_beats((playhead - clip_start).max(0.0));
        let notes = self.clipboard_notes_at(anchor);
        if notes.is_empty() {
            return;
        }
        let new_ids: Vec<u64> = notes.iter().map(|n| n.id).collect();
        self.run_edit_command(EditCommand::CreateMidiNotes { clip_id, notes }, cx);
        self.selection = new_ids.into_iter().collect();
        if midi_debug_enabled() {
            eprintln!(
                "[midi] paste anchor={:.3} count={}",
                anchor,
                self.selection.len()
            );
        }
        cx.notify();
    }

    /// Duplicate the selection in place, offset forward by the selection's beat
    /// span so the copies sit immediately after the originals. Selects the new
    /// notes. Does not use the clipboard.
    fn duplicate_selection(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let src: Vec<MidiNoteState> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| self.selection.contains(&n.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if src.is_empty() {
            return;
        }
        let min_start = src.iter().map(|n| n.start).fold(f32::INFINITY, f32::min);
        let max_end = src
            .iter()
            .map(|n| n.start + n.duration)
            .fold(0.0_f32, f32::max);
        // Offset by the span, snapped so duplicates stay grid-aligned.
        let offset = self.snap_beats(max_end - min_start).max(self.step_beats());
        let notes: Vec<MidiNoteState> = src
            .iter()
            .map(|n| {
                let mut note =
                    MidiNoteState::new(n.pitch, n.start + offset, n.duration, n.velocity);
                note.muted = n.muted;
                note
            })
            .collect();
        let new_ids: Vec<u64> = notes.iter().map(|n| n.id).collect();
        self.run_edit_command(EditCommand::CreateMidiNotes { clip_id, notes }, cx);
        self.selection = new_ids.into_iter().collect();
        if midi_debug_enabled() {
            eprintln!(
                "[midi] duplicate offset={:.3} count={}",
                offset,
                self.selection.len()
            );
        }
        cx.notify();
    }

    fn note_inspector_snapshot(&self, cx: &Context<Self>, clip_id: &str) -> NoteInspectorSnapshot {
        let selected = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|note| self.selection.contains(&note.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        NoteInspectorSnapshot { selected }
    }

    fn selected_note_ids(&self) -> Vec<u64> {
        self.selection.iter().copied().collect()
    }

    fn nudge_selected_pitch(&mut self, semitones: i32, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let ids = self.selected_note_ids();
        if ids.is_empty() {
            return;
        }
        let will_change = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes.iter().any(|note| {
                    ids.contains(&note.id)
                        && note.pitch != (note.pitch as i32 + semitones).clamp(0, 127) as u8
                })
            })
            .unwrap_or(false);
        if !will_change {
            return;
        }
        let target_ids = ids.clone();
        self.commit_note_transform(cx, &ids, move |state, cid| {
            state.transpose_midi_notes(cid, &target_ids, semitones);
        });
    }

    fn nudge_selected_start(&mut self, delta_beats: f32, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let ids = self.selected_note_ids();
        if ids.is_empty() {
            return;
        }
        let updates: Vec<(u64, f32, u8)> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|note| ids.contains(&note.id))
                    .filter_map(|note| {
                        let new_start = (note.start + delta_beats).max(0.0);
                        ((note.start - new_start).abs() > 1.0e-4)
                            .then_some((note.id, new_start, note.pitch))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if updates.is_empty() {
            return;
        }
        let ids: Vec<u64> = updates.iter().map(|(id, _, _)| *id).collect();
        self.commit_note_transform(cx, &ids, move |state, cid| {
            state.move_midi_notes(cid, &updates);
        });
    }

    fn nudge_selected_length(&mut self, delta_beats: f32, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let ids = self.selected_note_ids();
        if ids.is_empty() {
            return;
        }
        let updates: Vec<(u64, f32)> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|note| ids.contains(&note.id))
                    .filter_map(|note| {
                        let duration = (note.duration + delta_beats).max(MIN_NOTE_BEATS);
                        ((note.duration - duration).abs() > 1.0e-4).then_some((note.id, duration))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if updates.is_empty() {
            return;
        }
        let ids: Vec<u64> = updates.iter().map(|(id, _)| *id).collect();
        self.commit_note_transform(cx, &ids, move |state, cid| {
            for (id, duration) in updates {
                state.set_midi_note_length(cid, id, duration);
            }
        });
    }

    fn nudge_selected_velocity(&mut self, delta: i16, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let ids = self.selected_note_ids();
        if ids.is_empty() {
            return;
        }
        let updates: Vec<(u64, u8)> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|note| ids.contains(&note.id))
                    .filter_map(|note| {
                        let velocity = (note.velocity as i16 + delta).clamp(1, 127) as u8;
                        (note.velocity != velocity).then_some((note.id, velocity))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if updates.is_empty() {
            return;
        }
        let ids: Vec<u64> = updates.iter().map(|(id, _)| *id).collect();
        self.commit_note_transform(cx, &ids, move |state, cid| {
            for (id, velocity) in updates {
                state.set_midi_note_velocity(cid, id, velocity);
            }
        });
    }

    fn on_wheel(&mut self, event: &ScrollWheelEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let (dx, dy) = match event.delta {
            gpui::ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
            gpui::ScrollDelta::Lines(p) => (p.x * 36.0, p.y * 36.0),
        };
        if event.modifiers.control || event.modifiers.platform {
            // Zoom horizontal.
            let factor = (1.0015_f32).powf(-dy);
            self.ppb = (self.ppb * factor).clamp(20.0, 400.0);
        } else if event.modifiers.shift {
            self.scroll_x = (self.scroll_x - dy - dx).max(0.0);
        } else {
            self.scroll_y = (self.scroll_y - dy).clamp(0.0, self.max_scroll_y());
            self.scroll_x = (self.scroll_x - dx).max(0.0);
        }
        cx.notify();
    }
}

// ── Live display geometry during a drag ─────────────────────────────────────
struct DisplayNote {
    id: u64,
    pitch: u8,
    start: f32,
    duration: f32,
    velocity: u8,
}

#[derive(Clone)]
struct NoteInspectorSnapshot {
    selected: Vec<MidiNoteState>,
}

impl NoteInspectorSnapshot {
    fn count(&self) -> usize {
        self.selected.len()
    }

    fn pitch_label(&self) -> String {
        uniform_u8(&self.selected, |n| n.pitch)
            .map(|pitch| format!("{} ({})", note_name(pitch as i32), pitch))
            .unwrap_or_else(|| "Mixed".to_string())
    }

    fn start_label(&self) -> String {
        uniform_f32(&self.selected, |n| n.start)
            .map(format_beats)
            .unwrap_or_else(|| "Mixed".to_string())
    }

    fn length_label(&self) -> String {
        uniform_f32(&self.selected, |n| n.duration)
            .map(format_beats)
            .unwrap_or_else(|| "Mixed".to_string())
    }

    fn velocity_label(&self) -> String {
        uniform_u8(&self.selected, |n| n.velocity)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "Mixed".to_string())
    }

    fn end_label(&self) -> String {
        if self.selected.len() == 1 {
            let note = &self.selected[0];
            format_beats(note.start + note.duration)
        } else {
            let Some(first) = self.selected.first() else {
                return "--".to_string();
            };
            let (min_start, max_end) = self.selected.iter().fold(
                (first.start, first.start + first.duration),
                |(min_start, max_end), note| {
                    (
                        min_start.min(note.start),
                        max_end.max(note.start + note.duration),
                    )
                },
            );
            format!("{}..{}", format_beats(min_start), format_beats(max_end))
        }
    }
}

impl PianoRoll {
    fn display_note(&self, n: &MidiNoteState) -> DisplayNote {
        let mut start = n.start;
        let mut pitch = n.pitch;
        let mut duration = n.duration;
        match &self.drag {
            PianoDrag::Move {
                prev,
                dx_beats,
                dpitch,
                ..
            } => {
                if prev.iter().any(|(id, _, _)| *id == n.id) {
                    // Snap the absolute resulting start (WebUI semantics), not
                    // the raw delta, so notes always land on the grid.
                    start = self.snap_beats(n.start + dx_beats);
                    pitch = (n.pitch as i32 + dpitch).clamp(0, 127) as u8;
                }
            }
            PianoDrag::Resize { id, new_dur, .. } if *id == n.id => {
                duration = *new_dur;
            }
            _ => {}
        }
        DisplayNote {
            id: n.id,
            pitch,
            start,
            duration,
            velocity: n.velocity,
        }
    }

    fn note_to_rect(&self, note: &DisplayNote) -> (f32, f32, f32, f32) {
        let x = self.beat_to_x(note.start);
        let w = (note.duration * self.ppb).max(3.0);
        let y = self.pitch_to_y(note.pitch) + 1.0;
        let h = ROW_H - 2.0;
        (x, y, x + w, y + h)
    }

    fn marquee_hits(
        &self,
        cx: &Context<Self>,
        clip_id: &str,
        marquee: (f32, f32, f32, f32),
    ) -> HashSet<u64> {
        let tl = self.timeline.read(cx);
        let Some(notes) = tl.state.midi_clip_notes(clip_id) else {
            return HashSet::new();
        };
        notes
            .iter()
            .filter(|n| {
                let d = self.display_note(n);
                Self::rects_intersect(marquee, self.note_to_rect(&d))
            })
            .map(|n| n.id)
            .collect()
    }

    fn build_draw_note_preview(&self) -> Option<gpui::AnyElement> {
        let PianoDrag::DrawNote {
            pitch,
            start_beat,
            end_beat,
        } = &self.drag
        else {
            return None;
        };
        let (lo, hi) = normalize_range(*start_beat, *end_beat);
        let step = self.step_beats().max(MIN_NOTE_BEATS);
        let duration = (hi - lo).max(step);
        let x = self.beat_to_x(lo);
        let w = (duration * self.ppb).max(3.0);
        let y = self.pitch_to_y(*pitch);
        Some(
            div()
                .absolute()
                .left(px(x))
                .top(px(y + 1.0))
                .w(px(w))
                .h(px(ROW_H - 2.0))
                .rounded(px(2.0))
                .bg(Colors::with_alpha(Colors::accent_primary(), 0.35))
                .border(px(1.0))
                .border_color(Colors::with_alpha(Colors::accent_primary(), 0.85))
                .into_any_element(),
        )
    }

    fn build_erase_overlay(&self) -> Option<gpui::AnyElement> {
        let PianoDrag::EraseNotes {
            start_x,
            start_y,
            current_x,
            current_y,
            ..
        } = &self.drag
        else {
            return None;
        };
        let (view_w, view_h) = self.grid_view_size();
        let (left, top, right, bottom) = Self::normalized_marquee_rect(
            *start_x, *start_y, *current_x, *current_y, view_w, view_h,
        );
        let w = (right - left).max(0.0);
        let h = (bottom - top).max(0.0);
        if w < 1.0 && h < 1.0 {
            return None;
        }
        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(w.max(1.0)))
                .h(px(h.max(1.0)))
                .bg(Colors::with_alpha(Colors::status_error(), 0.12))
                .border(px(1.0))
                .border_color(Colors::with_alpha(Colors::status_error(), 0.75))
                .into_any_element(),
        )
    }

    fn build_marquee_overlay(&self) -> Option<gpui::AnyElement> {
        let PianoDrag::MarqueeSelect {
            start_x,
            start_y,
            current_x,
            current_y,
            dragging: true,
            ..
        } = &self.drag
        else {
            return None;
        };

        let (view_w, view_h) = self.grid_view_size();
        let (left, top, right, bottom) = Self::normalized_marquee_rect(
            *start_x, *start_y, *current_x, *current_y, view_w, view_h,
        );
        let w = (right - left).max(0.0);
        let h = (bottom - top).max(0.0);
        if w < 1.0 || h < 1.0 {
            return None;
        }

        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(w))
                .h(px(h))
                .bg(Colors::with_alpha(Colors::accent_primary(), 0.15))
                .border(px(1.0))
                .border_color(Colors::with_alpha(Colors::accent_primary(), 0.85))
                .into_any_element(),
        )
    }
}

impl Render for PianoRoll {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clip_id = self.editing_clip_id(cx);

        if clip_id != self.last_editing_clip {
            if midi_debug_enabled() {
                if let Some(cid) = clip_id.as_deref() {
                    let tl = self.timeline.read(cx);
                    let track_id = tl
                        .state
                        .tracks
                        .iter()
                        .find(|t| t.clips.iter().any(|c| c.id == cid))
                        .map(|t| t.id.as_str())
                        .unwrap_or("<none>");
                    let notes = tl.state.midi_clip_notes(cid).map(|n| n.len()).unwrap_or(0);
                    eprintln!(
                        "[midi] open_editor clip_id={} track_id={} notes={}",
                        cid, track_id, notes
                    );
                }
            }
            self.last_editing_clip = clip_id.clone();
            self.fitted_clip_id = None;
        }

        if let Some(cid) = clip_id.as_deref() {
            if self.fitted_clip_id.as_deref() != Some(cid) {
                self.fit_piano_roll_to_notes(cx, cid);
                self.fitted_clip_id = Some(cid.to_string());
            }
        }

        // Toolbar is always shown; the body shows a hint when no MIDI clip is
        // selected.
        let toolbar = self.render_toolbar(cx, clip_id.as_deref());

        let body: gpui::AnyElement = match clip_id {
            Some(cid) => self.render_body(cx, &cid).into_any_element(),
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.0))
                .text_color(Colors::text_muted())
                .child("Select or double-click a MIDI clip to edit")
                .into_any_element(),
        };

        div()
            .key_context("PianoRoll")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .bg(Colors::surface_base())
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::on_up))
            .on_scroll_wheel(cx.listener(Self::on_wheel))
            .child(toolbar)
            .child(body)
    }
}

impl PianoRoll {
    fn render_toolbar(&self, cx: &mut Context<Self>, clip_id: Option<&str>) -> impl IntoElement {
        let note_count = clip_id
            .and_then(|cid| {
                self.timeline
                    .read(cx)
                    .state
                    .midi_clip_notes(cid)
                    .map(|n| n.len())
            })
            .unwrap_or(0);
        let sel_count = self.selection.len();
        let tool = self.tool;
        let snap_on = self.snap_on;
        let grid_label = self.grid_res.label();
        let cc_label = cc_kind_label(self.active_cc);

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .h(px(30.0))
            .px(px(8.0))
            .border_b(px(1.0))
            .border_color(Colors::panel_border())
            .bg(Colors::surface_panel())
            .child(tool_btn(
                "pr-draw",
                "Draw",
                tool == PianoTool::Draw,
                cx.listener(|this, _, _w, cx| {
                    this.tool = PianoTool::Draw;
                    cx.notify();
                }),
            ))
            .child(tool_btn(
                "pr-select",
                "Select",
                tool == PianoTool::Select,
                cx.listener(|this, _, _w, cx| {
                    this.tool = PianoTool::Select;
                    cx.notify();
                }),
            ))
            .child(divider())
            .child(tool_btn(
                "pr-snap",
                "Snap",
                snap_on,
                cx.listener(|this, _, _w, cx| {
                    this.snap_on = !this.snap_on;
                    cx.notify();
                }),
            ))
            .child(tool_btn(
                "pr-grid",
                grid_label,
                false,
                cx.listener(|this, _, _w, cx| {
                    this.grid_res = this.grid_res.cycle();
                    cx.notify();
                }),
            ))
            .child(divider())
            .child(tool_btn(
                "pr-quantize",
                "Q",
                false,
                cx.listener(|this, _, _w, cx| this.quantize_selection(cx)),
            ))
            .child(tool_btn(
                "pr-delete",
                "Del",
                false,
                cx.listener(|this, _, _w, cx| this.delete_selection(cx)),
            ))
            .child(divider())
            .child(tool_btn(
                "pr-mute",
                "Mute",
                false,
                cx.listener(|this, _, _w, cx| this.toggle_mute_selection(cx)),
            ))
            .child(tool_btn(
                "pr-dup",
                "Dup",
                false,
                cx.listener(|this, _, _w, cx| this.duplicate_selection(cx)),
            ))
            .child(divider())
            .child(tool_btn(
                "pr-cc",
                &cc_label,
                false,
                cx.listener(|this, _, _w, cx| {
                    this.active_cc = cc_cycle(this.active_cc);
                    cx.notify();
                }),
            ))
            .child(divider())
            .child(tool_btn(
                "pr-fit",
                "Fit",
                false,
                cx.listener(|this, _, _w, cx| {
                    if let Some(cid) = this.editing_clip_id(cx) {
                        this.fit_piano_roll_to_notes(cx, &cid);
                        cx.notify();
                    }
                }),
            ))
            .child(tool_btn(
                "pr-c4",
                "C4",
                false,
                cx.listener(|this, _, _w, cx| {
                    this.scroll_to_pitch(60);
                    cx.notify();
                }),
            ))
            .child(div().flex_1())
            .when_some(self.on_pop_out.clone(), |row, pop_out| {
                row.child(
                    div()
                        .id("pr-pop-out")
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded_md()
                        .text_size(px(9.0))
                        .text_color(Colors::text_secondary())
                        .cursor(gpui::CursorStyle::PointingHand)
                        .hover(|s| s.bg(Colors::surface_hover()))
                        .on_click(move |_, window, cx| pop_out(window, cx))
                        .child("Pop out"),
                )
            })
            .child(
                div()
                    .text_size(px(9.0))
                    .text_color(Colors::text_muted())
                    .child(format!("{} notes · {} sel", note_count, sel_count)),
            )
    }

    fn render_body(&mut self, cx: &mut Context<Self>, clip_id: &str) -> impl IntoElement {
        let (view_w, view_h) = self.grid_view_size();
        let track_color = self.track_color_for_clip(cx, clip_id);
        let (bpb, _clip_start, clip_len, show_playhead, playhead_rel) = {
            let tl = self.timeline.read(cx);
            let bpb = tl.state.beats_per_bar().max(1.0);
            let (clip_start, clip_len) = self.clip_meta(cx, clip_id);
            let playhead_rel = tl.state.transport.playhead_beats - clip_start;
            let show_playhead =
                tl.state.transport.playing && playhead_rel >= 0.0 && playhead_rel <= clip_len;
            (bpb, clip_start, clip_len, show_playhead, playhead_rel)
        };

        // Visible ranges (only build geometry for what's on screen).
        let first_pitch = (self.y_to_pitch(view_h) as i32 - 1).max(0);
        let last_pitch = (self.y_to_pitch(0.0) as i32 + 1).min(PITCH_CNT - 1);
        let start_beat = self.x_to_beat(0.0);
        let end_beat = self.x_to_beat(view_w);

        // Piano key lane.
        // Label policy: show every note name when each row has enough vertical
        // room (>= 14 px), otherwise fall back to C-only labels so the lane
        // stays readable.
        let show_all_labels = ROW_H >= 14.0;
        let keys: Vec<_> = (first_pitch..=last_pitch)
            .map(|p| {
                let y = self.pitch_to_y(p as u8);
                let black = is_black(p);
                let is_c = p.rem_euclid(12) == 0;
                let label_color = if is_c {
                    Colors::text_primary()
                } else if black {
                    Colors::text_muted()
                } else {
                    Colors::text_secondary()
                };
                let show_label = is_c || show_all_labels;
                div()
                    .absolute()
                    .top(px(y))
                    .left_0()
                    .w_full()
                    .h(px(ROW_H))
                    .bg(if black {
                        Colors::surface_base()
                    } else {
                        Colors::surface_raised()
                    })
                    .border_b(px(1.0))
                    .border_color(Colors::border_subtle())
                    .flex()
                    .items_center()
                    .justify_end()
                    .pr(px(5.0))
                    .when(show_label, |this| {
                        this.child(
                            div()
                                .text_size(px(8.0))
                                .text_color(label_color)
                                .child(note_name(p)),
                        )
                    })
            })
            .collect();

        let grid_lines = self.build_grid_lines(
            start_beat,
            end_beat,
            view_w,
            view_h,
            first_pitch,
            last_pitch,
            bpb,
            clip_len,
        );
        let clip_bounds = self.build_clip_bounds_overlay(clip_len, view_w, view_h);
        let playhead_line = if show_playhead {
            Some(self.build_playhead_line(playhead_rel))
        } else {
            None
        };
        let ruler = self.build_ruler(start_beat, end_beat, bpb);
        let vel_grid = self.build_velocity_grid(start_beat, end_beat, bpb);
        let notes_geo = self.build_note_elements(cx, clip_id, track_color);
        let marquee_overlay = self.build_marquee_overlay();
        let draw_preview = self.build_draw_note_preview();
        let erase_overlay = self.build_erase_overlay();
        let vel_bars = self.build_velocity_bars(cx, clip_id, track_color);
        let note_inspector = self.render_note_inspector(cx, clip_id);
        let cc_label = cc_kind_label(self.active_cc);
        let cc_lane = self
            .render_cc_lane(cx, clip_id, start_beat, end_beat, bpb)
            .into_any_element();
        let grid_cursor = if self.tool == PianoTool::Draw {
            gpui::CursorStyle::Crosshair
        } else {
            gpui::CursorStyle::Arrow
        };

        // Capture grid bounds so empty-area clicks can be mapped to beat/pitch.
        let grid_bounds = self.grid_bounds.clone();
        let grid_canvas = canvas(
            move |bounds, _w, _cx| {
                grid_bounds.set(Some(bounds));
            },
            |_b, _r, _w, _cx| {},
        )
        .absolute()
        .inset_0();

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_row()
            // Left: piano keys.
            .child(
                div()
                    .w(px(KEY_W))
                    .h_full()
                    .flex()
                    .flex_col()
                    // Corner spacer so the keys line up with the grid (below the
                    // ruler row on the right).
                    .child(
                        div()
                            .h(px(RULER_H))
                            .w_full()
                            .bg(Colors::surface_panel())
                            .border_b(px(1.0))
                            .border_r(px(1.0))
                            .border_color(Colors::panel_border()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .overflow_hidden()
                            .bg(Colors::surface_panel())
                            .border_r(px(1.0))
                            .border_color(Colors::panel_border())
                            .children(keys),
                    )
                    .child(
                        div()
                            .h(px(VEL_H))
                            .w_full()
                            .border_t(px(1.0))
                            .border_color(Colors::panel_border())
                            .bg(Colors::surface_panel())
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(8.0))
                            .text_color(Colors::text_muted())
                            .child("VEL"),
                    )
                    // CC lane label (aligned to the CC strip on the right).
                    .child(
                        div()
                            .h(px(CC_H))
                            .w_full()
                            .border_t(px(1.0))
                            .border_color(Colors::panel_border())
                            .bg(Colors::surface_panel())
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(9.0))
                            .text_color(Colors::text_secondary())
                            .child(cc_label),
                    ),
            )
            // Right: grid + velocity lane.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    // Ruler header — bar/beat labels aligned to the grid below.
                    .child(
                        div()
                            .h(px(RULER_H))
                            .w_full()
                            .relative()
                            .overflow_hidden()
                            .bg(Colors::surface_panel())
                            .border_b(px(1.0))
                            .border_color(Colors::panel_border())
                            .children(ruler),
                    )
                    // Note grid.
                    .child(
                        div()
                            .id("piano-grid")
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .overflow_hidden()
                            .bg(Colors::surface_base())
                            .cursor(grid_cursor)
                            .child(grid_canvas)
                            .children(grid_lines)
                            .children(clip_bounds)
                            .when_some(playhead_line, |el, line| el.child(line))
                            .children(notes_geo)
                            .when_some(marquee_overlay, |el, overlay| el.child(overlay))
                            .when_some(draw_preview, |el, overlay| el.child(overlay))
                            .when_some(erase_overlay, |el, overlay| el.child(overlay))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_grid_down))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(Self::on_grid_right_down),
                            ),
                    )
                    // Velocity lane.
                    .child(
                        div()
                            .id("piano-vel")
                            .h(px(VEL_H))
                            .w_full()
                            .relative()
                            .overflow_hidden()
                            .border_t(px(1.0))
                            .border_color(Colors::panel_border())
                            .bg(Colors::surface_panel_alt())
                            .children(vel_grid)
                            .children(vel_bars),
                    )
                    // CC controller lane.
                    .child(cc_lane),
            )
            .child(note_inspector)
    }

    fn render_note_inspector(&self, cx: &mut Context<Self>, clip_id: &str) -> impl IntoElement {
        let snapshot = self.note_inspector_snapshot(cx, clip_id);
        let count = snapshot.count();
        let step = self.grid_res.beats().max(MIN_NOTE_BEATS);
        let fine_step = (step * 0.25).max(MIN_NOTE_BEATS);

        let mut content: Vec<gpui::AnyElement> = Vec::new();
        content.push(note_inspector_label("NOTE INSPECTOR").into_any_element());

        if count == 0 {
            content.push(
                div()
                    .text_size(px(10.0))
                    .text_color(Colors::text_muted())
                    .line_height(px(15.0))
                    .child("Select notes in the piano roll to edit pitch, timing, and velocity.")
                    .into_any_element(),
            );
        } else if count == 1 {
            let note = &snapshot.selected[0];
            content.push(note_value_row("Pitch", snapshot.pitch_label()).into_any_element());
            content.push(note_value_row("Start", format_beats(note.start)).into_any_element());
            content.push(note_value_row("Length", format_beats(note.duration)).into_any_element());
            content.push(
                note_value_row("End", format_beats(note.start + note.duration)).into_any_element(),
            );
            content.push(note_value_row("Velocity", note.velocity.to_string()).into_any_element());
            content.push(
                note_button_row(vec![
                    note_action_button(
                        "pr-note-pitch-down",
                        "-1 st",
                        cx.listener(|this, _, _w, cx| this.nudge_selected_pitch(-1, cx)),
                    )
                    .into_any_element(),
                    note_action_button(
                        "pr-note-pitch-up",
                        "+1 st",
                        cx.listener(|this, _, _w, cx| this.nudge_selected_pitch(1, cx)),
                    )
                    .into_any_element(),
                ])
                .into_any_element(),
            );
            content.push(
                note_button_row(vec![
                    note_action_button(
                        "pr-note-start-down",
                        "-Start",
                        cx.listener(move |this, _, _w, cx| this.nudge_selected_start(-step, cx)),
                    )
                    .into_any_element(),
                    note_action_button(
                        "pr-note-start-up",
                        "+Start",
                        cx.listener(move |this, _, _w, cx| this.nudge_selected_start(step, cx)),
                    )
                    .into_any_element(),
                ])
                .into_any_element(),
            );
            content.push(
                note_button_row(vec![
                    note_action_button(
                        "pr-note-len-down",
                        "-Len",
                        cx.listener(move |this, _, _w, cx| {
                            this.nudge_selected_length(-fine_step, cx)
                        }),
                    )
                    .into_any_element(),
                    note_action_button(
                        "pr-note-len-up",
                        "+Len",
                        cx.listener(move |this, _, _w, cx| {
                            this.nudge_selected_length(fine_step, cx)
                        }),
                    )
                    .into_any_element(),
                ])
                .into_any_element(),
            );
            content.push(
                note_button_row(vec![
                    note_action_button(
                        "pr-note-vel-down",
                        "Vel -5",
                        cx.listener(|this, _, _w, cx| this.nudge_selected_velocity(-5, cx)),
                    )
                    .into_any_element(),
                    note_action_button(
                        "pr-note-vel-up",
                        "Vel +5",
                        cx.listener(|this, _, _w, cx| this.nudge_selected_velocity(5, cx)),
                    )
                    .into_any_element(),
                ])
                .into_any_element(),
            );
            let mute_label = if note.muted { "Unmute" } else { "Mute" };
            content.push(
                note_button_row(vec![note_action_button(
                    "pr-note-mute",
                    mute_label,
                    cx.listener(|this, _, _w, cx| this.toggle_mute_selection(cx)),
                )
                .into_any_element()])
                .into_any_element(),
            );
        } else {
            content.push(note_value_row("Selected", count.to_string()).into_any_element());
            content.push(note_value_row("Pitch", snapshot.pitch_label()).into_any_element());
            content.push(note_value_row("Range", snapshot.end_label()).into_any_element());
            content.push(note_value_row("Start", snapshot.start_label()).into_any_element());
            content.push(note_value_row("Length", snapshot.length_label()).into_any_element());
            content.push(note_value_row("Velocity", snapshot.velocity_label()).into_any_element());
            content.push(
                note_button_row(vec![
                    note_action_button(
                        "pr-notes-trans-down",
                        "-1 st",
                        cx.listener(|this, _, _w, cx| this.nudge_selected_pitch(-1, cx)),
                    )
                    .into_any_element(),
                    note_action_button(
                        "pr-notes-trans-up",
                        "+1 st",
                        cx.listener(|this, _, _w, cx| this.nudge_selected_pitch(1, cx)),
                    )
                    .into_any_element(),
                ])
                .into_any_element(),
            );
            content.push(
                note_button_row(vec![
                    note_action_button(
                        "pr-notes-vel-down",
                        "Vel -5",
                        cx.listener(|this, _, _w, cx| this.nudge_selected_velocity(-5, cx)),
                    )
                    .into_any_element(),
                    note_action_button(
                        "pr-notes-vel-up",
                        "Vel +5",
                        cx.listener(|this, _, _w, cx| this.nudge_selected_velocity(5, cx)),
                    )
                    .into_any_element(),
                ])
                .into_any_element(),
            );
            content.push(
                note_button_row(vec![
                    note_action_button(
                        "pr-notes-quantize",
                        "Quantize",
                        cx.listener(|this, _, _w, cx| this.quantize_selection(cx)),
                    )
                    .into_any_element(),
                    note_action_button(
                        "pr-notes-delete",
                        "Delete",
                        cx.listener(|this, _, _w, cx| this.delete_selection(cx)),
                    )
                    .into_any_element(),
                ])
                .into_any_element(),
            );
            content.push(
                note_button_row(vec![
                    note_action_button(
                        "pr-notes-mute",
                        "Mute",
                        cx.listener(|this, _, _w, cx| this.toggle_mute_selection(cx)),
                    )
                    .into_any_element(),
                    note_action_button(
                        "pr-notes-duplicate",
                        "Duplicate",
                        cx.listener(|this, _, _w, cx| this.duplicate_selection(cx)),
                    )
                    .into_any_element(),
                ])
                .into_any_element(),
            );
        }

        div()
            .w(px(216.0))
            .h_full()
            .flex()
            .flex_col()
            .gap(px(7.0))
            .p(px(8.0))
            .border_l(px(1.0))
            .border_color(Colors::panel_border())
            .bg(Colors::surface_panel())
            .children(content)
    }

    fn track_color_for_clip(&self, cx: &Context<Self>, clip_id: &str) -> gpui::Rgba {
        let tl = self.timeline.read(cx);
        tl.state
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
            .map(|t| t.color)
            .unwrap_or_else(Colors::accent_primary)
    }

    /// Compute the visible vertical gridlines with a zoom-aware subdivision
    /// tier. Returns `(x_px, kind)` for each line in `[start_beat, end_beat]`.
    ///
    /// Tiering by `px_per_beat` (`self.ppb`):
    /// - always: bar lines
    /// - `ppb >= 10`: beat lines
    /// - subdivision (snap step) lines only when they're at least ~7 px apart
    ///   and the view is zoomed in enough — keeps far-zoom views uncluttered.
    fn visible_grid_lines(
        &self,
        start_beat: f32,
        end_beat: f32,
        bpb: f32,
    ) -> Vec<(f32, GridLineKind)> {
        let ppb = self.ppb.max(0.0001);
        let bpb = bpb.max(1.0);
        let show_beats = ppb >= 10.0;
        let sub_step = self.grid_res.beats().max(1.0 / 32.0);
        let show_subs = show_beats && sub_step * ppb >= 7.0 && ppb >= 24.0;

        let iter_step = if show_subs {
            sub_step
        } else if show_beats {
            1.0
        } else {
            bpb
        };

        let mut out = Vec::new();
        let mut beat = (start_beat / iter_step).floor() * iter_step;
        let mut guard = 0;
        while beat <= end_beat + iter_step && guard < 8000 {
            guard += 1;
            let b = beat;
            beat += iter_step;
            if b < -1.0e-3 {
                continue;
            }
            let kind = if is_multiple(b, bpb) {
                GridLineKind::Bar
            } else if is_multiple(b, 1.0) {
                GridLineKind::Beat
            } else {
                GridLineKind::Subdivision
            };
            let keep = match kind {
                GridLineKind::Bar => true,
                GridLineKind::Beat => show_beats,
                GridLineKind::Subdivision => show_subs,
            };
            if keep {
                out.push((self.beat_to_x(b), kind));
            }
        }
        out
    }

    fn build_clip_bounds_overlay(
        &self,
        clip_len: f32,
        view_w: f32,
        view_h: f32,
    ) -> Vec<gpui::AnyElement> {
        let mut out = Vec::new();
        let end_x = self.beat_to_x(clip_len);
        if end_x < view_w {
            out.push(
                div()
                    .absolute()
                    .left(px(end_x))
                    .top_0()
                    .w(px((view_w - end_x).max(0.0)))
                    .h(px(view_h))
                    .bg(Colors::with_alpha(Colors::surface_base(), 0.55))
                    .into_any_element(),
            );
        }
        out.push(
            div()
                .absolute()
                .left(px(0.0))
                .top_0()
                .w(px(1.0))
                .h(px(view_h))
                .bg(Colors::with_alpha(Colors::accent_primary(), 0.35))
                .into_any_element(),
        );
        if end_x > 0.0 && end_x <= view_w + 2.0 {
            out.push(
                div()
                    .absolute()
                    .left(px(end_x))
                    .top_0()
                    .w(px(1.0))
                    .h(px(view_h))
                    .bg(Colors::with_alpha(Colors::accent_primary(), 0.55))
                    .into_any_element(),
            );
        }
        out
    }

    fn build_playhead_line(&self, rel_beat: f32) -> gpui::AnyElement {
        let x = self.beat_to_x(rel_beat);
        div()
            .absolute()
            .left(px(x))
            .top_0()
            .w(px(1.0))
            .h_full()
            .bg(Colors::with_alpha(Colors::status_warning(), 0.9))
            .into_any_element()
    }

    fn build_grid_lines(
        &self,
        start_beat: f32,
        end_beat: f32,
        view_w: f32,
        _view_h: f32,
        first_pitch: i32,
        last_pitch: i32,
        bpb: f32,
        clip_len: f32,
    ) -> Vec<gpui::AnyElement> {
        let mut out: Vec<gpui::AnyElement> = Vec::new();

        // ── Pitch row backgrounds: shade black-key rows, highlight C rows ──
        for p in first_pitch..=last_pitch {
            let y = self.pitch_to_y(p as u8);
            if is_black(p) {
                out.push(
                    div()
                        .absolute()
                        .top(px(y))
                        .left_0()
                        .w(px(view_w))
                        .h(px(ROW_H))
                        .bg(Colors::with_alpha(Colors::surface_base(), 0.45))
                        .into_any_element(),
                );
            } else if p % 12 == 0 {
                // C row — a touch brighter so octaves are easy to scan.
                out.push(
                    div()
                        .absolute()
                        .top(px(y))
                        .left_0()
                        .w(px(view_w))
                        .h(px(ROW_H))
                        .bg(Colors::with_alpha(Colors::text_primary(), 0.03))
                        .into_any_element(),
                );
            }
        }

        // Clip end marker inside the visible beat range.
        let end_x = self.beat_to_x(clip_len);
        if end_x >= 0.0 && end_x <= view_w {
            out.push(
                div()
                    .absolute()
                    .left(px((end_x - 0.5).max(0.0)))
                    .top_0()
                    .w(px(1.0))
                    .h_full()
                    .bg(Colors::with_alpha(Colors::accent_primary(), 0.4))
                    .into_any_element(),
            );
        }

        // ── Vertical timing gridlines (zoom-aware hierarchy) ──
        for (x, kind) in self.visible_grid_lines(start_beat, end_beat.min(clip_len + bpb), bpb) {
            let (alpha, w) = match kind {
                GridLineKind::Bar => (0.26, 1.0),
                GridLineKind::Beat => (0.13, 1.0),
                GridLineKind::Subdivision => (0.06, 1.0),
            };
            out.push(
                div()
                    .absolute()
                    .top_0()
                    .left(px(x))
                    .w(px(w))
                    .h_full()
                    .bg(Colors::with_alpha(Colors::text_primary(), alpha))
                    .into_any_element(),
            );
        }

        // ── Horizontal pitch row lines ──
        // Draw a line for every visible semitone row so editing reads like a
        // real piano roll. C gets the strongest line (octave boundary), F gets
        // a medium line (the other white-white separator on a piano), and every
        // other row gets a faint hairline.
        for p in first_pitch..=last_pitch {
            let m = p.rem_euclid(12);
            let alpha = match m {
                0 => 0.14,  // C: octave boundary
                5 => 0.07,  // F: white/white separator
                _ => 0.035, // every other semitone row
            };
            let y = self.pitch_to_y(p as u8);
            out.push(
                div()
                    .absolute()
                    .top(px(y))
                    .left_0()
                    .w(px(view_w))
                    .h(px(1.0))
                    .bg(Colors::with_alpha(Colors::text_primary(), alpha))
                    .into_any_element(),
            );
        }

        out
    }

    /// Bar/beat ruler header labels, aligned to the note grid via `beat_to_x`.
    fn build_ruler(&self, start_beat: f32, end_beat: f32, bpb: f32) -> Vec<gpui::AnyElement> {
        let ppb = self.ppb.max(0.0001);
        let bpb = bpb.max(1.0);
        // Label each beat when zoomed in; otherwise label only bar starts.
        let label_beats = ppb >= 36.0;
        let step = if label_beats { 1.0 } else { bpb };

        let mut out: Vec<gpui::AnyElement> = Vec::new();
        let mut beat = (start_beat / step).floor() * step;
        let mut guard = 0;
        while beat <= end_beat + step && guard < 2000 {
            guard += 1;
            let b = beat;
            beat += step;
            if b < -1.0e-3 {
                continue;
            }
            let x = self.beat_to_x(b);
            let bar = (b / bpb).floor() as i32 + 1;
            let on_bar = is_multiple(b, bpb);
            let text = if label_beats {
                let beat_in_bar = (b - (bar - 1) as f32 * bpb).floor() as i32 + 1;
                format!("{}.{}", bar, beat_in_bar)
            } else {
                format!("{}", bar)
            };
            out.push(
                div()
                    .absolute()
                    .top_0()
                    .left(px(x + 2.0))
                    .text_size(px(8.5))
                    .text_color(if on_bar {
                        Colors::text_secondary()
                    } else {
                        Colors::text_muted()
                    })
                    .child(text)
                    .into_any_element(),
            );
            // Tick mark at the bottom of the ruler.
            out.push(
                div()
                    .absolute()
                    .left(px(x))
                    .bottom_0()
                    .w(px(1.0))
                    .h(px(if on_bar { 6.0 } else { 4.0 }))
                    .bg(Colors::with_alpha(
                        Colors::text_primary(),
                        if on_bar { 0.26 } else { 0.13 },
                    ))
                    .into_any_element(),
            );
        }
        out
    }

    /// Bar/beat vertical lines through the velocity lane (aligned with the grid;
    /// subdivisions omitted to keep the lane uncluttered).
    fn build_velocity_grid(
        &self,
        start_beat: f32,
        end_beat: f32,
        bpb: f32,
    ) -> Vec<gpui::AnyElement> {
        self.visible_grid_lines(start_beat, end_beat, bpb)
            .into_iter()
            .filter(|(_, kind)| *kind != GridLineKind::Subdivision)
            .map(|(x, kind)| {
                let alpha = if kind == GridLineKind::Bar {
                    0.20
                } else {
                    0.10
                };
                div()
                    .absolute()
                    .top_0()
                    .left(px(x))
                    .w(px(1.0))
                    .h_full()
                    .bg(Colors::with_alpha(Colors::text_primary(), alpha))
                    .into_any_element()
            })
            .collect()
    }

    fn build_note_elements(
        &mut self,
        cx: &mut Context<Self>,
        clip_id: &str,
        track_color: gpui::Rgba,
    ) -> Vec<gpui::AnyElement> {
        let (view_w, view_h) = self.grid_view_size();
        // Collect owned geometry first so the timeline read borrow is released
        // before we build per-note listeners (which borrow `cx` mutably).
        let geos: Vec<(u64, f32, f32, f32, bool, bool, bool)> = {
            let tl = self.timeline.read(cx);
            let Some(notes) = tl.state.midi_clip_notes(clip_id) else {
                return Vec::new();
            };
            notes
                .iter()
                .filter_map(|n| {
                    let d = self.display_note(n);
                    let x = self.beat_to_x(d.start);
                    let w = (d.duration * self.ppb).max(3.0);
                    let y = self.pitch_to_y(d.pitch);
                    // Cull off-screen notes.
                    if x + w < 0.0 || x > view_w || y + ROW_H < 0.0 || y > view_h {
                        return None;
                    }
                    Some((
                        d.id,
                        x,
                        y,
                        w,
                        self.selection.contains(&d.id),
                        self.erase_preview_ids.contains(&d.id),
                        n.muted,
                    ))
                })
                .collect()
        };

        geos.into_iter()
            .map(|(id, x, y, w, selected, erase_target, muted)| {
                let mut fill = track_color;
                fill.a = if erase_target {
                    0.45
                } else if muted {
                    // Muted notes read as hollow/dim so they stand apart from
                    // active notes without leaving the grid.
                    0.18
                } else if selected {
                    1.0
                } else {
                    0.78
                };
                let border = if erase_target {
                    Colors::status_error()
                } else if selected {
                    Colors::text_primary()
                } else if muted {
                    Colors::with_alpha(Colors::text_muted(), 0.7)
                } else {
                    Colors::with_alpha(track_color, 0.55)
                };
                let mut note = div()
                    .id(("pr-note", id as usize))
                    .absolute()
                    .left(px(x))
                    .top(px(y + 1.0))
                    .w(px(w))
                    .h(px(ROW_H - 2.0))
                    .rounded(px(2.0))
                    .bg(fill)
                    .border(px(1.0))
                    .border_color(border)
                    .cursor(gpui::CursorStyle::PointingHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.note_mouse_down(id, ev, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, ev: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            let (lx, ly) = this.grid_local(ev.position).unwrap_or((0.0, 0.0));
                            this.note_right_down(id, lx, ly, cx);
                        }),
                    );
                // Right-edge resize handle (only when the note is wide enough to
                // leave room for a separate move/resize zone).
                if w >= 12.0 {
                    note = note.child(
                        div()
                            .id(("pr-note-edge", id as usize))
                            .absolute()
                            .right_0()
                            .top_0()
                            .w(px(RESIZE_ZONE))
                            .h_full()
                            .cursor(gpui::CursorStyle::ResizeLeftRight)
                            .hover(|s| s.bg(Colors::with_alpha(Colors::text_primary(), 0.35)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.begin_resize_drag(id, ev, window, cx);
                                }),
                            ),
                    );
                }
                note.into_any_element()
            })
            .collect()
    }

    fn build_velocity_bars(
        &mut self,
        cx: &mut Context<Self>,
        clip_id: &str,
        track_color: gpui::Rgba,
    ) -> Vec<gpui::AnyElement> {
        let (view_w, _) = self.grid_view_size();
        let geos: Vec<(u64, u8, f32, bool)> = {
            let tl = self.timeline.read(cx);
            let Some(notes) = tl.state.midi_clip_notes(clip_id) else {
                return Vec::new();
            };
            notes
                .iter()
                .filter_map(|n| {
                    let d = self.display_note(n);
                    let x = self.beat_to_x(d.start);
                    if x < -8.0 || x > view_w {
                        return None;
                    }
                    Some((d.id, d.velocity, x, self.selection.contains(&d.id)))
                })
                .collect()
        };

        geos.into_iter()
            .map(|(id, vel, x, selected)| {
                let bar_h = (((vel as f32 - 1.0) / 126.0) * (VEL_H - 8.0)).max(1.0);
                let mut fill = track_color;
                fill.a = if selected { 1.0 } else { 0.5 };
                // Full-height invisible hit column so even low-velocity bars are
                // easy to grab; the colored bar sits inside it at the bottom.
                div()
                    .id(("pr-vel", id as usize))
                    .absolute()
                    .left(px(x))
                    .top_0()
                    .bottom_0()
                    .w(px(8.0))
                    .cursor(gpui::CursorStyle::ResizeUpDown)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.begin_velocity_drag(id, vel, ev, window, cx);
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .bottom(px(2.0))
                            .w(px(6.0))
                            .h(px(bar_h))
                            .rounded_t(px(1.0))
                            .bg(fill),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

// ── CC controller lane ───────────────────────────────────────────────────────
const CC_PRESETS: [MidiControllerKind; 7] = [
    MidiControllerKind::CC(1),
    MidiControllerKind::CC(7),
    MidiControllerKind::CC(10),
    MidiControllerKind::CC(11),
    MidiControllerKind::CC(64),
    MidiControllerKind::PitchBend,
    MidiControllerKind::ChannelPressure,
];

fn cc_kind_label(kind: MidiControllerKind) -> String {
    match kind {
        MidiControllerKind::CC(n) => format!("CC{n}"),
        MidiControllerKind::PitchBend => "Bend".to_string(),
        MidiControllerKind::ChannelPressure => "Press".to_string(),
        MidiControllerKind::PolyPressure => "Poly".to_string(),
    }
}

fn cc_cycle(kind: MidiControllerKind) -> MidiControllerKind {
    let idx = CC_PRESETS.iter().position(|k| *k == kind);
    match idx {
        Some(i) => CC_PRESETS[(i + 1) % CC_PRESETS.len()],
        None => CC_PRESETS[0],
    }
}

impl PianoRoll {
    fn cc_view_size(&self) -> (f32, f32) {
        match self.cc_bounds.get() {
            Some(b) => (
                f32::from(b.size.width).max(1.0),
                f32::from(b.size.height).max(1.0),
            ),
            None => (600.0, CC_H),
        }
    }

    fn cc_local(&self, window_pos: gpui::Point<Pixels>) -> Option<(f32, f32)> {
        let b = self.cc_bounds.get()?;
        let ox: f32 = b.origin.x.into();
        let oy: f32 = b.origin.y.into();
        let x: f32 = window_pos.x.into();
        let y: f32 = window_pos.y.into();
        Some((x - ox, y - oy))
    }

    /// Begin a CC paint (`erase = false`) or erase (`erase = true`) gesture:
    /// ensure the active lane, snapshot its points for undo, and apply the first
    /// edit at the cursor.
    fn begin_cc_paint(
        &mut self,
        erase: bool,
        lx: f32,
        ly: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus);
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        self.timeline.update(cx, |tl, _| {
            tl.state.ensure_controller_lane(&clip_id, kind);
        });
        self.cc_edit_prev = Some(
            self.timeline
                .read(cx)
                .state
                .controller_points_snapshot(&clip_id, kind),
        );
        self.drag = PianoDrag::CcPaint { erase };
        self.cc_paint_at(lx, ly, erase, cx);
        cx.notify();
    }

    /// Apply one CC edit at a local strip coordinate (live, not yet committed).
    fn cc_paint_at(&mut self, lx: f32, ly: f32, erase: bool, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        let beat = self.snap_beats(self.x_to_beat(lx));
        let (_, cc_h) = self.cc_view_size();
        let value = (1.0 - (ly / cc_h.max(1.0))).clamp(0.0, 1.0);
        let tol = (self.step_beats() * 0.5).max(1.0e-3);
        self.timeline.update(cx, |tl, tcx| {
            if erase {
                tl.state
                    .delete_controller_points_near(&clip_id, kind, beat, tol);
            } else {
                tl.state.put_controller_point(&clip_id, kind, beat, value);
            }
            tcx.notify();
        });
    }

    /// Hit-test the active lane's points; return the id of one within ~6 px of
    /// the local strip coordinate.
    fn cc_point_at(&self, cx: &Context<Self>, clip_id: &str, lx: f32, ly: f32) -> Option<u64> {
        let (_, cc_h) = self.cc_view_size();
        let kind = self.active_cc;
        let tl = self.timeline.read(cx);
        let points = tl.state.controller_lane_points(clip_id, kind)?;
        const R: f32 = 6.0;
        points.iter().find_map(|p| {
            let x = self.beat_to_x(p.beat);
            let y = (1.0 - p.value) * (cc_h - 6.0) + 3.0;
            ((lx - x).abs() <= R && (ly - y).abs() <= R).then_some(p.id)
        })
    }

    /// Begin dragging an existing CC point; snapshot the lane for undo.
    fn begin_cc_move(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        self.cc_edit_prev = Some(
            self.timeline
                .read(cx)
                .state
                .controller_points_snapshot(&clip_id, kind),
        );
        self.drag = PianoDrag::CcMove { id };
        cx.notify();
    }

    /// Move the dragged CC point to the cursor (beat snapped, value continuous).
    fn cc_move_to(&mut self, id: u64, lx: f32, ly: f32, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        let beat = self.snap_beats(self.x_to_beat(lx));
        let (_, cc_h) = self.cc_view_size();
        let value = (1.0 - (ly / cc_h.max(1.0))).clamp(0.0, 1.0);
        self.timeline.update(cx, |tl, tcx| {
            tl.state
                .set_controller_point(&clip_id, kind, id, beat, value);
            tcx.notify();
        });
    }

    /// Commit a finished CC gesture as one undoable command (skips no-ops).
    fn commit_cc_edit(&mut self, cx: &mut Context<Self>) {
        let Some(prev) = self.cc_edit_prev.take() else {
            return;
        };
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let kind = self.active_cc;
        let next = self
            .timeline
            .read(cx)
            .state
            .controller_points_snapshot(&clip_id, kind);
        if prev == next {
            return;
        }
        self.timeline.update(cx, |tl, tcx| {
            tl.record_executed_command(
                EditCommand::SetControllerPoints {
                    clip_id,
                    kind,
                    prev,
                    next,
                },
                tcx,
            );
        });
        if self.midi_editor_sink {
            crate::components::midi_editor_window::midi_editor_debug("edit command committed");
        }
    }

    fn build_cc_points(&self, cx: &Context<Self>, clip_id: &str) -> Vec<gpui::AnyElement> {
        let (view_w, cc_h) = self.cc_view_size();
        let kind = self.active_cc;
        let pts: Vec<(f32, f32)> = self
            .timeline
            .read(cx)
            .state
            .controller_lane_points(clip_id, kind)
            .map(|ps| ps.iter().map(|p| (p.beat, p.value)).collect())
            .unwrap_or_default();
        let accent = Colors::accent_primary();
        pts.into_iter()
            .filter_map(|(beat, value)| {
                let x = self.beat_to_x(beat);
                if x < -6.0 || x > view_w + 6.0 {
                    return None;
                }
                let y = (1.0 - value) * (cc_h - 6.0) + 3.0;
                Some(
                    div()
                        .absolute()
                        .left(px(x - 3.0))
                        .top_0()
                        .w(px(6.0))
                        .h_full()
                        // Stem from the point down to the lane floor.
                        .child(
                            div()
                                .absolute()
                                .left(px(2.0))
                                .top(px(y))
                                .w(px(2.0))
                                .bottom(px(0.0))
                                .bg(Colors::with_alpha(accent, 0.35)),
                        )
                        // Point dot.
                        .child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .top(px(y - 3.0))
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded(px(3.0))
                                .bg(accent),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// The CC strip (right column) plus its captured bounds + interaction.
    fn render_cc_lane(
        &mut self,
        cx: &mut Context<Self>,
        clip_id: &str,
        start_beat: f32,
        end_beat: f32,
        bpb: f32,
    ) -> impl IntoElement {
        let grid = self.build_velocity_grid(start_beat, end_beat, bpb);
        let points = self.build_cc_points(cx, clip_id);
        let cc_bounds = self.cc_bounds.clone();
        let canvas = canvas(
            move |bounds, _w, _cx| cc_bounds.set(Some(bounds)),
            |_b, _r, _w, _cx| {},
        )
        .absolute()
        .inset_0();
        div()
            .id("piano-cc")
            .h(px(CC_H))
            .w_full()
            .relative()
            .overflow_hidden()
            .border_t(px(1.0))
            .border_color(Colors::panel_border())
            .bg(Colors::surface_panel_alt())
            .cursor(gpui::CursorStyle::Crosshair)
            .child(canvas)
            .children(grid)
            .children(points)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if let Some((lx, ly)) = this.cc_local(ev.position) {
                        // Grab an existing point to move it; otherwise paint.
                        if let Some(cid) = this.editing_clip_id(cx) {
                            if let Some(id) = this.cc_point_at(cx, &cid, lx, ly) {
                                this.begin_cc_move(id, window, cx);
                                return;
                            }
                        }
                        this.begin_cc_paint(false, lx, ly, window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if let Some((lx, ly)) = this.cc_local(ev.position) {
                        this.begin_cc_paint(true, lx, ly, window, cx);
                    }
                }),
            )
    }
}

// ── Small toolbar helpers ───────────────────────────────────────────────────
fn divider() -> impl IntoElement {
    div()
        .w(px(1.0))
        .h(px(16.0))
        .bg(Colors::with_alpha(Colors::text_primary(), 0.08))
}

fn tool_btn(
    id: &'static str,
    label: &str,
    active: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(22.0))
        .min_w(px(24.0))
        .px(px(7.0))
        .rounded(px(4.0))
        .text_size(px(10.0))
        .text_color(if active {
            Colors::text_primary()
        } else {
            Colors::text_secondary()
        })
        .bg(if active {
            Colors::surface_hover()
        } else {
            Colors::with_alpha(Colors::text_primary(), 0.0)
        })
        .border(px(1.0))
        .border_color(if active {
            Colors::border_subtle()
        } else {
            Colors::with_alpha(Colors::text_primary(), 0.0)
        })
        .hover(|s| s.bg(Colors::surface_hover()))
        .cursor(gpui::CursorStyle::PointingHand)
        .on_click(move |ev, w, cx| on_click(ev, w, cx))
        .child(label.to_string())
}

fn note_inspector_label(label: &str) -> impl IntoElement {
    div()
        .text_size(px(9.0))
        .text_color(Colors::text_muted())
        .font_weight(gpui::FontWeight::BOLD)
        .child(label.to_string())
}

fn note_value_row(label: &str, value: String) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .min_h(px(22.0))
        .child(
            div()
                .text_size(px(9.0))
                .text_color(Colors::text_muted())
                .child(label.to_string()),
        )
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(10.0))
                .text_color(Colors::text_primary())
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(value),
        )
}

fn note_button_row(children: Vec<gpui::AnyElement>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(4.0))
        .children(children)
}

fn note_action_button(
    id: &'static str,
    label: &str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(22.0))
        .min_w(px(54.0))
        .px(px(6.0))
        .rounded(px(4.0))
        .text_size(px(10.0))
        .text_color(Colors::text_secondary())
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .bg(Colors::surface_raised())
        .hover(|s| s.bg(Colors::surface_hover()))
        .cursor(gpui::CursorStyle::PointingHand)
        .on_click(move |ev, w, cx| on_click(ev, w, cx))
        .child(label.to_string())
}

fn uniform_u8(notes: &[MidiNoteState], f: impl Fn(&MidiNoteState) -> u8) -> Option<u8> {
    let first = notes.first().map(&f)?;
    if notes.iter().all(|note| f(note) == first) {
        Some(first)
    } else {
        None
    }
}

fn uniform_f32(notes: &[MidiNoteState], f: impl Fn(&MidiNoteState) -> f32) -> Option<f32> {
    let first = notes.first().map(&f)?;
    if notes.iter().all(|note| (f(note) - first).abs() <= 1.0e-4) {
        Some(first)
    } else {
        None
    }
}

fn format_beats(value: f32) -> String {
    format!("{:.3}", value.max(0.0))
}
