//! Audio-clip time-stretch / pitch state and the pure math that drives it.
//!
//! This is **clip-level, non-destructive** playback-transform metadata. It never
//! mutates the source audio; the playback/export processors (added in a later
//! slice) read this state to transform the source on the fly. The data model and
//! math live here, decoupled from the audio engine, so they can be unit-tested in
//! isolation and serialized without pulling in realtime code.
//!
//! Source of truth: `AudioClipStretchState` lives on [`super::ClipState`]. It is
//! present on every clip but only meaningful for audio clips — MIDI clips carry a
//! default (`StretchMode::Off`) instance that is ignored.

/// How an audio clip's playback timing is transformed.
///
/// See the per-variant docs and `tasks` spec §2 for behaviour. Tags are stable
/// for serialization via [`StretchMode::to_tag`] / [`StretchMode::from_tag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StretchMode {
    /// No time stretch. Playback rate is normal; clip duration follows the
    /// source duration at the current project sample rate.
    #[default]
    Off,
    /// Classic sampler/tape behaviour: changing clip length changes pitch.
    /// No pitch preservation.
    Resample,
    /// Clip follows project tempo (loops). `stretch_ratio = source_bpm /
    /// project_bpm`. Constant-tempo today; API is shaped for tempo maps later.
    TempoSync,
    /// User sets duration / ratio / percent directly; the three stay in sync.
    Manual,
    /// Warp-marker mode. Marker data is stored and rendered now; per-segment
    /// warp DSP is pending and playback falls back to Manual-style stretch.
    Warp,
}

impl StretchMode {
    pub fn to_tag(self) -> u8 {
        match self {
            StretchMode::Off => 0,
            StretchMode::Resample => 1,
            StretchMode::TempoSync => 2,
            StretchMode::Manual => 3,
            StretchMode::Warp => 4,
        }
    }

    pub fn from_tag(tag: u8) -> Self {
        match tag {
            1 => StretchMode::Resample,
            2 => StretchMode::TempoSync,
            3 => StretchMode::Manual,
            4 => StretchMode::Warp,
            // Unknown / 0 → Off (also the backward-compat default).
            _ => StretchMode::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StretchMode::Off => "Off",
            StretchMode::Resample => "Resample",
            StretchMode::TempoSync => "Tempo Sync",
            StretchMode::Manual => "Manual",
            StretchMode::Warp => "Warp",
        }
    }
}

/// Stretch algorithm selection.
///
/// Only some variants are backed by real DSP today; the rest are honest
/// placeholders that alias onto an implemented algorithm until their dedicated
/// DSP lands (see the per-variant notes). The processor slice maps these tags to
/// concrete processors; the UI must not claim an aliased mode is its own engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StretchAlgorithm {
    /// Pick an algorithm from content type. Resolved by the processor slice.
    #[default]
    Auto,
    /// Reserved high-quality mode. **Aliases to PhaseVocoder** until a dedicated
    /// élastique-class engine is integrated.
    ElastiqueLike,
    /// General-purpose musical material. **Real** (basic phase vocoder) once the
    /// processor slice lands.
    PhaseVocoder,
    /// Drums / percussive material. **Aliases to PhaseVocoder** until transient-
    /// aware stretching is implemented.
    Transient,
    /// Vocals / monophonic instruments. **Aliases to PhaseVocoder** for now.
    Solo,
    /// Pads / ambience. **Aliases to PhaseVocoder** for now.
    Texture,
    /// Simple sample-rate / playback-rate conversion. **Real** resampling.
    ResampleOnly,
}

impl StretchAlgorithm {
    pub fn to_tag(self) -> u8 {
        match self {
            StretchAlgorithm::Auto => 0,
            StretchAlgorithm::ElastiqueLike => 1,
            StretchAlgorithm::PhaseVocoder => 2,
            StretchAlgorithm::Transient => 3,
            StretchAlgorithm::Solo => 4,
            StretchAlgorithm::Texture => 5,
            StretchAlgorithm::ResampleOnly => 6,
        }
    }

