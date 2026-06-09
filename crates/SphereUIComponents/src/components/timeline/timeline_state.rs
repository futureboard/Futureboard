/// `FUTUREBOARD_PLUGIN_DEBUG=1` enables eprintln traces for insert
/// mutations. Cached on first read.
fn plugin_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_PLUGIN_DEBUG").is_some())
}

/// `FUTUREBOARD_ROUTING_DEBUG=1` enables eprintln traces for send/routing
/// mutations (mirrors the DAUx-side flag). Cached on first read.
fn routing_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_ROUTING_DEBUG").is_some())
}

pub use crate::project::InputMonitorMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackType {
    Audio,
    Midi,
    Instrument,
    /// Sub-mix bus — other tracks route their output here for grouped
    /// processing before the master. Phase 3.
    Bus,
    /// FX return — receives sends from other tracks (aux/reverb returns).
    /// Phase 3.
    Return,
    Master,
}

impl TrackType {
    /// `true` for routing tracks (Bus/Return) that receive audio from other
    /// tracks rather than hosting clips directly.
    pub fn is_routing(self) -> bool {
        matches!(self, TrackType::Bus | TrackType::Return)
    }
}

/// `FUTUREBOARD_MIDI_DEBUG=1` enables eprintln traces for MIDI clip/note
/// mutations (mirrors the plugin/routing debug flags). Cached on first read.
pub fn midi_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var_os("FUTUREBOARD_FORENSIC_TRACE").is_some()
            || std::env::var_os("FUTUREBOARD_MIDI_DEBUG").is_some()
    })
}

/// `FUTUREBOARD_AUTOMATION_DEBUG=1` enables eprintln traces for automation
/// mode/target/point mutations and evaluation. Cached on first read.
pub fn automation_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_AUTOMATION_DEBUG").is_some())
}

/// `FUTUREBOARD_AUTOMATION_SYNC_DEBUG=1` enables `[automation-sync]` traces that
/// follow Track Volume automation through the base/effective model: which beat
/// was evaluated, the resolved value, and the before/after effective volume with
/// the edit reason (playback_tick / seek / point_edit / fader_drag). Cached on
/// first read. Separate from `FUTUREBOARD_AUTOMATION_DEBUG` so the high-volume
/// sync trace can be enabled on its own.
pub fn automation_sync_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_AUTOMATION_SYNC_DEBUG").is_some())
}

/// Origin of a track volume change, so the base/effective model can route the
/// write correctly and never let an automation-follow display update masquerade
/// as a user fader edit (which would fight automation / spam dirty).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeUpdateSource {
    /// User dragged the mixer/track-header/inspector fader — edits base only.
    UserFader,
    /// Automation read at the playhead — edits effective only.
    AutomationRead,
    /// Project load / programmatic reset — sets base and effective together.
    ProjectLoad,
}

/// Monotonic source of transient automation-point identities. Like MIDI note
/// ids these are NOT persisted — they only let the lane editor track selection
/// and in-flight drag targets across edits. Fresh ids are minted on create and
/// on project load.
fn next_automation_point_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Two automation points within this many beats are treated as the same slot
/// (a second add at the same beat replaces the existing point's value).
pub const AUTOMATION_BEAT_EPSILON: f32 = 1.0e-3;

/// Vertical padding (px) kept at the top/bottom of an automation lane so the
/// extreme 0.0/1.0 values never sit exactly on the lane border.
pub const AUTOMATION_LANE_PAD: f32 = 8.0;

/// Smallest allowed note length, in beats (1/32 note). Mirrors the WebUI
/// `MIN_DUR` guard so a note can never collapse to zero width.
pub const MIN_NOTE_BEATS: f32 = 1.0 / 32.0;

/// Default length for a newly created MIDI clip (one 4/4 bar at any BPM).
pub const DEFAULT_MIDI_CLIP_BEATS: f32 = 4.0;

/// Minimum visible MIDI clip length after edits (one bar).
pub const MIN_MIDI_CLIP_BEATS: f32 = 4.0;

#[inline]
fn snap_up_beats(value: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return value;
    }
    ((value / step).ceil() * step).max(step)
}

/// Monotonic source of transient note identities. Note ids are NOT persisted —
/// they exist only so the piano-roll editor can track selection / drag targets
/// across edits. Fresh ids are minted on create and on project load.
fn next_midi_note_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq)]
pub struct MidiNoteState {
    /// Transient identity (not serialized). Used by the piano-roll editor to
    /// track selection and in-flight drag targets.
    pub id: u64,
    pub pitch: u8,
    pub start: f32,    // beats relative to clip start
    pub duration: f32, // beats
    /// MIDI velocity in 1..=127.
    pub velocity: u8,
    /// Muted notes remain in clip data but emit no runtime note event.
    pub muted: bool,
}

impl MidiNoteState {
    /// Construct a note with a freshly minted transient id. `pitch` is clamped
    /// to 0..=127, `velocity` to 1..=127, and `duration` to at least
    /// [`MIN_NOTE_BEATS`]. The note is created unmuted.
    pub fn new(pitch: u8, start: f32, duration: f32, velocity: u8) -> Self {
        Self {
            id: next_midi_note_id(),
            pitch: pitch.min(127),
            start: start.max(0.0),
            duration: duration.max(MIN_NOTE_BEATS),
            velocity: velocity.clamp(1, 127),
            muted: false,
        }
    }
}

/// Source of transient identities for controller points (not serialized;
/// minted fresh on create and on project load, like [`next_midi_note_id`]).
fn next_controller_point_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Which MIDI controller stream a lane edits. CC carries its 0..=127 number;
/// the others are single global streams per channel. `PolyPressure` is modeled
/// for completeness but deferred — it needs per-note association the editor does
/// not yet provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiControllerKind {
    CC(u8),
    PitchBend,
    ChannelPressure,
    PolyPressure,
}

/// A single point in a controller lane. `value` is normalized `0.0..=1.0` in
/// state; the UI maps it to the controller's display range (e.g. 0..127 for CC).
#[derive(Debug, Clone, PartialEq)]
pub struct MidiControllerPoint {
    /// Transient identity (not serialized) for editor selection / drag targets.
    pub id: u64,
    /// Beats relative to the clip start.
    pub beat: f32,
    /// Normalized `0.0..=1.0`.
    pub value: f32,
}

impl MidiControllerPoint {
    /// Construct a point with a freshly minted transient id. `beat` clamps to
    /// `>= 0`, `value` to `0.0..=1.0`.
    pub fn new(beat: f32, value: f32) -> Self {
        Self {
            id: next_controller_point_id(),
            beat: beat.max(0.0),
            value: value.clamp(0.0, 1.0),
        }
    }
}

/// One controller lane inside a MIDI clip. Points travel with the clip in
/// clip-local beats. (Lane create / edit helpers land with the lane editor UI.)
#[derive(Debug, Clone, PartialEq)]
pub struct MidiControllerLane {
    pub kind: MidiControllerKind,
    pub points: Vec<MidiControllerPoint>,
    pub visible: bool,
    pub height: f32,
    pub collapsed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipType {
    Audio {
        file_id: String,
        /// Absolute path to the decoded source file, if this clip was created
        /// by importing a real audio file. Used as the waveform cache key.
        source_path: Option<String>,
    },
    Midi {
        notes: Vec<MidiNoteState>,
        /// MIDI controller (CC / pitch-bend / pressure) lanes for this clip.
        controller_lanes: Vec<MidiControllerLane>,
    },
}

/// Background import/decode state for a real audio file (waveform + engine).
#[derive(Debug, Clone, PartialEq)]
pub enum AudioImportState {
    Pending,
    Probing,
    Decoding { progress: f32 },
    GeneratingPeaks { progress: f32 },
    Ready,
    Failed { message: String },
}

impl Default for AudioImportState {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipState {
    pub id: String,
    pub name: String,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub source_duration_seconds: Option<f64>,
    pub offset_beats: f32,
    pub gain: f32,
    pub clip_type: ClipType,
    pub muted: bool,
    /// Populated for imported audio clips; drives clip chrome + waveform UI.
    pub audio_import: AudioImportState,
}

#[derive(Debug, Clone)]
pub struct ClipDragItem {
    pub clip_id: String,
    pub source_track_id: String,
    pub start_beat: f32,
}

/// Which edge of a clip an edge-resize gesture is dragging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipEdge {
    Left,
    Right,
}

/// In-flight clip edge-resize drag payload (mirrors [`ClipDragItem`]). Carries
/// the clip identity, which edge is dragged, and the original bounds so the
/// handler can resolve the new length from the live cursor position.
#[derive(Debug, Clone)]
pub struct ClipResizeDrag {
    pub clip_id: String,
    pub edge: ClipEdge,
    pub start_beat: f32,
    pub duration_beats: f32,
}

#[derive(Debug, Clone)]
pub struct TrackDragItem {
    pub track_id: String,
    pub origin_index: usize,
    pub name: String,
    pub color: gpui::Rgba,
}

/// In-flight automation point drag (move gesture). Held by the Timeline while
/// the mouse is down; the point is mutated live and committed once on release.
#[derive(Debug, Clone)]
pub struct AutomationPointDrag {
    pub track_id: String,
    pub lane_id: String,
    pub point_id: u64,
    /// Set once the point has actually moved, so a pure click (select only)
    /// never marks the project dirty.
    pub moved: bool,
}

/// In-flight tempo-point move on the global Tempo Track lane.
#[derive(Debug, Clone)]
pub struct TempoPointDrag {
    pub point_id: String,
    /// Set once the point has actually moved so a pure click never marks dirty.
    pub moved: bool,
}

/// In-flight time-signature marker drag on the global Time Signature lane.
#[derive(Debug, Clone)]
pub struct TimeSignaturePointDrag {
    pub point_id: String,
    pub moved: bool,
}

/// In-flight automation marquee (rubber-band) selection in beat/value space.
#[derive(Debug, Clone)]
pub struct AutomationMarquee {
    pub track_id: String,
    pub lane_id: String,
    pub start_beat: f32,
    pub start_value: f32,
    pub cur_beat: f32,
    pub cur_value: f32,
    pub additive: bool,
}

/// Per-track edit/display mode. `Clips` is normal clip editing; `Automation`
/// switches the lane to automation editing — points/line are drawn inside the
/// same track lane and clips are dimmed behind. UI-only state: toggling it
/// never marks the engine or project dirty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackLaneMode {
    Clips,
    Automation,
}

impl Default for TrackLaneMode {
    fn default() -> Self {
        TrackLaneMode::Clips
    }
}

/// Interpolation shape between an automation point and the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationCurve {
    Linear,
    Hold,
    /// S-curve — reserved. Evaluated as Linear until the curve math lands, but
    /// stored/round-tripped so existing data is never lost.
    Smooth,
}

impl Default for AutomationCurve {
    fn default() -> Self {
        AutomationCurve::Linear
    }
}

impl AutomationCurve {
    pub fn to_tag(self) -> u8 {
        match self {
            AutomationCurve::Linear => 0,
            AutomationCurve::Hold => 1,
            AutomationCurve::Smooth => 2,
        }
    }

    pub fn from_tag(tag: u8) -> Self {
        match tag {
            1 => AutomationCurve::Hold,
            2 => AutomationCurve::Smooth,
            _ => AutomationCurve::Linear,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AutomationCurve::Linear => "Linear",
            AutomationCurve::Hold => "Hold",
            AutomationCurve::Smooth => "Smooth",
        }
    }
}

// ── Tempo map ─────────────────────────────────────────────────────────────────

/// How the timeline maps musical time to horizontal pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineTimebase {
    /// Beat positions are spaced uniformly; tempo affects playback only.
    #[default]
    MusicalBeats,
    /// Beat positions map through TempoMap seconds; faster sections shrink.
    AbsoluteSeconds,
}

/// Whether a clip's anchor is stored in beats or wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipTimebase {
    #[default]
    Musical,
    Absolute,
}

/// Interpolation shape between a tempo point and the next one. Mirrors the
/// audio engine's tempo concept. `Smooth` is stored/round-tripped even though
/// it currently evaluates as `Linear` until the curve math lands engine-side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TempoCurve {
    #[default]
    Hold,
    Linear,
    Smooth,
}

impl TempoCurve {
    pub fn to_tag(self) -> u8 {
        match self {
            TempoCurve::Hold => 0,
            TempoCurve::Linear => 1,
            TempoCurve::Smooth => 2,
        }
    }

    pub fn from_tag(tag: u8) -> Self {
        match tag {
            1 => TempoCurve::Linear,
            2 => TempoCurve::Smooth,
            _ => TempoCurve::Hold,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TempoCurve::Hold => "Hold",
            TempoCurve::Linear => "Linear",
            TempoCurve::Smooth => "Smooth",
        }
    }
}

/// Monotonic source of stable tempo-point identities. Persisted in project
/// files so edits target a point by id even after the user drags it to a new
/// beat position.
fn next_tempo_point_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("tempo-{ts:x}-{seq:x}")
}

/// A tempo change anchored at a musical beat. The `curve` describes how tempo
/// moves from this point to the next one. Marker labels and the tempo-track
/// editor read `bpm` directly — transport BPM uses [`TempoMap::bpm_at_beat`].
#[derive(Debug, Clone, PartialEq)]
pub struct TempoPoint {
    pub id: String,
    pub beat: f64,
    pub bpm: f64,
    pub curve: TempoCurve,
}

impl TempoPoint {
    pub fn new(beat: f64, bpm: f64, curve: TempoCurve) -> Self {
        Self {
            id: next_tempo_point_id(),
            beat: beat.max(0.0),
            bpm: bpm.clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX),
            curve,
        }
    }

    pub fn with_id(id: impl Into<String>, beat: f64, bpm: f64, curve: TempoCurve) -> Self {
        Self {
            id: id.into(),
            beat: beat.max(0.0),
            bpm: bpm.clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX),
            curve,
        }
    }
}

/// Project-level tempo automation. This is global state owned by the project,
/// not by any track — the TempoTrack is only a view/controller over this map.
/// When `points` is empty the project plays at the timeline's base BPM; the
/// base BPM is supplied by the caller (`TimelineState::bpm`) so this map stays
/// self-contained and cheap to clone.
/// Cached hold-mode segment for beat/time conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TempoHoldSegment {
    start_beat: f64,
    start_seconds: f64,
    bpm: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TempoMap {
    /// Sorted (by beat) tempo markers in addition to the implicit base point at
    /// beat 0. Kept small; UI-thread only, so a linear scan is fine.
    pub points: Vec<TempoPoint>,
    /// Bumped on every edit so UI/engine caches can invalidate.
    revision: u64,
}

