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

use crate::components::timeline::timeline::Timeline;
use crate::components::timeline::timeline_state::{MidiNoteState, MIN_NOTE_BEATS};
use crate::theme::Colors;

// ── Layout constants (CSS px) ───────────────────────────────────────────────
const ROW_H: f32 = 14.0; // px per semitone
const PITCH_CNT: i32 = 128;
const TOTAL_H: f32 = PITCH_CNT as f32 * ROW_H;
const KEY_W: f32 = 56.0; // piano key lane width
const VEL_H: f32 = 72.0; // velocity lane height
const RULER_H: f32 = 18.0; // bar/beat ruler header height
const RESIZE_ZONE: f32 = 6.0; // px on the right edge that starts a resize

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
    format!("{}{}", NOTE_NAMES[pitch.rem_euclid(12) as usize], pitch / 12 - 1)
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
}

pub struct PianoRoll {
    timeline: Entity<Timeline>,
    tool: PianoTool,
    ppb: f32,
    snap_on: bool,
    grid_res: GridRes,
    selection: HashSet<u64>,
    scroll_x: f32,
    scroll_y: f32,
    drag: PianoDrag,
    /// Whether `scroll_y` has been centred on C4 yet (done once after the grid
    /// height is known).
    centered: bool,
    grid_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Last clip the editor rendered — used to emit the `open_editor` debug log
    /// exactly once when the edited clip changes (not every frame).
    last_editing_clip: Option<String>,
    focus: FocusHandle,
}

impl PianoRoll {
    pub fn new(timeline: Entity<Timeline>, cx: &mut Context<Self>) -> Self {
        Self {
            timeline,
            tool: PianoTool::Draw,
            ppb: 80.0,
            snap_on: true,
            grid_res: GridRes::Sixteenth,
            selection: HashSet::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            drag: PianoDrag::None,
            centered: false,
            grid_bounds: Rc::new(Cell::new(None)),
            last_editing_clip: None,
            focus: cx.focus_handle(),
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
            Some(b) => (f32::from(b.size.width).max(1.0), f32::from(b.size.height).max(1.0)),
            None => (600.0, 200.0),
        }
    }

    fn max_scroll_y(&self) -> f32 {
        let (_, h) = self.grid_view_size();
        (TOTAL_H - h).max(0.0)
    }