    pub fn from_tag(tag: u8) -> Self {
        match tag {
            1 => StretchAlgorithm::ElastiqueLike,
            2 => StretchAlgorithm::PhaseVocoder,
            3 => StretchAlgorithm::Transient,
            4 => StretchAlgorithm::Solo,
            5 => StretchAlgorithm::Texture,
            6 => StretchAlgorithm::ResampleOnly,
            _ => StretchAlgorithm::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StretchAlgorithm::Auto => "Auto",
            StretchAlgorithm::ElastiqueLike => "Élastique",
            StretchAlgorithm::PhaseVocoder => "Phase Vocoder",
            StretchAlgorithm::Transient => "Transient",
            StretchAlgorithm::Solo => "Solo",
            StretchAlgorithm::Texture => "Texture",
            StretchAlgorithm::ResampleOnly => "Resample Only",
        }
    }
}

/// A warp marker pinning a source sample position to a timeline beat.
///
/// Stored and rendered now; per-segment warp playback is pending (see
/// [`StretchMode::Warp`]).
#[derive(Debug, Clone, PartialEq)]
pub struct WarpMarker {
    pub id: u64,
    pub source_sample: u64,
    pub timeline_beat: f64,
    pub locked: bool,
}

/// Map an output-local sample position inside a stretched clip back to the
/// immutable source-window sample. This is the UI-side equivalent of the engine
/// clip source-position mapping.
pub fn clip_output_local_to_source_sample(
    output_local_sample: f64,
    source_start: u64,
    source_end: u64,
    effective_time_ratio: f64,
    reverse: bool,
) -> f64 {
    let ratio = if effective_time_ratio.is_finite() {
        effective_time_ratio.max(1e-6)
    } else {
        1.0
    };
    let advance = output_local_sample.max(0.0) / ratio;
    if reverse {
        (source_end as f64 - advance).max(source_start as f64)
    } else {
        (source_start as f64 + advance).min(source_end as f64)
    }
}

pub fn warp_timeline_beat_to_source_sample(
    timeline_beat: f64,
    source_start: u64,
    source_end: u64,
    global_ratio: f64,
    markers: &[WarpMarker],
) -> f64 {
    let source_start_f = source_start as f64;
    let source_end_f = source_end.max(source_start) as f64;
    if markers.is_empty() {
        return clip_output_local_to_source_sample(
            timeline_beat.max(0.0),
            source_start,
            source_end,
            global_ratio,
            false,
        );
    }

    if timeline_beat <= markers[0].timeline_beat {
        return markers[0]
            .source_sample
            .clamp(source_start, source_end.max(source_start)) as f64;
    }
    for pair in markers.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        if timeline_beat >= a.timeline_beat && timeline_beat <= b.timeline_beat {
            let span = (b.timeline_beat - a.timeline_beat).max(f64::EPSILON);
            let t = ((timeline_beat - a.timeline_beat) / span).clamp(0.0, 1.0);
            let source =
                a.source_sample as f64 + (b.source_sample as f64 - a.source_sample as f64) * t;
            return source.clamp(source_start_f, source_end_f);
        }
    }
    markers
        .last()
        .map(|marker| {
            marker
                .source_sample
                .clamp(source_start, source_end.max(source_start)) as f64
        })
        .unwrap_or(source_start_f)
}

/// Non-destructive, clip-level stretch + pitch + clip-processing state.
///
/// Fields mirror the `tasks` spec §1. Note that `clip_timeline_start_beats` /
/// `clip_timeline_duration_beats` are an informational cache of the owning
/// clip's timeline placement — the authoritative position stays on
/// [`super::ClipState`] (`start_beat` / `duration_beats`). Likewise `gain_db`,
/// `pan`, and the fade fields are clip-processing values whose reconciliation
/// with the existing linear `ClipState::gain` is owned by the inspector/playback
/// slices; here they are inert stored data.
///
/// `dirty` is a transient "needs re-process / re-render" flag and is **not**
/// persisted (it always loads as `false`).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioClipStretchState {
    pub mode: StretchMode,
    pub algorithm: StretchAlgorithm,

    pub original_sample_rate: u32,
    pub project_sample_rate: u32,

    pub original_duration_samples: u64,
    pub source_start_samples: u64,
    pub source_end_samples: u64,

    pub clip_timeline_start_beats: f64,
    pub clip_timeline_duration_beats: f64,

    pub stretch_ratio: f64,
    pub bpm_source: Option<f64>,
    pub bpm_target: Option<f64>,

    pub preserve_pitch: bool,
    pub pitch_shift_semitones: f32,
    pub formant_preserve: bool,

    pub transient_preserve: bool,
    pub transient_sensitivity: f32,

    pub reverse: bool,
    pub normalize_gain: bool,

    pub fade_in_ms: f32,
    pub fade_out_ms: f32,

    pub gain_db: f32,
    pub pan: f32,

    pub dirty: bool,

    /// Warp markers (spec §2 Warp). Stored and rendered; warp DSP pending.
    pub warp_markers: Vec<WarpMarker>,
}