impl TempoMap {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            revision: 0,
        }
    }

    pub fn with_points(points: Vec<TempoPoint>) -> Self {
        let mut map = Self::new();
        map.points = points;
        map.sort();
        map.bump_revision();
        map
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// True when the map carries any tempo automation (one or more markers).
    /// With no markers the project is a single static tempo.
    pub fn has_automation(&self) -> bool {
        !self.points.is_empty()
    }

    /// Hold-mode seconds at `beat` using step-hold segments between markers.
    pub fn seconds_at_beat(&self, beat: f64, base_bpm: f64) -> f64 {
        let beat = beat.max(0.0);
        let segments = self.hold_segments(base_bpm);
        let seg = hold_segment_at_beat(&segments, beat);
        seg.start_seconds + (beat - seg.start_beat) * 60.0 / seg.bpm.max(TEMPO_BPM_MIN)
    }

    /// Inverse of [`Self::seconds_at_beat`] for hold-mode segments.
    pub fn beat_at_seconds(&self, seconds: f64, base_bpm: f64) -> f64 {
        let seconds = seconds.max(0.0);
        let segments = self.hold_segments(base_bpm);
        if segments.is_empty() {
            return 0.0;
        }
        if seconds <= segments[0].start_seconds {
            return 0.0;
        }
        let idx = segments
            .partition_point(|seg| seg.start_seconds <= seconds)
            .saturating_sub(1);
        let seg = &segments[idx.min(segments.len() - 1)];
        let elapsed = seconds - seg.start_seconds;
        seg.start_beat + elapsed * seg.bpm.max(TEMPO_BPM_MIN) / 60.0
    }

    pub fn samples_at_beat(&self, beat: f64, base_bpm: f64, sample_rate: f64) -> u64 {
        (self.seconds_at_beat(beat, base_bpm) * sample_rate.max(1.0))
            .round()
            .max(0.0) as u64
    }

    pub fn beat_at_samples(&self, samples: u64, base_bpm: f64, sample_rate: f64) -> f64 {
        let seconds = samples as f64 / sample_rate.max(1.0);
        self.beat_at_seconds(seconds, base_bpm)
    }

    /// Effective BPM at `beat`, evaluating curves between markers. `base_bpm`
    /// is the implicit tempo at beat 0 (the timeline's nominal BPM).
    pub fn bpm_at_beat(&self, beat: f64, base_bpm: f64) -> f64 {
        if self.points.is_empty() {
            return base_bpm;
        }
        let beat = beat.max(0.0);
        // Build the effective point preceding `beat` and its successor without
        // allocating: walk the implicit base point followed by the markers.
        let first = &self.points[0];
        if beat < first.beat {
            // Before the first marker we sit on the implicit base point. The
            // base point holds (Hold) up to the first marker.
            return base_bpm;
        }
        // Find the last marker at or before `beat`.
        let mut idx = 0usize;
        for (i, p) in self.points.iter().enumerate() {
            if p.beat <= beat {
                idx = i;
            } else {
                break;
            }
        }
        let cur = &self.points[idx];
        let next = self.points.get(idx + 1);
        match (cur.curve, next) {
            (TempoCurve::Hold, _) | (_, None) => cur.bpm,
            (curve, Some(next)) => {
                let span = (next.beat - cur.beat).max(1e-9);
                let t = ((beat - cur.beat) / span).clamp(0.0, 1.0);
                let t = match curve {
                    TempoCurve::Smooth => t * t * (3.0 - 2.0 * t),
                    _ => t,
                };
                cur.bpm + (next.bpm - cur.bpm) * t
            }
        }
    }

    /// Ruler/tempo-track label for a stored marker BPM — never the transport
    /// playhead-evaluated tempo.
    pub fn format_marker_label(bpm: f64) -> String {
        if bpm.fract().abs() < 0.05 {
            format!("{bpm:.0}")
        } else {
            format!("{bpm:.1}")
        }
    }

    /// Assign generated ids to legacy points loaded without one.
    pub fn ensure_point_ids(&mut self) {
        for point in &mut self.points {
            if point.id.is_empty() {
                point.id = next_tempo_point_id();
            }
        }
    }

    /// Id of the marker governing `beat` (last point at or before `beat`).
    pub fn point_id_at_or_before_beat(&self, beat: f64) -> Option<&str> {
        let beat = beat.max(0.0);
        self.points
            .iter()
            .filter(|p| p.beat <= beat)
            .last()
            .map(|p| p.id.as_str())
    }

    /// Update only the matching tempo point's stored BPM by stable id.
    pub fn update_point_bpm_by_id(&mut self, id: &str, bpm: f64) -> bool {
        let bpm = bpm.clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX);
        if let Some(point) = self.points.iter_mut().find(|p| p.id == id) {
            point.bpm = bpm;
            self.bump_revision();
            true
        } else {
            false
        }
    }

    /// Insert a tempo marker, replacing any existing marker within a small beat
    /// epsilon. Keeps `points` sorted by beat.
    pub fn add_or_update_point(&mut self, beat: f64, bpm: f64, curve: TempoCurve) {
        let beat = beat.max(0.0);
        let bpm = bpm.clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX);
        if let Some(existing) = self
            .points
            .iter_mut()
            .find(|p| (p.beat - beat).abs() < 1e-6)
        {
            existing.bpm = bpm;
            existing.curve = curve;
        } else {
            self.points.push(TempoPoint::new(beat, bpm, curve));
        }
        self.sort();
        self.bump_revision();
    }

    /// Remove the marker nearest `beat` within `epsilon` beats. Returns whether
    /// a marker was removed.
    pub fn remove_point_near(&mut self, beat: f64, epsilon: f64) -> bool {
        if let Some(idx) = self
            .points
            .iter()
            .position(|p| (p.beat - beat).abs() <= epsilon)
        {
            self.points.remove(idx);
            self.bump_revision();
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.bump_revision();
    }

    /// Replace the map with exactly one marker (fixed tempo automation).
    pub fn reset_to_single_point(&mut self, beat: f64, bpm: f64, curve: TempoCurve) {
        self.points.clear();
        self.points.push(TempoPoint::new(beat, bpm, curve));
        self.sort();
        self.bump_revision();
    }

    pub fn remove_point_by_id(&mut self, id: &str) -> bool {
        if let Some(idx) = self.points.iter().position(|p| p.id == id) {
            self.points.remove(idx);
            self.bump_revision();
            true
        } else {
            false
        }
    }

    pub fn move_point_by_id(&mut self, id: &str, beat: f64, bpm: f64) -> bool {
        let beat = beat.max(0.0);
        let bpm = bpm.clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX);
        if self
            .points
            .iter()
            .any(|p| p.id != id && (p.beat - beat).abs() < TEMPO_BEAT_EPSILON)
        {
            return false;
        }
        if let Some(point) = self.points.iter_mut().find(|p| p.id == id) {
            point.beat = beat;
            point.bpm = bpm;
            self.sort();
            self.bump_revision();
            true
        } else {
            false
        }
    }

    pub fn update_point_curve_by_id(&mut self, id: &str, curve: TempoCurve) -> bool {
        if let Some(point) = self.points.iter_mut().find(|p| p.id == id) {
            point.curve = curve;
            self.bump_revision();
            true
        } else {
            false
        }
    }

    /// Hold a constant tempo from `beat` onward by removing later markers.
    pub fn set_fixed_from_beat(&mut self, beat: f64, bpm: f64, base_bpm: f64) {
        let beat = beat.max(0.0);
        let bpm = bpm.clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX);
        self.points.retain(|p| p.beat < beat - TEMPO_BEAT_EPSILON);
        if beat <= TEMPO_BEAT_EPSILON {
            self.reset_to_single_point(0.0, bpm, TempoCurve::Hold);
            return;
        }
        if !self
            .points
            .iter()
            .any(|p| (p.beat - beat).abs() < TEMPO_BEAT_EPSILON)
        {
            if self.points.is_empty() {
                self.add_or_update_point(0.0, base_bpm, TempoCurve::Hold);
            }
            self.add_or_update_point(beat, bpm, TempoCurve::Hold);
        } else {
            self.add_or_update_point(beat, bpm, TempoCurve::Hold);
        }
    }

    fn sort(&mut self) {
        self.points.sort_by(|a, b| {
            a.beat
                .partial_cmp(&b.beat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn hold_segments(&self, base_bpm: f64) -> Vec<TempoHoldSegment> {
        let base_bpm = base_bpm.clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX);
        let mut markers: Vec<(f64, f64)> = Vec::new();
        if self.points.is_empty() {
            markers.push((0.0, base_bpm));
        } else {
            if self.points[0].beat > 0.0 {
                markers.push((0.0, base_bpm));
            }
            for point in &self.points {
                markers.push((point.beat, point.bpm));
            }
        }
        let mut segments = Vec::with_capacity(markers.len());
        let mut start_seconds = 0.0;
        for (i, (beat, bpm)) in markers.iter().enumerate() {
            segments.push(TempoHoldSegment {
                start_beat: *beat,
                start_seconds,
                bpm: *bpm,
            });
            if let Some((next_beat, _)) = markers.get(i + 1) {
                start_seconds += (next_beat - beat) * 60.0 / bpm.max(TEMPO_BPM_MIN);
            }
        }
        segments
    }
}

fn hold_segment_at_beat(segments: &[TempoHoldSegment], beat: f64) -> TempoHoldSegment {
    if segments.is_empty() {
        return TempoHoldSegment {
            start_beat: 0.0,
            start_seconds: 0.0,
            bpm: TEMPO_BPM_MIN,
        };
    }
    let idx = segments
        .partition_point(|seg| seg.start_beat <= beat)
        .saturating_sub(1);
    segments[idx.min(segments.len() - 1)]
}

// ── Time signature map ───────────────────────────────────────────────────────

pub type TimeSignaturePointId = String;

pub const TS_NUMERATOR_MIN: u16 = 1;
pub const TS_NUMERATOR_MAX: u16 = 64;
pub const TS_ALLOWED_DENOMINATORS: [u16; 6] = [1, 2, 4, 8, 16, 32];
pub const TS_BEAT_EPSILON: f64 = 1e-6;

fn next_time_signature_point_id() -> TimeSignaturePointId {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("ts-{ts:x}-{seq:x}")
}

/// Normalize a denominator to one of the allowed note values.
pub fn normalize_time_signature_denominator(denominator: u16) -> u16 {
    TS_ALLOWED_DENOMINATORS
        .iter()
        .copied()
        .min_by_key(|allowed| (denominator as i32 - *allowed as i32).unsigned_abs())
        .unwrap_or(4)
}

/// Quarter-note beats per bar for a time signature.
pub fn beats_per_bar_from_sig(numerator: u16, denominator: u16) -> f64 {
    let num = numerator.clamp(TS_NUMERATOR_MIN, TS_NUMERATOR_MAX) as f64;
    let den = normalize_time_signature_denominator(denominator).max(1) as f64;
    num * (4.0 / den)
}

/// One denominator-note unit expressed in quarter-note beats (N/D => 4/D).
pub fn denominator_unit_quarter_beats(denominator: u16) -> f64 {
    4.0 / normalize_time_signature_denominator(denominator).max(1) as f64
}

/// Default accent grouping for a meter. Sum always equals `numerator`.
pub fn default_time_signature_grouping(numerator: u16, denominator: u16) -> Vec<u16> {
    let numerator = numerator.clamp(TS_NUMERATOR_MIN, TS_NUMERATOR_MAX);
    let denominator = normalize_time_signature_denominator(denominator);
    match (numerator, denominator) {
        (2, 4) => vec![2],
        (3, 4) => vec![3],
        (4, 4) => vec![4],
        (5, 8) => vec![2, 3],
        (6, 8) => vec![3, 3],
        (7, 8) => vec![2, 2, 3],
        (9, 8) => vec![3, 3, 3],
        (12, 8) => vec![3, 3, 3, 3],
        (n, 8) if n % 2 == 1 && n > 3 => {
            let pairs = ((n - 3) / 2) as usize;
            let mut groups = vec![2; pairs];
            groups.push(3);
            groups
        }
        _ => vec![numerator],
    }
}

pub fn normalize_time_signature_grouping(
    numerator: u16,
    denominator: u16,
    grouping: &[u16],
) -> Vec<u16> {
    let numerator = numerator.clamp(TS_NUMERATOR_MIN, TS_NUMERATOR_MAX);
    if grouping.is_empty()
        || grouping.iter().any(|&g| g == 0)
        || grouping.iter().map(|&g| g as u32).sum::<u32>() != numerator as u32
    {
        default_time_signature_grouping(numerator, denominator)
    } else {
        grouping.to_vec()
    }
}

/// Cumulative denominator-beat indices (0-based) where each accent group begins.
pub fn time_signature_group_starts(grouping: &[u16]) -> Vec<u16> {
    let mut starts = vec![0u16];
    let mut acc = 0u16;
    for (i, &grp) in grouping.iter().enumerate() {
        if i > 0 {
            starts.push(acc);
        }
        acc = acc.saturating_add(grp);
    }
    starts
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimeSignaturePoint {
    pub id: TimeSignaturePointId,
    pub beat: f64,
    pub numerator: u16,
    pub denominator: u16,
    /// Accent grouping in denominator-beat units (e.g. 5/8 => [2, 3]).
    pub grouping: Vec<u16>,
}

impl TimeSignaturePoint {
    pub fn new(beat: f64, numerator: u16, denominator: u16) -> Self {
        let numerator = numerator.clamp(TS_NUMERATOR_MIN, TS_NUMERATOR_MAX);
        let denominator = normalize_time_signature_denominator(denominator);
        Self {
            id: next_time_signature_point_id(),
            beat: beat.max(0.0),
            numerator,
            denominator,
            grouping: default_time_signature_grouping(numerator, denominator),
        }
    }

    pub fn with_id(
        id: impl Into<String>,
        beat: f64,
        numerator: u16,
        denominator: u16,
    ) -> Self {
        let numerator = numerator.clamp(TS_NUMERATOR_MIN, TS_NUMERATOR_MAX);
        let denominator = normalize_time_signature_denominator(denominator);
        Self {
            id: id.into(),
            beat: beat.max(0.0),
            numerator,
            denominator,
            grouping: default_time_signature_grouping(numerator, denominator),
        }
    }

    pub fn with_grouping(
        id: impl Into<String>,
        beat: f64,
        numerator: u16,
        denominator: u16,
        grouping: Vec<u16>,
    ) -> Self {
        let numerator = numerator.clamp(TS_NUMERATOR_MIN, TS_NUMERATOR_MAX);
        let denominator = normalize_time_signature_denominator(denominator);
        Self {
            id: id.into(),
            beat: beat.max(0.0),
            numerator,
            denominator,
            grouping: normalize_time_signature_grouping(numerator, denominator, &grouping),
        }
    }

    pub fn effective_grouping(&self) -> Vec<u16> {
        normalize_time_signature_grouping(self.numerator, self.denominator, &self.grouping)
    }

    pub fn group_starts(&self) -> Vec<u16> {
        time_signature_group_starts(&self.effective_grouping())
    }

    pub fn label(&self) -> String {
        TimeSignatureMap::format_marker_label(self.numerator, self.denominator)
    }
}

/// One arrangement bar background span in quarter-note beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarBackgroundRect {
    pub bar: i64,
    pub start_beat: f64,
    pub end_beat: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BarBeat {
    pub bar: i64,
    /// 1-based denominator-beat index within the bar (1 = downbeat).
    pub beat_in_bar: u16,
    /// Fractional position within the current denominator beat (0..1).
    pub sub_beat_fraction: f64,
    pub numerator: u16,
    pub denominator: u16,
}

/// Project-level time signature markers. Global timing data — not owned by any
/// track. Ruler labels, grid grouping, transport display, and metronome accents
/// all evaluate this map at the relevant beat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeSignatureMap {
    pub points: Vec<TimeSignaturePoint>,
    revision: u64,
}

impl TimeSignatureMap {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            revision: 0,
        }
    }

    pub fn with_default_4_4() -> Self {
        let mut map = Self::new();
        map.points.push(TimeSignaturePoint::new(0.0, 4, 4));
        map.bump_revision();
        map
    }

    pub fn with_points(points: Vec<TimeSignaturePoint>) -> Self {
        let mut map = Self::new();
        map.points = points;
        map.sort();
        map.bump_revision();
        map
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn has_markers(&self) -> bool {
        !self.points.is_empty()
    }

    pub fn format_marker_label(numerator: u16, denominator: u16) -> String {
        format!(
            "{}/{}",
            numerator,
            normalize_time_signature_denominator(denominator)
        )
    }

    pub fn ensure_point_ids(&mut self) {
        for point in &mut self.points {
            if point.id.is_empty() {
                point.id = next_time_signature_point_id();
            }
        }
    }

    /// Seed beat-0 4/4 when empty (legacy projects / first show).
    pub fn ensure_default_point(&mut self) {
        if self.points.is_empty() {
            self.points.push(TimeSignaturePoint::new(0.0, 4, 4));
            self.bump_revision();
        }
        self.ensure_point_ids();
    }

    pub fn time_signature_at_beat(&self, beat: f64) -> TimeSignaturePoint {
        let beat = beat.max(0.0);
        if self.points.is_empty() {
            return TimeSignaturePoint::with_id("implicit-4-4", 0.0, 4, 4);
        }
        let mut idx = 0usize;
        for (i, p) in self.points.iter().enumerate() {
            if p.beat <= beat + TS_BEAT_EPSILON {
                idx = i;
            } else {
                break;
            }
        }
        self.points[idx].clone()
    }

    pub fn beats_per_bar_at_beat(&self, beat: f64) -> f64 {
        let pt = self.time_signature_at_beat(beat);
        beats_per_bar_from_sig(pt.numerator, pt.denominator)
    }

    pub fn bar_beat_at_beat(&self, beat: f64) -> BarBeat {
        let beat = beat.max(0.0);
        let points = self.sorted_points();
        let mut global_bar: i64 = 1;

        for (i, pt) in points.iter().enumerate() {
            let seg_start = pt.beat;
            let seg_end = points
                .get(i + 1)
                .map(|p| p.beat)
                .unwrap_or(f64::INFINITY);
            if beat + TS_BEAT_EPSILON < seg_start {
                continue;
            }
            let bpb = beats_per_bar_from_sig(pt.numerator, pt.denominator);
            let denom_unit = denominator_unit_quarter_beats(pt.denominator);
            if beat < seg_end - TS_BEAT_EPSILON || i + 1 == points.len() {
                let rel = (beat - seg_start).max(0.0);
                let bar_offset = (rel / bpb).floor() as i64;
                let beat_in_bar_q = rel - bar_offset as f64 * bpb;
                let denom_idx = (beat_in_bar_q / denom_unit).floor();
                let sub_frac = if denom_unit > TS_BEAT_EPSILON {
                    (beat_in_bar_q / denom_unit).fract()
                } else {
                    0.0
                };
                return BarBeat {
                    bar: global_bar + bar_offset,
                    beat_in_bar: (denom_idx as u16).saturating_add(1),
                    sub_beat_fraction: sub_frac,
                    numerator: pt.numerator,
                    denominator: pt.denominator,
                };
            }
            let bars_in_seg = ((seg_end - seg_start) / bpb).floor() as i64;
            global_bar += bars_in_seg.max(0);
        }

        BarBeat {
            bar: 1,
            beat_in_bar: 1,
            sub_beat_fraction: 0.0,
            numerator: 4,
            denominator: 4,
        }
    }

    pub fn beat_at_bar_beat(&self, bar: i64, beat_in_bar: u16) -> f64 {
        let bar = bar.max(1);
        let beat_in_bar = beat_in_bar.max(1);
        let points = self.sorted_points();
        let mut global_bar: i64 = 1;

        for (i, pt) in points.iter().enumerate() {
            let seg_start = pt.beat;
            let seg_end = points
                .get(i + 1)
                .map(|p| p.beat)
                .unwrap_or(f64::INFINITY);
            let bpb = beats_per_bar_from_sig(pt.numerator, pt.denominator);
            let denom_unit = denominator_unit_quarter_beats(pt.denominator);
            let bars_in_seg = if seg_end.is_finite() {
                ((seg_end - seg_start) / bpb).floor() as i64
            } else {
                i64::MAX / 2
            };

            if bar < global_bar + bars_in_seg || i + 1 == points.len() {
                let bar_offset = (bar - global_bar).max(0);
                return seg_start
                    + bar_offset as f64 * bpb
                    + (beat_in_bar.saturating_sub(1) as f64) * denom_unit;
            }
            global_bar += bars_in_seg;
        }
        0.0
    }

    /// Snap a marker beat to the start of its current bar (MVP bar-boundary insert).
    pub fn snap_marker_beat_to_bar_boundary(&self, beat: f64) -> f64 {
        let bb = self.bar_beat_at_beat(beat.max(0.0));
        self.bar_start_beat(bb.bar)
    }

    pub fn bar_start_beat(&self, bar: i64) -> f64 {
        self.beat_at_bar_beat(bar, 1)
    }

    pub fn next_bar_beat(&self, beat: f64) -> f64 {
        let bb = self.bar_beat_at_beat(beat);
        let bpb = beats_per_bar_from_sig(bb.numerator, bb.denominator);
        self.bar_start_beat(bb.bar) + bpb
    }

    pub fn previous_bar_beat(&self, beat: f64) -> f64 {
        let bb = self.bar_beat_at_beat(beat);
        if bb.bar <= 1 {
            return 0.0;
        }
        self.bar_start_beat(bb.bar - 1)
    }

    pub fn format_position_at_beat(&self, beat: f64) -> String {
        let bb = self.bar_beat_at_beat(beat);
        format!("{}.{}", bb.bar, bb.beat_in_bar)
    }

    /// Global bar number containing `beat`.
    pub fn bar_at_beat(&self, beat: f64) -> i64 {
        self.bar_beat_at_beat(beat).bar
    }

    /// Enumerate bar spans intersecting a visible beat range for background paint.
    pub fn visible_bar_rects(&self, visible_start: f64, visible_end: f64) -> Vec<BarBackgroundRect> {
        const MAX_BARS: i64 = 4096;
        let visible_start = visible_start.max(0.0);
        let visible_end = visible_end.max(visible_start);
        let mut bar = self.bar_at_beat(visible_start);
        let mut rects = Vec::new();
        for _ in 0..MAX_BARS {
            let start_beat = self.bar_start_beat(bar);
            if start_beat >= visible_end - TS_BEAT_EPSILON {
                break;
            }
            let end_beat = self.bar_start_beat(bar + 1);
            if end_beat > visible_start + TS_BEAT_EPSILON
                && start_beat < visible_end - TS_BEAT_EPSILON
            {
                rects.push(BarBackgroundRect {
                    bar,
                    start_beat,
                    end_beat,
                });
            }
            bar += 1;
        }
        rects
    }

    pub fn add_or_update_point(&mut self, beat: f64, numerator: u16, denominator: u16) {
        let beat = self.snap_marker_beat_to_bar_boundary(beat.max(0.0));
        let numerator = numerator.clamp(TS_NUMERATOR_MIN, TS_NUMERATOR_MAX);
        let denominator = normalize_time_signature_denominator(denominator);
        if let Some(existing) = self
            .points
            .iter_mut()
            .find(|p| (p.beat - beat).abs() < TS_BEAT_EPSILON)
        {
            existing.numerator = numerator;
            existing.denominator = denominator;
            existing.grouping = default_time_signature_grouping(numerator, denominator);
        } else {
            self.points
                .push(TimeSignaturePoint::new(beat, numerator, denominator));
        }
        self.sort();
        self.bump_revision();
    }

    pub fn update_point_by_id(
        &mut self,
        id: &str,
        numerator: u16,
        denominator: u16,
    ) -> bool {
        let numerator = numerator.clamp(TS_NUMERATOR_MIN, TS_NUMERATOR_MAX);
        let denominator = normalize_time_signature_denominator(denominator);
        if let Some(point) = self.points.iter_mut().find(|p| p.id == id) {
            point.numerator = numerator;
            point.denominator = denominator;
            point.grouping = default_time_signature_grouping(numerator, denominator);
            self.bump_revision();
            true
        } else {
            false
        }
    }

    pub fn move_point_by_id(&mut self, id: &str, beat: f64) -> bool {
        let beat = self.snap_marker_beat_to_bar_boundary(beat.max(0.0));
        if self
            .points
            .iter()
            .any(|p| p.id != id && (p.beat - beat).abs() < TS_BEAT_EPSILON)
        {
            return false;
        }
        if let Some(point) = self.points.iter_mut().find(|p| p.id == id) {
            point.beat = beat;
            self.sort();
            self.bump_revision();
            true
        } else {
            false
        }
    }

    pub fn remove_point_by_id(&mut self, id: &str) -> bool {
        if let Some(idx) = self.points.iter().position(|p| p.id == id) {
            self.points.remove(idx);
            self.bump_revision();
            true
        } else {
            false
        }
    }

    pub fn reset_to_single_point(&mut self, beat: f64, numerator: u16, denominator: u16) {
        self.points.clear();
        self.points
            .push(TimeSignaturePoint::new(beat, numerator, denominator));
        self.sort();
        self.bump_revision();
    }

    fn sort(&mut self) {
        self.points
            .sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));
    }

    fn sorted_points(&self) -> Vec<TimeSignaturePoint> {
        let mut points = if self.points.is_empty() {
            vec![TimeSignaturePoint::with_id("implicit-4-4", 0.0, 4, 4)]
        } else {
            self.points.clone()
        };
        points.sort_by(|a, b| a.beat.partial_cmp(&b.beat).unwrap_or(std::cmp::Ordering::Equal));
        points
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

/// BPM clamp range for tempo points (matches the audio engine spec).
pub const TEMPO_BPM_MIN: f64 = 20.0;
pub const TEMPO_BPM_MAX: f64 = 999.0;

/// Default expanded height for the global Tempo Track lane (px).
pub const TEMPO_TRACK_HEIGHT: f32 = 72.0;
/// Collapsed/minimal Tempo Track lane height (px).
pub const TEMPO_TRACK_HEIGHT_COLLAPSED: f32 = 48.0;
/// Vertical padding inside the tempo lane curve area (px).
pub const TEMPO_LANE_PAD: f32 = 6.0;
/// Two tempo points within this many beats are treated as the same slot.
pub const TEMPO_BEAT_EPSILON: f64 = 1e-6;

/// Global/system lanes rendered between the ruler and normal tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalLaneKind {
    Tempo,
    TimeSignature,
    Marker,
    Arranger,
}

/// Map a BPM value to a lane-local y coordinate (high BPM near the top).
pub fn bpm_to_y(bpm: f64, lane_height: f32, min_bpm: f64, max_bpm: f64) -> f32 {
    let pad = TEMPO_LANE_PAD;
    let usable = (lane_height - 2.0 * pad).max(1.0);
    let span = (max_bpm - min_bpm).max(1e-9);
    let t = ((bpm - min_bpm) / span).clamp(0.0, 1.0);
    pad + ((1.0 - t) as f32) * usable
}

/// Inverse of [`bpm_to_y`]: lane-local y → BPM.
pub fn y_to_bpm(y: f32, lane_height: f32, min_bpm: f64, max_bpm: f64) -> f64 {
    let pad = TEMPO_LANE_PAD;
    let usable = (lane_height - 2.0 * pad).max(1.0);
    let t = ((y - pad) / usable).clamp(0.0, 1.0);
    let span = (max_bpm - min_bpm).max(1e-9);
    (max_bpm - t as f64 * span).clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX)
}

/// What a single automation lane controls. `TrackVolume`/`TrackPan` are wired
/// first; `PluginParameter`/`SendLevel` carry their descriptor so they can be
/// persisted and shown in the picker even before runtime application lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationTarget {
    TrackVolume,
    TrackPan,
    TrackMute,
    PluginParameter {
        insert_id: String,
        parameter_id: String,
        parameter_name: String,
    },
    SendLevel {
        send_id: String,
    },
}

impl AutomationTarget {
    /// Short label shown on the lane header / target picker.
    pub fn display_name(&self) -> String {
        match self {
            AutomationTarget::TrackVolume => "Volume".to_string(),
            AutomationTarget::TrackPan => "Pan".to_string(),
            AutomationTarget::TrackMute => "Mute".to_string(),
            AutomationTarget::PluginParameter { parameter_name, .. } => parameter_name.clone(),
            AutomationTarget::SendLevel { send_id } => format!("Send {send_id}"),
        }
    }

    /// Value used for the automation line before the first point / when a lane
    /// has no points yet. Normalized 0.0..=1.0.
    pub fn default_value(&self) -> f32 {
        match self {
            AutomationTarget::TrackVolume => volume::db_to_norm(0.0),
            AutomationTarget::TrackPan => 0.5,
            AutomationTarget::TrackMute => 0.0,
            AutomationTarget::PluginParameter { .. } => 0.5,
            AutomationTarget::SendLevel { .. } => 0.0,
        }
    }

    /// Stable discriminant tag for binary persistence.
    pub fn to_tag(&self) -> u8 {
        match self {
            AutomationTarget::TrackVolume => 0,
            AutomationTarget::TrackPan => 1,
            AutomationTarget::TrackMute => 2,
            AutomationTarget::PluginParameter { .. } => 3,
            AutomationTarget::SendLevel { .. } => 4,
        }
    }

