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
    canvas, deferred, div, fill, point, px, size, svg, Bounds, Context, Entity, FocusHandle,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Render, ScrollWheelEvent, StatefulInteractiveElement,
    Styled, Subscription, Window,
};

use crate::assets;
use crate::components::edit::{normalize_range, EditCommand};
use crate::components::timeline::timeline::Timeline;
use crate::components::timeline::timeline_state::{
    midi_debug_enabled, MidiControllerKind, MidiControllerPoint, MidiNoteState, TimelineState,
    MIN_NOTE_BEATS,
};
use crate::theme::Colors;

// ── Layout constants (CSS px) ───────────────────────────────────────────────
mod cc_lane;
mod render;

const ROW_H: f32 = 14.0; // px per semitone
const PITCH_CNT: i32 = 128;
const TOTAL_H: f32 = PITCH_CNT as f32 * ROW_H;

/// Reversed-pitch row mapping shared by the note grid and the left piano-key
/// lane: higher pitches sit at the top, lower at the bottom, offset by the
/// vertical scroll, clamped to the valid MIDI range. This is the single source
/// of truth for "which row is which pitch" — the keys and the grid both go
/// through it (via [`PianoRoll::y_to_pitch`]) so they can never drift apart.
/// Out-of-lane detection is layered on top in [`PianoRoll::key_lane_pitch_at`].
fn local_y_to_pitch(local_y: f32, scroll_y: f32) -> u8 {
    let row = ((local_y + scroll_y) / ROW_H).floor() as i32;
    (PITCH_CNT - 1 - row).clamp(0, PITCH_CNT - 1) as u8
}

/// Piano-roll chrome theme (spec Part 7).
struct PianoRollTheme {
    key_lane_width: f32,
}

fn piano_roll_theme() -> PianoRollTheme {
    PianoRollTheme {
        key_lane_width: 72.0,
    }
}

fn key_lane_width() -> f32 {
    piano_roll_theme().key_lane_width
}
/// Height of the single unified controller lane (velocity / CC / pitch-bend /
/// pressure). Replaces the old stacked velocity + CC lanes — one lane at a time.
const LANE_H: f32 = 140.0;
/// Paint above the piano grid/body so the lane selector is never hidden by the
/// editor's scroll/clip containers.
const LANE_MENU_PRIORITY: usize = 100;
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

/// Internal clipboard format version. Bumped if [`ClipboardNote`] layout or
/// semantics change so a paste can reject data it doesn't understand instead of
/// mis-reading it. The clipboard is process-local today, but versioning keeps
/// the contract explicit for a future cross-process / serialized clipboard.
const MIDI_CLIPBOARD_VERSION: u32 = 1;

/// Versioned clipboard payload — a version tag plus the copied notes.
#[derive(Clone)]
struct ClipboardPayload {
    version: u32,
    notes: Vec<ClipboardNote>,
}

thread_local! {
    /// Process-global MIDI note clipboard. Lives outside any single editor so
    /// copy in the docked piano roll can paste in the floating one (both run on
    /// the GPUI main thread). Holds relative timing — not real notes.
    static MIDI_NOTE_CLIPBOARD: std::cell::RefCell<ClipboardPayload> = const {
        std::cell::RefCell::new(ClipboardPayload {
            version: MIDI_CLIPBOARD_VERSION,
            notes: Vec::new(),
        })
    };
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
    /// Click/drag a note to delete it.
    Erase,
    /// Click a note to split it at the cursor beat.
    Split,
    /// Click a note to toggle its muted state.
    Mute,
}

/// What the single unified controller lane currently shows and edits. Replaces
/// the old always-on stacked velocity + CC lanes — exactly one is shown at a
/// time. `Velocity` edits note-owned velocity; `Controller` edits a controller
/// automation lane (CC / pitch-bend / pressure) by [`MidiControllerKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerLaneKind {
    Velocity,
    Controller(MidiControllerKind),
}

/// Lane choices presented by the selector / cycled by the keyboard commands,
/// in display order. Custom CC numbers (not in this list) are reachable via the
/// selector's stepper and are preserved as data either way.
const LANE_CYCLE: [ControllerLaneKind; 9] = [
    ControllerLaneKind::Velocity,
    ControllerLaneKind::Controller(MidiControllerKind::CC(1)),
    ControllerLaneKind::Controller(MidiControllerKind::CC(7)),
    ControllerLaneKind::Controller(MidiControllerKind::CC(10)),
    ControllerLaneKind::Controller(MidiControllerKind::CC(11)),
    ControllerLaneKind::Controller(MidiControllerKind::CC(64)),
    ControllerLaneKind::Controller(MidiControllerKind::PitchBend),
    ControllerLaneKind::Controller(MidiControllerKind::ChannelPressure),
    ControllerLaneKind::Controller(MidiControllerKind::PolyPressure),
];

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