impl Default for AudioClipStretchState {
    fn default() -> Self {
        // Backward-compat load defaults (spec §13): a clip with no stretch info
        // is an un-stretched, pitch-preserving clip at 1.0×.
        Self {
            mode: StretchMode::Off,
            algorithm: StretchAlgorithm::Auto,
            original_sample_rate: 0,
            project_sample_rate: 0,
            original_duration_samples: 0,
            source_start_samples: 0,
            source_end_samples: 0,
            clip_timeline_start_beats: 0.0,
            clip_timeline_duration_beats: 0.0,
            stretch_ratio: 1.0,
            bpm_source: None,
            bpm_target: None,
            preserve_pitch: true,
            pitch_shift_semitones: 0.0,
            formant_preserve: false,
            transient_preserve: true,
            transient_sensitivity: 0.5,
            reverse: false,
            normalize_gain: false,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            gain_db: 0.0,
            pan: 0.0,
            dirty: false,
            warp_markers: Vec::new(),
        }
    }
}

impl AudioClipStretchState {
    /// Clamp bounds for a sane, non-zero, finite stretch ratio.
    pub const MIN_RATIO: f64 = 0.05;
    pub const MAX_RATIO: f64 = 20.0;

    // ── Pure math helpers (no clamping — exact for tests/spec §18) ──────────

    /// `100% → 1.0`, `200% → 2.0`, `50% → 0.5`.
    pub fn ratio_from_percent(percent: f64) -> f64 {
        percent / 100.0
    }

    /// Inverse of [`ratio_from_percent`]. `1.0 → 100%`, `2.0 → 200%`.
    pub fn percent_from_ratio(ratio: f64) -> f64 {
        ratio * 100.0
    }

    /// TempoSync ratio = `source_bpm / project_bpm` (spec §2 TempoSync). A
    /// non-positive project tempo degrades to `1.0` rather than dividing by zero.
    pub fn source_bpm_to_project_bpm_ratio(source_bpm: f64, project_bpm: f64) -> f64 {
        if project_bpm.abs() < f64::EPSILON {
            1.0
        } else {
            source_bpm / project_bpm
        }
    }

    /// Pitch multiplier from semitones: `2^(semitones / 12)`. `+12 → 2.0`,
    /// `-12 → 0.5` (spec §6).
    pub fn pitch_ratio_from_semitones(semitones: f32) -> f64 {
        2.0_f64.powf(semitones as f64 / 12.0)
    }

    // ── Derived getters ────────────────────────────────────────────────────

    /// Current stretch as a percentage (`stretch_ratio * 100`).
    pub fn stretch_percent(&self) -> f64 {
        Self::percent_from_ratio(self.stretch_ratio)
    }

    /// Length of the active source window in samples.
    pub fn source_len_samples(&self) -> u64 {
        self.source_end_samples
            .saturating_sub(self.source_start_samples)
    }

    /// Ratio actually applied to playback timing. `Off` is always `1.0`
    /// regardless of the stored `stretch_ratio`.
    pub fn effective_ratio(&self) -> f64 {
        match self.mode {
            StretchMode::Off => 1.0,
            _ => self.stretch_ratio,
        }
    }

    /// Effective playback duration of the source window after stretching, in
    /// samples. `ratio 2.0` → twice as long; `ratio 0.5` → half (spec §2 Manual).
    pub fn effective_duration_samples(&self) -> u64 {
        let len = self.source_len_samples() as f64;
        (len * self.effective_ratio()).round().max(0.0) as u64
    }