    /// Best-effort mapping from a legacy lane name (pre-target persistence)
    /// onto a concrete target so old projects keep working.
    pub fn from_legacy_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        if lower.contains("pan") {
            AutomationTarget::TrackPan
        } else if lower.contains("mute") {
            AutomationTarget::TrackMute
        } else {
            AutomationTarget::TrackVolume
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationPoint {
    /// Transient identity (not serialized) — lets the lane editor track
    /// selection and in-flight drag targets across edits.
    pub id: u64,
    pub beat: f32,
    /// Normalized value in `0.0..=1.0`.
    pub value: f32,
    pub curve: AutomationCurve,
    /// UI-only selection flag. Never serialized.
    pub selected: bool,
}

impl AutomationPoint {
    pub fn new(beat: f32, value: f32) -> Self {
        Self {
            id: next_automation_point_id(),
            beat: beat.max(0.0),
            value: value.clamp(0.0, 1.0),
            curve: AutomationCurve::Linear,
            selected: false,
        }
    }

    pub fn with_curve(beat: f32, value: f32, curve: AutomationCurve) -> Self {
        let mut p = Self::new(beat, value);
        p.curve = curve;
        p
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationLaneState {
    pub id: String,
    /// Display name. Mirrors `target.display_name()` for built-ins but kept as
    /// a field for back-compat with the persisted `parameter_name`.
    pub name: String,
    pub target: AutomationTarget,
    pub enabled: bool,
    /// Whether the dedicated expanded sub-lane is shown (separate from the
    /// in-track automation overlay shown by [`TrackLaneMode::Automation`]).
    pub visible: bool,
    pub points: Vec<AutomationPoint>,
}

impl AutomationLaneState {
    /// Build an empty lane for `target` with an auto-derived id/name.
    pub fn new(id: impl Into<String>, target: AutomationTarget) -> Self {
        Self {
            id: id.into(),
            name: target.display_name(),
            target,
            enabled: true,
            visible: false,
            points: Vec::new(),
        }
    }

    /// Re-sort points by beat. Call after any add/move so evaluation and line
    /// rendering can assume ascending order.
    pub fn sort_points(&mut self) {
        self.points.sort_by(|a, b| {
            a.beat
                .partial_cmp(&b.beat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

/// Plugin format identifier mirrored from `project::PluginFormat`. Kept
/// in the UI state so we can render an icon/badge without depending on
/// the project crate from render code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPluginFormat {
    Vst3,
    Clap,
    Au,
    Lv2,
    Unknown,
}

impl InsertPluginFormat {
    pub fn label(self) -> &'static str {
        match self {
            InsertPluginFormat::Vst3 => "VST3",
            InsertPluginFormat::Clap => "CLAP",
            InsertPluginFormat::Au => "AU",
            InsertPluginFormat::Lv2 => "LV2",
            InsertPluginFormat::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRuntimeBackend {
    InProcess,
    ExternalBridge,
}

impl PluginRuntimeBackend {
    pub fn label(self) -> &'static str {
        match self {
            PluginRuntimeBackend::InProcess => "in_process",
            PluginRuntimeBackend::ExternalBridge => "external_bridge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRuntimeState {
    Loading,
    Ready,
    EditorOpening,
    EditorOpen,
    Failed(String),
    Crashed,
    Unloaded,
}

/// Load progress of an insert slot. Drives the chip color / label.
/// `Loading` is reserved for Phase 2 when actual plugin instantiation
/// runs on a worker thread. Phase 1 transitions Empty → Ready directly
/// because the runtime doesn't yet instantiate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertLoadStatus {
    Empty,
    Loading,
    Ready,
    Failed(String),
    Disabled,
}

impl Default for InsertLoadStatus {
    fn default() -> Self {
        InsertLoadStatus::Empty
    }
}

/// Read-only parameter snapshot — populated in Phase 5 by the param
/// event drain pump. Phase 1 keeps the vec empty.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginParameterState {
    pub id: u32,
    pub name: String,
    pub value_normalized: f32,
}

/// UI-side mirror of `project::ProjectInsert`. The runtime owns the
/// actual plugin processor; this struct only stores descriptor +
/// transient UI state (bypass, load status, last-seen parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct InsertSlotState {
    pub id: String,
    /// Stable plugin identifier (`plugin_uid` / classId) — primary key
    /// against the plugin registry. `None` while the slot is empty.
    pub plugin_id: Option<String>,
    pub plugin_path: Option<std::path::PathBuf>,
    pub plugin_format: Option<InsertPluginFormat>,
    /// Display label shown on the mixer strip. "Empty" when no plugin
    /// is loaded; the plugin's `display_name` otherwise.
    pub display_name: String,
    pub enabled: bool,
    pub bypassed: bool,
    pub load_status: InsertLoadStatus,
    pub runtime_backend: PluginRuntimeBackend,
    pub runtime_state: PluginRuntimeState,
    pub host_pid: Option<u32>,
    pub parameters: Vec<PluginParameterState>,
}

impl InsertSlotState {
    pub fn empty(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            plugin_id: None,
            plugin_path: None,
            plugin_format: None,
            display_name: "Empty".to_string(),
            enabled: true,
            bypassed: false,
            load_status: InsertLoadStatus::Empty,
            runtime_backend: PluginRuntimeBackend::InProcess,
            runtime_state: PluginRuntimeState::Unloaded,
            host_pid: None,
            parameters: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.plugin_id.is_none()
    }
}

/// A single aux send from this track to a Bus/Return track (Phase 3). The
/// runtime sums `gain_db`-scaled signal into the target's input. UI stores the
/// descriptor; DAUx owns the realtime accumulation.
#[derive(Debug, Clone, PartialEq)]
pub struct SendSlotState {
    pub id: String,
    /// Id of the destination Bus/Return track.
    pub target_track_id: String,
    /// Display label for the destination (resolved at edit time; refreshed
    /// from the track list on render).
    pub target_name: String,
    pub enabled: bool,
    /// `true` = tap before the source track fader; `false` = post-fader.
    /// Realtime currently honours post-fader only (pre-fader is a refinement).
    pub pre_fader: bool,
    pub gain_db: f32,
}

impl SendSlotState {
    /// Linear send gain from `gain_db` (clamped to a sane range).
    pub fn gain_linear(&self) -> f32 {
        10f32.powf(self.gain_db.clamp(-60.0, 6.0) / 20.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackInputRouting {
    None,
    AllInputs,
    AudioDeviceChannel {
        device_id: String,
        channel: u32,
    },
    AudioDeviceChannels {
        device_id: String,
        channels: Vec<u32>,
    },
    MidiDevice {
        device_id: String,
    },
}

impl TrackInputRouting {
    pub fn label(&self) -> String {
        match self {
            Self::None => "None".to_string(),
            Self::AllInputs => "All Inputs".to_string(),
            Self::AudioDeviceChannel { device_id, channel } => {
                format!("{device_id} ch {}", channel + 1)
            }
            Self::AudioDeviceChannels {
                device_id,
                channels,
            } => {
                let labels = channels
                    .iter()
                    .map(|channel| (channel + 1).to_string())
                    .collect::<Vec<_>>()
                    .join("+");
                format!("{device_id} ch {labels}")
            }
            Self::MidiDevice { device_id } => device_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackOutputRouting {
    Main,
    Bus { bus_id: String },
    HardwareOutput { device_id: String, channel: u32 },
    None,
}

impl TrackOutputRouting {
    pub fn label(&self) -> String {
        match self {
            Self::Main => "Main".to_string(),
            Self::Bus { bus_id } => bus_id.clone(),
            Self::HardwareOutput { device_id, channel } => {
                format!("{device_id} ch {}", channel + 1)
            }
            Self::None => "None".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackAudioFormat {
    Mono,
    Stereo,
}

impl TrackAudioFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::Stereo => "Stereo",
        }
    }
}

fn input_route_matches_audio_format(
    input: &TrackInputRouting,
    audio_format: TrackAudioFormat,
) -> bool {
    match input {
        TrackInputRouting::None | TrackInputRouting::AllInputs => true,
        TrackInputRouting::AudioDeviceChannel { .. } => audio_format == TrackAudioFormat::Mono,
        TrackInputRouting::AudioDeviceChannels { channels, .. } => match audio_format {
            TrackAudioFormat::Mono => channels.len() == 1,
            TrackAudioFormat::Stereo => channels.len() == 2,
        },
        TrackInputRouting::MidiDevice { .. } => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackMidiInputRouting {
    None,
    AllInputs,
    MidiDevice { device_id: String },
}

impl TrackMidiInputRouting {
    pub fn label(&self) -> String {
        match self {
            Self::None => "None".to_string(),
            Self::AllInputs => "All MIDI Inputs".to_string(),
            Self::MidiDevice { device_id } => device_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRoutingState {
    pub input: TrackInputRouting,
    pub output: TrackOutputRouting,
    pub audio_format: TrackAudioFormat,
    pub midi_input: TrackMidiInputRouting,
    /// `None` means All channels. `Some` is clamped to 1..=16 by mutation
    /// helpers and project-load conversion.
    pub midi_channel: Option<u8>,
}

impl TrackRoutingState {
    pub fn for_track_type(track_type: TrackType) -> Self {
        match track_type {
            TrackType::Audio => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::Main,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::None,
                midi_channel: None,
            },
            TrackType::Instrument => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::Main,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::AllInputs,
                midi_channel: None,
            },
            TrackType::Midi => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::None,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::AllInputs,
                midi_channel: None,
            },
            TrackType::Bus | TrackType::Return => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::Main,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::None,
                midi_channel: None,
            },
            TrackType::Master => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::Main,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::None,
                midi_channel: None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackState {
    pub id: String,
    pub name: String,
    pub track_type: TrackType,
    pub color: gpui::Rgba,
    /// Manual/base normalized fader position in `0.0..=1.0`. `1.0` is the top of
    /// the fader (≈ +6 dB) and `0.0` is the bottom (≈ -60 dB). See
    /// `Volume::norm_to_db`. This is the value the user sets directly and the
    /// value persisted as `volume_norm`; Track Volume automation does NOT write
    /// here — it drives [`Self::volume_effective`] instead.
    pub volume: f32,
    /// Automation-evaluated effective volume at the current playhead. UI-only and
    /// not persisted — recomputed from the Track Volume automation lane on
    /// playback ticks, seeks, and point edits (see
    /// [`TimelineState::recompute_effective_volumes`]). Equals [`Self::volume`]
    /// whenever automation read is off or there is no active volume automation.
    pub volume_effective: f32,
    /// Whether Track Volume automation drives the effective volume / display.
    /// UI-only, not persisted; defaults to `true` so existing automated projects
    /// follow their curves on load.
    pub volume_automation_read: bool,
    /// Pan position in `-1.0..=1.0`. `-1.0` is hard left, `+1.0` is hard right.
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub armed: bool,
    /// Input monitoring mode (Off / Auto / Input).
    pub input_monitor: InputMonitorMode,
    /// Latest peak meter levels in `0.0..=1.0`. Currently a static placeholder
    /// per track; will be driven by the audio engine when that lands.
    pub meter_level_l: f32,
    pub meter_level_r: f32,
    /// Held peak levels (slow release) driving the peak-hold tick. UI-only.
    pub meter_peak_hold_l: f32,
    pub meter_peak_hold_r: f32,
    /// Latched clip indicator — set when the engine peak reached/exceeded
    /// 0 dBFS, auto-cleared once the held peak falls back. UI-only.
    pub meter_clip: bool,
    pub clips: Vec<ClipState>,
    pub automation_lanes: Vec<AutomationLaneState>,
    /// Per-track edit mode (Clip vs Automation). UI-only; not persisted.
    pub lane_mode: TrackLaneMode,
    /// Which automation target the lane editor is currently focused on. Drives
    /// which lane renders/edits while in [`TrackLaneMode::Automation`]. UI-only.
    pub selected_automation_target: Option<AutomationTarget>,
    /// Insert (effect) plugin chain — ordered. Audio flows through these
    /// in order before volume/pan/sends in the runtime. The UI stores
    /// only descriptor + transient state; the runtime owns the actual
    /// plugin processor.
    pub inserts: Vec<InsertSlotState>,
    /// Canonical MIDI destination for this instrument track — the
    /// `plugin_instance_id` of the first enabled instrument insert (e.g.
    /// `insert-track-1-1`). Set when a VSTi is assigned; used for piano
    /// preview, clip playback, and external-bridge routing.
    pub instrument_plugin_instance_id: Option<String>,
    /// Aux sends to Bus/Return tracks (Phase 3). Empty for most tracks.
    pub sends: Vec<SendSlotState>,
    /// Persisted routing choices. Device discovery is not wired yet, so device
    /// variants are preserved but not created by the Inspector.
    pub routing: TrackRoutingState,
}

impl TrackState {
    /// `true` when this track has an enabled Track Volume automation lane that
    /// actually carries points — i.e. automation can resolve a value.
    pub fn has_active_volume_automation(&self) -> bool {
        self.automation_lanes.iter().any(|l| {
            l.enabled && matches!(l.target, AutomationTarget::TrackVolume) && !l.points.is_empty()
        })
    }

    /// The normalized volume the UI fader / readout should display: the
    /// automation-evaluated effective value when automation read is active and a
    /// volume lane exists, otherwise the manual/base value. Faders still WRITE
    /// the base via [`TimelineState::set_track_volume`] — this is display only,
    /// so an automation-follow repaint can never be mistaken for a user edit.
    pub fn display_volume(&self) -> f32 {
        if self.volume_automation_read && self.has_active_volume_automation() {
            self.volume_effective
        } else {
            self.volume
        }
    }

    pub fn instrument_insert(&self) -> Option<&InsertSlotState> {
        if self.track_type == TrackType::Instrument {
            self.inserts.first()
        } else {
            None
        }
    }

    pub fn instrument_insert_mut(&mut self) -> Option<&mut InsertSlotState> {
        if self.track_type == TrackType::Instrument {
            self.inserts.first_mut()
        } else {
            None
        }
    }

    pub fn effect_inserts(&self) -> &[InsertSlotState] {
        if self.track_type == TrackType::Instrument {
            self.inserts.get(1..).unwrap_or(&[])
        } else {
            self.inserts.as_slice()
        }
    }

    pub fn effect_inserts_mut(&mut self) -> &mut [InsertSlotState] {
        if self.track_type == TrackType::Instrument {
            let start = self.inserts.len().min(1);
            &mut self.inserts[start..]
        } else {
            self.inserts.as_mut_slice()
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateTrackOptions {
    pub track_type: TrackType,
    pub name: String,
    pub color: gpui::Rgba,
    pub volume: f32,
    pub pan: f32,
    pub armed: bool,
    pub input_monitor: InputMonitorMode,
}

/// Volume / dB mapping helpers. Linear in dB between the soft floor and a
/// little headroom above unity.
pub mod volume {
    pub const MIN_DB: f32 = -60.0;
    pub const MAX_DB: f32 = 6.0;

    pub fn norm_to_db(norm: f32) -> f32 {
        let n = norm.clamp(0.0, 1.0);
        MIN_DB + n * (MAX_DB - MIN_DB)
    }

    pub fn db_to_norm(db: f32) -> f32 {
        ((db - MIN_DB) / (MAX_DB - MIN_DB)).clamp(0.0, 1.0)
    }

    pub fn format_db(norm: f32) -> String {
        let db = norm_to_db(norm);
        if norm <= 0.001 || db <= MIN_DB + 0.05 {
            "-∞".to_string()
        } else if db >= 0.0 {
            format!("+{:.1}", db)
        } else {
            format!("{:.1}", db)
        }
    }
}

/// Playback follow / auto-scroll behavior for the timeline viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoScrollMode {
    /// Never auto-scroll. User controls the viewport entirely.
    Off,
    /// When the playhead reaches the right edge, page forward by ~90% of
    /// the viewport width so the playhead lands near the left side again.
    /// Cheap, predictable, and friendly to low-end GPUs.
    Page,
    /// Keep the playhead at a roughly fixed fraction of the viewport while
    /// playing. Smoother, but more scroll churn — left as an opt-in.
    Continuous,
}

impl Default for AutoScrollMode {
    fn default() -> Self {
        AutoScrollMode::Page
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineTool {
    Pointer,
    Pen,
    Cut,
    Glue,
    Mute,
    Time,
    Automation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineViewport {
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub target_scroll_x: f32,
    pub target_scroll_y: f32,
    pub pixels_per_second: f32,
    pub pixels_per_beat: f32,
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub track_area_height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransportState {
    pub playing: bool,
    pub recording: bool,
    pub metronome_enabled: bool,
    pub playhead_beats: f32,
    pub loop_enabled: bool,
    pub loop_start_beats: f32,
    pub loop_end_beats: f32,
    pub last_engine_frame: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineSelection {
    pub selected_track_id: Option<String>,
    pub selected_clip_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRangeSelection {
    pub start_beat: f64,
    pub end_beat: f64,
    pub track_ids: Vec<String>,
}

impl TimelineRangeSelection {
    pub fn new(start_beat: f64, end_beat: f64, track_ids: Vec<String>) -> Self {
        let (start_beat, end_beat) = if start_beat <= end_beat {
            (start_beat, end_beat)
        } else {
            (end_beat, start_beat)
        };
        Self {
            start_beat,
            end_beat,
            track_ids,
        }
    }

    pub fn as_f32_range(&self) -> (f32, f32) {
        (self.start_beat as f32, self.end_beat as f32)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackLayout {
    pub track_ids: Vec<TrackId>,
    pub track_height: f32,
    pub scroll_y: f32,
}

impl TrackLayout {
    pub fn from_tracks(tracks: &[TrackState], scroll_y: f32) -> Self {
        Self {
            track_ids: tracks.iter().map(|track| track.id.clone()).collect(),
            track_height: TRACK_HEIGHT,
            scroll_y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapSettings {
    pub enabled: bool,
    pub division: SnapDivision,
    pub beats_per_bar: f64,
    pub auto_step_beats: f64,
}

impl SnapSettings {
    pub fn from_timeline(state: &TimelineState) -> Self {
        Self {
            enabled: state.snap_to_grid,
            division: state.grid_division,
            beats_per_bar: state.beats_per_bar() as f64,
            auto_step_beats: state.get_grid_sub_beats(state.pixels_per_beat()) as f64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapDivision {
    Auto,
    Off,
    Bar1,
    Div1_1,
    Div1_2,
    Div1_4,
    Div1_8,
    Div1_16,
    Div1_32,
    Div1_64,
}

impl SnapDivision {
    pub fn label(&self) -> &'static str {
        match self {
            SnapDivision::Auto => "Auto",
            SnapDivision::Off => "Off",
            SnapDivision::Bar1 => "1 bar",
            SnapDivision::Div1_1 => "1/1",
            SnapDivision::Div1_2 => "1/2",
            SnapDivision::Div1_4 => "1/4",
            SnapDivision::Div1_8 => "1/8",
            SnapDivision::Div1_16 => "1/16",
            SnapDivision::Div1_32 => "1/32",
            SnapDivision::Div1_64 => "1/64",
        }
    }

    pub fn step_beats(&self, bpb: f32) -> f32 {
        match self {
            SnapDivision::Auto => 0.0,
            SnapDivision::Off => 0.0,
            SnapDivision::Bar1 => bpb,
            SnapDivision::Div1_1 => 4.0,
            SnapDivision::Div1_2 => 2.0,
            SnapDivision::Div1_4 => 1.0,
            SnapDivision::Div1_8 => 0.5,
            SnapDivision::Div1_16 => 0.25,
            SnapDivision::Div1_32 => 0.125,
            SnapDivision::Div1_64 => 0.0625,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridLineLevel {
    Bar,
    Beat,
    Sub,
}

pub struct GridLine {
    pub x: f32,
    pub beat: f32,
    pub level: GridLineLevel,
    pub show_label: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MasterBusState {
    pub volume: f32,
    pub meter_level_l: f32,
    pub meter_level_r: f32,
    /// Held peak levels (slow release) for the master peak-hold tick. UI-only.
    pub meter_peak_hold_l: f32,
    pub meter_peak_hold_r: f32,
    /// Latched master clip indicator. UI-only.
    pub meter_clip: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineState {
    pub bpm: f32,
    /// Project-level tempo automation. Always active and owned by the project;
    /// the TempoTrack (when shown) is only a view/editor over this. When empty
    /// the project plays at the static `bpm`.
    pub tempo_map: TempoMap,
    /// Global time signature markers (authoritative for bar/beat layout).
    pub time_signature_map: TimeSignatureMap,
    /// Legacy single signature — kept in sync with the marker at beat 0 for
    /// templates and engine fallbacks.
    pub time_signature_num: u32,
    pub time_signature_den: u32,
    pub viewport: TimelineViewport,
    pub transport: TransportState,
    pub tracks: Vec<TrackState>,
    pub master: MasterBusState,
    pub selection: TimelineSelection,
    pub active_tool: TimelineTool,
    pub snap_to_grid: bool,
    pub grid_division: SnapDivision,
    pub dragging_track_id: Option<TrackId>,
    pub drag_origin_index: Option<usize>,
    pub drag_current_y: f32,
    pub drag_target_index: Option<usize>,
    /// True when the timeline viewport should follow the playhead during
    /// playback. Toggled off temporarily when the user manually scrolls or
    /// drags the viewport; can be re-enabled from the Follow button.
    pub follow_playhead: bool,
    pub auto_scroll_mode: AutoScrollMode,
    /// Arrangement time-range selection in beats. UI-only; never marks the
    /// project or engine dirty by itself.
    pub arrangement_range: Option<TimelineRangeSelection>,
    /// When true, the global Tempo Track lane is shown below the ruler.
    pub show_tempo_track: bool,
    /// Compact collapsed height for the Tempo Track lane header/curve.
    pub tempo_track_collapsed: bool,
    /// Selected tempo marker on the Tempo Track (stable persisted id).
    pub selected_tempo_point_id: Option<String>,
    pub show_time_signature_track: bool,
    pub time_signature_track_collapsed: bool,
    pub selected_time_signature_point_id: Option<String>,
}

impl Default for TimelineState {
    /// Clean, empty project. No tracks, no clips, no MIDI — the real runtime
    /// startup state. Use [`TimelineState::demo_project`] when you explicitly
    /// want the seeded demo content (development / screenshots).
    fn default() -> Self {
        Self {
            bpm: 120.0,
            tempo_map: TempoMap::new(),
            time_signature_map: TimeSignatureMap::with_default_4_4(),
            time_signature_num: 4,
            time_signature_den: 4,
            viewport: TimelineViewport {
                scroll_x: 0.0,
                scroll_y: 0.0,
                target_scroll_x: 0.0,
                target_scroll_y: 0.0,
                pixels_per_second: 150.0,
                pixels_per_beat: 75.0,
                viewport_width: 0.0,
                viewport_height: 500.0,
                track_area_height: 500.0,
            },
            transport: TransportState {
                playing: false,
                recording: false,
                metronome_enabled: false,
                playhead_beats: 0.0,
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 16.0,
                last_engine_frame: 0,
            },
            tracks: Vec::new(),
            master: MasterBusState {
                volume: volume::db_to_norm(0.0),
                meter_level_l: 0.0,
                meter_level_r: 0.0,
                meter_peak_hold_l: 0.0,
                meter_peak_hold_r: 0.0,
                meter_clip: false,
            },
            selection: TimelineSelection {
                selected_track_id: None,
                selected_clip_ids: Vec::new(),
            },
            active_tool: TimelineTool::Pointer,
            snap_to_grid: true,
            grid_division: SnapDivision::Div1_16,
            dragging_track_id: None,
            drag_origin_index: None,
            drag_current_y: 0.0,
            drag_target_index: None,
            follow_playhead: true,
            auto_scroll_mode: AutoScrollMode::Page,
            arrangement_range: None,
            show_tempo_track: false,
            tempo_track_collapsed: false,
            selected_tempo_point_id: None,
            show_time_signature_track: false,
            time_signature_track_collapsed: false,
            selected_time_signature_point_id: None,
        }
    }
}

impl TimelineState {
    /// Seeded demo project — three tracks with synthetic audio/MIDI clips.
    /// Intended for development, screenshots, and the demo seed flag in the
    /// app entry point; never used by the real runtime default.
    pub fn demo_project() -> Self {
        let track1 = TrackState {
            id: "track-1".to_string(),
            name: "Audio 1".to_string(),
            track_type: TrackType::Audio,
            color: crate::theme::Colors::track_color_for_index(0),
            volume: volume::db_to_norm(-3.0),
            volume_effective: volume::db_to_norm(-3.0),
            volume_automation_read: true,
            pan: 0.0,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: InputMonitorMode::Off,
            meter_level_l: 0.62,
            meter_level_r: 0.68,
            meter_peak_hold_l: 0.0,
            meter_peak_hold_r: 0.0,
            meter_clip: false,
            clips: vec![
                ClipState {
                    id: "clip-1".to_string(),
                    name: "vocals_dry.wav".to_string(),
                    start_beat: 1.0,
                    duration_beats: 8.0,
                    source_duration_seconds: None,
                    offset_beats: 0.0,
                    gain: 1.0,
                    clip_type: ClipType::Audio {
                        file_id: "file-vocals-dry".to_string(),
                        source_path: None,
                    },
                    muted: false,
                    audio_import: AudioImportState::Ready,
                },
                ClipState {
                    id: "clip-2".to_string(),
                    name: "vocals_harmony.wav".to_string(),
                    start_beat: 10.0,
                    duration_beats: 6.0,
                    source_duration_seconds: None,
                    offset_beats: 0.0,
                    gain: 1.0,
                    clip_type: ClipType::Audio {
                        file_id: "file-vocals-harmony".to_string(),
                        source_path: None,
                    },
                    muted: false,
                    audio_import: AudioImportState::Ready,
                },
            ],
            automation_lanes: vec![AutomationLaneState {
                id: "lane-1".to_string(),
                name: "Volume".to_string(),
                target: AutomationTarget::TrackVolume,
                enabled: true,
                visible: false,
                points: vec![
                    AutomationPoint::new(0.0, 0.8),
                    AutomationPoint::new(4.0, 0.5),
                    AutomationPoint::new(8.0, 0.8),
                ],
            }],
            lane_mode: TrackLaneMode::Clips,
            selected_automation_target: None,
            inserts: Vec::new(),
            sends: Vec::new(),
            routing: TrackRoutingState::for_track_type(TrackType::Audio),
            instrument_plugin_instance_id: None,
        };

        let track2 = TrackState {
            id: "track-2".to_string(),
            name: "Audio 2".to_string(),
            track_type: TrackType::Audio,
            color: crate::theme::Colors::track_color_for_index(1),
            volume: volume::db_to_norm(-6.0),
            volume_effective: volume::db_to_norm(-6.0),
            volume_automation_read: true,
            pan: -0.2,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: InputMonitorMode::Off,
            meter_level_l: 0.42,
            meter_level_r: 0.48,
            meter_peak_hold_l: 0.0,
            meter_peak_hold_r: 0.0,
            meter_clip: false,
            clips: vec![ClipState {
                id: "clip-3".to_string(),
                name: "drums_loop_120.wav".to_string(),
                start_beat: 0.0,
                duration_beats: 16.0,
                source_duration_seconds: None,
                offset_beats: 0.0,
                gain: 1.0,
                clip_type: ClipType::Audio {
                    file_id: "file-drums-loop".to_string(),
                    source_path: None,
                },
                muted: false,
                audio_import: AudioImportState::Ready,
            }],
            automation_lanes: vec![],
            lane_mode: TrackLaneMode::Clips,
            selected_automation_target: None,
            inserts: Vec::new(),
            sends: Vec::new(),
            routing: TrackRoutingState::for_track_type(TrackType::Audio),
            instrument_plugin_instance_id: None,
        };

        let track3 = TrackState {
            id: "track-3".to_string(),
            name: "Synth 3".to_string(),
            track_type: TrackType::Midi,
            color: crate::theme::Colors::track_color_for_index(2),
            volume: volume::db_to_norm(-1.5),
            volume_effective: volume::db_to_norm(-1.5),
            volume_automation_read: true,
            pan: 0.3,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: InputMonitorMode::Off,
            meter_level_l: 0.15,
            meter_level_r: 0.12,
            meter_peak_hold_l: 0.0,
            meter_peak_hold_r: 0.0,
            meter_clip: false,
            clips: vec![ClipState {
                id: "clip-4".to_string(),
                name: "synth_lead.mid".to_string(),
                start_beat: 4.0,
                duration_beats: 8.0,
                source_duration_seconds: None,
                offset_beats: 0.0,
                gain: 1.0,
                clip_type: ClipType::Midi {
                    notes: vec![
                        MidiNoteState::new(60, 0.0, 1.0, 100),
                        MidiNoteState::new(64, 1.0, 1.0, 100),
                        MidiNoteState::new(67, 2.0, 1.0, 100),
                        MidiNoteState::new(72, 3.0, 2.0, 110),
                        MidiNoteState::new(67, 5.0, 1.0, 90),
                        MidiNoteState::new(64, 6.0, 1.0, 90),
                        MidiNoteState::new(60, 7.0, 1.0, 80),
                    ],
                    controller_lanes: Vec::new(),
                },
                muted: false,
                audio_import: AudioImportState::default(),
            }],
            automation_lanes: vec![],
            lane_mode: TrackLaneMode::Clips,
            selected_automation_target: None,
            inserts: Vec::new(),
            sends: Vec::new(),
            routing: TrackRoutingState::for_track_type(TrackType::Midi),
            instrument_plugin_instance_id: None,
        };

        Self {
            bpm: 120.0,
            tempo_map: TempoMap::new(),
            time_signature_map: TimeSignatureMap::with_default_4_4(),
            time_signature_num: 4,
            time_signature_den: 4,
            viewport: TimelineViewport {
                scroll_x: 0.0,
                scroll_y: 0.0,
                target_scroll_x: 0.0,
                target_scroll_y: 0.0,
                pixels_per_second: 150.0, // Default zoom level in Web UI
                pixels_per_beat: 75.0,
                viewport_width: 0.0,
                viewport_height: 500.0,
                track_area_height: 500.0,
            },
            transport: TransportState {
                playing: false,
                recording: false,
                metronome_enabled: false,
                playhead_beats: 2.0,
                loop_enabled: true,
                loop_start_beats: 0.0,
                loop_end_beats: 16.0,
                last_engine_frame: 0,
            },
            tracks: vec![track1, track2, track3],
            master: MasterBusState {
                volume: volume::db_to_norm(0.0),
                meter_level_l: 0.0,
                meter_level_r: 0.0,
                meter_peak_hold_l: 0.0,
                meter_peak_hold_r: 0.0,
                meter_clip: false,
            },
            selection: TimelineSelection {
                selected_track_id: Some("track-1".to_string()),
                selected_clip_ids: vec![],
            },
            active_tool: TimelineTool::Pointer,
            snap_to_grid: true,
            grid_division: SnapDivision::Div1_16,
            dragging_track_id: None,
            drag_origin_index: None,
            drag_current_y: 0.0,
            drag_target_index: None,
            follow_playhead: true,
            auto_scroll_mode: AutoScrollMode::Page,
            arrangement_range: None,
            show_tempo_track: false,
            tempo_track_collapsed: false,
            selected_tempo_point_id: None,
            show_time_signature_track: false,
            time_signature_track_collapsed: false,
            selected_time_signature_point_id: None,
        }
    }

    /// Mutating variant of [`TimelineState::demo_project`]. Seeds the given
    /// state with demo content in-place.
    pub fn seed_demo_content(&mut self) {
        *self = Self::demo_project();
    }
}

// ── Time conversions and coordinate helpers ───────────────────────────────────────

pub const HEADER_WIDTH: f32 = 320.0; // Keep it slightly wider for native controls
pub const TRACK_HEIGHT: f32 = 76.0;
pub const RULER_HEIGHT: f32 = 30.0;
pub type TrackId = String;

pub fn beat_to_x(beat: f64, viewport: &TimelineViewport) -> f32 {
    ((beat.max(0.0) as f32) * viewport.pixels_per_beat - viewport.scroll_x).round()
}

pub fn x_to_beat(x: f32, viewport: &TimelineViewport) -> f64 {
    ((x + viewport.scroll_x) / viewport.pixels_per_beat.max(0.0001)).max(0.0) as f64
}

pub fn snap_beat(beat: f64, snap: SnapSettings) -> f64 {
    if !snap.enabled || snap.division == SnapDivision::Off {
        return beat.max(0.0);
    }
    let step = match snap.division {
        SnapDivision::Auto => snap.auto_step_beats,
        SnapDivision::Bar1 => snap.beats_per_bar,
        other => other.step_beats(snap.beats_per_bar as f32) as f64,
    };
    if step <= 0.0 {
        return beat.max(0.0);
    }
    ((beat / step).round() * step).max(0.0)
}

pub fn track_at_y(y: f32, layout: &TrackLayout) -> Option<TrackId> {
    if layout.track_height <= 0.0 {
        return None;
    }
    let index = ((y + layout.scroll_y).max(0.0) / layout.track_height).floor() as usize;
    layout.track_ids.get(index).cloned()
}

pub fn clip_rect(
    clip: &ClipState,
    viewport: &TimelineViewport,
    layout: &TrackLayout,
) -> gpui::Bounds<gpui::Pixels> {
    let x = beat_to_x(clip.start_beat as f64, viewport);
    let w =
        ((clip.duration_beats.max(0.0) as f64 * viewport.pixels_per_beat as f64) as f32).max(1.0);
    let y = -layout.scroll_y;
    gpui::bounds(
        gpui::point(gpui::px(x), gpui::px(y)),
        gpui::size(gpui::px(w), gpui::px(layout.track_height)),
    )
}

// ── Automation coordinate + evaluation helpers ───────────────────────────────

/// Map a normalized automation value (`0.0..=1.0`) to a local y within a lane
/// of `lane_height` px. Top of the usable area is `value = 1.0`. Respects
/// [`AUTOMATION_LANE_PAD`] top/bottom so the extremes never hug the border.
pub fn automation_value_to_y(value: f32, lane_height: f32) -> f32 {
    let usable = (lane_height - 2.0 * AUTOMATION_LANE_PAD).max(1.0);
    AUTOMATION_LANE_PAD + (1.0 - value.clamp(0.0, 1.0)) * usable
}

/// Inverse of [`automation_value_to_y`]: local y in a lane back to a normalized
/// value (clamped to `0.0..=1.0`).
pub fn automation_y_to_value(y: f32, lane_height: f32) -> f32 {
    let usable = (lane_height - 2.0 * AUTOMATION_LANE_PAD).max(1.0);
    (1.0 - (y - AUTOMATION_LANE_PAD) / usable).clamp(0.0, 1.0)
}

/// Evaluate an automation curve at `beat`. `points` must be sorted ascending by
/// beat. With no points the `default` value is returned; before the first point
/// the first point's value is held; after the last point the last value is
/// held. Between points the leading point's [`AutomationCurve`] decides the
/// shape (`Hold` steps, everything else interpolates linearly — `Smooth` is a
/// TODO and currently behaves as `Linear`).
pub fn evaluate_automation(points: &[AutomationPoint], beat: f64, default: f32) -> f32 {
    if points.is_empty() {
        return default;
    }
    let beat = beat as f32;
    if beat <= points[0].beat {
        return points[0].value;
    }
    let last = points.len() - 1;
    if beat >= points[last].beat {
        return points[last].value;
    }
    // Find the segment [a, b] containing `beat`.
    for i in 0..last {
        let a = &points[i];
        let b = &points[i + 1];
        if beat >= a.beat && beat <= b.beat {
            return match a.curve {
                AutomationCurve::Hold => a.value,
                // Linear + Smooth (Smooth TODO) interpolate linearly for now.
                _ => {
                    let span = (b.beat - a.beat).max(1.0e-6);
                    let t = ((beat - a.beat) / span).clamp(0.0, 1.0);
                    a.value + (b.value - a.value) * t
                }
            };
        }
    }
    points[last].value
}

impl TimelineState {
    pub fn seconds_per_beat(&self) -> f32 {
        60.0 / self.bpm.max(1.0)
    }

    pub fn seconds_to_beats(&self, seconds: f64) -> f32 {
        (seconds * self.bpm.max(1.0) as f64 / 60.0) as f32
    }

    pub fn beats_to_seconds(&self, beats: f32) -> f32 {
        beats * self.seconds_per_beat()
    }

    pub fn pixels_per_beat(&self) -> f32 {
        self.viewport.pixels_per_second * self.seconds_per_beat()
    }

    fn sync_pixels_per_beat(&mut self) {
        self.viewport.pixels_per_beat = self.pixels_per_beat();
    }

    pub fn beats_per_bar(&self) -> f32 {
        self.beats_per_bar_at_beat(self.transport.playhead_beats as f64) as f32
    }

    pub fn beats_per_bar_at_beat(&self, beat: f64) -> f64 {
        self.time_signature_map.beats_per_bar_at_beat(beat)
    }

    pub fn time_signature_at_playhead(&self) -> TimeSignaturePoint {
        self.time_signature_map
            .time_signature_at_beat(self.transport.playhead_beats as f64)
    }

    pub fn time_signature_has_markers(&self) -> bool {
        self.time_signature_map.points.len() > 1
            || self
                .time_signature_map
                .points
                .first()
                .is_some_and(|p| p.beat > TS_BEAT_EPSILON)
    }

    pub fn sync_legacy_time_signature_fields(&mut self) {
        let pt = self
            .time_signature_map
            .time_signature_at_beat(0.0);
        self.time_signature_num = pt.numerator as u32;
        self.time_signature_den = pt.denominator as u32;
    }

    pub const TIME_SIGNATURE_TRACK_HEIGHT: f32 = 48.0;
    pub const TIME_SIGNATURE_TRACK_HEIGHT_COLLAPSED: f32 = 36.0;

    pub fn time_signature_track_height(&self) -> f32 {
        if !self.show_time_signature_track {
            return 0.0;
        }
        if self.time_signature_track_collapsed {
            Self::TIME_SIGNATURE_TRACK_HEIGHT_COLLAPSED
        } else {
            Self::TIME_SIGNATURE_TRACK_HEIGHT
        }
    }

    pub fn global_lanes_height(&self) -> f32 {
        self.tempo_track_height() + self.time_signature_track_height()
    }

    pub fn show_time_signature_track_lane(&mut self) {
        self.show_time_signature_track = true;
        self.time_signature_map.ensure_default_point();
    }

    pub fn hide_time_signature_track_lane(&mut self) {
        self.show_time_signature_track = false;
        self.selected_time_signature_point_id = None;
    }

    pub fn select_time_signature_point(&mut self, id: &str) {
        self.selected_time_signature_point_id = Some(id.to_string());
    }

    pub fn add_time_signature_point(
        &mut self,
        beat: f64,
        numerator: u16,
        denominator: u16,
    ) -> Option<String> {
        self.time_signature_map
            .add_or_update_point(beat, numerator, denominator);
        self.time_signature_map.ensure_point_ids();
        self.sync_legacy_time_signature_fields();
        self.time_signature_map
            .points
            .iter()
            .find(|p| (p.beat - beat).abs() < TS_BEAT_EPSILON)
            .map(|p| p.id.clone())
    }

    pub fn move_time_signature_point(&mut self, id: &str, beat: f64) -> bool {
        if self.time_signature_map.move_point_by_id(id, beat) {
            self.sync_legacy_time_signature_fields();
            true
        } else {
            false
        }
    }

    pub fn update_time_signature_point(
        &mut self,
        id: &str,
        numerator: u16,
        denominator: u16,
    ) -> bool {
        if self
            .time_signature_map
            .update_point_by_id(id, numerator, denominator)
        {
            self.sync_legacy_time_signature_fields();
            true
        } else {
            false
        }
    }

    pub fn delete_time_signature_point(&mut self, id: &str) -> bool {
        if self.time_signature_map.remove_point_by_id(id) {
            if self.selected_time_signature_point_id.as_deref() == Some(id) {
                self.selected_time_signature_point_id = None;
            }
            self.time_signature_map.ensure_default_point();
            self.sync_legacy_time_signature_fields();
            true
        } else {
            false
        }
    }

    pub fn clear_time_signature_markers(&mut self, playhead_beat: f64) {
        let pt = self
            .time_signature_map
            .time_signature_at_beat(playhead_beat);
        self.time_signature_map.reset_to_single_point(
            0.0,
            pt.numerator,
            pt.denominator,
        );
        self.sync_legacy_time_signature_fields();
        self.selected_time_signature_point_id = None;
    }

    pub fn time_signature_point_at(&self, beat: f64, beat_tol: f64) -> Option<String> {
        self.time_signature_map
            .points
            .iter()
            .find(|p| (p.beat - beat).abs() <= beat_tol)
            .map(|p| p.id.clone())
    }

    pub fn time_to_content_x(&self, time_sec: f32) -> f32 {
        (time_sec * self.viewport.pixels_per_second - self.viewport.scroll_x).round()
    }

    pub fn content_x_to_time(&self, x: f32) -> f32 {
        ((x + self.viewport.scroll_x) / self.viewport.pixels_per_second).max(0.0)
    }

    pub fn beats_to_x(&self, beats: f32) -> f32 {
        beat_to_x(beats as f64, &self.viewport)
    }

    /// Effective BPM at a given beat, honoring tempo automation. Falls back to
    /// the static `bpm` when the tempo map has no markers.
    pub fn effective_bpm_at_beat(&self, beat: f64) -> f64 {
        self.tempo_map.bpm_at_beat(beat, self.bpm as f64)
    }

    /// Effective BPM at the current playhead position.
    pub fn effective_bpm_at_playhead(&self) -> f64 {
        self.effective_bpm_at_beat(self.transport.playhead_beats as f64)
    }

    /// Whether tempo automation is active (one or more markers present).
    pub fn tempo_has_automation(&self) -> bool {
        self.tempo_map.has_automation()
    }

    /// Height of the global Tempo Track lane when visible, else 0.
    pub fn tempo_track_height(&self) -> f32 {
        if !self.show_tempo_track {
            return 0.0;
        }
        if self.tempo_track_collapsed {
            TEMPO_TRACK_HEIGHT_COLLAPSED
        } else {
            TEMPO_TRACK_HEIGHT
        }
    }

    /// Y offset from the timeline top to the track-list content area.
    pub fn arrangement_content_top(&self) -> f32 {
        RULER_HEIGHT + self.global_lanes_height()
    }

    /// Visible global/system lanes (Tempo then Time Signature when shown).
    pub fn visible_global_lanes(&self) -> Vec<GlobalLaneKind> {
        let mut lanes = Vec::new();
        if self.show_tempo_track {
            lanes.push(GlobalLaneKind::Tempo);
        }
        if self.show_time_signature_track {
            lanes.push(GlobalLaneKind::TimeSignature);
        }
        lanes
    }

    /// Secondary label for the Tempo lane header (fixed BPM or automation range).
    pub fn tempo_lane_header_subtitle(&self) -> String {
        let bpm = self.effective_bpm_at_playhead();
        if self.tempo_map.points.len() <= 1 {
            if bpm.fract().abs() < 0.05 {
                format!("Fixed {:.0} BPM", bpm)
            } else {
                format!("Fixed {:.1} BPM", bpm)
            }
        } else {
            let mut min = bpm;
            let mut max = bpm;
            for p in &self.tempo_map.points {
                min = min.min(p.bpm);
                max = max.max(p.bpm);
            }
            if (max - min).abs() < 0.5 {
                if bpm.fract().abs() < 0.05 {
                    format!("{:.0} BPM", bpm)
                } else {
                    format!("{:.1} BPM", bpm)
                }
            } else {
                format!("{:.0}–{:.0} BPM", min.round(), max.round())
            }
        }
    }

    /// Secondary label for the Time Signature lane header.
    pub fn time_signature_lane_header_subtitle(&self) -> String {
        let pt = self.time_signature_at_playhead();
        if !self.time_signature_has_markers() {
            format!("Fixed {}", pt.label())
        } else {
            let count = self.time_signature_map.points.len();
            if count > 1 {
                format!("{} · {} markers", pt.label(), count)
            } else {
                pt.label()
            }
        }
    }

    /// Scroll/zoom the arrangement so all tempo automation points are visible.
    pub fn fit_tempo_automation_in_view(&mut self) {
        if self.tempo_map.points.is_empty() {
            return;
        }
        let min_beat = self
            .tempo_map
            .points
            .iter()
            .map(|p| p.beat)
            .fold(f64::INFINITY, f64::min)
            .max(0.0);
        let max_beat = self
            .tempo_map
            .points
            .iter()
            .map(|p| p.beat)
            .fold(0.0, f64::max);
        let pad = 8.0;
        let span_beats = (max_beat - min_beat + pad * 2.0).max(16.0);
        let width = self.viewport.viewport_width.max(200.0);
        let needed_ppb = width / span_beats as f32;
        let current_ppb = self.pixels_per_beat().max(0.0001);
        if needed_ppb < current_ppb {
            let factor = (needed_ppb / current_ppb).clamp(0.05, 1.0);
            self.zoom_by(factor, width * 0.5);
        }
        let scroll = ((min_beat - pad).max(0.0) as f32 * self.pixels_per_beat()).max(0.0);
        self.viewport.scroll_x = scroll;
        self.viewport.target_scroll_x = scroll;
    }

    /// Auto-fit BPM range for the Tempo Track curve with padding.
    pub fn tempo_lane_bpm_range(&self) -> (f64, f64) {
        let mut min = self.bpm as f64;
        let mut max = self.bpm as f64;
        for p in &self.tempo_map.points {
            min = min.min(p.bpm);
            max = max.max(p.bpm);
        }
        let pad = ((max - min) * 0.15).max(10.0);
        let mut min_bpm = (min - pad).clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX);
        let mut max_bpm = (max + pad).clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX);
        if (max_bpm - min_bpm) < 20.0 {
            let mid = (min_bpm + max_bpm) * 0.5;
            min_bpm = (mid - 10.0).max(TEMPO_BPM_MIN);
            max_bpm = (mid + 10.0).min(TEMPO_BPM_MAX);
        }
        (min_bpm, max_bpm)
    }

    /// Show the Tempo Track lane and ensure at least one anchor point exists.
    pub fn show_tempo_track_lane(&mut self) {
        self.show_tempo_track = true;
        self.ensure_tempo_anchor_point();
    }

    pub fn hide_tempo_track_lane(&mut self) {
        self.show_tempo_track = false;
        self.selected_tempo_point_id = None;
    }

    /// Seed beat-0 marker when the map is empty so the lane always has data.
    pub fn ensure_tempo_anchor_point(&mut self) {
        if self.tempo_map.points.is_empty() {
            let bpm = self.bpm as f64;
            self.tempo_map
                .add_or_update_point(0.0, bpm, TempoCurve::Hold);
        }
        self.tempo_map.ensure_point_ids();
    }

    pub fn select_tempo_point(&mut self, id: &str) {
        self.selected_tempo_point_id = Some(id.to_string());
    }

    pub fn clear_time_signature_point_selection(&mut self) {
        self.selected_time_signature_point_id = None;
    }

    pub fn clear_tempo_point_selection(&mut self) {
        self.selected_tempo_point_id = None;
    }

    pub fn tempo_point_at(
        &self,
        beat: f64,
        bpm: f64,
        beat_tol: f64,
        bpm_tol: f64,
    ) -> Option<String> {
        self.tempo_map
            .points
            .iter()
            .find(|p| (p.beat - beat).abs() <= beat_tol && (p.bpm - bpm).abs() <= bpm_tol)
            .map(|p| p.id.clone())
    }

    pub fn add_tempo_point(&mut self, beat: f64, bpm: f64) -> Option<String> {
        let beat = beat.max(0.0);
        let bpm = bpm.clamp(TEMPO_BPM_MIN, TEMPO_BPM_MAX);
        if self.tempo_map.points.is_empty() && beat > TEMPO_BEAT_EPSILON {
            self.tempo_map
                .add_or_update_point(0.0, self.bpm as f64, TempoCurve::Hold);
        }
        self.tempo_map
            .add_or_update_point(beat, bpm, TempoCurve::Hold);
        self.tempo_map.ensure_point_ids();
        self.tempo_map
            .points
            .iter()
            .find(|p| (p.beat - beat).abs() < TEMPO_BEAT_EPSILON)
            .map(|p| p.id.clone())
    }

    pub fn move_tempo_point(&mut self, id: &str, beat: f64, bpm: f64) -> bool {
        self.tempo_map.move_point_by_id(id, beat, bpm)
    }

    pub fn delete_tempo_point(&mut self, id: &str) -> bool {
        if self.tempo_map.remove_point_by_id(id) {
            if self.selected_tempo_point_id.as_deref() == Some(id) {
                self.selected_tempo_point_id = None;
            }
            true
        } else {
            false
        }
    }

    pub fn set_tempo_point_curve(&mut self, id: &str, curve: TempoCurve) -> bool {
        self.tempo_map.update_point_curve_by_id(id, curve)
    }

    pub fn set_fixed_tempo_from_beat(&mut self, beat: f64, bpm: f64) {
        let base = self.bpm as f64;
        self.tempo_map.set_fixed_from_beat(beat, bpm, base);
        self.tempo_map.ensure_point_ids();
    }

    /// BPM values rendered as tempo-track point handles (for tests/debug).
    pub fn tempo_track_render_bpm_values(&self) -> Vec<f64> {
        if self.tempo_map.points.is_empty() {
            vec![self.bpm as f64]
        } else {
            self.tempo_map.points.iter().map(|p| p.bpm).collect()
        }
    }

    /// Effective BPM across the visible beat range (flat line check for tests).
    pub fn tempo_track_bpm_samples(&self, viewport_width: f32) -> Vec<f64> {
        let (start, end) = self.visible_beat_range(viewport_width);
        let cols = viewport_width.ceil().max(1.0) as usize;
        (0..=cols)
            .map(|col| {
                let beat = start as f64 + (end - start) as f64 * (col as f64 / cols as f64);
                self.effective_bpm_at_beat(beat)
            })
            .collect()
    }

    pub fn x_to_beats(&self, x: f32) -> f32 {
        x_to_beat(x, &self.viewport) as f32
    }

    pub fn beat_to_x(&self, beat: f32) -> f32 {
        self.beats_to_x(beat)
    }

    pub fn x_to_beat(&self, x: f32) -> f64 {
        x_to_beat(x, &self.viewport)
    }

    pub fn lane_y_to_track_id(&self, y: f32) -> Option<TrackId> {
        track_at_y(
            y,
            &TrackLayout::from_tracks(&self.tracks, self.viewport.scroll_y),
        )
    }

    pub fn update_viewport_size(&mut self, width: f32, height: f32) {
        self.viewport.viewport_width = width.max(0.0);
        self.viewport.viewport_height = height.max(0.0);
        self.viewport.track_area_height = height.max(0.0);
        self.sync_pixels_per_beat();
    }

    pub fn get_visible_beat_range(&self, width: f32) -> (f32, f32) {
        let start = self.x_to_beats(0.0);
        let end = self.x_to_beats(width);
        (start, end)
    }

    pub fn visible_beat_range(&self, viewport_width: f32) -> (f32, f32) {
        self.get_visible_beat_range(viewport_width)
    }

    pub fn build_interval_list(&self) -> Vec<f32> {
        let bpb = self.beats_per_bar();
        let mut result = Vec::new();
        for &sub in &[
            1.0 / 32.0,
            1.0 / 16.0,
            1.0 / 8.0,
            1.0 / 4.0,
            1.0 / 2.0,
            1.0,
            2.0,
        ] {
            if sub < bpb {
                result.push(sub);
            }
        }
        for &mult in &[1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0] {
            result.push(bpb * mult);
        }
        result
    }

    pub fn get_grid_interval_beats(&self, ppb: f32) -> f32 {
        let min_beats = 100.0 / ppb.max(1.0);
        let intervals = self.build_interval_list();
        for &n in &intervals {
            if n >= min_beats {
                return n;
            }
        }
        *intervals.last().unwrap_or(&4.0)
    }

    pub fn get_grid_sub_beats(&self, ppb: f32) -> f32 {
        let _bpb = self.beats_per_bar();
        let interval = self.get_grid_interval_beats(ppb);
        let intervals = self.build_interval_list();
        if let Some(idx) = intervals.iter().position(|&x| x == interval) {
            if idx > 0 {
                return intervals[idx - 1];
            }
        }
        interval
    }

    pub fn snap_time(&self, seconds: f32) -> f32 {
        if !self.snap_to_grid || self.grid_division == SnapDivision::Off {
            return seconds;
        }
        let ppb = self.viewport.pixels_per_second * self.seconds_per_beat();
        let bpb = self.beats_per_bar();
        let sub_div = match self.grid_division {
            SnapDivision::Auto => self.get_grid_sub_beats(ppb),
            SnapDivision::Bar1 => bpb,
            _ => self.grid_division.step_beats(bpb),
        };
        if sub_div <= 0.0 {
            return seconds;
        }
        let spb = self.seconds_per_beat();
        let total_beats = seconds / spb;
        let snapped = (total_beats / sub_div).round() * sub_div;
        (snapped * spb).max(0.0)
    }

    pub fn get_arrangement_grid_lines(&self, viewport_width: f32) -> Vec<GridLine> {
        let power = crate::perf::power_mode();
        const MIN_LINE_SPACING_PX_BASE: f32 = 8.0;
        // Force sub lines further apart on low-end so we draw fewer of them.
        let min_line_spacing_px = if matches!(power, crate::perf::PowerMode::LowEnd) {
            16.0
        } else {
            MIN_LINE_SPACING_PX_BASE
        };
        const MIN_LABEL_SPACING_PX: f32 = 46.0;
        const MAX_GRID_LINES_BASE: usize = 1200;
        let max_grid_lines = (MAX_GRID_LINES_BASE as f32 * power.grid_line_budget_scale()) as usize;

        let ppb = self.pixels_per_beat().max(0.0001);
        let viewport_width = viewport_width.max(1.0);
        let (start_beat, end_beat) = self.visible_beat_range(viewport_width);
        let start_beat = start_beat.max(0.0);
        let end_beat = end_beat.max(start_beat);
        let max_bpb = self.beats_per_bar_at_beat(end_beat as f64).max(1.0) as f32;

        let mut lines: Vec<GridLine> = Vec::new();
        let mut occupied_x: Vec<i32> = Vec::new();

        let mut add_line = |beat: f32, level: GridLineLevel| {
            if beat < start_beat - max_bpb || beat > end_beat + max_bpb {
                return;
            }
            let rb = (beat * 100000.0).round() / 100000.0;
            let x = self.beat_to_x(rb).round();
            let x_key = x as i32;
            if x < -1.0 || x > viewport_width + 1.0 {
                return;
            }
            if occupied_x
                .iter()
                .any(|existing| (x_key - *existing).abs() < 1)
            {
                return;
            }
            occupied_x.push(x_key);
            lines.push(GridLine {
                x,
                beat: rb,
                level,
                show_label: false,
            });
        };

        // Bar + denominator-beat lines follow time-signature segments.
        let ts_points = if self.time_signature_map.points.is_empty() {
            vec![TimeSignaturePoint::with_id("implicit-4-4", 0.0, 4, 4)]
        } else {
            self.time_signature_map.points.clone()
        };
        for (i, pt) in ts_points.iter().enumerate() {
            let seg_start = pt.beat as f32;
            let seg_end = ts_points
                .get(i + 1)
                .map(|p| p.beat as f32)
                .unwrap_or(f32::INFINITY);
            let bpb = beats_per_bar_from_sig(pt.numerator, pt.denominator) as f32;
            let denom_unit = denominator_unit_quarter_beats(pt.denominator) as f32;
            if seg_end < start_beat {
                continue;
            }
            let rel_start = start_beat.max(seg_start);
            let first_bar = ((rel_start - seg_start) / bpb).floor() - 1.0;
            let rel_end = end_beat.min(seg_end);
            let last_bar = ((rel_end - seg_start) / bpb).ceil() + 1.0;
            let mut bar = first_bar;
            while bar <= last_bar {
                let bar_start = seg_start + bar * bpb;
                if bar_start >= seg_start - TS_BEAT_EPSILON as f32
                    && bar_start < seg_end - TS_BEAT_EPSILON as f32
                {
                    add_line(bar_start, GridLineLevel::Bar);
                    for beat_idx in 1..pt.numerator {
                        let tick = bar_start + beat_idx as f32 * denom_unit;
                        if tick < seg_end - TS_BEAT_EPSILON as f32 {
                            add_line(tick, GridLineLevel::Beat);
                        }
                    }
                }
                bar += 1.0;
            }
        }

        let sub_step = if !power.allow_sub_grid_lines() {
            None
        } else if ppb >= 96.0 {
            Some(0.25_f32)
        } else if ppb >= 48.0 {
            Some(0.5_f32)
        } else {
            None
        };

        if let Some(step) = sub_step.filter(|step| step * ppb >= min_line_spacing_px) {
            let first_sub = (start_beat / step).floor() - 1.0;
            let last_sub = (end_beat / step).ceil() + 1.0;
            let mut slot = first_sub;
            while slot <= last_sub {
                let beat = slot * step;
                let denom_unit = denominator_unit_quarter_beats(
                    self.time_signature_map
                        .time_signature_at_beat(beat as f64)
                        .denominator,
                ) as f32;
                let on_denom_grid = if denom_unit > TS_BEAT_EPSILON as f32 {
                    ((beat / denom_unit).fract()).abs() < 1e-4
                        || ((beat / denom_unit).fract() - 1.0).abs() < 1e-4
                } else {
                    false
                };
                if !on_denom_grid {
                    add_line(beat, GridLineLevel::Sub);
                }
                slot += 1.0;
            }
        }

        lines.sort_by(|a, b| a.x.total_cmp(&b.x));

        if lines.len() > max_grid_lines {
            lines.truncate(max_grid_lines);
        }

        let mut last_label_x = f32::NEG_INFINITY;
        let mut ruler_labels = 0u64;
        for line in &mut lines {
            let denom_unit = denominator_unit_quarter_beats(
                self.time_signature_map
                    .time_signature_at_beat(line.beat as f64)
                    .denominator,
            ) as f32;
            let can_label_level = match line.level {
                GridLineLevel::Bar => true,
                GridLineLevel::Beat => denom_unit * ppb >= 24.0,
                GridLineLevel::Sub => false,
            };
            if can_label_level && line.x - last_label_x >= MIN_LABEL_SPACING_PX {
                line.show_label = true;
                last_label_x = line.x;
                ruler_labels += 1;
            }
        }

        if crate::perf::enabled() {
            let major = lines
                .iter()
                .filter(|l| matches!(l.level, GridLineLevel::Bar))
                .count() as u64;
            let minor = lines.len() as u64 - major;
            crate::perf::count("visible_major_lines", major);
            crate::perf::count("visible_minor_lines", minor);
            crate::perf::count("ruler_labels_drawn", ruler_labels);
        }

        lines
    }

    pub fn format_bar_beat(&self, beats: f32) -> String {
        self.format_bar_beat_at(beats as f64)
    }

    pub fn format_bar_beat_at(&self, beats: f64) -> String {
        let bb = self.time_signature_map.bar_beat_at_beat(beats);
        format!("{}.{}", bb.bar, bb.beat_in_bar)
    }

    // ── Identity helpers ─────────────────────────────────────────────────────

    pub fn next_track_id(&self) -> String {
        // Find the highest numeric suffix on "track-N" ids, plus one.
        let mut n = 0u32;
        for t in &self.tracks {
            if let Some(rest) = t.id.strip_prefix("track-") {
                if let Ok(v) = rest.parse::<u32>() {
                    if v > n {
                        n = v;
                    }
                }
            }
        }
        format!("track-{}", n + 1)
    }

    pub fn next_clip_id(&self) -> String {
        let mut n = 0u32;
        for t in &self.tracks {
            for c in &t.clips {
                if let Some(rest) = c.id.strip_prefix("clip-") {
                    if let Ok(v) = rest.parse::<u32>() {
                        if v > n {
                            n = v;
                        }
                    }
                }
            }
        }
        format!("clip-{}", n + 1)
    }

    pub fn track_index_at_y(&self, y: f32) -> Option<usize> {
        if y < 0.0 {
            return None;
        }
        let idx = ((y + self.viewport.scroll_y) / TRACK_HEIGHT).floor() as usize;
        if idx < self.tracks.len() {
            Some(idx)
        } else {
            None
        }
    }

    pub fn track_insert_index_at_y(&self, y: f32) -> usize {
        if self.tracks.is_empty() {
            return 0;
        }
        let content_y = (y + self.viewport.scroll_y).max(0.0);
        ((content_y / TRACK_HEIGHT).round() as usize).clamp(0, self.tracks.len())
    }

    pub fn begin_track_drag(&mut self, track_id: &str, origin_index: usize, y: f32) {
        self.dragging_track_id = Some(track_id.to_string());
        self.drag_origin_index = Some(origin_index);
        self.drag_current_y = y;
        self.drag_target_index = Some(origin_index.min(self.tracks.len()));
    }

    pub fn update_track_drag(&mut self, y: f32) {
        self.drag_current_y = y;
        self.drag_target_index = Some(self.track_insert_index_at_y(y));
    }

    pub fn clear_track_drag(&mut self) {
        self.dragging_track_id = None;
        self.drag_origin_index = None;
        self.drag_current_y = 0.0;
        self.drag_target_index = None;
    }

    pub fn reorder_track(&mut self, track_id: &str, target_index: usize) -> bool {
        let Some(origin_index) = self.tracks.iter().position(|track| track.id == track_id) else {
            self.clear_track_drag();
            return false;
        };
        let target_index = target_index.clamp(0, self.tracks.len());
        let insert_index = if origin_index < target_index {
            target_index.saturating_sub(1)
        } else {
            target_index
        };
        if insert_index == origin_index {
            self.clear_track_drag();
            return false;
        }

        let track = self.tracks.remove(origin_index);
        let insert_index = insert_index.min(self.tracks.len());
        self.tracks.insert(insert_index, track);
        if let Some(selected) = self.selection.selected_track_id.as_deref() {
            if !self.tracks.iter().any(|track| track.id == selected) {
                self.selection.selected_track_id =
                    self.tracks.get(insert_index).map(|t| t.id.clone());
            }
        }
        self.clear_track_drag();
        true
    }

    /// Snap a beat value to the current grid (or return it unchanged when snap is off).
    pub fn snap_beats(&self, beats: f32) -> f32 {
        let mut snap = SnapSettings::from_timeline(self);
        snap.beats_per_bar = self.beats_per_bar_at_beat(beats as f64);
        snap_beat(beats as f64, snap) as f32
    }

    pub fn selected_range_track_ids(&self) -> Vec<String> {
        self.selection
            .selected_track_id
            .iter()
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn track_ids_between(&self, a: &str, b: &str) -> Vec<String> {
        let Some(a_index) = self.tracks.iter().position(|track| track.id == a) else {
            return Vec::new();
        };
        let Some(b_index) = self.tracks.iter().position(|track| track.id == b) else {
            return vec![a.to_string()];
        };
        let (lo, hi) = if a_index <= b_index {
            (a_index, b_index)
        } else {
            (b_index, a_index)
        };
        self.tracks[lo..=hi]
            .iter()
            .map(|track| track.id.clone())
            .collect()
    }

    /// Create a new audio track with auto-assigned id/color.
    pub fn create_audio_track(&mut self) -> String {
        let name = format!("Audio {}", self.tracks.len() + 1);
        let log_name = name.clone();
        let id = self.create_track(CreateTrackOptions {
            track_type: TrackType::Audio,
            name,
            color: self.track_color_for_index(self.tracks.len()),
            volume: volume::db_to_norm(0.0),
            pan: 0.0,
            armed: false,
            input_monitor: InputMonitorMode::Off,
        });
        eprintln!("[import] created track id={} name={}", id, log_name);
        id
    }

    pub fn create_midi_track(&mut self) -> String {
        let name = format!("MIDI {}", self.tracks.len() + 1);
        self.create_track(CreateTrackOptions {
            track_type: TrackType::Midi,
            name,
            color: self.track_color_for_index(self.tracks.len()),
            volume: volume::db_to_norm(0.0),
            pan: 0.0,
            armed: false,
            input_monitor: InputMonitorMode::Off,
        })
    }

    // ── MIDI clip / note mutations ────────────────────────────────────────
    // Single source of truth for piano-roll edits. The piano-roll editor calls
    // these inside `Timeline::update` and then marks the project dirty so the
    // engine sync + autosave see the change. Notes are stored relative to the
    // clip start (matches the WebUI model). Every mutation clamps to valid
    // ranges so a bad gesture can never produce an out-of-range note.

    /// Grid step in beats for snapping clip bounds (matches arrangement snap).
    pub fn midi_snap_step_beats(&self) -> f32 {
        let bpb = self.beats_per_bar();
        if !self.snap_to_grid || self.grid_division == SnapDivision::Off {
            return 0.25;
        }
        match self.grid_division {
            SnapDivision::Auto => {
                self.get_grid_sub_beats(self.viewport.pixels_per_second * self.seconds_per_beat())
            }
            SnapDivision::Bar1 => bpb,
            other => other.step_beats(bpb),
        }
        .max(1.0 / 32.0)
    }

    fn next_midi_clip_display_name(&self) -> String {
        let mut count = 0u32;
        for track in &self.tracks {
            for clip in &track.clips {
                if matches!(clip.clip_type, ClipType::Midi { .. }) {
                    count += 1;
                }
            }
        }
        format!("MIDI {}", count + 1)
    }

    /// Expand `clip.duration_beats` so every note fits inside the clip, with
    /// optional grid padding. Does not shrink. Returns `true` if length changed.
    pub fn ensure_midi_clip_contains_notes(clip: &mut ClipState, snap_beats: f32) -> bool {
        let ClipType::Midi { notes, .. } = &clip.clip_type else {
            return false;
        };
        let max_note_end = notes
            .iter()
            .map(|n| n.start.max(0.0) + n.duration.max(MIN_NOTE_BEATS))
            .fold(0.0f32, f32::max);
        let min_len = DEFAULT_MIDI_CLIP_BEATS.max(MIN_MIDI_CLIP_BEATS);
        let needed =
            snap_up_beats(max_note_end.max(min_len), snap_beats.max(1.0 / 32.0)).max(min_len);
        if needed > clip.duration_beats + 1.0e-4 {
            let old = clip.duration_beats;
            clip.duration_beats = needed;
            if midi_debug_enabled() {
                eprintln!(
                    "[midi] clip auto-expanded clip={} old_len={:.3} new_len={:.3} notes={}",
                    clip.id,
                    old,
                    needed,
                    notes.len()
                );
            }
            return true;
        }
        false
    }

    /// Expand a MIDI clip's length so it contains all of its notes, snapping the
    /// new length up to the current grid. Never shrinks — note deletes leave the
    /// clip length untouched (expansion is sticky). Returns `true` if the length
    /// grew. This is the single auto-expand entry point for note edits.
    pub fn expand_clip_to_contain_notes(&mut self, clip_id: &str) -> bool {
        let step = self.midi_snap_step_beats();
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if clip.id == clip_id {
                    return Self::ensure_midi_clip_contains_notes(clip, step);
                }
            }
        }
        false
    }

    /// Create an empty MIDI clip on `track_id` at `start_beat` (snapped by the
    /// caller if desired). Returns the new clip id, or `None` if the track is
    /// missing. The clip is selected so the editor can pick it up immediately.
    pub fn create_midi_clip(
        &mut self,
        track_id: &str,
        start_beat: f32,
        length_beats: f32,
    ) -> Option<String> {
        let clip = self.build_midi_clip(track_id, start_beat, length_beats)?;
        let clip_id = clip.id.clone();
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.clips.push(clip);
        }
        self.selection.selected_track_id = Some(track_id.to_string());
        self.selection.selected_clip_ids = vec![clip_id.clone()];
        if crate::forensic_trace::midi_model_trace_enabled() {
            eprintln!(
                "[midi-model] clip_created track={track_id} clip={clip_id} \
                 start_beats={start_beat:.3} length_beats={length_beats:.3}"
            );
        }
        Some(clip_id)
    }

    /// Borrow the notes of a MIDI clip by id.
    pub fn midi_clip_notes(&self, clip_id: &str) -> Option<&Vec<MidiNoteState>> {
        for track in &self.tracks {
            for clip in &track.clips {
                if clip.id == clip_id {
                    if let ClipType::Midi { notes, .. } = &clip.clip_type {
                        return Some(notes);
                    }
                }
            }
        }
        None
    }

    pub(crate) fn midi_clip_notes_mut(&mut self, clip_id: &str) -> Option<&mut Vec<MidiNoteState>> {
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if clip.id == clip_id {
                    if let ClipType::Midi { notes, .. } = &mut clip.clip_type {
                        return Some(notes);
                    }
                }
            }
        }
        None
    }

    /// Length of a clip in beats, if it exists.
    pub fn clip_duration_beats(&self, clip_id: &str) -> Option<f32> {
        for track in &self.tracks {
            if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                return Some(clip.duration_beats);
            }
        }
        None
    }

    /// Clamp a note start/duration so it fits inside `clip_len`. Returns `None`
    /// when the note would lie entirely outside the clip.
    pub fn clamp_note_to_clip_bounds(
        start: f32,
        duration: f32,
        clip_len: f32,
    ) -> Option<(f32, f32)> {
        let start = start.max(0.0);
        if start >= clip_len {
            return None;
        }
        let max_dur = (clip_len - start).max(MIN_NOTE_BEATS);
        let duration = duration.max(MIN_NOTE_BEATS).min(max_dur);
        if start + duration > clip_len + 1.0e-4 {
            return None;
        }
        Some((start, duration))
    }

    /// Clips intersecting a beat range on any track.
    pub fn clips_intersecting_beats(&self, start: f32, end: f32) -> Vec<String> {
        let (lo, hi) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let mut ids = Vec::new();
        for track in &self.tracks {
            for clip in &track.clips {
                let clip_end = clip.start_beat + clip.duration_beats;
                if clip.start_beat < hi && clip_end > lo {
                    ids.push(clip.id.clone());
                }
            }
        }
        ids
    }

    /// Create a MIDI clip, returning the full clip state for undo commands.
    pub fn build_midi_clip(
        &mut self,
        track_id: &str,
        start_beat: f32,
        length_beats: f32,
    ) -> Option<ClipState> {
        if !self.tracks.iter().any(|t| t.id == track_id) {
            return None;
        }
        let clip_id = self.next_clip_id();
        let name = self.next_midi_clip_display_name();
        let len = length_beats.max(MIN_MIDI_CLIP_BEATS);
        Some(ClipState {
            id: clip_id,
            name,
            start_beat: start_beat.max(0.0),
            duration_beats: len,
            source_duration_seconds: None,
            offset_beats: 0.0,
            gain: 1.0,
            clip_type: ClipType::Midi {
                notes: Vec::new(),
                controller_lanes: Vec::new(),
            },
            muted: false,
            audio_import: AudioImportState::default(),
        })
    }

    /// Add a note to a MIDI clip. Returns the new note id.
    pub fn add_midi_note(
        &mut self,
        clip_id: &str,
        pitch: u8,
        start: f32,
        duration: f32,
        velocity: u8,
    ) -> Option<u64> {
        let clip_len = self.clip_duration_beats(clip_id)?;
        let (start, duration) = Self::clamp_note_to_clip_bounds(start, duration, clip_len)?;
        let note = MidiNoteState::new(pitch, start, duration, velocity);
        let id = note.id;
        let notes = self.midi_clip_notes_mut(clip_id)?;
        notes.push(note);
        if crate::forensic_trace::midi_model_trace_enabled() {
            eprintln!(
                "[midi-model] note_added clip={clip_id} pitch={} start_beats={start:.3} \
                 length_beats={duration:.3} velocity={}",
                pitch.min(127),
                velocity.clamp(1, 127)
            );
        }
        Some(id)
    }

    /// Apply absolute start/pitch to a set of notes (move gesture). Each tuple
    /// is `(note_id, new_start_beats, new_pitch)`.
    pub fn move_midi_notes(&mut self, clip_id: &str, updates: &[(u64, f32, u8)]) {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return;
        };
        // Notes keep their duration and may pass the current clip end; the clip
        // is auto-expanded below so a moved note never lives outside its clip.
        // Start clamps to clip-local beat 0; pitch clamps to 0..=127.
        for (id, new_start, new_pitch) in updates {
            if let Some(note) = notes.iter_mut().find(|n| n.id == *id) {
                note.start = new_start.max(0.0);
                note.pitch = (*new_pitch).min(127);
            }
        }
        self.expand_clip_to_contain_notes(clip_id);
        if midi_debug_enabled() {
            eprintln!("[midi] move_notes clip={} count={}", clip_id, updates.len());
        }
    }

    /// Set a note's length (resize gesture), clamped to [`MIN_NOTE_BEATS`].
    pub fn resize_midi_note(&mut self, clip_id: &str, id: u64, new_duration: f32) {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return;
        };
        // Right-edge resize may grow the note past the clip end; the clip is
        // auto-expanded below rather than the note being clamped to fit.
        if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
            note.duration = new_duration.max(MIN_NOTE_BEATS);
            if midi_debug_enabled() {
                eprintln!(
                    "[midi] resize_note clip={} id={} dur={:.3}",
                    clip_id, id, note.duration
                );
            }
        }
        self.expand_clip_to_contain_notes(clip_id);
    }

    /// Delete the given note ids from a MIDI clip. Returns how many were removed.
    pub fn delete_midi_notes(&mut self, clip_id: &str, ids: &[u64]) -> usize {
        if ids.is_empty() {
            return 0;
        }
        let ids: std::collections::HashSet<u64> = ids.iter().copied().collect();
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return 0;
        };
        let before = notes.len();
        notes.retain(|n| !ids.contains(&n.id));
        let removed = before - notes.len();
        if removed > 0 && midi_debug_enabled() {
            eprintln!("[midi] delete_notes clip={} removed={}", clip_id, removed);
        }
        removed
    }

    /// Set a note's velocity (1..=127).
    pub fn set_midi_note_velocity(&mut self, clip_id: &str, id: u64, velocity: u8) {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return;
        };
        if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
            note.velocity = velocity.clamp(1, 127);
        }
    }

    /// Set a note's pitch (0..=127). Returns true when the note changed.
    pub fn set_midi_note_pitch(&mut self, clip_id: &str, id: u64, pitch: u8) -> bool {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return false;
        };
        let Some(note) = notes.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        let pitch = pitch.min(127);
        if note.pitch == pitch {
            return false;
        }
        note.pitch = pitch;
        true
    }

    /// Set a note's start in clip-local beats. Returns true when the note changed.
    pub fn set_midi_note_start(&mut self, clip_id: &str, id: u64, start: f32) -> bool {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return false;
        };
        let Some(note) = notes.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        let start = start.max(0.0);
        if (note.start - start).abs() <= 1.0e-4 {
            return false;
        }
        note.start = start;
        self.expand_clip_to_contain_notes(clip_id);
        true
    }

    /// Set a note's length in beats. Returns true when the note changed.
    pub fn set_midi_note_length(&mut self, clip_id: &str, id: u64, duration: f32) -> bool {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return false;
        };
        let Some(note) = notes.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        let duration = duration.max(MIN_NOTE_BEATS);
        if (note.duration - duration).abs() <= 1.0e-4 {
            return false;
        }
        note.duration = duration;
        self.expand_clip_to_contain_notes(clip_id);
        true
    }

    /// Set velocity for selected notes. Returns the number of notes changed.
    pub fn set_midi_notes_velocity_bulk(
        &mut self,
        clip_id: &str,
        ids: &[u64],
        velocity: u8,
    ) -> usize {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return 0;
        };
        let velocity = velocity.clamp(1, 127);
        let mut changed = 0;
        for note in notes.iter_mut() {
            if ids.contains(&note.id) && note.velocity != velocity {
                note.velocity = velocity;
                changed += 1;
            }
        }
        changed
    }

    /// Overwrite the mutable fields of existing notes from full snapshots,
    /// matched by id. Used by the `EditMidiNotes` undo command — the note set is
    /// not changed, only field values. Auto-expands the clip afterwards.
    pub fn overwrite_midi_notes(&mut self, clip_id: &str, states: &[MidiNoteState]) {
        if let Some(notes) = self.midi_clip_notes_mut(clip_id) {
            for s in states {
                if let Some(note) = notes.iter_mut().find(|n| n.id == s.id) {
                    note.pitch = s.pitch;
                    note.start = s.start;
                    note.duration = s.duration;
                    note.velocity = s.velocity;
                    note.muted = s.muted;
                }
            }
        }
        self.expand_clip_to_contain_notes(clip_id);
    }

    // ── MIDI controller lanes ─────────────────────────────────────────────
    pub fn midi_clip_controller_lanes(&self, clip_id: &str) -> Option<&Vec<MidiControllerLane>> {
        for track in &self.tracks {
            for clip in &track.clips {
                if clip.id == clip_id {
                    if let ClipType::Midi {
                        controller_lanes, ..
                    } = &clip.clip_type
                    {
                        return Some(controller_lanes);
                    }
                }
            }
        }
        None
    }

    fn controller_lanes_mut(&mut self, clip_id: &str) -> Option<&mut Vec<MidiControllerLane>> {
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if clip.id == clip_id {
                    if let ClipType::Midi {
                        controller_lanes, ..
                    } = &mut clip.clip_type
                    {
                        return Some(controller_lanes);
                    }
                }
            }
        }
        None
    }

    /// Points of a specific controller lane, if the lane exists.
    pub fn controller_lane_points(
        &self,
        clip_id: &str,
        kind: MidiControllerKind,
    ) -> Option<&Vec<MidiControllerPoint>> {
        self.midi_clip_controller_lanes(clip_id)?
            .iter()
            .find(|l| l.kind == kind)
            .map(|l| &l.points)
    }

    /// Clone of a lane's points (for undo prev/next snapshots).
    pub fn controller_points_snapshot(
        &self,
        clip_id: &str,
        kind: MidiControllerKind,
    ) -> Vec<MidiControllerPoint> {
        self.controller_lane_points(clip_id, kind)
            .cloned()
            .unwrap_or_default()
    }

    /// Ensure a visible lane of `kind` exists. Returns true if newly created.
    pub fn ensure_controller_lane(&mut self, clip_id: &str, kind: MidiControllerKind) -> bool {
        let Some(lanes) = self.controller_lanes_mut(clip_id) else {
            return false;
        };
        if lanes.iter().any(|l| l.kind == kind) {
            return false;
        }
        lanes.push(MidiControllerLane {
            kind,
            points: Vec::new(),
            visible: true,
            height: 80.0,
            collapsed: false,
        });
        true
    }

    /// Remove a controller lane only when it has no points. This backs the MIDI
    /// editor's safe "Remove lane" action and prevents accidental data loss.
    pub fn remove_empty_controller_lane(
        &mut self,
        clip_id: &str,
        kind: MidiControllerKind,
    ) -> bool {
        let Some(lanes) = self.controller_lanes_mut(clip_id) else {
            return false;
        };
        let Some(index) = lanes
            .iter()
            .position(|lane| lane.kind == kind && lane.points.is_empty())
        else {
            return false;
        };
        lanes.remove(index);
        true
    }

    fn sort_lane_points(points: &mut [MidiControllerPoint]) {
        points.sort_by(|a, b| {
            a.beat
                .partial_cmp(&b.beat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Overwrite a lane's points wholesale (used by undo). Creates the lane if
    /// missing so undo can restore points into a removed lane.
    pub fn set_controller_lane_points(
        &mut self,
        clip_id: &str,
        kind: MidiControllerKind,
        mut points: Vec<MidiControllerPoint>,
    ) {
        self.ensure_controller_lane(clip_id, kind);
        Self::sort_lane_points(&mut points);
        if let Some(lanes) = self.controller_lanes_mut(clip_id) {
            if let Some(lane) = lanes.iter_mut().find(|l| l.kind == kind) {
                lane.points = points;
            }
        }
    }

    /// Add or update a point at `beat` (merging within ~1e-3 beats). `value`
    /// clamps to `0.0..=1.0`. Creates the lane if missing.
    pub fn put_controller_point(
        &mut self,
        clip_id: &str,
        kind: MidiControllerKind,
        beat: f32,
        value: f32,
    ) {
        self.ensure_controller_lane(clip_id, kind);
        let beat = beat.max(0.0);
        let value = value.clamp(0.0, 1.0);
        if let Some(lanes) = self.controller_lanes_mut(clip_id) {
            if let Some(lane) = lanes.iter_mut().find(|l| l.kind == kind) {
                if let Some(p) = lane
                    .points
                    .iter_mut()
                    .find(|p| (p.beat - beat).abs() < 1.0e-3)
                {
                    p.value = value;
                } else {
                    lane.points.push(MidiControllerPoint::new(beat, value));
                    Self::sort_lane_points(&mut lane.points);
                }
            }
        }
    }

    /// Move an existing point (by id) to a new beat/value, re-sorting the lane.
    /// `beat` clamps to `>= 0`, `value` to `0.0..=1.0`. Returns true if found.
    pub fn set_controller_point(
        &mut self,
        clip_id: &str,
        kind: MidiControllerKind,
        id: u64,
        beat: f32,
        value: f32,
    ) -> bool {
        if let Some(lanes) = self.controller_lanes_mut(clip_id) {
            if let Some(lane) = lanes.iter_mut().find(|l| l.kind == kind) {
                if let Some(p) = lane.points.iter_mut().find(|p| p.id == id) {
                    p.beat = beat.max(0.0);
                    p.value = value.clamp(0.0, 1.0);
                    Self::sort_lane_points(&mut lane.points);
                    return true;
                }
            }
        }
        false
    }

    /// Delete points within `tol` beats of `beat`. Returns how many were removed.
    pub fn delete_controller_points_near(
        &mut self,
        clip_id: &str,
        kind: MidiControllerKind,
        beat: f32,
        tol: f32,
    ) -> usize {
        if let Some(lanes) = self.controller_lanes_mut(clip_id) {
            if let Some(lane) = lanes.iter_mut().find(|l| l.kind == kind) {
                let before = lane.points.len();
                lane.points.retain(|p| (p.beat - beat).abs() > tol);
                return before - lane.points.len();
            }
        }
        0
    }

    /// Set the muted flag on the given note ids. Returns the number changed.
    pub fn set_midi_notes_muted(&mut self, clip_id: &str, ids: &[u64], muted: bool) -> usize {
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return 0;
        };
        let mut changed = 0;
        for note in notes.iter_mut() {
            if ids.contains(&note.id) && note.muted != muted {
                note.muted = muted;
                changed += 1;
            }
        }
        changed
    }

    /// Transpose selected notes by semitones. Returns the number of notes changed.
    pub fn transpose_midi_notes(&mut self, clip_id: &str, ids: &[u64], semitones: i32) -> usize {
        if semitones == 0 {
            return 0;
        }
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return 0;
        };
        let mut changed = 0;
        for note in notes.iter_mut() {
            if ids.contains(&note.id) {
                let pitch = (note.pitch as i32 + semitones).clamp(0, 127) as u8;
                if note.pitch != pitch {
                    note.pitch = pitch;
                    changed += 1;
                }
            }
        }
        changed
    }

    /// Quantize the given note starts (or all notes when `ids` is empty) to the
    /// supplied grid step in beats. Rounds to the nearest step.
    pub fn quantize_midi_notes(&mut self, clip_id: &str, ids: &[u64], step_beats: f32) {
        if step_beats <= 0.0 {
            return;
        }
        let Some(notes) = self.midi_clip_notes_mut(clip_id) else {
            return;
        };
        let mut count = 0;
        for note in notes.iter_mut() {
            if ids.is_empty() || ids.contains(&note.id) {
                note.start = ((note.start / step_beats).round() * step_beats).max(0.0);
                count += 1;
            }
        }
        if midi_debug_enabled() {
            eprintln!(
                "[midi] quantize clip={} count={} step={:.4}",
                clip_id, count, step_beats
            );
        }
    }

    pub fn track_color_for_index(&self, index: usize) -> gpui::Rgba {
        crate::theme::Colors::track_color_for_index(index)
    }

    pub fn create_track(&mut self, options: CreateTrackOptions) -> String {
        let id = self.next_track_id();
        let track_type = options.track_type;
        self.tracks.push(TrackState {
            id: id.clone(),
            name: options.name,
            track_type,
            color: options.color,
            volume: options.volume.clamp(0.0, 1.0),
            volume_effective: options.volume.clamp(0.0, 1.0),
            volume_automation_read: true,
            pan: options.pan.clamp(-1.0, 1.0),
            muted: false,
            solo: false,
            armed: options.armed,
            input_monitor: options.input_monitor,
            meter_level_l: 0.0,
            meter_level_r: 0.0,
            meter_peak_hold_l: 0.0,
            meter_peak_hold_r: 0.0,
            meter_clip: false,
            clips: Vec::new(),
            automation_lanes: Vec::new(),
            lane_mode: TrackLaneMode::Clips,
            selected_automation_target: None,
            inserts: Vec::new(),
            sends: Vec::new(),
            routing: TrackRoutingState::for_track_type(track_type),
            instrument_plugin_instance_id: None,
        });
        id
    }

    pub fn selected_audio_track_id(&self) -> Option<String> {
        let selected = self.selection.selected_track_id.as_deref()?;
        self.tracks
            .iter()
            .find(|track| track.id == selected && matches!(track.track_type, TrackType::Audio))
            .map(|track| track.id.clone())
    }

    // ── Single-source-of-truth mutations ─────────────────────────────────────
    // These are the only paths that should mutate per-track UI state. Both the
    // timeline TrackHeader and the bottom-panel Mixer call into these, so the
    // two views can never drift apart.

    pub fn set_master_volume(&mut self, norm: f32) {
        self.master.volume = norm.clamp(0.0, 1.0);
    }

    /// Set a track's manual/base fader volume (the `UserFader` path). When
    /// automation read is off — or there is no active volume automation — the
    /// effective volume follows the base immediately so the display and runtime
    /// track the fader. When automation read is on with an active lane, base is
    /// updated underneath but effective stays automation-driven (DAW behavior).
    pub fn set_track_volume(&mut self, track_id: &str, norm: f32) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            let v = norm.clamp(0.0, 1.0);
            t.volume = v;
            if !(t.volume_automation_read && t.has_active_volume_automation()) {
                t.volume_effective = v;
            }
            if automation_sync_debug_enabled() {
                eprintln!(
                    "[automation-sync] target=TrackVolume({}) base={:.3}({}) effective={:.3} reason=fader_drag",
                    t.id,
                    v,
                    volume::format_db(v),
                    t.volume_effective,
                );
            }
        }
    }

    /// Toggle whether Track Volume automation drives this track's effective
    /// value. Returns `true` if the flag changed. The caller should follow with
    /// [`Self::recompute_effective_volumes`] at the current playhead so the
    /// fader/inspector preview updates immediately.
    pub fn set_track_volume_automation_read(&mut self, track_id: &str, read: bool) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.volume_automation_read != read {
                t.volume_automation_read = read;
                if !read {
                    t.volume_effective = t.volume;
                }
                return true;
            }
        }
        false
    }

    /// Recompute every track's effective volume from its Track Volume automation
    /// lane at `beat`. UI-only: faders/inspector read [`TrackState::display_volume`]
    /// which prefers the effective value. Returns `true` if any effective value
    /// changed (so the caller can `notify`). `reason` is only used for the
    /// `[automation-sync]` trace and should be one of `playback_tick`, `seek`,
    /// or `point_edit`.
    pub fn recompute_effective_volumes(&mut self, beat: f32, reason: &str) -> bool {
        let debug = automation_sync_debug_enabled();
        let mut changed = false;
        for track in &mut self.tracks {
            let resolved = track
                .automation_lanes
                .iter()
                .find(|l| {
                    l.enabled
                        && matches!(l.target, AutomationTarget::TrackVolume)
                        && !l.points.is_empty()
                })
                .map(|l| evaluate_automation(&l.points, beat as f64, l.target.default_value()));
            let new_effective = match (track.volume_automation_read, resolved) {
                (true, Some(v)) => v,
                _ => track.volume,
            };
            if (track.volume_effective - new_effective).abs() > 1.0e-5 {
                if debug {
                    eprintln!(
                        "[automation-sync] target=TrackVolume({}) beat={:.3} value={:.3}({}) base={:.3}({}) effective {:.3}→{:.3} reason={}",
                        track.id,
                        beat,
                        new_effective,
                        volume::format_db(new_effective),
                        track.volume,
                        volume::format_db(track.volume),
                        track.volume_effective,
                        new_effective,
                        reason,
                    );
                }
                track.volume_effective = new_effective;
                changed = true;
            }
        }
        changed
    }

    /// Rename a track. Trims surrounding whitespace and ignores an
    /// all-whitespace name (keeps the previous one). Returns `true` if the
    /// stored name actually changed, so callers only mark dirty on a real edit.
    pub fn rename_track(&mut self, track_id: &str, name: &str) -> bool {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return false;
        }
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.name != trimmed {
                t.name = trimmed.to_string();
                return true;
            }
        }
        false
    }

    /// Set a track's color. Returns `true` if it changed.
    pub fn set_track_color(&mut self, track_id: &str, color: gpui::Rgba) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.color != color {
                t.color = color;
                return true;
            }
        }
        false
    }

    pub fn set_track_pan(&mut self, track_id: &str, pan: f32) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.pan = pan.clamp(-1.0, 1.0);
        }
    }

    pub fn set_track_input_routing(&mut self, track_id: &str, input: TrackInputRouting) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if !input_route_matches_audio_format(&input, t.routing.audio_format) {
                return false;
            }
            if t.routing.input != input {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] input track={} old={:?} new={:?}",
                        track_id, t.routing.input, input
                    );
                }
                t.routing.input = input;
                return true;
            }
        }
        false
    }

    pub fn set_track_output_routing(&mut self, track_id: &str, output: TrackOutputRouting) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.routing.output != output {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] output track={} old={:?} new={:?}",
                        track_id, t.routing.output, output
                    );
                }
                t.routing.output = output;
                return true;
            }
        }
        false
    }

    pub fn set_track_audio_format(
        &mut self,
        track_id: &str,
        audio_format: TrackAudioFormat,
    ) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.routing.audio_format != audio_format {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] audio_format track={} old={:?} new={:?}",
                        track_id, t.routing.audio_format, audio_format
                    );
                }
                t.routing.audio_format = audio_format;
                if !input_route_matches_audio_format(&t.routing.input, audio_format) {
                    t.routing.input = TrackInputRouting::None;
                }
                return true;
            }
        }
        false
    }

    pub fn set_track_midi_input(
        &mut self,
        track_id: &str,
        midi_input: TrackMidiInputRouting,
    ) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.routing.midi_input != midi_input {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] midi_input track={} old={:?} new={:?}",
                        track_id, t.routing.midi_input, midi_input
                    );
                }
                t.routing.midi_input = midi_input;
                return true;
            }
        }
        false
    }

    pub fn set_track_midi_channel(&mut self, track_id: &str, channel: Option<u8>) -> bool {
        let channel = channel.map(|ch| ch.clamp(1, 16));
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.routing.midi_channel != channel {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] midi_channel track={} old={:?} new={:?}",
                        track_id, t.routing.midi_channel, channel
                    );
                }
                t.routing.midi_channel = channel;
                return true;
            }
        }
        false
    }

    pub fn toggle_track_mute(&mut self, track_id: &str) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.muted = !t.muted;
            return true;
        }
        false
    }

    pub fn toggle_track_solo(&mut self, track_id: &str) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.solo = !t.solo;
            return true;
        }
        false
    }

    pub fn toggle_track_arm(&mut self, track_id: &str) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.armed = !t.armed;
            return true;
        }
        false
    }

    /// Append an empty insert slot to a track and return the slot id.
    /// Phase 1 — purely UI state; runtime is updated on the next project
    /// sync (the engine ignores unknown plugin descriptors gracefully).
    pub fn add_insert(&mut self, track_id: &str) -> Option<String> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let slot_id = Self::next_insert_slot_id(track);
        let slot = InsertSlotState::empty(&slot_id);
        if plugin_debug_enabled() {
            eprintln!("[plugin] add_insert track={} slot_id={}", track_id, slot_id);
        }
        track.inserts.push(slot);
        crate::forensic_trace::log_trace_plugin(track_id, &slot_id);
        Some(slot_id)
    }

    pub fn ensure_insert_slot_at(&mut self, track_id: &str, slot_index: usize) -> Option<String> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        while track.inserts.len() <= slot_index {
            let slot_id = Self::next_insert_slot_id(track);
            if plugin_debug_enabled() {
                eprintln!("[plugin] add_insert track={} slot_id={}", track_id, slot_id);
            }
            track.inserts.push(InsertSlotState::empty(&slot_id));
        }
        track.inserts.get(slot_index).map(|slot| slot.id.clone())
    }

    fn next_insert_slot_id(track: &TrackState) -> String {
        let mut suffix = track.inserts.len() + 1;
        loop {
            let candidate = format!("insert-{}-{}", track.id, suffix);
            if track.inserts.iter().all(|slot| slot.id != candidate) {
                return candidate;
            }
            suffix += 1;
        }
    }

    /// Assign a plugin to an insert slot. The caller resolves the
    /// `plugin_id` → display metadata before calling so the UI doesn't
    /// have to know about the plugin registry directly.
    pub fn set_insert_plugin(
        &mut self,
        track_id: &str,
        insert_id: &str,
        plugin_id: String,
        plugin_path: Option<std::path::PathBuf>,
        plugin_format: InsertPluginFormat,
        display_name: String,
    ) {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        let is_instrument_slot =
            matches!(track.track_type, TrackType::Instrument | TrackType::Midi)
                && track
                    .inserts
                    .first()
                    .map(|first| first.id == insert_id)
                    .unwrap_or(false);
        let Some(slot) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return;
        };
        slot.plugin_id = Some(plugin_id);
        slot.plugin_path = plugin_path;
        slot.plugin_format = Some(plugin_format);
        slot.display_name = display_name;
        slot.load_status = InsertLoadStatus::Ready;
        slot.runtime_backend = PluginRuntimeBackend::InProcess;
        slot.runtime_state = PluginRuntimeState::Ready;
        slot.host_pid = None;
        slot.bypassed = false;
        slot.parameters.clear();
        crate::forensic_trace::log_trace_plugin(track_id, insert_id);
        if is_instrument_slot {
            track.instrument_plugin_instance_id = Some(insert_id.to_string());
            eprintln!("[instrument-route] track={track_id} instrument_instance={insert_id}");
            eprintln!("[instrument-route] plugin_instance_id={insert_id}");
            eprintln!("[instrument-route] route_ok=true");
        }
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] set_insert_plugin track={} slot={} -> {}",
                track_id, insert_id, slot.display_name
            );
        }
    }

    pub fn set_insert_runtime(
        &mut self,
        track_id: &str,
        insert_id: &str,
        backend: PluginRuntimeBackend,
        state: PluginRuntimeState,
        host_pid: Option<u32>,
    ) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return false;
        };
        let Some(slot) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return false;
        };
        let status = match &state {
            PluginRuntimeState::Loading | PluginRuntimeState::EditorOpening => {
                InsertLoadStatus::Loading
            }
            PluginRuntimeState::Ready | PluginRuntimeState::EditorOpen => InsertLoadStatus::Ready,
            PluginRuntimeState::Failed(message) => InsertLoadStatus::Failed(message.clone()),
            PluginRuntimeState::Crashed => {
                InsertLoadStatus::Failed("Plugin host crashed".to_string())
            }
            PluginRuntimeState::Unloaded => InsertLoadStatus::Disabled,
        };
        let changed = slot.runtime_backend != backend
            || slot.runtime_state != state
            || slot.host_pid != host_pid
            || slot.load_status != status;
        slot.runtime_backend = backend;
        slot.runtime_state = state;
        slot.host_pid = host_pid;
        slot.load_status = status;
        changed
    }

    pub fn remove_insert(&mut self, track_id: &str, insert_id: &str) {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return;
        };
        track.inserts.retain(|i| i.id != insert_id);
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] remove_insert track={} slot={}",
                track_id, insert_id
            );
        }
    }

    /// Move an insert slot one position earlier (`up = true`) or later within
    /// the track's chain. Returns `true` if the order changed. Reordering the
    /// `Vec` is sufficient for the engine — the next project sync carries the
    /// new chain order down to the runtime.
    pub fn move_insert(&mut self, track_id: &str, insert_id: &str, up: bool) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return false;
        };
        let Some(idx) = track.inserts.iter().position(|i| i.id == insert_id) else {
            return false;
        };
        let target = if up {
            if idx == 0 {
                return false;
            }
            idx - 1
        } else {
            if idx + 1 >= track.inserts.len() {
                return false;
            }
            idx + 1
        };
        track.inserts.swap(idx, target);
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] move_insert track={} slot={} {}",
                track_id,
                insert_id,
                if up { "up" } else { "down" }
            );
        }
        true
    }

    pub fn toggle_insert_bypass(&mut self, track_id: &str, insert_id: &str) -> Option<bool> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let slot = track.inserts.iter_mut().find(|i| i.id == insert_id)?;
        slot.bypassed = !slot.bypassed;
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] toggle_bypass track={} slot={} -> {}",
                track_id, insert_id, slot.bypassed
            );
        }
        Some(slot.bypassed)
    }

    pub fn toggle_insert_enabled(&mut self, track_id: &str, insert_id: &str) -> Option<bool> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let slot = track.inserts.iter_mut().find(|i| i.id == insert_id)?;
        slot.enabled = !slot.enabled;
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] toggle_enabled track={} slot={} -> {}",
                track_id, insert_id, slot.enabled
            );
        }
        Some(slot.enabled)
    }

    /// Set an insert slot's load status by id (Phase 2b engine readback).
    /// Returns `true` if the status actually changed, so callers can decide
    /// whether to repaint. Used by the audio sync completion handler to flip
    /// `Failed` when the engine reports a native plugin failed to instantiate.
    pub fn set_insert_load_status(
        &mut self,
        track_id: &str,
        insert_id: &str,
        status: InsertLoadStatus,
    ) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return false;
        };
        let Some(slot) = track.inserts.iter_mut().find(|i| i.id == insert_id) else {
            return false;
        };
        if slot.load_status == status {
            return false;
        }
        if plugin_debug_enabled() {
            eprintln!(
                "[plugin] set_load_status track={} slot={} -> {:?}",
                track_id, insert_id, status
            );
        }
        slot.load_status = status;
        true
    }

    /// Add an aux send from `track_id` to the first Bus/Return track that
    /// isn't already a target (Phase 3 — a richer target picker is a follow-up,
    /// mirroring how inserts auto-seeded before the picker overlay). Returns
    /// the new send id, or `None` if there is no eligible routing track or the
    /// track already sends to every routing track.
    pub fn add_send(&mut self, track_id: &str) -> Option<String> {
        let existing: Vec<String> = self
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| t.sends.iter().map(|s| s.target_track_id.clone()).collect())
            .unwrap_or_default();
        let target = self
            .tracks
            .iter()
            .find(|t| t.id != track_id && t.track_type.is_routing() && !existing.contains(&t.id))?;
        let target_id = target.id.clone();
        let target_name = target.name.clone();

        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let send_id = format!("send-{}-{}", track.id, track.sends.len() + 1);
        track.sends.push(SendSlotState {
            id: send_id.clone(),
            target_track_id: target_id.clone(),
            target_name,
            enabled: true,
            pre_fader: false,
            gain_db: 0.0,
        });
        if routing_debug_enabled() {
            eprintln!(
                "[routing] add_send track={} send={} -> {}",
                track_id, send_id, target_id
            );
        }
        Some(send_id)
    }

    pub fn remove_send(&mut self, track_id: &str, send_id: &str) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.sends.retain(|s| s.id != send_id);
            if routing_debug_enabled() {
                eprintln!("[routing] remove_send track={} send={}", track_id, send_id);
            }
        }
    }

    pub fn toggle_send_enabled(&mut self, track_id: &str, send_id: &str) -> Option<bool> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let send = track.sends.iter_mut().find(|s| s.id == send_id)?;
        send.enabled = !send.enabled;
        Some(send.enabled)
    }

    pub fn cycle_track_input_monitor(&mut self, track_id: &str) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.input_monitor = t.input_monitor.cycle();
            return true;
        }
        false
    }

    pub fn select_track(&mut self, track_id: &str) {
        self.selection.selected_track_id = Some(track_id.to_string());
        self.selection.selected_clip_ids.clear();
        self.arrangement_range = None;
    }

    pub fn select_clip(&mut self, clip_id: &str) {
        self.selection.selected_clip_ids = vec![clip_id.to_string()];
        self.arrangement_range = None;
        if let Some(track) = self
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
        {
            self.selection.selected_track_id = Some(track.id.clone());
        }
    }

    pub fn select_clip_additive(&mut self, clip_id: &str) {
        self.arrangement_range = None;
        if let Some(pos) = self
            .selection
            .selected_clip_ids
            .iter()
            .position(|id| id == clip_id)
        {
            self.selection.selected_clip_ids.remove(pos);
        } else {
            self.selection.selected_clip_ids.push(clip_id.to_string());
        }
        if let Some(track) = self
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
        {
            self.selection.selected_track_id = Some(track.id.clone());
        }
    }

    pub fn rename_clip(&mut self, clip_id: &str, name: &str) -> bool {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return false;
        }
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                if clip.name != trimmed {
                    clip.name = trimmed.to_string();
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn set_clip_start(&mut self, clip_id: &str, start_beat: f32) -> bool {
        let start_beat = start_beat.max(0.0);
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                if (clip.start_beat - start_beat).abs() > 0.0001 {
                    clip.start_beat = start_beat;
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn set_clip_length(&mut self, clip_id: &str, duration_beats: f32) -> bool {
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                let min_len = match &clip.clip_type {
                    ClipType::Midi { notes, .. } => {
                        let last_note_end = notes
                            .iter()
                            .map(|note| note.start.max(0.0) + note.duration.max(MIN_NOTE_BEATS))
                            .fold(0.0_f32, f32::max);
                        MIN_MIDI_CLIP_BEATS.max(last_note_end)
                    }
                    ClipType::Audio { .. } => 0.25,
                };
                let duration_beats = duration_beats.max(min_len);
                if (clip.duration_beats - duration_beats).abs() > 0.0001 {
                    clip.duration_beats = duration_beats;
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn set_clip_muted(&mut self, clip_id: &str, muted: bool) -> bool {
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                if clip.muted != muted {
                    clip.muted = muted;
                    return true;
                }
                return false;
            }
        }
        false
    }

    pub fn set_clip_gain(&mut self, clip_id: &str, gain: f32) -> bool {
        let gain = gain.clamp(0.0, 4.0);
        for track in &mut self.tracks {
            if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == clip_id) {
                if (clip.gain - gain).abs() > 0.0001 {
                    clip.gain = gain;
                    return true;
                }
                return false;
            }
        }
        false
    }

    // ── Automation: mode, target, lanes, points ──────────────────────────────
    // Single source of truth for automation edits. The TrackHeader toggle, the
    // lane editor, and keyboard commands all route through these. Selection and
    // mode toggles are UI-only (never dirty the engine); point add/move/delete
    // and target/lane changes are committed edits the caller marks dirty once.

    pub fn track_lane_mode(&self, track_id: &str) -> TrackLaneMode {
        self.find_track(track_id)
            .map(|t| t.lane_mode)
            .unwrap_or(TrackLaneMode::Clips)
    }

    /// Toggle a track between Clip and Automation mode. UI-only. Returns the new
    /// mode. Selecting Automation mode also makes sure a lane exists for the
    /// active target so the editor has something to draw.
    pub fn toggle_track_lane_mode(&mut self, track_id: &str) -> Option<TrackLaneMode> {
        let new_mode = {
            let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
            track.lane_mode = match track.lane_mode {
                TrackLaneMode::Clips => TrackLaneMode::Automation,
                TrackLaneMode::Automation => TrackLaneMode::Clips,
            };
            track.lane_mode
        };
        if new_mode == TrackLaneMode::Automation {
            let target = self.active_automation_target(track_id);
            let _ = self.ensure_automation_lane(track_id, target);
        }
        if automation_debug_enabled() {
            eprintln!("[automation] mode track={} mode={:?}", track_id, new_mode);
        }
        Some(new_mode)
    }

    /// The target the lane editor is focused on for a track (selected target,
    /// else the first existing lane's target, else Track Volume).
    pub fn active_automation_target(&self, track_id: &str) -> AutomationTarget {
        let Some(track) = self.find_track(track_id) else {
            return AutomationTarget::TrackVolume;
        };
        if let Some(target) = track.selected_automation_target.clone() {
            return target;
        }
        track
            .automation_lanes
            .first()
            .map(|l| l.target.clone())
            .unwrap_or(AutomationTarget::TrackVolume)
    }

    /// Targets offered by the picker for a track: Volume, Pan, then one entry
    /// per insert plugin parameter (when metadata is available).
    pub fn available_automation_targets(&self, track_id: &str) -> Vec<AutomationTarget> {
        let mut out = vec![AutomationTarget::TrackVolume, AutomationTarget::TrackPan];
        if let Some(track) = self.find_track(track_id) {
            for insert in &track.inserts {
                if insert.is_empty() {
                    continue;
                }
                for param in &insert.parameters {
                    out.push(AutomationTarget::PluginParameter {
                        insert_id: insert.id.clone(),
                        parameter_id: param.id.to_string(),
                        parameter_name: format!("{}: {}", insert.display_name, param.name),
                    });
                }
            }
        }
        out
    }

    /// Point the lane editor at `target`, creating its lane if needed. Committed
    /// edit (changes which lane renders/persists). Returns the lane id.
    pub fn set_track_automation_target(
        &mut self,
        track_id: &str,
        target: AutomationTarget,
    ) -> Option<String> {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.selected_automation_target = Some(target.clone());
        }
        if automation_debug_enabled() {
            eprintln!(
                "[automation] target track={} target={}",
                track_id,
                target.display_name()
            );
        }
        self.ensure_automation_lane(track_id, target)
    }

    /// Cycle to the next available target (Volume → Pan → plugin params → …).
    pub fn cycle_automation_target(&mut self, track_id: &str) -> Option<String> {
        let targets = self.available_automation_targets(track_id);
        if targets.is_empty() {
            return None;
        }
        let current = self.active_automation_target(track_id);
        let idx = targets.iter().position(|t| *t == current).unwrap_or(0);
        let next = targets[(idx + 1) % targets.len()].clone();
        self.set_track_automation_target(track_id, next)
    }

    fn lane_index_for_target(track: &TrackState, target: &AutomationTarget) -> Option<usize> {
        track
            .automation_lanes
            .iter()
            .position(|l| l.target == *target)
    }

    /// Ensure a lane exists for `target` on `track_id`; returns its id.
    pub fn ensure_automation_lane(
        &mut self,
        track_id: &str,
        target: AutomationTarget,
    ) -> Option<String> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        if let Some(idx) = Self::lane_index_for_target(track, &target) {
            return Some(track.automation_lanes[idx].id.clone());
        }
        let lane_id = format!("autolane-{}-{}", track.id, track.automation_lanes.len() + 1);
        track
            .automation_lanes
            .push(AutomationLaneState::new(lane_id.clone(), target));
        if automation_debug_enabled() {
            eprintln!(
                "[automation] create_lane track={} lane={}",
                track_id, lane_id
            );
        }
        Some(lane_id)
    }

    /// Id of the lane the editor is currently focused on for a track.
    pub fn active_automation_lane_id(&self, track_id: &str) -> Option<String> {
        let track = self.find_track(track_id)?;
        let target = self.active_automation_target(track_id);
        track
            .automation_lanes
            .iter()
            .find(|l| l.target == target)
            .map(|l| l.id.clone())
    }

    fn lane_mut(&mut self, track_id: &str, lane_id: &str) -> Option<&mut AutomationLaneState> {
        self.tracks
            .iter_mut()
            .find(|t| t.id == track_id)?
            .automation_lanes
            .iter_mut()
            .find(|l| l.id == lane_id)
    }

    pub fn automation_lane(&self, track_id: &str, lane_id: &str) -> Option<&AutomationLaneState> {
        self.find_track(track_id)?
            .automation_lanes
            .iter()
            .find(|l| l.id == lane_id)
    }

    /// Add a point at `(beat, value)` to a lane. If a point already sits within
    /// [`AUTOMATION_BEAT_EPSILON`] beats, its value is replaced instead. Returns
    /// the affected point id. Committed edit — caller marks dirty once.
    pub fn add_automation_point(
        &mut self,
        track_id: &str,
        lane_id: &str,
        beat: f32,
        value: f32,
    ) -> Option<u64> {
        let lane = self.lane_mut(track_id, lane_id)?;
        let beat = beat.max(0.0);
        let value = value.clamp(0.0, 1.0);
        let id = if let Some(existing) = lane
            .points
            .iter_mut()
            .find(|p| (p.beat - beat).abs() <= AUTOMATION_BEAT_EPSILON)
        {
            existing.value = value;
            existing.id
        } else {
            let point = AutomationPoint::new(beat, value);
            let id = point.id;
            lane.points.push(point);
            id
        };
        lane.sort_points();
        if automation_debug_enabled() {
            eprintln!(
                "[automation] add_point lane={} beat={:.3} value={:.3}",
                lane_id, beat, value
            );
        }
        // Preview the edited curve at the playhead so the fader/inspector follow
        // a Track Volume point edit immediately (even while stopped).
        let playhead = self.transport.playhead_beats;
        self.recompute_effective_volumes(playhead, "point_edit");
        Some(id)
    }

    /// Move a point to a new beat/value (clamped + re-sorted). Committed on
    /// release by the caller.
    pub fn move_automation_point(
        &mut self,
        track_id: &str,
        lane_id: &str,
        point_id: u64,
        beat: f32,
        value: f32,
    ) {
        let Some(lane) = self.lane_mut(track_id, lane_id) else {
            return;
        };
        if let Some(p) = lane.points.iter_mut().find(|p| p.id == point_id) {
            p.beat = beat.max(0.0);
            p.value = value.clamp(0.0, 1.0);
        }
        lane.sort_points();
        if automation_debug_enabled() {
            eprintln!(
                "[automation] move_point lane={} id={} beat={:.3} value={:.3}",
                lane_id, point_id, beat, value
            );
        }
        let playhead = self.transport.playhead_beats;
        self.recompute_effective_volumes(playhead, "point_edit");
    }

    /// Set a point's curve type. Committed edit.
    pub fn set_automation_point_curve(
        &mut self,
        track_id: &str,
        lane_id: &str,
        point_id: u64,
        curve: AutomationCurve,
    ) {
        if let Some(lane) = self.lane_mut(track_id, lane_id) {
            if let Some(p) = lane.points.iter_mut().find(|p| p.id == point_id) {
                p.curve = curve;
            }
        }
    }

    /// Select a single point (or add to the selection when `additive`). UI-only.
    pub fn select_automation_point(
        &mut self,
        track_id: &str,
        lane_id: &str,
        point_id: u64,
        additive: bool,
    ) {
        let Some(lane) = self.lane_mut(track_id, lane_id) else {
            return;
        };
        for p in lane.points.iter_mut() {
            if p.id == point_id {
                p.selected = if additive { !p.selected } else { true };
            } else if !additive {
                p.selected = false;
            }
        }
    }

    /// Clear automation point selection on a track. UI-only. Returns true when
    /// anything was actually deselected.
    pub fn clear_automation_selection(&mut self, track_id: &str) -> bool {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return false;
        };
        let mut changed = false;
        for lane in track.automation_lanes.iter_mut() {
            for p in lane.points.iter_mut() {
                if p.selected {
                    p.selected = false;
                    changed = true;
                }
            }
        }
        changed
    }

    /// Select every point in a lane. UI-only. Returns the count selected.
    pub fn select_all_automation_points(&mut self, track_id: &str, lane_id: &str) -> usize {
        let Some(lane) = self.lane_mut(track_id, lane_id) else {
            return 0;
        };
        for p in lane.points.iter_mut() {
            p.selected = true;
        }
        lane.points.len()
    }

    /// Select all points inside a beat/value rectangle (marquee). UI-only.
    pub fn marquee_select_automation(
        &mut self,
        track_id: &str,
        lane_id: &str,
        beat_lo: f32,
        beat_hi: f32,
        value_lo: f32,
        value_hi: f32,
        additive: bool,
    ) -> usize {
        let Some(lane) = self.lane_mut(track_id, lane_id) else {
            return 0;
        };
        let (b0, b1) = if beat_lo <= beat_hi {
            (beat_lo, beat_hi)
        } else {
            (beat_hi, beat_lo)
        };
        let (v0, v1) = if value_lo <= value_hi {
            (value_lo, value_hi)
        } else {
            (value_hi, value_lo)
        };
        let mut count = 0;
        for p in lane.points.iter_mut() {
            let inside = p.beat >= b0 && p.beat <= b1 && p.value >= v0 && p.value <= v1;
            if inside {
                p.selected = true;
                count += 1;
            } else if !additive {
                p.selected = false;
            }
        }
        count
    }

    /// Find the closest automation point to `(beat, value)` within the given
    /// tolerances (in beats / normalized value). Returns its id. Used by the
    /// lane editor for click hit-testing.
    pub fn automation_point_at(
        &self,
        track_id: &str,
        lane_id: &str,
        beat: f32,
        value: f32,
        beat_tol: f32,
        value_tol: f32,
    ) -> Option<u64> {
        let lane = self.automation_lane(track_id, lane_id)?;
        let mut best: Option<(f32, u64)> = None;
        for p in &lane.points {
            let db = (p.beat - beat).abs();
            let dv = (p.value - value).abs();
            if db <= beat_tol && dv <= value_tol {
                // Rank by normalized combined distance so the nearest wins.
                let score = (db / beat_tol.max(1.0e-6)).hypot(dv / value_tol.max(1.0e-6));
                if best.map(|(s, _)| score < s).unwrap_or(true) {
                    best = Some((score, p.id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    pub fn selected_automation_point_count(&self, track_id: &str) -> usize {
        self.find_track(track_id)
            .map(|t| {
                t.automation_lanes
                    .iter()
                    .flat_map(|l| l.points.iter())
                    .filter(|p| p.selected)
                    .count()
            })
            .unwrap_or(0)
    }

    /// Delete every selected automation point on a track. Committed edit —
    /// caller marks dirty once. Returns how many were removed.
    pub fn delete_selected_automation_points(&mut self, track_id: &str) -> usize {
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) else {
            return 0;
        };
        let mut removed = 0;
        for lane in track.automation_lanes.iter_mut() {
            let before = lane.points.len();
            lane.points.retain(|p| !p.selected);
            removed += before - lane.points.len();
        }
        if removed > 0 && automation_debug_enabled() {
            eprintln!(
                "[automation] delete_points track={} count={}",
                track_id, removed
            );
        }
        removed
    }

    pub fn find_track(&self, track_id: &str) -> Option<&TrackState> {
        self.tracks.iter().find(|t| t.id == track_id)
    }

    pub fn find_clip(&self, clip_id: &str) -> Option<(&TrackState, &ClipState)> {
        for t in &self.tracks {
            if let Some(c) = t.clips.iter().find(|c| c.id == clip_id) {
                return Some((t, c));
            }
        }
        None
    }

    pub fn delete_track(&mut self, track_id: &str) {
        if let Some(index) = self.tracks.iter().position(|track| track.id == track_id) {
            self.tracks.remove(index);
            if self.selection.selected_track_id.as_deref() == Some(track_id) {
                self.selection.selected_track_id = self
                    .tracks
                    .get(index.saturating_sub(1))
                    .map(|t| t.id.clone());
            }
            self.selection.selected_clip_ids.clear();
        }
    }

    pub fn delete_clip(&mut self, clip_id: &str) {
        for track in &mut self.tracks {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                track.clips.remove(index);
                self.selection.selected_clip_ids.retain(|id| id != clip_id);
                self.selection.selected_track_id = Some(track.id.clone());
                return;
            }
        }
    }

    pub fn duplicate_clip(&mut self, clip_id: &str) {
        let next_id = self.next_clip_id();
        let snap_step = if self.snap_to_grid && self.grid_division != SnapDivision::Off {
            Some((self.grid_division.step_beats(self.beats_per_bar())).max(0.0))
        } else {
            None
        };
        for track in &mut self.tracks {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                let mut duplicate = track.clips[index].clone();
                duplicate.id = next_id;
                duplicate.name = format!("{} Copy", duplicate.name);
                let raw_start = duplicate.start_beat + duplicate.duration_beats;
                duplicate.start_beat = snap_step
                    .filter(|step| *step > 0.0)
                    .map(|step| (raw_start / step).round() * step)
                    .unwrap_or(raw_start)
                    .max(0.0);
                let duplicate_id = duplicate.id.clone();
                track.clips.insert(index + 1, duplicate);
                self.selection.selected_track_id = Some(track.id.clone());
                self.selection.selected_clip_ids = vec![duplicate_id];
                return;
            }
        }
    }

    /// Multiplicative zoom around a content-space x anchor. Updates
    /// `pixels_per_second` and shifts `scroll_x` so the time under `anchor_x`
    /// (a screen-space x inside the track lane, already net of header) stays
    /// fixed under the cursor. Smooth — accepts arbitrary positive `factor`.
    pub fn zoom_by(&mut self, factor: f32, anchor_x: f32) {
        let factor = factor.max(0.0001);
        let old_pps = self.viewport.pixels_per_second.max(0.0001);
        let new_pps = (old_pps * factor).clamp(4.0, 4000.0);
        if (new_pps - old_pps).abs() < 0.0001 {
            return;
        }
        // Time under anchor before the change.
        let anchor_time = (anchor_x + self.viewport.scroll_x) / old_pps;
        self.viewport.pixels_per_second = new_pps;
        self.sync_pixels_per_beat();
        // Re-solve scroll_x so the same anchor_time lands under anchor_x.
        let new_scroll = (anchor_time * new_pps - anchor_x).max(0.0);
        self.viewport.scroll_x = new_scroll;
        self.viewport.target_scroll_x = new_scroll;
    }

    pub fn scroll_by(&mut self, delta_x: f32, delta_y: f32, max_x: f32, max_y: f32) {
        self.viewport.target_scroll_x =
            (self.viewport.target_scroll_x + delta_x).clamp(0.0, max_x.max(0.0));
        self.viewport.target_scroll_y =
            (self.viewport.target_scroll_y + delta_y).clamp(0.0, max_y.max(0.0));
        self.smooth_scroll_towards_target();
    }

    pub fn set_scroll_immediate(&mut self, x: f32, y: f32, max_x: f32, max_y: f32) {
        self.viewport.scroll_x = x.clamp(0.0, max_x.max(0.0));
        self.viewport.scroll_y = y.clamp(0.0, max_y.max(0.0));
        self.viewport.target_scroll_x = self.viewport.scroll_x;
        self.viewport.target_scroll_y = self.viewport.scroll_y;
    }

    pub fn clamp_scroll(&mut self, max_x: f32, max_y: f32) {
        let max_x = max_x.max(0.0);
        let max_y = max_y.max(0.0);
        self.viewport.scroll_x = self.viewport.scroll_x.clamp(0.0, max_x);
        self.viewport.scroll_y = self.viewport.scroll_y.clamp(0.0, max_y);
        self.viewport.target_scroll_x = self.viewport.target_scroll_x.clamp(0.0, max_x);
        self.viewport.target_scroll_y = self.viewport.target_scroll_y.clamp(0.0, max_y);
    }

    /// Keep the playhead inside the visible viewport while playing.
    /// Returns true when the viewport scrolled (caller should `cx.notify`).
    /// Cheap — no allocation, just a couple of float comparisons.
    pub fn update_auto_scroll_for_playhead(&mut self, playhead_beats: f32) -> bool {
        if !self.follow_playhead || self.auto_scroll_mode == AutoScrollMode::Off {
            return false;
        }
        let viewport_width = self.viewport.viewport_width;
        if viewport_width <= 1.0 {
            return false;
        }
        let pps = self.viewport.pixels_per_second.max(0.0001);
        let playhead_content_x = playhead_beats.max(0.0) * self.seconds_per_beat() * pps;
        let scroll_x = self.viewport.scroll_x;

        // Trigger thresholds: right 15% / left 5% of the viewport.
        let right_trigger = scroll_x + viewport_width * 0.85;
        let left_trigger = scroll_x + viewport_width * 0.05;

        let new_scroll_x = match self.auto_scroll_mode {
            AutoScrollMode::Off => return false,
            AutoScrollMode::Page => {
                if playhead_content_x > right_trigger {
                    // Page forward so the playhead lands ~10% into the new viewport.
                    (playhead_content_x - viewport_width * 0.10).max(0.0)
                } else if playhead_content_x < scroll_x {
                    // Playhead seeked or wrapped back behind the viewport — recenter.
                    (playhead_content_x - viewport_width * 0.10).max(0.0)
                } else {
                    return false;
                }
            }
            AutoScrollMode::Continuous => {
                if playhead_content_x > right_trigger || playhead_content_x < left_trigger {
                    (playhead_content_x - viewport_width * 0.40).max(0.0)
                } else {
                    return false;
                }
            }
        };

        if (new_scroll_x - scroll_x).abs() < 0.5 {
            return false;
        }
        if std::env::var_os("FUTUREBOARD_AUTOSCROLL_DEBUG").is_some() {
            eprintln!(
                "[autoscroll] playhead_x={:.1} viewport=[{:.1}..{:.1}] scroll_x: {:.1} -> {:.1} mode={:?}",
                playhead_content_x,
                scroll_x,
                scroll_x + viewport_width,
                scroll_x,
                new_scroll_x,
                self.auto_scroll_mode
            );
        }
        self.viewport.scroll_x = new_scroll_x;
        self.viewport.target_scroll_x = new_scroll_x;
        true
    }

    /// Called when the user manually scrolls/drags the viewport — temporarily
    /// disables follow-playhead so playback won't fight the user. Re-enable
    /// by toggling the Follow control.
    pub fn note_user_scrolled(&mut self) {
        if self.follow_playhead {
            self.follow_playhead = false;
        }
    }

    pub fn set_follow_playhead(&mut self, follow: bool) {
        self.follow_playhead = follow;
    }

    pub fn smooth_scroll_towards_target(&mut self) -> bool {
        let dx = self.viewport.target_scroll_x - self.viewport.scroll_x;
        let dy = self.viewport.target_scroll_y - self.viewport.scroll_y;
        if dx.abs() < 0.35 && dy.abs() < 0.35 {
            self.viewport.scroll_x = self.viewport.target_scroll_x;
            self.viewport.scroll_y = self.viewport.target_scroll_y;
            return false;
        }
        self.viewport.scroll_x += dx * 0.42;
        self.viewport.scroll_y += dy * 0.42;
        true
    }

    pub fn move_clip_to_track(&mut self, clip_id: &str, target_track_id: &str, start_beat: f32) {
        let start_beat = self.snap_beats(start_beat).max(0.0);
        let mut moved_clip = None;
        let mut source_track_id = None;

        for track in &mut self.tracks {
            if let Some(index) = track.clips.iter().position(|clip| clip.id == clip_id) {
                let mut clip = track.clips.remove(index);
                clip.start_beat = start_beat;
                moved_clip = Some(clip);
                source_track_id = Some(track.id.clone());
                break;
            }
        }

        let Some(clip) = moved_clip else {
            return;
        };

        let target_id = if self.tracks.iter().any(|track| track.id == target_track_id) {
            target_track_id.to_string()
        } else {
            source_track_id.unwrap_or_else(|| target_track_id.to_string())
        };

        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == target_id) {
            track.clips.push(clip);
            self.selection.selected_track_id = Some(track.id.clone());
            self.selection.selected_clip_ids = vec![clip_id.to_string()];
        }
    }

    /// Resize a clip by dragging one edge to `new_edge_beat` (absolute beats;
    /// snapped here). The opposite edge stays fixed. Enforces a minimum length
    /// and, for MIDI clips, never shrinks below the last note end. Left-edge
    /// resizes re-offset clip-local notes so they keep their absolute position,
    /// clamping so the earliest note never crosses clip-local beat 0.
    ///
    /// UI-mutating only — the caller marks the project dirty once on commit.
    /// Returns `true` when a matching clip was found.
    pub fn resize_clip(&mut self, clip_id: &str, edge: ClipEdge, new_edge_beat: f32) -> bool {
        let snapped = self.snap_beats(new_edge_beat).max(0.0);
        let Some(track) = self
            .tracks
            .iter_mut()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
        else {
            return false;
        };
        let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) else {
            return false;
        };

        let is_midi = matches!(clip.clip_type, ClipType::Midi { .. });
        let min_len = if is_midi { MIN_MIDI_CLIP_BEATS } else { 0.25 };
        // Clip-local end of the furthest note — the floor for any MIDI shrink.
        let last_note_end = if let ClipType::Midi { notes, .. } = &clip.clip_type {
            notes
                .iter()
                .map(|n| n.start.max(0.0) + n.duration.max(MIN_NOTE_BEATS))
                .fold(0.0_f32, f32::max)
        } else {
            0.0
        };

        match edge {
            ClipEdge::Right => {
                // Right edge moves; start fixed. Cannot shrink below the last
                // note end or the minimum length.
                let dur = (snapped - clip.start_beat).max(min_len).max(last_note_end);
                clip.duration_beats = dur;
            }
            ClipEdge::Left => {
                let old_start = clip.start_beat;
                let old_right = old_start + clip.duration_beats;
                // Keep the right edge fixed; clamp the new start to [0, right-min].
                let mut new_start = snapped.min(old_right - min_len).max(0.0);
                // Trimming from the left must not push the earliest note < 0.
                if let ClipType::Midi { notes, .. } = &clip.clip_type {
                    if let Some(min_local) = notes.iter().map(|n| n.start).reduce(f32::min) {
                        let max_start = (old_start + min_local).max(0.0);
                        new_start = new_start.min(max_start);
                    }
                }
                let delta = old_start - new_start;
                if let ClipType::Midi { notes, .. } = &mut clip.clip_type {
                    for note in notes.iter_mut() {
                        note.start = (note.start + delta).max(0.0);
                    }
                }
                clip.start_beat = new_start;
                clip.duration_beats = (old_right - new_start).max(min_len);
            }
        }

        if midi_debug_enabled() {
            eprintln!(
                "[midi] resize_clip clip={} edge={:?} start={:.3} len={:.3}",
                clip_id, edge, clip.start_beat, clip.duration_beats
            );
        }
        true
    }

    /// Drop a clip onto the timeline. `drop_x` and `drop_y` are in the track
    /// area coordinate system (header_width and ruler_height already stripped).
    /// Imports a clip with unknown metadata. The 2-bar duration is a temporary
    /// placeholder and must be replaced by DirectAudioEngine metadata.
    pub fn import_audio_at(
        &mut self,
        source_path: String,
        clip_name: String,
        drop_x: f32,
        drop_y: f32,
    ) -> String {
        eprintln!(
            "[import] drop path={} clip={} drop_x={:.1} drop_y={:.1}",
            source_path, clip_name, drop_x, drop_y
        );
        // Resolve target track: an existing lane under drop_y, otherwise create one.
        let track_id = match self.track_index_at_y(drop_y) {
            Some(idx) if matches!(self.tracks[idx].track_type, TrackType::Audio) => {
                self.tracks[idx].id.clone()
            }
            _ => self.create_audio_track(),
        };

        // Resolve start beat with snap.
        let raw_beats = self.x_to_beats(drop_x.max(0.0));
        let start_beat = self.snap_beats(raw_beats).max(0.0);

        self.insert_audio_clip(track_id, source_path, clip_name, start_beat)
    }

    pub fn import_audio_to_selected_or_new_track(
        &mut self,
        source_path: String,
        clip_name: String,
    ) -> String {
        let track_id = self
            .selected_audio_track_id()
            .unwrap_or_else(|| self.create_audio_track());
        eprintln!(
            "[import] browser path={} clip={} resolved_track_id={}",
            source_path, clip_name, track_id
        );
        let start_beat = self.snap_beats(self.x_to_beats(0.0)).max(0.0);
        self.insert_audio_clip(track_id, source_path, clip_name, start_beat)
    }

    pub fn insert_recorded_clip(
        &mut self,
        track_id: &str,
        source_path: String,
        clip_name: String,
        start_beat: f32,
        duration_seconds: f64,
        bpm: f32,
    ) -> String {
        let duration_beats = (duration_seconds.max(0.0) * bpm.max(1.0) as f64 / 60.0) as f32;
        self.insert_audio_clip_with_duration(
            track_id.to_string(),
            source_path,
            clip_name,
            start_beat,
            duration_beats.max(0.01),
            Some(duration_seconds),
        )
    }

    // ── Realtime recording preview clip (Part 1) ─────────────────────────
    //
    // A temporary, UI-only clip drawn while a take is recording. It has no
    // source path so it is never sent to the engine or persisted; the
    // arrangement renderer lays it out like any clip, and `waveform_canvas`
    // draws its streamed peaks from the recording-preview registry.

    /// Create (or replace) the live recording preview clip on `track_id`.
    pub fn begin_recording_preview_clip(&mut self, clip_id: &str, track_id: &str, start_beat: f32) {
        self.remove_recording_preview_clip(clip_id);
        let clip = ClipState {
            id: clip_id.to_string(),
            name: "Recording…".to_string(),
            start_beat: start_beat.max(0.0),
            duration_beats: 0.01,
            source_duration_seconds: None,
            offset_beats: 0.0,
            gain: 1.0,
            clip_type: ClipType::Audio {
                file_id: String::new(),
                source_path: None,
            },
            muted: false,
            audio_import: AudioImportState::Pending,
        };
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.clips.push(clip);
        }
    }

    /// Grow the preview clip as recording proceeds. Returns `true` if changed.
    pub fn set_recording_preview_clip_length(
        &mut self,
        clip_id: &str,
        duration_beats: f32,
    ) -> bool {
        let next = duration_beats.max(0.01);
        for track in &mut self.tracks {
            if let Some(c) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                if (c.duration_beats - next).abs() > f32::EPSILON {
                    c.duration_beats = next;
                    return true;
                }
                return false;
            }
        }
        false
    }

    /// Remove the preview clip (take finished / cancelled). Returns `true` if
    /// a clip was removed.
    pub fn remove_recording_preview_clip(&mut self, clip_id: &str) -> bool {
        let mut removed = false;
        for track in &mut self.tracks {
            let before = track.clips.len();
            track.clips.retain(|c| c.id != clip_id);
            removed |= track.clips.len() != before;
        }
        if removed {
            self.selection.selected_clip_ids.retain(|id| id != clip_id);
        }
        removed
    }

    fn insert_audio_clip_with_duration(
        &mut self,
        track_id: String,
        source_path: String,
        clip_name: String,
        start_beat: f32,
        duration_beats: f32,
        source_duration_seconds: Option<f64>,
    ) -> String {
        let track_id = if self.tracks.iter().any(|track| track.id == track_id) {
            track_id
        } else {
            eprintln!(
                "[recording] target track id={track_id} missing; creating fallback audio track"
            );
            self.create_audio_track()
        };

        let clip_id = self.next_clip_id();
        let new_clip = ClipState {
            id: clip_id.clone(),
            name: clip_name,
            start_beat: start_beat.max(0.0),
            duration_beats,
            source_duration_seconds,
            offset_beats: 0.0,
            gain: 1.0,
            clip_type: ClipType::Audio {
                file_id: source_path.clone(),
                source_path: Some(source_path),
            },
            muted: false,
            audio_import: AudioImportState::Pending,
        };

        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
            track.clips.push(new_clip);
        }
        self.selection.selected_track_id = Some(track_id);
        self.selection.selected_clip_ids = vec![clip_id.clone()];
        clip_id
    }

    fn insert_audio_clip(
        &mut self,
        track_id: String,
        source_path: String,
        clip_name: String,
        start_beat: f32,
    ) -> String {
        let track_id = if self.tracks.iter().any(|track| track.id == track_id) {
            track_id
        } else {
            eprintln!(
                "[import] target track id={} missing; creating fallback audio track",
                track_id
            );
            self.create_audio_track()
        };

        let duration_beats = 8.0;
        eprintln!(
            "[audio-import] WARNING using fallback duration because metadata is pending: path={} duration_beats=8.0",
            source_path
        );
        self.insert_audio_clip_with_duration(
            track_id,
            source_path,
            clip_name,
            start_beat,
            duration_beats,
            None,
        )
    }

    pub fn update_audio_clip_metadata(
        &mut self,
        source_path: &str,
        format: &str,
        sample_rate: u32,
        channels: u16,
        total_frames: u64,
        duration_seconds: f64,
    ) -> bool {
        if duration_seconds <= 0.0 {
            return false;
        }
        let duration_beats = self.seconds_to_beats(duration_seconds);
        let mut changed = false;
        let mut matched = false;
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if let ClipType::Audio {
                    source_path: Some(path),
                    ..
                } = &clip.clip_type
                {
                    if path == source_path {
                        matched = true;
                        clip.source_duration_seconds = Some(duration_seconds);
                        if (clip.duration_beats - duration_beats).abs() > 0.001 {
                            clip.duration_beats = duration_beats;
                            changed = true;
                        }
                    }
                }
            }
        }
        if matched {
            self.log_audio_meta(
                source_path,
                format,
                sample_rate,
                channels,
                total_frames,
                duration_seconds,
            );
            self.log_audio_import(duration_beats);
        }
        changed
    }

    pub fn set_audio_import_for_path(&mut self, source_path: &str, state: AudioImportState) {
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if let ClipType::Audio {
                    source_path: Some(path),
                    ..
                } = &clip.clip_type
                {
                    if path == source_path {
                        clip.audio_import = state.clone();
                    }
                }
            }
        }
    }

    pub fn audio_source_duration_seconds(&self, source_path: &str) -> Option<f64> {
        self.tracks.iter().find_map(|track| {
            track.clips.iter().find_map(|clip| {
                if let ClipType::Audio {
                    source_path: Some(path),
                    ..
                } = &clip.clip_type
                {
                    if path == source_path {
                        return clip.source_duration_seconds;
                    }
                }
                None
            })
        })
    }

    fn log_audio_meta(
        &self,
        source_path: &str,
        format: &str,
        sample_rate: u32,
        channels: u16,
        total_frames: u64,
        duration_seconds: f64,
    ) {
        eprintln!("[audio-meta] path={}", source_path);
        eprintln!("[audio-meta] format={}", format);
        eprintln!("[audio-meta] sample_rate={}", sample_rate);
        eprintln!("[audio-meta] channels={}", channels);
        eprintln!("[audio-meta] total_frames={}", total_frames);
        eprintln!("[audio-meta] duration_seconds={:.6}", duration_seconds);
    }

    fn log_audio_import(&self, duration_beats: f32) {
        let bars_4_4 = duration_beats / 4.0;
        eprintln!("[audio-import] bpm={:.3}", self.bpm);
        eprintln!("[audio-import] duration_beats={:.6}", duration_beats);
        eprintln!("[audio-import] bars_4_4={:.6}", bars_4_4);
    }
}