#[derive(Debug, Clone)]
pub enum UiMidiPreviewCommand {
    NoteOn {
        track_id: String,
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    NoteOff {
        track_id: String,
        channel: u8,
        pitch: u8,
    },
    AllNotesOff {
        track_id: String,
    },
    MidiPanic {
        track_id: String,
    },
}

impl UiMidiPreviewCommand {
    pub fn track_id(&self) -> &str {
        match self {
            Self::NoteOn { track_id, .. }
            | Self::NoteOff { track_id, .. }
            | Self::AllNotesOff { track_id }
            | Self::MidiPanic { track_id } => track_id,
        }
    }
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
    /// `grab_pitch` is the original pitch of the note under the pointer — the
    /// anchor for the live audition preview while the pitch is dragged.
    Move {
        start_x: f32,
        start_y: f32,
        prev: Vec<(u64, f32, u8)>,
        dx_beats: f32,
        dpitch: i32,
        grab_pitch: u8,
        clone_on_commit: bool,
    },
    /// Resize a single note from its right edge (also used to drag-extend a
    /// freshly drawn note). `new_dur` is the live length.
    Resize {
        id: u64,
        start_x: f32,
        prev_dur: f32,
        new_dur: f32,
    },
    /// Drag a velocity bar. When the grabbed note is part of a multi-selection,
    /// every selected note moves by the same delta. `prev` snapshots each
    /// affected note's `(id, orig_velocity)` so the live delta is reproducible
    /// and undo can restore exact values.
    Velocity {
        prev: Vec<(u64, u8)>,
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
    /// Shift+drag a straight ramp across the active CC lane. Replaces points in
    /// the spanned beat range with an evenly-spaced line from the gesture anchor
    /// to the cursor. Pre-drag points live in `cc_edit_prev`; one undo on release.
    CcLine {
        anchor_beat: f32,
        anchor_value: f32,
    },
    /// Dragging the bar/beat ruler to seek the project playhead.
    RulerSeek,
}

pub struct PianoRoll {
    timeline: Entity<Timeline>,
    /// When `true`, commit logs use `[MIDI Editor]` (floating window instance).
    pub midi_editor_sink: bool,
    /// Docked editor only: opens the floating MIDI editor window.
    on_pop_out: Option<std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + Send + Sync>>,
    on_midi_preview:
        Option<std::sync::Arc<dyn Fn(UiMidiPreviewCommand, &mut gpui::App) + Send + Sync>>,
    tool: PianoTool,
    ppb: f32,
    snap_on: bool,
    grid_res: GridRes,
    /// Quantize strength resolution, independent of the visual grid.
    quantize_res: GridRes,
    selection: HashSet<u64>,
    scroll_x: f32,
    scroll_y: f32,
    drag: PianoDrag,
    /// Selection snapshot taken when a marquee gesture begins (for modifier modes).
    selection_before_marquee: HashSet<u64>,
    /// Notes highlighted during an erase drag.
    erase_preview_ids: HashSet<u64>,
    /// When true (Quantize button hovered), the grid shows ghost outlines at the
    /// positions the affected notes would snap to.
    quantize_preview: bool,
    /// Last clip-local beat the pointer was over the note grid. Used as the
    /// anchor for paste-at-mouse (`Ctrl/Cmd+Shift+V`) and status feedback.
    hover_beat: Option<f32>,
    /// Last pitch row the pointer was over in the note grid, for compact status.
    hover_pitch: Option<u8>,
    /// Detailed note hover text: pitch, start, length, velocity.
    hover_note_status: Option<String>,
    /// Live value text shown while dragging velocity/CC values.
    drag_value_status: Option<String>,
    /// Last clip id we ran [`Self::fit_piano_roll_to_notes`] for.
    fitted_clip_id: Option<String>,
    grid_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Bounds of the bar/beat ruler header; used to seek from window-space drag.
    ruler_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The controller kind shown/edited when the unified lane is NOT in velocity
    /// mode. Also remembered while velocity is shown, so switching back restores
    /// the last controller. Switching the lane never touches hidden lane data.
    active_cc: MidiControllerKind,
    /// `true` when the unified lane shows note velocities; `false` shows the
    /// `active_cc` controller lane. Default: velocity.
    lane_shows_velocity: bool,
    /// `false` collapses the controller lane entirely (grid uses the full
    /// height). Toggled from the selector / commands.
    lane_visible: bool,
    /// Selector dropdown open state.
    lane_menu_open: bool,
    /// CC number bound to the selector's "Custom CC" stepper (0..=127).
    custom_cc: u8,
    /// Bounds of the CC strip, captured at paint for cursor → beat/value mapping.
    cc_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Lane points snapshotted when a CC paint/erase gesture begins (undo prev).
    cc_edit_prev: Option<Vec<MidiControllerPoint>>,
    /// Last clip the editor rendered — used to emit the `open_editor` debug log
    /// exactly once when the edited clip changes (not every frame).
    last_editing_clip: Option<String>,
    active_preview_note: Option<(String, u8, u8)>,
    /// Pitch currently sounding from the left key lane (the audition note); also
    /// drives the pressed-key highlight. `None` while no key is being auditioned
    /// (including mid-drag when the cursor has left the lane).
    key_lane_pressed_pitch: Option<u8>,
    /// `true` between a key mouse-down and the matching mouse-up — i.e. the user
    /// is scrubbing the piano-key lane. Kept separate from
    /// [`Self::key_lane_pressed_pitch`] so a drag that wanders off the lane (note
    /// off, no sounding pitch) still resumes auditioning when it returns.
    piano_key_drag_active: bool,
    /// Bounds of the left key-lane keys area, captured at paint so a window-space
    /// cursor can be hit-tested + mapped to a pitch with the same math the grid
    /// uses. `None` until the first frame lays the lane out.
    key_lane_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    focus: FocusHandle,
    focus_lost_subscription: Option<Subscription>,
}

impl PianoRoll {
    pub fn new(timeline: Entity<Timeline>, cx: &mut Context<Self>) -> Self {
        Self {
            timeline,
            midi_editor_sink: false,
            on_pop_out: None,
            on_midi_preview: None,
            tool: PianoTool::Draw,
            ppb: 80.0,
            snap_on: true,
            grid_res: GridRes::Sixteenth,
            quantize_res: GridRes::Sixteenth,
            selection: HashSet::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            drag: PianoDrag::None,
            selection_before_marquee: HashSet::new(),
            erase_preview_ids: HashSet::new(),
            quantize_preview: false,
            hover_beat: None,
            hover_pitch: None,
            hover_note_status: None,
            drag_value_status: None,
            fitted_clip_id: None,
            grid_bounds: Rc::new(Cell::new(None)),
            ruler_bounds: Rc::new(Cell::new(None)),
            active_cc: MidiControllerKind::CC(1),
            lane_shows_velocity: true,
            lane_visible: true,
            lane_menu_open: false,
            custom_cc: 74,
            cc_bounds: Rc::new(Cell::new(None)),
            cc_edit_prev: None,
            last_editing_clip: None,
            active_preview_note: None,
            key_lane_pressed_pitch: None,
            piano_key_drag_active: false,
            key_lane_bounds: Rc::new(Cell::new(None)),
            focus: cx.focus_handle(),
            focus_lost_subscription: None,
        }
    }

    pub fn set_pop_out_handler(
        &mut self,
        handler: Option<std::sync::Arc<dyn Fn(&mut Window, &mut gpui::App) + Send + Sync>>,
    ) {
        self.on_pop_out = handler;
    }

    pub fn set_midi_preview_handler(
        &mut self,
        handler: Option<std::sync::Arc<dyn Fn(UiMidiPreviewCommand, &mut gpui::App) + Send + Sync>>,
    ) {
        self.on_midi_preview = handler;
    }

    fn prune_transient_state(&mut self, cx: &Context<Self>, clip_id: Option<&str>) {
        let Some(clip_id) = clip_id else {
            self.selection.clear();
            self.selection_before_marquee.clear();
            self.erase_preview_ids.clear();
            self.drag = PianoDrag::None;
            self.cc_edit_prev = None;
            self.hover_beat = None;
            self.hover_pitch = None;
            self.hover_note_status = None;
            self.drag_value_status = None;
            self.fitted_clip_id = None;
            return;
        };

        let valid_note_ids: HashSet<u64> = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(clip_id)
            .map(|notes| notes.iter().map(|note| note.id).collect())
            .unwrap_or_default();

        self.selection.retain(|id| valid_note_ids.contains(id));
        self.selection_before_marquee
            .retain(|id| valid_note_ids.contains(id));
        self.erase_preview_ids
            .retain(|id| valid_note_ids.contains(id));

        let drag_is_stale = match &mut self.drag {
            PianoDrag::None
            | PianoDrag::MarqueeSelect { .. }
            | PianoDrag::DrawNote { .. }
            | PianoDrag::CcPaint { .. }
            | PianoDrag::CcMove { .. }
            | PianoDrag::CcLine { .. }
            | PianoDrag::RulerSeek => false,
            PianoDrag::Move { prev, .. } => {
                prev.retain(|(id, _, _)| valid_note_ids.contains(id));
                prev.is_empty()
            }
            PianoDrag::Resize { id, .. } => !valid_note_ids.contains(id),
            PianoDrag::Velocity { prev, .. } => {
                prev.retain(|(id, _)| valid_note_ids.contains(id));
                prev.is_empty()
            }
            PianoDrag::EraseNotes { erased, .. } => {
                erased.retain(|id| valid_note_ids.contains(id));
                false
            }
        };
        if drag_is_stale {
            self.drag = PianoDrag::None;
        }
    }

    pub fn selected_note_count(&self) -> usize {
        self.selection.len()
    }

