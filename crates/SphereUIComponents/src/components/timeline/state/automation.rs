use super::*;

/// Two automation points within this many beats are treated as the same slot
/// (a second add at the same beat replaces the existing point's value).
pub const AUTOMATION_BEAT_EPSILON: f32 = 1.0e-3;

/// Vertical padding (px) kept at the top/bottom of an automation lane so the
/// extreme 0.0/1.0 values never sit exactly on the lane border.
pub const AUTOMATION_LANE_PAD: f32 = 8.0;

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
}