#[cfg(test)]
mod tempo_map_tests {
    use super::*;

    #[test]
    fn empty_map_uses_base_bpm() {
        let map = TempoMap::new();
        assert!(!map.has_automation());
        assert_eq!(map.bpm_at_beat(0.0, 120.0), 120.0);
        assert_eq!(map.bpm_at_beat(100.0, 120.0), 120.0);
    }

    #[test]
    fn hold_marker_steps_bpm() {
        let mut map = TempoMap::new();
        map.add_or_update_point(8.0, 140.0, TempoCurve::Hold);
        assert!(map.has_automation());
        // Before the marker we sit on the implicit base point.
        assert_eq!(map.bpm_at_beat(0.0, 120.0), 120.0);
        assert_eq!(map.bpm_at_beat(7.9, 120.0), 120.0);
        // From the marker onward the held tempo applies.
        assert_eq!(map.bpm_at_beat(8.0, 120.0), 140.0);
        assert_eq!(map.bpm_at_beat(99.0, 120.0), 140.0);
    }

    #[test]
    fn linear_curve_interpolates_between_markers() {
        let mut map = TempoMap::new();
        map.add_or_update_point(0.0, 100.0, TempoCurve::Linear);
        map.add_or_update_point(4.0, 200.0, TempoCurve::Hold);
        // Halfway between the two markers = midpoint BPM.
        assert!((map.bpm_at_beat(2.0, 120.0) - 150.0).abs() < 1e-6);
        // At/after the last marker the held value applies.
        assert!((map.bpm_at_beat(4.0, 120.0) - 200.0).abs() < 1e-6);
    }