    /// `true` when this editor's grid currently holds keyboard focus. Used by
    /// `StudioLayout` to route Ctrl+A/C/V/X/Delete to the MIDI editor (its own
    /// `on_key_down`) instead of the timeline clip commands.
    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    pub fn handle_key_event(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.on_key(event, window, cx);
    }

    pub fn grid_label(&self) -> &'static str {
        self.grid_res.label()
    }

    fn toolbar_status(&self, note_count: usize, sel_count: usize) -> String {
        let pointer = match (self.hover_pitch, self.hover_beat) {
            (Some(pitch), Some(beat)) => format!("{} @ {:.2}", note_name(pitch as i32), beat),
            _ => "Pointer: —".to_string(),
        };
        let drag = match &self.drag {
            PianoDrag::Velocity { prev, .. } => self
                .drag_value_status
                .clone()
                .unwrap_or_else(|| format!("Vel drag · {} note{}", prev.len(), plural(prev.len()))),
            PianoDrag::DrawNote {
                pitch,
                start_beat,
                end_beat,
            } => {
                let (lo, hi) = normalize_range(*start_beat, *end_beat);
                format!(
                    "Draw {} · {:.2}+{:.2}",
                    note_name(*pitch as i32),
                    lo,
                    (hi - lo).max(self.step_beats())
                )
            }
            PianoDrag::Move {
                dx_beats, dpitch, ..
            } => format!("Move Δ{:.2} beat Δ{} st", dx_beats, dpitch),
            PianoDrag::Resize { new_dur, .. } => format!("Length {:.2}", new_dur),
            PianoDrag::CcPaint { erase } => self.drag_value_status.clone().unwrap_or_else(|| {
                if *erase {
                    "CC erase".to_string()
                } else {
                    "CC draw".to_string()
                }
            }),
            PianoDrag::CcMove { .. } => self
                .drag_value_status
                .clone()
                .unwrap_or_else(|| "CC move".to_string()),
            PianoDrag::CcLine { .. } => self
                .drag_value_status
                .clone()
                .unwrap_or_else(|| "CC line".to_string()),
            PianoDrag::RulerSeek => self
                .drag_value_status
                .clone()
                .unwrap_or_else(|| "Seek".to_string()),
            PianoDrag::EraseNotes { erased, .. } => {
                format!("Erase {} note{}", erased.len(), plural(erased.len()))
            }
            PianoDrag::MarqueeSelect { mode, dragging, .. } => {
                if *dragging {
                    format!("Select · {}", mode.label())
                } else {
                    "Select".to_string()
                }
            }
            PianoDrag::None => self.hover_note_status.clone().unwrap_or(pointer),
        };
        format!("{} notes · {} sel · {}", note_count, sel_count, drag)
    }

    // ── Unified controller lane selection ────────────────────────────────
    /// What the single bottom lane currently shows/edits.
    fn current_lane(&self) -> ControllerLaneKind {
        if self.lane_shows_velocity {
            ControllerLaneKind::Velocity
        } else {
            ControllerLaneKind::Controller(self.active_cc)
        }
    }

    /// Switch which controller the unified lane shows. Only changes what is
    /// displayed/edited — hidden lane data (velocity stays on notes, CC points
    /// stay in their lanes) is never touched. Always makes the lane visible.
    fn set_lane(&mut self, kind: ControllerLaneKind, cx: &mut Context<Self>) {
        match kind {
            ControllerLaneKind::Velocity => self.lane_shows_velocity = true,
            ControllerLaneKind::Controller(k) => {
                self.lane_shows_velocity = false;
                self.active_cc = k;
            }
        }
        self.lane_visible = true;
        self.lane_menu_open = false;
        // Geometry of the active lane may differ; force a fresh bounds capture.
        self.cc_bounds.set(None);
        cx.notify();
    }

    /// Step through [`LANE_CYCLE`] (Next/Previous controller lane commands).
    fn cycle_lane(&mut self, dir: i32, cx: &mut Context<Self>) {
        let cur = self.current_lane();
        let n = LANE_CYCLE.len() as i32;
        let idx = LANE_CYCLE.iter().position(|k| *k == cur).unwrap_or(0) as i32;
        let next = (((idx + dir) % n) + n) % n;
        self.set_lane(LANE_CYCLE[next as usize], cx);
    }

    fn toggle_lane_visible(&mut self, cx: &mut Context<Self>) {
        self.lane_visible = !self.lane_visible;
        self.lane_menu_open = false;
        cx.notify();
    }

    /// Display name of the active lane (header + selector button).
    fn lane_name(&self) -> String {
        match self.current_lane() {
            ControllerLaneKind::Velocity => "Velocity".to_string(),
            ControllerLaneKind::Controller(k) => cc_kind_label(k),
        }
    }

    /// Value-range caption for the active lane header.
    fn lane_range(&self) -> &'static str {
        match self.current_lane() {
            ControllerLaneKind::Velocity => "1–127",
            ControllerLaneKind::Controller(MidiControllerKind::PitchBend) => "-8192..8191",
            ControllerLaneKind::Controller(_) => "0–127",
        }
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
            "midi:lane-next" => self.cycle_lane(1, cx),
            "midi:lane-prev" => self.cycle_lane(-1, cx),
            "midi:lane-velocity" => self.set_lane(ControllerLaneKind::Velocity, cx),
            "midi:lane-cc" => self.set_lane(ControllerLaneKind::Controller(self.active_cc), cx),
            "midi:lane-toggle" => self.toggle_lane_visible(cx),
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

    fn preview_target(&self, cx: &Context<Self>) -> Option<(String, u8)> {
        use crate::components::timeline::timeline_state::{ClipType, TrackType};
        let tl = self.timeline.read(cx);
        let channel_for = |track: &crate::components::timeline::timeline_state::TrackState| {
            track
                .routing
                .midi_channel
                .map(|ch| ch.saturating_sub(1).min(15))
                .unwrap_or(0)
        };
        if let Some(clip_id) = tl.state.selection.selected_clip_ids.first() {
            if let Some((track, clip)) = tl.state.find_clip(clip_id) {
                if matches!(clip.clip_type, ClipType::Midi { .. }) {
                    return Some((track.id.clone(), channel_for(track)));
                }
            }
        }
        if let Some(track_id) = tl.state.selection.selected_track_id.as_deref() {
            if let Some(track) = tl.state.find_track(track_id) {
                if matches!(track.track_type, TrackType::Instrument | TrackType::Midi) {
                    return Some((track.id.clone(), channel_for(track)));
                }
            }
        }
        tl.state
            .tracks
            .iter()
            .find(|track| {
                matches!(track.track_type, TrackType::Instrument | TrackType::Midi)
                    && track.instrument_insert().is_some()
            })
            .map(|track| (track.id.clone(), channel_for(track)))
    }

    fn begin_preview_note(
        &mut self,
        pitch: u8,
        velocity: u8,
        reason: &str,
        cx: &mut Context<Self>,
    ) {
        self.end_preview_note("replace", cx);
        let Some((track_id, channel)) = self.preview_target(cx) else {
            eprintln!(
                "[MidiEditor] sending PreviewNoteOn skipped pitch={} reason={} no_midi_track",
                pitch, reason
            );
            return;
        };
        eprintln!(
            "[MidiEditor] sending PreviewNoteOn track_id={} pitch={} channel={} reason={}",
            track_id, pitch, channel, reason
        );
        if let Some(handler) = self.on_midi_preview.clone() {
            handler(
                UiMidiPreviewCommand::NoteOn {
                    track_id: track_id.clone(),
                    channel,
                    pitch,
                    velocity,
                },
                cx,
            );
            self.active_preview_note = Some((track_id, channel, pitch));
        }
    }