    // ── Mutations through the timeline ────────────────────────────────────
    fn with_timeline<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Timeline, &mut Context<Timeline>) -> R,
    ) -> R {
        self.timeline.update(cx, |tl, tcx| {
            let r = f(tl, tcx);
            tl.mark_project_changed(tcx);
            tcx.notify();
            r
        })
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

    fn mark_project_dirty(&mut self, cx: &mut Context<Self>) {
        self.timeline.update(cx, |tl, tcx| tl.mark_project_changed(tcx));
    }

    // ── Mouse handlers ─────────────────────────────────────────────────────
    // Notes are interactive elements that handle their own select/move/resize/
    // delete (and stop propagation), so the grid surface only deals with empty
    // space: create a note (Draw tool) or clear the selection (Select tool).
    fn on_grid_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        let Some((lx, ly)) = self.grid_local(event.position) else {
            // Bounds not captured yet (first frame) — ignore to avoid creating
            // a note at the wrong coordinate.
            return;
        };
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        if self.tool == PianoTool::Draw {
            let pitch = self.y_to_pitch(ly);
            let start = self.snap_beats(self.x_to_beat(lx));
            let dur = self.step_beats().max(MIN_NOTE_BEATS);
            let new_id = self.with_timeline(cx, |tl, _| {
                tl.state.add_midi_note(&clip_id, pitch, start, dur, 100)
            });
            if let Some(id) = new_id {
                self.selection = HashSet::from([id]);
                // Drag right to extend the freshly drawn note.
                self.drag = PianoDrag::Resize {
                    id,
                    start_x: event.position.x.into(),
                    prev_dur: dur,
                    new_dur: dur,
                };
            }
            cx.notify();
        } else if !self.selection.is_empty() {
            self.selection.clear();
            cx.notify();
        }
    }

    /// Note body mouse-down: (multi-)select and begin a move drag.
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

    /// Right-click a note: delete it (or the whole selection if part of one).
    fn note_right_down(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let ids: Vec<u64> = if self.selection.contains(&id) && self.selection.len() > 1 {
            self.selection.iter().copied().collect()
        } else {
            vec![id]
        };
        self.with_timeline(cx, |tl, _| {
            tl.state.delete_midi_notes(&clip_id, &ids);
        });
        self.selection.clear();
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
        if event.pressed_button != Some(MouseButton::Left) {
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
                let mut d = (*prev_dur + (cur_x - *start_x) / self.ppb.max(0.0001)).max(MIN_NOTE_BEATS);
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
        }
    }

    fn on_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let drag = std::mem::replace(&mut self.drag, PianoDrag::None);
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
                let changed = updates.iter().zip(prev.iter()).any(|((_, ns, np), (_, os, op))| {
                    (ns - os).abs() > 1e-4 || np != op
                });
                if !changed {
                    return;
                }
                self.with_timeline(cx, |tl, _| tl.state.move_midi_notes(&clip_id, &updates));
            }
            PianoDrag::Resize { id, new_dur, prev_dur, .. } => {
                if (new_dur - prev_dur).abs() < 0.0001 {
                    return;
                }
                self.with_timeline(cx, |tl, _| tl.state.resize_midi_note(&clip_id, id, new_dur));
            }
            PianoDrag::Velocity { .. } => {
                // Velocity was applied live (silent). Mark dirty once now so
                // the change is saved / synced.
                self.mark_project_dirty(cx);
            }
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
                // Stop the key from bubbling to the layout shortcut dispatcher,
                // which would otherwise run `edit:delete` and remove the whole
                // MIDI clip on top of deleting the notes.
                cx.stop_propagation();
                let ids: Vec<u64> = self.selection.iter().copied().collect();
                self.with_timeline(cx, |tl, _| {
                    tl.state.delete_midi_notes(&clip_id, &ids);
                });
                self.selection.clear();
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
            "escape" => {
                cx.stop_propagation();
                self.drag = PianoDrag::None;
                self.selection.clear();
                cx.notify();
            }
            _ => {}
        }
    }

    fn quantize_selection(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let ids: Vec<u64> = self.selection.iter().copied().collect();
        let step = self.grid_res.beats();
        self.with_timeline(cx, |tl, _| {
            tl.state.quantize_midi_notes(&clip_id, &ids, step);
        });
    }

    fn delete_selection(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        if self.selection.is_empty() {
            return;
        }
        let ids: Vec<u64> = self.selection.iter().copied().collect();
        self.with_timeline(cx, |tl, _| {
            tl.state.delete_midi_notes(&clip_id, &ids);
        });
        self.selection.clear();
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
}

impl Render for PianoRoll {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clip_id = self.editing_clip_id(cx);

        // Emit the open_editor log once when the edited clip changes (PART C).
        if clip_id != self.last_editing_clip {
            if std::env::var_os("FUTUREBOARD_MIDI_DEBUG").is_some() {
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
                        "[Native MIDI] open_editor clip_id={} track_id={} notes={}",
                        cid, track_id, notes
                    );
                }
            }
            self.last_editing_clip = clip_id.clone();
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
            .on_scroll_wheel(cx.listener(Self::on_wheel))
            .child(toolbar)
            .child(body)
    }
}