    #[test]
    fn add_replaces_marker_at_same_beat_and_clear_resets() {
        let mut map = TempoMap::new();
        map.add_or_update_point(4.0, 130.0, TempoCurve::Hold);
        map.add_or_update_point(4.0, 150.0, TempoCurve::Linear);
        assert_eq!(map.points.len(), 1);
        assert_eq!(map.points[0].bpm, 150.0);
        assert_eq!(map.points[0].curve, TempoCurve::Linear);

        map.clear();
        assert!(!map.has_automation());
    }

    #[test]
    fn hold_tempo_time_conversions_match_engine() {
        let mut map = TempoMap::new();
        map.add_or_update_point(4.0, 160.0, TempoCurve::Hold);
        assert!((map.seconds_at_beat(0.0, 120.0) - 0.0).abs() < 1e-9);
        assert!((map.seconds_at_beat(4.0, 120.0) - 2.0).abs() < 1e-9);
        assert!((map.seconds_at_beat(8.0, 120.0) - 3.5).abs() < 1e-9);
        assert!((map.beat_at_seconds(2.0, 120.0) - 4.0).abs() < 1e-9);
        assert!((map.beat_at_seconds(3.5, 120.0) - 8.0).abs() < 1e-9);
        assert_eq!(map.samples_at_beat(4.0, 120.0, 48_000.0), 96_000);
        assert_eq!(map.samples_at_beat(8.0, 120.0, 48_000.0), 168_000);
    }