    fn end_preview_note(&mut self, reason: &str, cx: &mut Context<Self>) {
        let Some((track_id, channel, pitch)) = self.active_preview_note.take() else {
            return;
        };
        eprintln!(
            "[MidiEditor] sending PreviewNoteOff track_id={} pitch={} channel={} reason={}",
            track_id, pitch, channel, reason
        );
        if let Some(handler) = self.on_midi_preview.clone() {
            handler(
                UiMidiPreviewCommand::NoteOff {
                    track_id,
                    channel,
                    pitch,
                },
                cx,
            );
        }
    }

    pub fn preview_all_notes_off(&mut self, reason: &str, cx: &mut Context<Self>) {
        let target = self
            .active_preview_note
            .as_ref()
            .map(|(track_id, _, _)| track_id.clone())
            .or_else(|| self.preview_target(cx).map(|(track_id, _)| track_id));
        self.active_preview_note = None;
        // Any all-notes-off (escape, focus loss, clip change, editor close,
        // destructive edit) must also end a piano-key scrub so the pressed key
        // clears and a stale drag can't keep re-triggering.
        self.key_lane_pressed_pitch = None;
        self.piano_key_drag_active = false;
        let Some(track_id) = target else {
            return;
        };
        eprintln!(
            "[MidiEditor] sending PreviewAllNotesOff track_id={} reason={}",
            track_id, reason
        );
        if let Some(handler) = self.on_midi_preview.clone() {
            handler(UiMidiPreviewCommand::AllNotesOff { track_id }, cx);
        }
    }

    pub fn midi_panic(&mut self, reason: &str, cx: &mut Context<Self>) {
        let target = self
            .active_preview_note
            .as_ref()
            .map(|(track_id, _, _)| track_id.clone())
            .or_else(|| self.preview_target(cx).map(|(track_id, _)| track_id));
        self.active_preview_note = None;
        self.key_lane_pressed_pitch = None;
        self.piano_key_drag_active = false;
        let Some(track_id) = target else {
            return;
        };
        eprintln!(
            "[MidiEditor] sending MidiPanic track_id={} reason={}",
            track_id, reason
        );
        if let Some(handler) = self.on_midi_preview.clone() {
            handler(UiMidiPreviewCommand::MidiPanic { track_id }, cx);
        }
    }