impl PianoRoll {
    fn render_toolbar(&self, cx: &mut Context<Self>, clip_id: Option<&str>) -> impl IntoElement {
        let note_count = clip_id
            .and_then(|cid| self.timeline.read(cx).state.midi_clip_notes(cid).map(|n| n.len()))
            .unwrap_or(0);
        let sel_count = self.selection.len();
        let tool = self.tool;
        let snap_on = self.snap_on;
        let grid_label = self.grid_res.label();

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
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(9.0))
                    .text_color(Colors::text_muted())
                    .child(format!("{} notes · {} sel", note_count, sel_count)),
            )
    }

    fn render_body(&mut self, cx: &mut Context<Self>, clip_id: &str) -> impl IntoElement {
        // Centre on C4 once the real grid height is known (i.e. after the first
        // paint has captured the grid bounds via the canvas callback).
        if !self.centered && self.grid_bounds.get().is_some() {
            let (_, h) = self.grid_view_size();
            if h > 1.0 {
                let target = (self.pitch_to_y(60) + self.scroll_y) - h / 2.0;
                self.scroll_y = target.clamp(0.0, self.max_scroll_y());
                self.centered = true;
            }
        }

        let (view_w, view_h) = self.grid_view_size();
        let track_color = self.track_color_for_clip(cx, clip_id);
        let bpb = self.timeline.read(cx).state.beats_per_bar().max(1.0);

        // Visible ranges (only build geometry for what's on screen).
        let first_pitch = (self.y_to_pitch(view_h) as i32 - 1).max(0);
        let last_pitch = (self.y_to_pitch(0.0) as i32 + 1).min(PITCH_CNT - 1);
        let start_beat = self.x_to_beat(0.0);
        let end_beat = self.x_to_beat(view_w);

        // Piano key lane.
        let keys: Vec<_> = (first_pitch..=last_pitch)
            .map(|p| {
                let y = self.pitch_to_y(p as u8);
                let black = is_black(p);
                let is_c = p % 12 == 0;
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
                    .when(is_c, |this| {
                        this.child(
                            div()
                                .text_size(px(8.0))
                                .text_color(Colors::text_secondary())
                                .child(note_name(p)),
                        )
                    })
            })
            .collect();

        // Note + grid rendering.
        let grid_lines =
            self.build_grid_lines(start_beat, end_beat, view_w, view_h, first_pitch, last_pitch, bpb);
        let ruler = self.build_ruler(start_beat, end_beat, bpb);
        let vel_grid = self.build_velocity_grid(start_beat, end_beat, bpb);
        let notes_geo = self.build_note_elements(cx, clip_id, track_color);
        let vel_bars = self.build_velocity_bars(cx, clip_id, track_color);
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
                            .children(notes_geo)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_grid_down)),
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
                    ),
            )
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

    fn build_grid_lines(
        &self,
        start_beat: f32,
        end_beat: f32,
        view_w: f32,
        _view_h: f32,
        first_pitch: i32,
        last_pitch: i32,
        bpb: f32,
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

        // ── Vertical timing gridlines (zoom-aware hierarchy) ──
        for (x, kind) in self.visible_grid_lines(start_beat, end_beat, bpb) {
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

        // ── Horizontal octave boundary lines (top of each C row) ──
        for p in first_pitch..=last_pitch {
            if p % 12 == 0 {
                let y = self.pitch_to_y(p as u8);
                out.push(
                    div()
                        .absolute()
                        .top(px(y))
                        .left_0()
                        .w(px(view_w))
                        .h(px(1.0))
                        .bg(Colors::with_alpha(Colors::text_primary(), 0.12))
                        .into_any_element(),
                );
            }
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
    fn build_velocity_grid(&self, start_beat: f32, end_beat: f32, bpb: f32) -> Vec<gpui::AnyElement> {
        self.visible_grid_lines(start_beat, end_beat, bpb)
            .into_iter()
            .filter(|(_, kind)| *kind != GridLineKind::Subdivision)
            .map(|(x, kind)| {
                let alpha = if kind == GridLineKind::Bar { 0.20 } else { 0.10 };
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
        let geos: Vec<(u64, f32, f32, f32, bool)> = {
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
                    Some((d.id, x, y, w, self.selection.contains(&d.id)))
                })
                .collect()
        };

        geos.into_iter()
            .map(|(id, x, y, w, selected)| {
                let mut fill = track_color;
                fill.a = if selected { 1.0 } else { 0.78 };
                let border = if selected {
                    Colors::text_primary()
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
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            this.note_right_down(id, cx);
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