    #[test]
    fn tempo_marker_bpm_values_are_independent() {
        let mut map = TempoMap::new();
        map.add_or_update_point(0.0, 120.0, TempoCurve::Hold);
        map.add_or_update_point(4.0, 132.0, TempoCurve::Hold);
        map.ensure_point_ids();

        assert_eq!(map.points[0].bpm, 120.0);
        assert_eq!(map.points[1].bpm, 132.0);
        assert_eq!(TempoMap::format_marker_label(map.points[0].bpm), "120");
        assert_eq!(TempoMap::format_marker_label(map.points[1].bpm), "132");
        assert_eq!(map.bpm_at_beat(0.0, 120.0), 120.0);
        assert_eq!(map.bpm_at_beat(3.9, 120.0), 120.0);
        assert_eq!(map.bpm_at_beat(4.0, 120.0), 132.0);

        let id_b = map.points[1].id.clone();
        assert!(map.update_point_bpm_by_id(&id_b, 140.0));

        assert_eq!(map.points[0].bpm, 120.0);
        assert_eq!(map.points[1].bpm, 140.0);
        assert_eq!(TempoMap::format_marker_label(map.points[0].bpm), "120");
        assert_eq!(TempoMap::format_marker_label(map.points[1].bpm), "140");
        assert_eq!(map.bpm_at_beat(0.0, 120.0), 120.0);
        assert_eq!(map.bpm_at_beat(4.0, 120.0), 140.0);
    }
}