    /// Stop any sounding preview/audition note AND panic the track before a
    /// destructive edit (delete / erase / cut) removes note data. The engine's
    /// `AllNotesOff` handler resolves the track instrument and sends explicit
    /// note-offs for tracked preview notes plus CC64/CC123/CC120/CC121, so a
    /// note that was sounding when its data is destroyed cannot get stuck.
    fn cleanup_midi_before_destructive_edit(&mut self, reason: &str, cx: &mut Context<Self>) {
        self.preview_all_notes_off(reason, cx);
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
        local_y_to_pitch(local_y, self.scroll_y)
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

    fn cancel_active_gesture(&mut self, cx: &mut Context<Self>) {
        if self.piano_key_drag_active || self.key_lane_pressed_pitch.is_some() {
            eprintln!(
                "[PianoKeyPreview] cancel active={:?}",
                self.key_lane_pressed_pitch
            );
        }
        self.preview_all_notes_off("cancel", cx);
        if matches!(self.drag, PianoDrag::MarqueeSelect { .. }) {
            self.cancel_marquee_select(cx);
        } else if !matches!(self.drag, PianoDrag::None) {
            self.drag = PianoDrag::None;
            self.erase_preview_ids.clear();
            self.cc_edit_prev = None;
            self.drag_value_status = None;
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

    fn ruler_local(&self, window_pos: gpui::Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.ruler_bounds.get()?;
        let ox: f32 = bounds.origin.x.into();
        let oy: f32 = bounds.origin.y.into();
        let x: f32 = window_pos.x.into();
        let y: f32 = window_pos.y.into();
        Some((x - ox, y - oy))
    }

    fn seek_ruler_at(&mut self, lx: f32, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let (clip_start, clip_len) = self.clip_meta(cx, &clip_id);
        let rel_beat = self.x_to_beat(lx).clamp(0.0, clip_len.max(0.0));
        let project_beat = clip_start + rel_beat;
        self.timeline
            .update(cx, |tl, tcx| tl.seek_to_beat(project_beat, tcx));
        self.hover_beat = Some(rel_beat);
        self.drag_value_status = Some(format!("Seek {:.2}", project_beat));
        cx.notify();
    }

    /// Pitch under a window-space point **iff** it is inside the left piano-key
    /// lane, else `None` (the cursor has left the lane → the scrub note must
    /// stop). Uses the exact same [`Self::y_to_pitch`] mapping as the note grid,
    /// so the keys and the grid rows can never drift apart — there is no
    /// second copy of the pitch math.
    fn key_lane_pitch_at(&self, window_pos: gpui::Point<Pixels>) -> Option<u8> {
        let bounds = self.key_lane_bounds.get()?;
        let ox: f32 = bounds.origin.x.into();
        let oy: f32 = bounds.origin.y.into();
        let w: f32 = bounds.size.width.into();
        let h: f32 = bounds.size.height.into();
        let local_x = f32::from(window_pos.x) - ox;
        let local_y = f32::from(window_pos.y) - oy;
        if local_x < 0.0 || local_x > w || local_y < 0.0 || local_y > h {
            return None;
        }
        Some(self.y_to_pitch(local_y))
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
        // Any grid interaction dismisses the lane selector dropdown.
        self.lane_menu_open = false;
        window.focus(&self.focus, cx);
        let Some((lx, ly)) = self.grid_local(event.position) else {
            // Bounds not captured yet (first frame) — ignore to avoid creating
            // a note at the wrong coordinate.
            return;
        };
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };

        let marquee_modifier = event.modifiers.shift
            || event.modifiers.control
            || event.modifiers.platform
            || event.modifiers.alt;

        // A held modifier always means marquee select, whatever the active tool.
        if marquee_modifier {
            let mode = MarqueeSelectionMode::from_modifiers(&event.modifiers);
            self.begin_marquee_select(lx, ly, mode, cx);
            return;
        }

        match self.tool {
            PianoTool::Draw => {
                let pitch = self.y_to_pitch(ly);
                let start = self.snap_beats(self.x_to_beat(lx));
                if let Some((track_id, channel)) = self.preview_target(cx) {
                    eprintln!(
                        "[MidiEditor] draw_start pitch={} velocity=100 track_id={} channel={}",
                        pitch, track_id, channel
                    );
                }
                self.begin_preview_note(pitch, 100, "draw_start", cx);
                self.drag = PianoDrag::DrawNote {
                    pitch,
                    start_beat: start,
                    end_beat: start,
                };
                cx.notify();
            }
            PianoTool::Select => {
                self.begin_marquee_select(lx, ly, MarqueeSelectionMode::Replace, cx);
            }
            PianoTool::Erase => {
                // Begin an erase drag from empty space (sweeps notes like the
                // right-drag erase).
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
            // Split/Mute act on notes only — empty-grid clicks do nothing.
            PianoTool::Split | PianoTool::Mute => {}
        }
    }

    fn on_grid_right_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.focus, cx);
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
        window.focus(&self.focus, cx);
        // Tool-specific note actions take precedence over select/move.
        match self.tool {
            PianoTool::Erase => {
                self.erase_note(id, cx);
                return;
            }
            PianoTool::Mute => {
                self.mute_note(id, cx);
                return;
            }
            PianoTool::Split => {
                if let Some((lx, _)) = self.grid_local(event.position) {
                    let beat = self.x_to_beat(lx);
                    self.split_note(id, beat, cx);
                }
                return;
            }
            PianoTool::Draw | PianoTool::Select => {}
        }
        let shift = event.modifiers.shift;
        let ctrl = event.modifiers.control || event.modifiers.platform;
        let clone_on_commit = event.modifiers.alt;
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
        // Anchor pitch/velocity of the grabbed note for the live audition.
        let (grab_pitch, grab_vel) = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .and_then(|notes| notes.iter().find(|n| n.id == id))
            .map(|n| (n.pitch, n.velocity))
            .unwrap_or((60, 100));
        let prev = self.snapshot_selection(cx, &clip_id);
        self.drag = PianoDrag::Move {
            start_x: event.position.x.into(),
            start_y: event.position.y.into(),
            prev,
            dx_beats: 0.0,
            dpitch: 0,
            grab_pitch,
            clone_on_commit,
        };
        // Audition the grabbed pitch immediately; on_move switches it as the
        // drag changes pitch, on_up / cancel stops it.
        self.begin_preview_note(grab_pitch, grab_vel, "note_move_start", cx);
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
        window.focus(&self.focus, cx);
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

    /// Velocity bar mouse-down: begin a velocity drag. Grabbing a bar that is
    /// already part of a multi-selection drags every selected note's velocity by
    /// the same delta; otherwise it selects just that note and drags it alone.
    fn begin_velocity_drag(
        &mut self,
        id: u64,
        orig_vel: u8,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let multi = self.selection.len() > 1 && self.selection.contains(&id);
        let prev: Vec<(u64, u8)> = if multi {
            let Some(clip_id) = self.editing_clip_id(cx) else {
                return;
            };
            let sel = &self.selection;
            self.timeline
                .read(cx)
                .state
                .midi_clip_notes(&clip_id)
                .map(|notes| {
                    notes
                        .iter()
                        .filter(|n| sel.contains(&n.id))
                        .map(|n| (n.id, n.velocity))
                        .collect()
                })
                .unwrap_or_else(|| vec![(id, orig_vel)])
        } else {
            self.selection = HashSet::from([id]);
            vec![(id, orig_vel)]
        };
        let value = self.velocity_from_window_y(event.position);
        self.apply_velocity_value(&prev, value, cx);
        self.drag = PianoDrag::Velocity { prev };
        cx.notify();
    }

    fn velocity_from_window_y(&self, position: gpui::Point<Pixels>) -> u8 {
        let local_y = self.cc_local(position).map(|(_, y)| y).unwrap_or_else(|| {
            self.cc_bounds
                .get()
                .map(|bounds| {
                    let origin_y: f32 = bounds.origin.y.into();
                    let y: f32 = position.y.into();
                    y - origin_y
                })
                .unwrap_or(LANE_H * 0.5)
        });
        let (_, lane_h) = self.cc_view_size();
        let usable_h = (lane_h - 8.0).max(1.0);
        let norm = (1.0 - ((local_y - 2.0) / usable_h)).clamp(0.0, 1.0);
        (1.0 + norm * 126.0).round().clamp(1.0, 127.0) as u8
    }

    fn apply_velocity_value(&mut self, prev: &[(u64, u8)], value: u8, cx: &mut Context<Self>) {
        if prev.is_empty() {
            return;
        }
        self.drag_value_status = Some(if prev.len() == 1 {
            format!("Velocity: {value}")
        } else {
            format!("Velocity: {value} · {} notes", prev.len())
        });
        if let Some(clip_id) = self.editing_clip_id(cx) {
            self.with_timeline_silent(cx, |tl, _| {
                for (id, _) in prev {
                    tl.state.set_midi_note_velocity(&clip_id, *id, value);
                }
            });
        }
    }

    fn velocity_note_at_x(&self, cx: &Context<Self>, clip_id: &str, lx: f32) -> Option<(u64, u8)> {
        let tl = self.timeline.read(cx);
        let notes = tl.state.midi_clip_notes(clip_id)?;
        notes
            .iter()
            .filter_map(|note| {
                let d = self.display_note(note);
                let x = self.beat_to_x(d.start);
                let distance = if lx < x {
                    x - lx
                } else if lx > x + 8.0 {
                    lx - (x + 8.0)
                } else {
                    0.0
                };
                (distance <= 14.0).then_some((distance, d.id, d.velocity))
            })
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(_, id, velocity)| (id, velocity))
    }

    fn begin_velocity_lane_click(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let Some((lx, _)) = self.cc_local(event.position) else {
            return;
        };
        let Some((id, velocity)) = self.velocity_note_at_x(cx, &clip_id, lx) else {
            return;
        };
        self.begin_velocity_drag(id, velocity, event, window, cx);
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
        // Piano-key lane drag-scrub: audition whichever key is under the cursor.
        // Driven from this root-level move handler (not a per-key one) so fast
        // vertical drags keep tracking even as the cursor crosses key edges. It
        // is mutually exclusive with grid drags — it only starts from a key
        // mouse-down — so handle it first and return.
        if self.piano_key_drag_active {
            match self.key_lane_pitch_at(event.position) {
                Some(pitch) => {
                    // Debounce: only (re)trigger when the pitch actually changes.
                    if self.key_lane_pressed_pitch != Some(pitch) {
                        eprintln!(
                            "[PianoKeyPreview] move old={:?} new={}",
                            self.key_lane_pressed_pitch, pitch
                        );
                        // `begin_preview_note` sends note-off for the previous
                        // pitch before note-on for the new one (no stuck notes).
                        self.begin_preview_note(pitch, 100, "piano_key_drag", cx);
                        self.key_lane_pressed_pitch = Some(pitch);
                        cx.notify();
                    }
                }
                None => {
                    // Cursor left the lane: stop the current note but keep the
                    // drag active so returning to the lane resumes auditioning.
                    if self.key_lane_pressed_pitch.take().is_some() {
                        eprintln!("[PianoKeyPreview] off note=outside_lane");
                        self.end_preview_note("piano_key_drag_out", cx);
                        cx.notify();
                    }
                }
            }
            return;
        }
        if matches!(self.drag, PianoDrag::RulerSeek) {
            if let Some((lx, _)) = self.ruler_local(event.position) {
                self.seek_ruler_at(lx, cx);
            }
            return;
        }
        // Track the grid beat under the pointer so paste-at-mouse has an anchor.
        // Cheap field write, no repaint.
        if let Some((lx, ly)) = self.grid_local(event.position) {
            self.hover_beat = Some(self.x_to_beat(lx));
            self.hover_pitch = Some(self.y_to_pitch(ly));
        }
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
            PianoDrag::CcLine {
                anchor_beat,
                anchor_value,
            } => {
                if let Some((lx, ly)) = self.cc_local(event.position) {
                    self.cc_line_to(anchor_beat, anchor_value, lx, ly, cx);
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
        if matches!(self.drag, PianoDrag::Move { .. }) {
            let cur_x: f32 = event.position.x.into();
            let cur_y: f32 = event.position.y.into();
            let ppb = self.ppb.max(0.0001);
            let mut audition_pitch: Option<u8> = None;
            if let PianoDrag::Move {
                start_x,
                start_y,
                dx_beats,
                dpitch,
                grab_pitch,
                ..
            } = &mut self.drag
            {
                // Store the raw beat delta; snapping is applied per-note against
                // each note's absolute start in `display_note` / commit.
                *dx_beats = (cur_x - *start_x) / ppb;
                *dpitch = -(((cur_y - *start_y) / ROW_H).round() as i32);
                audition_pitch = Some((*grab_pitch as i32 + *dpitch).clamp(0, 127) as u8);
            }
            // Switch the live audition note when the dragged pitch changes; a
            // horizontal-only (timing) move never retriggers.
            if let Some(pitch) = audition_pitch {
                let changed = self
                    .active_preview_note
                    .as_ref()
                    .map(|(_, _, p)| *p != pitch)
                    .unwrap_or(true);
                if changed {
                    self.begin_preview_note(pitch, 100, "note_move_pitch", cx);
                }
            }
            cx.notify();
            return;
        }
        match &mut self.drag {
            PianoDrag::None => {}
            PianoDrag::Move { .. } => {}
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
            PianoDrag::Velocity { prev } => {
                let prev = prev.clone();
                let value = self.velocity_from_window_y(event.position);
                self.apply_velocity_value(&prev, value, cx);
            }
            PianoDrag::MarqueeSelect { .. } => {}
            PianoDrag::DrawNote { .. } | PianoDrag::EraseNotes { .. } => {}
            PianoDrag::CcPaint { .. } | PianoDrag::CcMove { .. } | PianoDrag::CcLine { .. } => {}
            PianoDrag::RulerSeek => {}
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
        self.cleanup_midi_before_destructive_edit("note_erase_sweep", cx);
        self.run_edit_command(EditCommand::DeleteMidiNotes { clip_id, notes }, cx);
        self.selection.retain(|id| !erased.contains(id));
        cx.notify();
    }

    fn on_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.end_preview_note("mouse_up", cx);
        // End a piano-key scrub. Fires here for release anywhere — including
        // outside the lane/window — because this is wired to both `on_mouse_up`
        // and `on_mouse_up_out` on the root. A key-lane drag never edits clip
        // notes, so just clear its state (the note-off was sent above).
        if self.piano_key_drag_active || self.key_lane_pressed_pitch.is_some() {
            eprintln!(
                "[PianoKeyPreview] off note={:?} reason=mouse_up",
                self.key_lane_pressed_pitch
            );
            self.piano_key_drag_active = false;
            self.key_lane_pressed_pitch = None;
            cx.notify();
            return;
        }
        if matches!(
            self.drag,
            PianoDrag::CcPaint { .. } | PianoDrag::CcMove { .. } | PianoDrag::CcLine { .. }
        ) {
            self.drag = PianoDrag::None;
            self.drag_value_status = None;
            self.commit_cc_edit(cx);
            return;
        }
        if matches!(self.drag, PianoDrag::RulerSeek) {
            self.drag = PianoDrag::None;
            self.drag_value_status = None;
            cx.notify();
            return;
        }
        if matches!(self.drag, PianoDrag::MarqueeSelect { .. }) {
            self.commit_marquee_select(cx);
            return;
        }
        let drag = std::mem::replace(&mut self.drag, PianoDrag::None);
        self.drag_value_status = None;
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
                clone_on_commit,
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
                if clone_on_commit {
                    let source_notes: std::collections::HashMap<u64, MidiNoteState> = self
                        .timeline
                        .read(cx)
                        .state
                        .midi_clip_notes(&clip_id)
                        .map(|notes| notes.iter().map(|n| (n.id, n.clone())).collect())
                        .unwrap_or_default();
                    let notes: Vec<MidiNoteState> = updates
                        .iter()
                        .filter_map(|(id, start, pitch)| {
                            let source = source_notes.get(id)?;
                            let mut note = MidiNoteState::new(
                                *pitch,
                                *start,
                                source.duration,
                                source.velocity,
                            );
                            note.muted = source.muted;
                            Some(note)
                        })
                        .collect();
                    if notes.is_empty() {
                        return;
                    }
                    let new_ids: Vec<u64> = notes.iter().map(|note| note.id).collect();
                    self.run_edit_command(EditCommand::CreateMidiNotes { clip_id, notes }, cx);
                    self.selection = new_ids.into_iter().collect();
                    cx.notify();
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
            PianoDrag::Velocity { prev: orig, .. } => {
                // Velocity was applied live (silent). Reconstruct the pre-drag
                // state from the per-note original velocities and record one
                // undoable edit covering every affected note.
                let ids: Vec<u64> = orig.iter().map(|(id, _)| *id).collect();
                let next = self.snapshot_notes(cx, &clip_id, &ids);
                let prev: Vec<MidiNoteState> = next
                    .iter()
                    .map(|n| {
                        let mut p = n.clone();
                        if let Some((_, v)) = orig.iter().find(|(id, _)| *id == n.id) {
                            p.velocity = *v;
                        }
                        p
                    })
                    .collect();
                self.push_note_edit(cx, clip_id, prev, next);
            }
            PianoDrag::MarqueeSelect { .. } => {}
            PianoDrag::DrawNote { .. } | PianoDrag::EraseNotes { .. } => {}
            PianoDrag::CcPaint { .. } | PianoDrag::CcMove { .. } | PianoDrag::CcLine { .. } => {}
            PianoDrag::RulerSeek => {}
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let key = event.keystroke.key.as_str();
        let ctrl = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        let shift = event.keystroke.modifiers.shift;
        match key {
            "up" if !ctrl && !self.selection.is_empty() => {
                cx.stop_propagation();
                self.transpose_selection(1, cx);
            }
            "down" if !ctrl && !self.selection.is_empty() => {
                cx.stop_propagation();
                self.transpose_selection(-1, cx);
            }
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
            "x" if ctrl => {
                // Cut = copy then delete the selection. `delete_selection` records
                // one undoable edit, so the cut is a single undo step.
                cx.stop_propagation();
                if !self.selection.is_empty() {
                    self.copy_selection(cx);
                    self.delete_selection(cx);
                }
            }
            "v" if ctrl && shift => {
                cx.stop_propagation();
                self.paste_clipboard_at_mouse(cx);
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
                if !matches!(self.drag, PianoDrag::None) {
                    self.cancel_active_gesture(cx);
                } else {
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
        let step = self.quantize_res.beats();
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
        // Stop/panic any sounding preview before the note data disappears so a
        // held audition (e.g. delete pressed mid-move) cannot get stuck.
        self.cleanup_midi_before_destructive_edit("note_delete", cx);
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

    /// Erase tool: delete a single note by id, undoable.
    fn erase_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let note = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .and_then(|ns| ns.iter().find(|n| n.id == id).cloned());
        let Some(note) = note else {
            return;
        };
        self.cleanup_midi_before_destructive_edit("note_erase", cx);
        self.run_edit_command(
            EditCommand::DeleteMidiNotes {
                clip_id,
                notes: vec![note],
            },
            cx,
        );
        self.selection.remove(&id);
        cx.notify();
    }

    /// Mute tool: toggle a single note's muted state, undoable.
    fn mute_note(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let was = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .and_then(|ns| ns.iter().find(|n| n.id == id).map(|n| n.muted));
        let Some(was) = was else {
            return;
        };
        self.run_edit_command(
            EditCommand::SetMidiNotesMuted {
                clip_id,
                prev: vec![(id, was)],
                muted: !was,
            },
            cx,
        );
        cx.notify();
    }

    /// Split tool: cut a note at `beat` (clip-local) into two contiguous notes.
    /// Snaps the cut when snap is on; refuses cuts that would leave a part
    /// shorter than [`MIN_NOTE_BEATS`]. Selects the two resulting parts.
    fn split_note(&mut self, id: u64, beat: f32, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let original = self
            .timeline
            .read(cx)
            .state
            .midi_clip_notes(&clip_id)
            .and_then(|ns| ns.iter().find(|n| n.id == id).cloned());
        let Some(original) = original else {
            return;
        };
        let cut = if self.snap_on {
            self.snap_beats(beat)
        } else {
            beat
        };
        let left_len = cut - original.start;
        let right_len = (original.start + original.duration) - cut;
        if left_len < MIN_NOTE_BEATS || right_len < MIN_NOTE_BEATS {
            return;
        }
        let mut left =
            MidiNoteState::new(original.pitch, original.start, left_len, original.velocity);
        left.muted = original.muted;
        let mut right = MidiNoteState::new(original.pitch, cut, right_len, original.velocity);
        right.muted = original.muted;
        let new_ids = [left.id, right.id];
        self.run_edit_command(
            EditCommand::SplitMidiNote {
                clip_id,
                original,
                parts: vec![left, right],
            },
            cx,
        );
        self.selection = new_ids.into_iter().collect();
        cx.notify();
    }

    /// Transpose the selected notes by `delta` semitones (clamped to 0..=127),
    /// recorded as one undoable edit. No-op on empty selection.
    fn transpose_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        if delta == 0 || self.selection.is_empty() {
            return;
        }
        let ids: Vec<u64> = self.selection.iter().copied().collect();
        let target = ids.clone();
        self.commit_note_transform(cx, &ids, move |state, cid| {
            state.transpose_midi_notes(cid, &target, delta);
        });
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
        MIDI_NOTE_CLIPBOARD.with(|cb| {
            *cb.borrow_mut() = ClipboardPayload {
                version: MIDI_CLIPBOARD_VERSION,
                notes: clip,
            };
        });
        if midi_debug_enabled() {
            eprintln!("[midi] copy notes={}", notes.len());
        }
    }

    /// Build notes from the clipboard anchored so the earliest note lands at
    /// `anchor_beat` (clip-local). Returns an empty vec when the clipboard is
    /// empty. New notes get fresh transient ids.
    fn clipboard_notes_at(&self, anchor_beat: f32) -> Vec<MidiNoteState> {
        MIDI_NOTE_CLIPBOARD.with(|cb| {
            let payload = cb.borrow();
            // Reject data this build doesn't understand rather than mis-reading it.
            if payload.version != MIDI_CLIPBOARD_VERSION {
                return Vec::new();
            }
            payload
                .notes
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

    /// Clip-local paste anchor at the playhead, falling back to clip beat 0 when
    /// the playhead sits outside the clip.
    fn playhead_paste_anchor(&self, cx: &Context<Self>, clip_id: &str) -> f32 {
        let (clip_start, _clip_len) = self.clip_meta(cx, clip_id);
        let playhead = self.timeline.read(cx).state.transport.playhead_beats;
        self.snap_beats((playhead - clip_start).max(0.0))
    }

    /// Paste the clipboard at the playhead. The pasted notes become the new
    /// selection.
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let anchor = self.playhead_paste_anchor(cx, &clip_id);
        self.paste_clipboard_anchored(clip_id, anchor, cx);
    }

    /// Paste the clipboard at the last grid beat the pointer was over, falling
    /// back to the playhead anchor when the pointer hasn't been over the grid.
    fn paste_clipboard_at_mouse(&mut self, cx: &mut Context<Self>) {
        let Some(clip_id) = self.editing_clip_id(cx) else {
            return;
        };
        let anchor = match self.hover_beat {
            Some(beat) => self.snap_beats(beat.max(0.0)),
            None => self.playhead_paste_anchor(cx, &clip_id),
        };
        self.paste_clipboard_anchored(clip_id, anchor, cx);
    }

    /// Shared paste implementation: build clipboard notes at `anchor`, insert as
    /// one undoable command, and select the new notes.
    fn paste_clipboard_anchored(&mut self, clip_id: String, anchor: f32, cx: &mut Context<Self>) {
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

    /// Scale horizontal zoom by `factor`, clamped to the same range as wheel
    /// zoom. Used by the toolbar zoom buttons.
    fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.ppb = (self.ppb * factor).clamp(20.0, 400.0);
        cx.notify();
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




// ── CC controller lane ───────────────────────────────────────────────────────
fn cc_kind_label(kind: MidiControllerKind) -> String {
    match kind {
        MidiControllerKind::CC(1) => "CC1 Mod".to_string(),
        MidiControllerKind::CC(7) => "CC7 Volume".to_string(),
        MidiControllerKind::CC(10) => "CC10 Pan".to_string(),
        MidiControllerKind::CC(11) => "CC11 Expr".to_string(),
        MidiControllerKind::CC(64) => "CC64 Sustain".to_string(),
        MidiControllerKind::CC(n) => format!("CC{n}"),
        MidiControllerKind::PitchBend => "Pitch Bend".to_string(),
        MidiControllerKind::ChannelPressure => "Ch Pressure".to_string(),
        MidiControllerKind::PolyPressure => "Poly Pressure".to_string(),
    }
}


// ── Small toolbar helpers ───────────────────────────────────────────────────
fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn note_count_for_clip(
    cx: &Context<PianoRoll>,
    timeline: &Entity<Timeline>,
    clip_id: &str,
) -> usize {
    timeline
        .read(cx)
        .state
        .midi_clip_notes(clip_id)
        .map(|notes| notes.len())
        .unwrap_or(0)
}

fn controller_display_value(kind: MidiControllerKind, value: f32) -> String {
    match kind {
        MidiControllerKind::PitchBend => {
            let semis = (value.clamp(0.0, 1.0) * 2.0 - 1.0) * 2.0;
            format!("{semis:+.2} st")
        }
        _ => format!("{}", (value.clamp(0.0, 1.0) * 127.0).round() as i32),
    }
}

fn controller_default_value(kind: MidiControllerKind) -> f32 {
    match kind {
        MidiControllerKind::PitchBend => 0.5,
        MidiControllerKind::CC(_)
        | MidiControllerKind::ChannelPressure
        | MidiControllerKind::PolyPressure => 0.0,
    }
}

fn evaluate_controller_points(
    points: &[MidiControllerPoint],
    beat: f32,
    default_value: f32,
) -> f32 {
    if points.is_empty() {
        return default_value.clamp(0.0, 1.0);
    }
    let beat = beat.max(0.0);
    if beat <= points[0].beat {
        return points[0].value;
    }
    let last = points.len() - 1;
    if beat >= points[last].beat {
        return points[last].value;
    }
    for pair in points.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        if beat >= a.beat && beat <= b.beat {
            let span = (b.beat - a.beat).max(1.0e-6);
            let t = ((beat - a.beat) / span).clamp(0.0, 1.0);
            return (a.value + (b.value - a.value) * t).clamp(0.0, 1.0);
        }
    }
    default_value.clamp(0.0, 1.0)
}

fn value_chip(label: &str, left: f32, top: f32) -> impl IntoElement {
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .px(px(6.0))
        .py(px(2.0))
        .rounded_md()
        .bg(Colors::surface_card())
        .border(px(1.0))
        .border_color(Colors::border_subtle())
        .text_size(px(9.0))
        .text_color(Colors::text_primary())
        .child(label.to_string())
}

fn toolbar_group(label: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .px(px(3.0))
        .py(px(2.0))
        .rounded_md()
        .bg(Colors::surface_panel_alt())
        .border(px(1.0))
        .border_color(Colors::divider())
        .child(
            div()
                .px(px(3.0))
                .text_size(px(8.0))
                .text_color(Colors::text_faint())
                .child(label),
        )
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

// ── WGPU render snapshot shape (scaffold) ────────────────────────────────────
// Immutable, flat description of everything the dense viewport needs to draw,
// already culled to the visible range and resolved to pixel coordinates. Built
// on the UI thread; intended for a future WGPU renderer to consume instead of
// thousands of GPUI elements. Not yet wired into paint — the element path above
// remains the live renderer, so this only fixes the data contract.

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct NoteRenderItem {
    pub id: u64,
    pub pitch: u8,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub selected: bool,
    pub muted: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct VelocityRenderItem {
    pub id: u64,
    pub x: f32,
    pub velocity: u8,
    pub selected: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct ControllerPointRenderItem {
    pub id: u64,
    pub x: f32,
    pub value: f32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct MidiEditorRenderSnapshot {
    pub clip_id: String,
    pub visible_beat_range: (f32, f32),
    pub visible_pitch_range: (u8, u8),
    pub notes: Vec<NoteRenderItem>,
    pub velocity: Vec<VelocityRenderItem>,
    pub controller_points: Vec<ControllerPointRenderItem>,
}

impl PianoRoll {
    /// Build the immutable render snapshot for the dense WGPU viewport path.
    /// Mirrors the visible-range culling used by the GPUI element builders so a
    /// future renderer produces identical geometry. Read-only; not yet consumed.
    #[allow(dead_code)]
    pub fn build_render_snapshot(
        &self,
        cx: &Context<Self>,
        clip_id: &str,
    ) -> MidiEditorRenderSnapshot {
        let (view_w, view_h) = self.grid_view_size();
        let first_pitch = (self.y_to_pitch(view_h) as i32 - 1).max(0) as u8;
        let last_pitch = (self.y_to_pitch(0.0) as i32 + 1).min(PITCH_CNT - 1) as u8;
        let start_beat = self.x_to_beat(0.0);
        let end_beat = self.x_to_beat(view_w);

        let tl = self.timeline.read(cx);
        let mut notes = Vec::new();
        let mut velocity = Vec::new();
        if let Some(ns) = tl.state.midi_clip_notes(clip_id) {
            for n in ns {
                let d = self.display_note(n);
                let x = self.beat_to_x(d.start);
                let w = (d.duration * self.ppb).max(3.0);
                let y = self.pitch_to_y(d.pitch);
                if x + w < 0.0 || x > view_w || y + ROW_H < 0.0 || y > view_h {
                    continue;
                }
                let selected = self.selection.contains(&d.id);
                notes.push(NoteRenderItem {
                    id: d.id,
                    pitch: d.pitch,
                    x,
                    y,
                    w,
                    selected,
                    muted: n.muted,
                });
                if x >= -8.0 && x <= view_w {
                    velocity.push(VelocityRenderItem {
                        id: d.id,
                        x,
                        velocity: d.velocity,
                        selected,
                    });
                }
            }
        }

        let mut controller_points = Vec::new();
        if let Some(ps) = tl.state.controller_lane_points(clip_id, self.active_cc) {
            for p in ps {
                let x = self.beat_to_x(p.beat);
                if x < -6.0 || x > view_w + 6.0 {
                    continue;
                }
                controller_points.push(ControllerPointRenderItem {
                    id: p.id,
                    x,
                    value: p.value,
                });
            }
        }

        MidiEditorRenderSnapshot {
            clip_id: clip_id.to_string(),
            visible_beat_range: (start_beat, end_beat),
            visible_pitch_range: (first_pitch, last_pitch),
            notes,
            velocity,
            controller_points,
        }
    }
}

#[cfg(test)]
mod piano_key_pitch_tests {
    use super::*;

    const MAX_PITCH: u8 = (PITCH_CNT - 1) as u8;

    /// Top of the lane is the highest pitch (reversed pitch order).
    #[test]
    fn top_row_is_highest_pitch() {
        assert_eq!(local_y_to_pitch(0.0, 0.0), MAX_PITCH);
    }

    /// Each row down drops exactly one semitone.
    #[test]
    fn moving_down_one_row_lowers_pitch_by_one() {
        assert_eq!(local_y_to_pitch(ROW_H, 0.0), MAX_PITCH - 1);
        assert_eq!(local_y_to_pitch(ROW_H * 2.0, 0.0), MAX_PITCH - 2);
    }

    /// Scrolling shifts which pitch is at the lane top, consistently (spec test
    /// 7: scroll + drag keeps the mapping correct).
    #[test]
    fn scroll_offsets_mapping_by_whole_rows() {
        let scroll = ROW_H * 3.0;
        assert_eq!(local_y_to_pitch(0.0, scroll), MAX_PITCH - 3);
        // ...and a click one row further down with the same scroll is one lower.
        assert_eq!(local_y_to_pitch(ROW_H, scroll), MAX_PITCH - 4);
    }

    /// The key lane mapping is the exact inverse of `pitch_to_y` (the layout
    /// used to position the keys) for the middle of each key's row — so the key
    /// under the cursor always matches the pitch that gets auditioned.
    #[test]
    fn round_trips_with_key_layout() {
        let scroll = ROW_H * 2.5;
        for pitch in [0u8, 24, 60, 100, MAX_PITCH] {
            // `pitch_to_y`: (PITCH_CNT - 1 - pitch) * ROW_H - scroll.
            let row_top = (PITCH_CNT - 1 - pitch as i32) as f32 * ROW_H - scroll;
            let row_mid = row_top + ROW_H * 0.5;
            assert_eq!(local_y_to_pitch(row_mid, scroll), pitch, "pitch {pitch}");
        }
    }

    /// Out-of-range rows clamp to the MIDI bounds (never panic / wrap).
    #[test]
    fn clamps_beyond_the_range() {
        assert_eq!(local_y_to_pitch(1.0e6, 0.0), 0);
        assert_eq!(local_y_to_pitch(-1.0e6, 0.0), MAX_PITCH);
    }
}