    /// Time-stretch ratio actually used for playback / clip length, resolving
    /// `TempoSync` against the project tempo. `Off` → `1.0`; `Warp` falls back to
    /// the stored manual ratio; `TempoSync` with no source BPM → `1.0`.
    ///
    /// This is the single source of truth shared by the inspector (clip-length
    /// coupling), the engine snapshot (`speed_ratio`), and tests, so visual and
    /// audible length never diverge.
    pub fn effective_time_ratio(&self, project_bpm: f64) -> f64 {
        match self.mode {
            StretchMode::Off => 1.0,
            StretchMode::Resample | StretchMode::Manual | StretchMode::Warp => self.stretch_ratio,
            StretchMode::TempoSync => match self.bpm_source {
                Some(source_bpm) => Self::source_bpm_to_project_bpm_ratio(source_bpm, project_bpm),
                None => 1.0,
            },
        }
    }

    /// Source-read rate (source samples consumed per output sample) for the
    /// resample DSP path: folds the time-stretch reciprocal with the explicit
    /// pitch shift — `speed = pitch_ratio / time_ratio`. Clamped to the engine's
    /// accepted `speed_ratio` range.
    ///
    /// `preserve_pitch` does not change this number yet: the pitch-preserving
    /// processor is a basic fallback that still resamples (see the engine's
    /// `resolve_clip_processor`), so the flag affects processor *selection* and
    /// diagnostics, not the math, until a real time-stretcher lands.
    pub fn resample_speed_ratio(&self, project_bpm: f64) -> f64 {
        let ratio = self.effective_time_ratio(project_bpm).max(1e-6);
        let pitch = Self::pitch_ratio_from_semitones(self.pitch_shift_semitones);
        (pitch / ratio).clamp(0.01, 16.0)
    }

    /// Whether changing clip length changes pitch (tape-style), given the mode
    /// and `preserve_pitch` (spec §6 behaviour matrix).
    pub fn pitch_linked_to_duration(&self) -> bool {
        match self.mode {
            StretchMode::Resample => true,
            StretchMode::Manual | StretchMode::TempoSync | StretchMode::Warp => {
                !self.preserve_pitch
            }
            StretchMode::Off => false,
        }
    }

    /// Net pitch multiplier applied to playback, combining the explicit semitone
    /// shift with the duration-linked component when pitch is not preserved.
    pub fn playback_pitch_ratio(&self) -> f64 {
        let semis = Self::pitch_ratio_from_semitones(self.pitch_shift_semitones);
        if self.pitch_linked_to_duration() {
            let r = self.effective_ratio();
            if r.abs() < f64::EPSILON {
                semis
            } else {
                // Stretching longer (ratio > 1) reads the source slower → lower
                // pitch, hence the reciprocal.
                semis / r
            }
        } else {
            semis
        }
    }

    // ── Mutators (clamped; mark dirty) ─────────────────────────────────────

    /// Set the stretch ratio, clamped to `[MIN_RATIO, MAX_RATIO]`.
    pub fn set_stretch_ratio(&mut self, ratio: f64) {
        let clamped = if ratio.is_finite() {
            ratio.clamp(Self::MIN_RATIO, Self::MAX_RATIO)
        } else {
            1.0
        };
        if (self.stretch_ratio - clamped).abs() > f64::EPSILON {
            self.stretch_ratio = clamped;
            self.dirty = true;
        }
    }

    /// Set the stretch by percent (`200%` → ratio `2.0`).
    pub fn set_stretch_percent(&mut self, percent: f64) {
        self.set_stretch_ratio(Self::ratio_from_percent(percent));
    }

    /// Trim adjusts the active source window but **never** the stretch ratio
    /// (spec §7 — normal edge drag changes the visible source range only).
    pub fn apply_trim(&mut self, source_start_samples: u64, source_end_samples: u64) {
        self.source_start_samples = source_start_samples;
        self.source_end_samples = source_end_samples.max(source_start_samples);
        self.dirty = true;
        // stretch_ratio intentionally left unchanged.
    }

    /// Stretch-drag sets a new timeline length (in samples) for the same source
    /// window and recomputes the ratio so the window fills it (spec §7 — stretch
    /// edge drag keeps the source range, updates `stretch_ratio`).
    pub fn apply_stretch_to_timeline_samples(&mut self, new_timeline_len_samples: u64) {
        let src = self.source_len_samples();
        if src > 0 {
            self.set_stretch_ratio(new_timeline_len_samples as f64 / src as f64);
        }
    }