#[cfg(test)]
mod tempo_track_tests {
    use super::*;

    #[test]
    fn tempo_lane_header_subtitle_fixed_and_range() {
        let mut state = TimelineState::default();
        state.bpm = 120.0;
        assert_eq!(state.tempo_lane_header_subtitle(), "Fixed 120 BPM");
        state
            .tempo_map
            .add_or_update_point(0.0, 120.0, TempoCurve::Hold);
        state
            .tempo_map
            .add_or_update_point(16.0, 160.0, TempoCurve::Hold);
        assert_eq!(state.tempo_lane_header_subtitle(), "120–160 BPM");
    }

    #[test]
    fn time_signature_lane_header_subtitle_fixed_and_markers() {
        let mut state = TimelineState::default();
        assert_eq!(state.time_signature_lane_header_subtitle(), "Fixed 4/4");
        state.time_signature_map.add_or_update_point(0.0, 4, 4);
        state.time_signature_map.add_or_update_point(16.0, 6, 8);
        assert_eq!(
            state.time_signature_lane_header_subtitle(),
            "4/4 · 2 markers"
        );
    }

    #[test]
    fn show_tempo_track_enables_global_lane() {
        let mut state = TimelineState::default();
        assert!(!state.show_tempo_track);
        assert!(state.visible_global_lanes().is_empty());

        state.show_tempo_track_lane();
        assert!(state.show_tempo_track);
        assert_eq!(state.visible_global_lanes(), vec![GlobalLaneKind::Tempo]);
    }

    #[test]
    fn tempo_track_renders_two_point_bpm_values() {
        let mut state = TimelineState::default();
        state
            .tempo_map
            .add_or_update_point(0.0, 120.0, TempoCurve::Hold);
        state
            .tempo_map
            .add_or_update_point(4.0, 160.0, TempoCurve::Hold);
        state.show_tempo_track_lane();

        let values = state.tempo_track_render_bpm_values();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], 120.0);
        assert_eq!(values[1], 160.0);
        assert_eq!(TempoMap::format_marker_label(values[0]), "120");
        assert_eq!(TempoMap::format_marker_label(values[1]), "160");
    }

    #[test]
    fn editing_one_tempo_point_leaves_other_unchanged() {
        let mut state = TimelineState::default();
        state
            .tempo_map
            .add_or_update_point(0.0, 120.0, TempoCurve::Hold);
        state
            .tempo_map
            .add_or_update_point(4.0, 160.0, TempoCurve::Hold);
        state.tempo_map.ensure_point_ids();
        let id_b = state.tempo_map.points[1].id.clone();
        let rev_before = state.tempo_map.revision();

        assert!(state.move_tempo_point(&id_b, 4.0, 170.0));
        assert_eq!(state.tempo_map.points[0].bpm, 120.0);
        assert_eq!(state.tempo_map.points[1].bpm, 170.0);
        assert!(state.tempo_map.revision() > rev_before);
    }

    #[test]
    fn fixed_tempo_renders_flat_line_across_viewport() {
        let mut state = TimelineState::default();
        state.bpm = 120.0;
        state
            .tempo_map
            .reset_to_single_point(0.0, 120.0, TempoCurve::Hold);
        state.show_tempo_track_lane();
        state.update_viewport_size(800.0, 500.0);

        let samples = state.tempo_track_bpm_samples(800.0);
        assert!(!samples.is_empty());
        for bpm in samples {
            assert!((bpm - 120.0).abs() < 1e-6);
        }
    }
}

#[cfg(test)]
mod time_signature_map_tests {
    use super::*;

    #[test]
    fn default_4_4_bar_boundaries() {
        let map = TimeSignatureMap::with_default_4_4();
        assert!((map.bar_start_beat(1) - 0.0).abs() < 1e-9);
        assert!((map.bar_start_beat(2) - 4.0).abs() < 1e-9);
        assert!((map.bar_start_beat(3) - 8.0).abs() < 1e-9);
        let bb0 = map.bar_beat_at_beat(0.0);
        assert_eq!(bb0.bar, 1);
        assert_eq!(bb0.beat_in_bar, 1);
        let bb4 = map.bar_beat_at_beat(4.0);
        assert_eq!(bb4.bar, 2);
        assert_eq!(bb4.beat_in_bar, 1);
    }

    #[test]
    fn change_from_4_4_to_3_4() {
        let mut map = TimeSignatureMap::with_default_4_4();
        map.add_or_update_point(16.0, 3, 4);
        assert_eq!(map.format_position_at_beat(0.0), "1.1");
        assert_eq!(map.format_position_at_beat(4.0), "2.1");
        assert_eq!(map.format_position_at_beat(8.0), "3.1");
        assert_eq!(map.format_position_at_beat(12.0), "4.1");
        assert_eq!(map.format_position_at_beat(16.0), "5.1");
        assert_eq!(map.format_position_at_beat(19.0), "6.1");
        assert_eq!(map.format_position_at_beat(22.0), "7.1");
    }

    #[test]
    fn seven_eight_beats_per_bar() {
        let mut map = TimeSignatureMap::new();
        map.add_or_update_point(0.0, 7, 8);
        assert!((map.beats_per_bar_at_beat(0.0) - 3.5).abs() < 1e-9);
        assert!((map.bar_start_beat(1) - 0.0).abs() < 1e-9);
        assert!((map.bar_start_beat(2) - 3.5).abs() < 1e-9);
        assert!((map.bar_start_beat(3) - 7.0).abs() < 1e-9);
    }

    #[test]
    fn marker_bpm_values_are_independent() {
        let mut map = TimeSignatureMap::with_default_4_4();
        map.add_or_update_point(16.0, 3, 4);
        map.ensure_point_ids();
        assert_eq!(map.points[0].label(), "4/4");
        assert_eq!(map.points[1].label(), "3/4");
        let id_b = map.points[1].id.clone();
        assert!(map.update_point_by_id(&id_b, 7, 8));
        assert_eq!(map.points[0].label(), "4/4");
        assert_eq!(map.points[1].label(), "7/8");
    }

    #[test]
    fn five_eight_ruler_denominator_ticks() {
        let mut map = TimeSignatureMap::new();
        map.add_or_update_point(0.0, 5, 8);
        assert!((map.bar_start_beat(1) - 0.0).abs() < 1e-9);
        assert!((map.bar_start_beat(2) - 2.5).abs() < 1e-9);
        assert_eq!(map.format_position_at_beat(0.0), "1.1");
        assert_eq!(map.format_position_at_beat(0.5), "1.2");
        assert_eq!(map.format_position_at_beat(1.0), "1.3");
        assert_eq!(map.format_position_at_beat(1.5), "1.4");
        assert_eq!(map.format_position_at_beat(2.0), "1.5");
        assert_eq!(map.format_position_at_beat(2.5), "2.1");
    }

    #[test]
    fn six_eight_ruler_denominator_ticks() {
        let mut map = TimeSignatureMap::new();
        map.add_or_update_point(0.0, 6, 8);
        assert!((map.bar_start_beat(2) - 3.0).abs() < 1e-9);
        assert_eq!(map.format_position_at_beat(2.5), "1.6");
        assert_eq!(map.format_position_at_beat(3.0), "2.1");
    }

    #[test]
    fn default_grouping_for_compound_meters() {
        let pt = TimeSignaturePoint::new(0.0, 5, 8);
        assert_eq!(pt.effective_grouping(), vec![2, 3]);
        let pt6 = TimeSignaturePoint::new(0.0, 6, 8);
        assert_eq!(pt6.effective_grouping(), vec![3, 3]);
        let pt7 = TimeSignaturePoint::new(0.0, 7, 8);
        assert_eq!(pt7.effective_grouping(), vec![2, 2, 3]);
    }

    #[test]
    fn marker_boundary_label_meter_change() {
        let mut map = TimeSignatureMap::new();
        map.add_or_update_point(0.0, 5, 8);
        map.add_or_update_point(2.5, 6, 8);
        assert_eq!(map.format_position_at_beat(2.0), "1.5");
        assert_eq!(map.format_position_at_beat(2.5), "2.1");
        assert_eq!(map.format_position_at_beat(3.0), "2.2");
    }

    #[test]
    fn visible_bar_background_rects_across_changing_meters() {
        let mut map = TimeSignatureMap::new();
        map.add_or_update_point(0.0, 5, 8);
        map.add_or_update_point(2.5, 6, 8);
        map.add_or_update_point(5.5, 5, 8);
        let rects = map.visible_bar_rects(0.0, 8.0);
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[0].bar, 1);
        assert!((rects[0].start_beat - 0.0).abs() < 1e-9);
        assert!((rects[0].end_beat - 2.5).abs() < 1e-9);
        assert_eq!(rects[1].bar, 2);
        assert!((rects[1].start_beat - 2.5).abs() < 1e-9);
        assert!((rects[1].end_beat - 5.5).abs() < 1e-9);
        assert_eq!(rects[2].bar, 3);
        assert!((rects[2].start_beat - 5.5).abs() < 1e-9);
        assert!((rects[2].end_beat - 8.0).abs() < 1e-9);
    }

    #[test]
    fn visible_bar_rects_follow_scroll_window() {
        let mut map = TimeSignatureMap::new();
        map.add_or_update_point(0.0, 5, 8);
        map.add_or_update_point(2.5, 6, 8);
        map.add_or_update_point(5.5, 5, 8);
        let rects = map.visible_bar_rects(3.0, 6.0);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].bar, 2);
        assert!((rects[0].start_beat - 2.5).abs() < 1e-9);
        assert_eq!(rects[1].bar, 3);
        assert!((rects[1].start_beat - 5.5).abs() < 1e-9);
    }
}

#[cfg(test)]
mod midi_edit_tests {
    use super::*;
    use crate::components::edit::EditCommand;

    /// Build an empty state with one MIDI clip and return `(state, clip_id)`.
    fn state_with_midi_clip() -> (TimelineState, String) {
        let mut state = TimelineState::default();
        let track_id = state.create_track(CreateTrackOptions {
            track_type: TrackType::Midi,
            name: "Test".into(),
            color: gpui::Rgba {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            },
            volume: 1.0,
            pan: 0.0,
            armed: false,
            input_monitor: InputMonitorMode::Off,
        });
        let clip = state
            .build_midi_clip(&track_id, 0.0, 4.0)
            .expect("clip builds");
        let clip_id = clip.id.clone();
        EditCommand::CreateClip { track_id, clip }.execute(&mut state);
        (state, clip_id)
    }

    fn note(state: &TimelineState, clip_id: &str, id: u64) -> MidiNoteState {
        state
            .midi_clip_notes(clip_id)
            .unwrap()
            .iter()
            .find(|n| n.id == id)
            .cloned()
            .unwrap()
    }

    #[test]
    fn edit_midi_notes_velocity_roundtrips() {
        let (mut state, clip_id) = state_with_midi_clip();
        let id = state.add_midi_note(&clip_id, 60, 0.0, 1.0, 100).unwrap();

        let prev = state.midi_clip_notes(&clip_id).unwrap().clone();
        state.set_midi_notes_velocity_bulk(&clip_id, &[id], 40);
        let next = state.midi_clip_notes(&clip_id).unwrap().clone();
        assert_eq!(note(&state, &clip_id, id).velocity, 40);

        let cmd = EditCommand::EditMidiNotes {
            clip_id: clip_id.clone(),
            prev,
            next,
        };
        cmd.undo(&mut state);
        assert_eq!(note(&state, &clip_id, id).velocity, 100, "undo restores");
        cmd.execute(&mut state);
        assert_eq!(note(&state, &clip_id, id).velocity, 40, "redo reapplies");
    }

    #[test]
    fn edit_midi_notes_move_roundtrips() {
        let (mut state, clip_id) = state_with_midi_clip();
        let id = state.add_midi_note(&clip_id, 60, 0.0, 1.0, 100).unwrap();

        let prev = state.midi_clip_notes(&clip_id).unwrap().clone();
        state.move_midi_notes(&clip_id, &[(id, 2.0, 67)]);
        let next = state.midi_clip_notes(&clip_id).unwrap().clone();

        let cmd = EditCommand::EditMidiNotes {
            clip_id: clip_id.clone(),
            prev,
            next,
        };
        cmd.undo(&mut state);
        let n = note(&state, &clip_id, id);
        assert_eq!((n.start, n.pitch), (0.0, 60), "undo restores start+pitch");
        cmd.execute(&mut state);
        let n = note(&state, &clip_id, id);
        assert_eq!((n.start, n.pitch), (2.0, 67), "redo reapplies");
    }

    #[test]
    fn controller_point_edit_and_undo_roundtrip() {
        let (mut state, clip_id) = state_with_midi_clip();
        let kind = MidiControllerKind::CC(1);
        let prev = state.controller_points_snapshot(&clip_id, kind);
        state.put_controller_point(&clip_id, kind, 1.0, 0.5);
        state.put_controller_point(&clip_id, kind, 2.0, 0.75);
        let next = state.controller_points_snapshot(&clip_id, kind);
        assert_eq!(next.len(), 2);

        let cmd = EditCommand::SetControllerPoints {
            clip_id: clip_id.clone(),
            kind,
            prev,
            next,
        };
        cmd.undo(&mut state);
        assert_eq!(
            state.controller_points_snapshot(&clip_id, kind).len(),
            0,
            "undo clears the lane"
        );
        cmd.execute(&mut state);
        assert_eq!(
            state.controller_points_snapshot(&clip_id, kind).len(),
            2,
            "redo restores points"
        );
    }

    #[test]
    fn put_controller_point_merges_within_epsilon() {
        let (mut state, clip_id) = state_with_midi_clip();
        let kind = MidiControllerKind::CC(7);
        state.put_controller_point(&clip_id, kind, 1.0, 0.2);
        state.put_controller_point(&clip_id, kind, 1.0, 0.9);
        let pts = state.controller_points_snapshot(&clip_id, kind);
        assert_eq!(pts.len(), 1, "same-beat edit updates in place");
        assert_eq!(pts[0].value, 0.9);
    }

    #[test]
    fn set_controller_point_moves_in_place() {
        let (mut state, clip_id) = state_with_midi_clip();
        let kind = MidiControllerKind::CC(1);
        state.put_controller_point(&clip_id, kind, 1.0, 0.5);
        let id = state.controller_points_snapshot(&clip_id, kind)[0].id;
        assert!(state.set_controller_point(&clip_id, kind, id, 3.0, 0.25));
        let snapshot = state.controller_points_snapshot(&clip_id, kind);
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].beat, 3.0);
        assert_eq!(snapshot[0].value, 0.25);
        assert_eq!(snapshot[0].id, id, "id is preserved across a move");
    }

    #[test]
    fn delete_controller_points_near_removes_in_tolerance() {
        let (mut state, clip_id) = state_with_midi_clip();
        let kind = MidiControllerKind::CC(11);
        state.put_controller_point(&clip_id, kind, 1.0, 0.5);
        state.put_controller_point(&clip_id, kind, 3.0, 0.5);
        let removed = state.delete_controller_points_near(&clip_id, kind, 1.05, 0.25);
        assert_eq!(removed, 1);
        assert_eq!(state.controller_points_snapshot(&clip_id, kind).len(), 1);
    }

    #[test]
    fn set_midi_notes_muted_roundtrips() {
        let (mut state, clip_id) = state_with_midi_clip();
        let id = state.add_midi_note(&clip_id, 60, 0.0, 1.0, 100).unwrap();
        assert!(!note(&state, &clip_id, id).muted);

        let cmd = EditCommand::SetMidiNotesMuted {
            clip_id: clip_id.clone(),
            prev: vec![(id, false)],
            muted: true,
        };
        cmd.execute(&mut state);
        assert!(note(&state, &clip_id, id).muted, "execute mutes");
        cmd.undo(&mut state);
        assert!(!note(&state, &clip_id, id).muted, "undo unmutes");
    }

    #[test]
    fn split_midi_note_roundtrips() {
        let (mut state, clip_id) = state_with_midi_clip();
        let id = state.add_midi_note(&clip_id, 60, 0.0, 2.0, 100).unwrap();
        let original = note(&state, &clip_id, id).clone();
        let left = MidiNoteState::new(60, 0.0, 1.0, 100);
        let right = MidiNoteState::new(60, 1.0, 1.0, 100);
        let (left_id, right_id) = (left.id, right.id);

        let cmd = EditCommand::SplitMidiNote {
            clip_id: clip_id.clone(),
            original,
            parts: vec![left, right],
        };
        cmd.execute(&mut state);
        let notes = state.midi_clip_notes(&clip_id).unwrap();
        assert!(notes.iter().all(|n| n.id != id), "original removed");
        assert!(notes.iter().any(|n| n.id == left_id), "left part added");
        assert!(notes.iter().any(|n| n.id == right_id), "right part added");

        cmd.undo(&mut state);
        let notes = state.midi_clip_notes(&clip_id).unwrap();
        assert!(notes.iter().any(|n| n.id == id), "undo restores original");
        assert!(
            notes.iter().all(|n| n.id != left_id && n.id != right_id),
            "undo removes both parts"
        );
    }
}