    /// Apply constant-tempo sync against a project tempo: stores the target tempo
    /// and sets the ratio from the source/target BPM pair.
    pub fn apply_tempo_sync(&mut self, project_bpm: f64) {
        self.bpm_target = Some(project_bpm);
        if let Some(source_bpm) = self.bpm_source {
            self.set_stretch_ratio(Self::source_bpm_to_project_bpm_ratio(
                source_bpm,
                project_bpm,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    /// A Manual-mode clip over a fixed source window, for duration/ratio tests.
    fn manual_clip(source_len: u64) -> AudioClipStretchState {
        AudioClipStretchState {
            mode: StretchMode::Manual,
            source_start_samples: 0,
            source_end_samples: source_len,
            ..AudioClipStretchState::default()
        }
    }

    #[test]
    fn stretch_ratio_from_percent() {
        approx(AudioClipStretchState::ratio_from_percent(100.0), 1.0);
        approx(AudioClipStretchState::ratio_from_percent(200.0), 2.0);
        approx(AudioClipStretchState::ratio_from_percent(50.0), 0.5);
    }

    #[test]
    fn percent_from_ratio() {
        approx(AudioClipStretchState::percent_from_ratio(1.0), 100.0);
        approx(AudioClipStretchState::percent_from_ratio(2.0), 200.0);
        approx(AudioClipStretchState::percent_from_ratio(0.5), 50.0);
    }

    #[test]
    fn source_bpm_to_project_bpm_ratio() {
        // 120 BPM source loop in a 140 BPM project → ratio 120 / 140 (faster).
        approx(
            AudioClipStretchState::source_bpm_to_project_bpm_ratio(120.0, 140.0),
            120.0 / 140.0,
        );
        // Degenerate project tempo degrades to 1.0 rather than dividing by zero.
        approx(
            AudioClipStretchState::source_bpm_to_project_bpm_ratio(120.0, 0.0),
            1.0,
        );
    }

    #[test]
    fn pitch_ratio_from_semitones() {
        approx(AudioClipStretchState::pitch_ratio_from_semitones(0.0), 1.0);
        approx(AudioClipStretchState::pitch_ratio_from_semitones(12.0), 2.0);
        approx(
            AudioClipStretchState::pitch_ratio_from_semitones(-12.0),
            0.5,
        );
    }

    #[test]
    fn clip_duration_after_manual_stretch() {
        let mut s = manual_clip(1000);
        s.set_stretch_percent(200.0); // twice as long / slower
        approx(s.stretch_ratio, 2.0);
        assert_eq!(s.effective_duration_samples(), 2000);

        s.set_stretch_percent(50.0); // half length / faster
        approx(s.stretch_ratio, 0.5);
        assert_eq!(s.effective_duration_samples(), 500);
    }

    #[test]
    fn trim_does_not_change_stretch_ratio() {
        let mut s = manual_clip(1000);
        s.set_stretch_ratio(1.5);
        s.apply_trim(100, 600);
        // The ratio is unchanged; only the source window moved (spec §7).
        approx(s.stretch_ratio, 1.5);
        assert_eq!(s.source_len_samples(), 500);
    }

    #[test]
    fn stretch_drag_does_change_stretch_ratio() {
        let mut s = manual_clip(1000);
        // Dragging the edge so the same 1000-sample window fills 1500 samples
        // of timeline → ratio 1.5 (spec §7).
        s.apply_stretch_to_timeline_samples(1500);
        approx(s.stretch_ratio, 1.5);
        assert_eq!(s.source_len_samples(), 1000);
    }

    #[test]
    fn off_mode_ignores_ratio_for_effective_duration() {
        let mut s = manual_clip(1000);
        s.set_stretch_ratio(2.0);
        s.mode = StretchMode::Off;
        // Off always plays 1:1 regardless of the stored ratio.
        approx(s.effective_ratio(), 1.0);
        assert_eq!(s.effective_duration_samples(), 1000);
    }

    #[test]
    fn resample_links_pitch_to_duration() {
        let mut s = manual_clip(1000);
        s.mode = StretchMode::Resample;
        s.set_stretch_ratio(2.0); // twice as long → an octave down
        approx(s.playback_pitch_ratio(), 0.5);
    }

    #[test]
    fn preserve_pitch_decouples_pitch_from_duration() {
        let mut s = manual_clip(1000);
        s.preserve_pitch = true;
        s.set_stretch_ratio(2.0);
        s.pitch_shift_semitones = 12.0; // explicit +1 octave only
        approx(s.playback_pitch_ratio(), 2.0);
    }

    #[test]
    fn output_to_source_maps_stretched_halfway_to_source_quarter() {
        approx(
            clip_output_local_to_source_sample(500.0, 0, 1_000, 2.0, false),
            250.0,
        );
    }

    #[test]
    fn output_to_source_maps_compressed_faster_through_source() {
        approx(
            clip_output_local_to_source_sample(250.0, 0, 1_000, 0.5, false),
            500.0,
        );
    }

    #[test]
    fn output_to_source_reverse_starts_at_source_end() {
        approx(
            clip_output_local_to_source_sample(0.0, 0, 1_000, 2.0, true),
            1_000.0,
        );
        approx(
            clip_output_local_to_source_sample(500.0, 0, 1_000, 2.0, true),
            750.0,
        );
    }

    #[test]
    fn output_to_source_honors_trimmed_source_window() {
        approx(
            clip_output_local_to_source_sample(400.0, 100, 900, 2.0, false),
            300.0,
        );
    }

    #[test]
    fn warp_without_markers_uses_global_ratio() {
        approx(
            warp_timeline_beat_to_source_sample(500.0, 0, 1_000, 2.0, &[]),
            250.0,
        );
    }

    #[test]
    fn warp_with_two_markers_maps_linearly_between_them() {
        let markers = vec![
            WarpMarker {
                id: 1,
                source_sample: 100,
                timeline_beat: 1.0,
                locked: false,
            },
            WarpMarker {
                id: 2,
                source_sample: 900,
                timeline_beat: 5.0,
                locked: false,
            },
        ];
        approx(
            warp_timeline_beat_to_source_sample(3.0, 0, 1_000, 1.0, &markers),
            500.0,
        );
    }

    #[test]
    fn effective_time_ratio_resolves_modes() {
        let mut s = manual_clip(1000);
        s.set_stretch_ratio(1.5);
        approx(s.effective_time_ratio(120.0), 1.5);

        s.mode = StretchMode::Off;
        approx(s.effective_time_ratio(120.0), 1.0);

        s.mode = StretchMode::TempoSync;
        s.bpm_source = Some(120.0);
        approx(s.effective_time_ratio(140.0), 120.0 / 140.0);
        s.bpm_source = None;
        approx(s.effective_time_ratio(140.0), 1.0);
    }

    #[test]
    fn resample_speed_ratio_folds_time_and_pitch() {
        let mut s = manual_clip(1000);
        // ratio 2.0 (twice as long) → read source at half speed.
        s.set_stretch_ratio(2.0);
        approx(s.resample_speed_ratio(120.0), 0.5);
        // ratio 0.5 (half length) → read source twice as fast.
        s.set_stretch_ratio(0.5);
        approx(s.resample_speed_ratio(120.0), 2.0);
        // +12 semitones with no time stretch → read twice as fast (octave up).
        s.set_stretch_ratio(1.0);
        s.pitch_shift_semitones = 12.0;
        approx(s.resample_speed_ratio(120.0), 2.0);
        // Off mode ignores the stored ratio for the time component.
        s.mode = StretchMode::Off;
        s.pitch_shift_semitones = 0.0;
        approx(s.resample_speed_ratio(120.0), 1.0);
    }

    #[test]
    fn set_stretch_ratio_clamps_to_bounds() {
        let mut s = manual_clip(1000);
        s.set_stretch_ratio(1000.0);
        approx(s.stretch_ratio, AudioClipStretchState::MAX_RATIO);
        s.set_stretch_ratio(0.0);
        approx(s.stretch_ratio, AudioClipStretchState::MIN_RATIO);
        s.set_stretch_ratio(f64::NAN);
        approx(s.stretch_ratio, 1.0);
    }
}
