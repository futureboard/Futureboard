//! Neural Amp Modeler (NAM) capture engine — a distinct processor from the
//! classic modeled [`super::amp::Amp`], selectable as an alternative engine for
//! the same Tone/Amp slot (see [`super::ToneEngineKind`]).
//!
//! Loading a `.nam` file parses JSON and builds a neural network — real
//! allocation, definitely not audio-thread work. [`prepare_nam_runtime`] does
//! that on the control thread and hands back a [`PreparedNamRuntime`] the
//! caller boxes and pushes into [`NamCapture::submit`]. The audio thread only
//! ever adopts it at a block boundary ([`NamCapture::begin_block`]); the
//! previous runtime is cross-faded out over a short window, then handed back
//! to the control thread ([`NamCapture::poll_garbage`]) to actually drop —
//! never inside [`NamCapture::process`].

use builtin_dsp_core::make_eq_biquad;
use nam_rs::{Model, NamModel};

use super::handoff::HandoffCell;
use super::StereoBiquad;

/// Target integrated loudness (LUFS) captures are normalized to when loudness
/// normalization is enabled, matching the reference NAM plugin's convention.
const TARGET_LUFS: f32 = -18.0;

/// A `.nam` file's declared sample rate must match the engine's to within this
/// many Hz, else the load is rejected (nam-rs does not resample; a mismatch
/// silently mis-runs the model's dilations/recurrence otherwise).
const SAMPLE_RATE_TOLERANCE_HZ: f64 = 0.5;

/// Roughly how long a runtime swap crossfades for, in milliseconds.
const SWAP_FADE_MS: f32 = 8.0;

/// A `.nam` failed to parse/build, or its sample rate doesn't match the engine.
#[derive(Debug)]
pub enum NamLoadError {
    Parse(nam_rs::Error),
    SampleRateMismatch { expected: f64, engine: f64 },
}

impl std::fmt::Display for NamLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NamLoadError::Parse(e) => write!(f, "NAM capture failed to load: {e}"),
            NamLoadError::SampleRateMismatch { expected, engine } => write!(
                f,
                "NAM capture expects {expected} Hz but the engine runs at {engine} Hz \
                 (nam-rs does not resample — reload at a matching rate)"
            ),
        }
    }
}

impl std::error::Error for NamLoadError {}

/// Info handed back to the host/UI after a successful load — enough to show
/// the capture's name, warn about startup latency, and offer "Bypass Cab" when
/// the capture already models a full rig (amp + cab + mic).
#[derive(Debug, Clone)]
pub struct NamCaptureInfo {
    pub name: String,
    pub full_rig: bool,
    pub receptive_field: usize,
    pub sample_rate: f64,
}

/// A fully-built, ready-to-run capture. Boxed and moved into [`NamCapture`]'s
/// hand-off cell; built entirely on the control thread by
/// [`prepare_nam_runtime`].
pub struct PreparedNamRuntime {
    name: String,
    model_l: Model,
    /// `None` for a mono capture: the single model's output is mirrored to
    /// both channels rather than running two redundant inferences.
    model_r: Option<Model>,
    sample_rate: f64,
    receptive_field: usize,
    /// Precomputed linear gain to bring the capture to [`TARGET_LUFS`], or
    /// `1.0` if the file carries no loudness metadata. Computed once here so
    /// the hot path is a single multiply, not a per-sample dB calculation.
    loudness_gain: f32,
    full_rig: bool,
}

impl std::fmt::Debug for PreparedNamRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedNamRuntime")
            .field("name", &self.name)
            .field("sample_rate", &self.sample_rate)
            .field("receptive_field", &self.receptive_field)
            .field("full_rig", &self.full_rig)
            .finish_non_exhaustive()
    }
}

/// Parse and build a `.nam` capture off the audio thread. `stereo` selects
/// whether a second, independent model is built for the right channel (true
/// stereo width) or the left model's output is mirrored (mono bus / cheaper).
/// Rejects a sample-rate mismatch outright rather than silently mis-running —
/// a resampling adapter is a documented future addition, not implemented yet.
pub fn prepare_nam_runtime(
    json: &str,
    name: String,
    engine_sample_rate: f64,
    stereo: bool,
    full_rig: bool,
) -> Result<PreparedNamRuntime, NamLoadError> {
    let nam_model = NamModel::from_json_str(json).map_err(NamLoadError::Parse)?;
    let expected = nam_model.expected_sample_rate();
    if (expected - engine_sample_rate).abs() > SAMPLE_RATE_TOLERANCE_HZ {
        return Err(NamLoadError::SampleRateMismatch {
            expected,
            engine: engine_sample_rate,
        });
    }

    let model_l = Model::from_nam(&nam_model).map_err(NamLoadError::Parse)?;
    let model_r = if stereo {
        Some(Model::from_nam(&nam_model).map_err(NamLoadError::Parse)?)
    } else {
        None
    };
    let receptive_field = model_l.receptive_field();
    let loudness_gain = nam_model
        .loudness()
        .map(|l| 10f32.powf((TARGET_LUFS - l) / 20.0).clamp(0.05, 20.0))
        .unwrap_or(1.0);

    Ok(PreparedNamRuntime {
        name,
        model_l,
        model_r,
        sample_rate: expected,
        receptive_field,
        loudness_gain,
        full_rig,
    })
}

impl PreparedNamRuntime {
    pub fn info(&self) -> NamCaptureInfo {
        NamCaptureInfo {
            name: self.name.clone(),
            full_rig: self.full_rig,
            receptive_field: self.receptive_field,
            sample_rate: self.sample_rate,
        }
    }

    #[inline]
    fn process(&mut self, left: f32, right: f32, loudness_on: bool) -> (f32, f32) {
        let gain = if loudness_on { self.loudness_gain } else { 1.0 };
        let l = self.model_l.process_sample(left) * gain;
        let r = match self.model_r.as_mut() {
            Some(model_r) => model_r.process_sample(right) * gain,
            None => l,
        };
        (l, r)
    }

    fn reset(&mut self) {
        self.model_l.reset();
        if let Some(model_r) = self.model_r.as_mut() {
            model_r.reset();
        }
    }
}

/// The audio-thread-resident NAM engine: a preallocated DC blocker, live trim/
/// mix/loudness knobs, and the lock-free hand-off machinery that lets the
/// control thread swap in a freshly-built [`PreparedNamRuntime`] without ever
/// blocking or allocating on the audio thread.
pub(super) struct NamCapture {
    active: Option<Box<PreparedNamRuntime>>,
    /// The just-replaced runtime, still running so [`Self::process`] can
    /// crossfade away from it instead of cutting over with a click.
    fading_out: Option<Box<PreparedNamRuntime>>,
    /// A retiree that didn't fit in `retired` (control thread hasn't drained
    /// it yet). Held here — never dropped on the audio thread — until a later
    /// block finds `retired` empty again.
    retire_overflow: Option<Box<PreparedNamRuntime>>,

    /// Control thread → audio thread: a freshly built runtime awaiting adoption.
    pending: HandoffCell<PreparedNamRuntime>,
    /// Audio thread → control thread: a retired runtime awaiting disposal.
    retired: HandoffCell<PreparedNamRuntime>,

    sample_rate: f32,
    dc_hpf: StereoBiquad,

    input_trim: f32,
    output_trim: f32,
    loudness_norm_on: bool,
    mix: f32,

    /// 0 = fully `fading_out`, 1 = fully `active`. Sits at 1.0 when no fade
    /// is in progress.
    fade: f32,
    fade_step: f32,
}

impl NamCapture {
    pub(super) fn new(sample_rate: f32) -> Self {
        let mut me = Self {
            active: None,
            fading_out: None,
            retire_overflow: None,
            pending: HandoffCell::new(),
            retired: HandoffCell::new(),
            sample_rate: sample_rate.max(1.0),
            dc_hpf: StereoBiquad::none(),
            input_trim: 1.0,
            output_trim: 1.0,
            loudness_norm_on: true,
            mix: 1.0,
            fade: 1.0,
            fade_step: 1.0,
        };
        me.recompute_sample_rate_derived();
        me
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.recompute_sample_rate_derived();
    }

    fn recompute_sample_rate_derived(&mut self) {
        self.dc_hpf
            .set(make_eq_biquad("highpass", 20.0, 0.0, 0.707, self.sample_rate));
        let fade_len = (self.sample_rate * (SWAP_FADE_MS / 1_000.0)).max(1.0);
        self.fade_step = 1.0 / fade_len;
    }

    pub(super) fn reset(&mut self) {
        self.dc_hpf.reset();
        if let Some(rt) = self.active.as_mut() {
            rt.reset();
        }
        if let Some(rt) = self.fading_out.as_mut() {
            rt.reset();
        }
    }

    /// Live knob update (control thread only): trims in dB, mix in 0..100 %.
    pub(super) fn configure(&mut self, input_trim_db: f32, output_trim_db: f32, mix_pct: f32, loudness_norm_on: bool) {
        self.input_trim = db_to_linear(input_trim_db);
        self.output_trim = db_to_linear(output_trim_db);
        self.mix = (mix_pct / 100.0).clamp(0.0, 1.0);
        self.loudness_norm_on = loudness_norm_on;
    }

    /// Control thread: push a freshly-built runtime for the audio thread to
    /// adopt at the next block boundary. Any not-yet-adopted runtime already
    /// waiting is dropped here (safe: the audio thread never touched it).
    pub(super) fn submit(&self, runtime: Box<PreparedNamRuntime>) {
        if let Some(bumped) = self.pending.put(runtime) {
            drop(bumped);
        }
    }

    /// Control thread: drop any runtime the audio thread has retired. Call
    /// periodically (e.g. an idle/UI timer); also safe to call before
    /// [`Self::submit`] as an opportunistic sweep.
    pub(super) fn poll_garbage(&mut self) {
        if let Some(dead) = self.retired.take() {
            drop(dead);
        }
    }

    /// Info about the currently active capture, if one is loaded.
    pub(super) fn active_info(&self) -> Option<NamCaptureInfo> {
        self.active.as_ref().map(|rt| rt.info())
    }

    /// Latency contributed by the active capture's receptive field, in
    /// samples (0 if none loaded or an LSTM capture, which has no warmup).
    pub(super) fn latency_samples(&self) -> usize {
        self.active.as_ref().map(|rt| rt.receptive_field).unwrap_or(0)
    }

    /// Audio thread: adopt a pending runtime and retire a finished fade.
    /// Called once per audio block, never per sample — this is the only place
    /// the swap happens.
    pub(super) fn begin_block(&mut self) {
        // Drain a previous overflow now that a block boundary has passed.
        if let Some(carry) = self.retire_overflow.take() {
            if let Some(bounced) = self.retired.put(carry) {
                self.retire_overflow = Some(bounced);
            }
        }

        // Only start a new swap once any in-progress fade has fully resolved,
        // so at most one runtime is ever "in flight" toward retirement.
        if self.fading_out.is_none() {
            if let Some(new_rt) = self.pending.take() {
                if let Some(old) = self.active.replace(new_rt) {
                    self.fading_out = Some(old);
                    self.fade = 0.0;
                }
            }
        }

        if self.fade >= 1.0 {
            if let Some(done) = self.fading_out.take() {
                if let Some(bounced) = self.retired.put(done) {
                    self.retire_overflow = Some(bounced);
                }
            }
        }
    }

    /// Audio thread hot path: input trim → model(s) → DC block → loudness →
    /// output trim → wet/dry mix. Crossfades against `fading_out` if a swap is
    /// in progress. No allocation, no locks, no swap logic (see
    /// [`Self::begin_block`]).
    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let dry = (left, right);
        let xin = (left * self.input_trim, right * self.input_trim);

        let new_out = match self.active.as_mut() {
            Some(rt) => rt.process(xin.0, xin.1, self.loudness_norm_on),
            None => xin,
        };

        let wet = if let Some(old_rt) = self.fading_out.as_mut() {
            let old_out = old_rt.process(xin.0, xin.1, self.loudness_norm_on);
            self.fade = (self.fade + self.fade_step).min(1.0);
            (
                old_out.0 * (1.0 - self.fade) + new_out.0 * self.fade,
                old_out.1 * (1.0 - self.fade) + new_out.1 * self.fade,
            )
        } else {
            new_out
        };

        let (mut ol, mut or) = self.dc_hpf.run(wet.0, wet.1);
        ol *= self.output_trim;
        or *= self.output_trim;

        let m = self.mix;
        (dry.0 * (1.0 - m) + ol * m, dry.1 * (1.0 - m) + or * m)
    }
}

#[inline]
fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY_WAVENET_48K: &str = r#"{
        "version": "0.5.4", "architecture": "WaveNet",
        "config": { "layers": [{
            "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
            "kernel_size": 1, "dilations": [1], "activation": "ReLU",
            "gated": false, "head_bias": false
        }], "head": null, "head_scale": 1.0 },
        "weights": [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        "sample_rate": 48000.0
    }"#;

    #[test]
    fn rejects_sample_rate_mismatch() {
        let err = prepare_nam_runtime(TINY_WAVENET_48K, "t".into(), 44_100.0, false, false)
            .expect_err("mismatched rate must be rejected");
        assert!(matches!(err, NamLoadError::SampleRateMismatch { .. }));
    }

    #[test]
    fn loads_and_processes_at_matching_rate() {
        let prepared = prepare_nam_runtime(TINY_WAVENET_48K, "t".into(), 48_000.0, false, false)
            .expect("matching rate must load");
        assert_eq!(prepared.model_r.is_some(), false);

        let mut cap = NamCapture::new(48_000.0);
        cap.configure(0.0, 0.0, 100.0, false);
        cap.submit(Box::new(prepared));
        cap.begin_block();
        for _ in 0..64 {
            let (l, r) = cap.process(0.1, -0.1);
            assert!(l.is_finite() && r.is_finite());
        }
        assert!(cap.active_info().is_some());
    }

    #[test]
    fn stereo_capture_builds_two_independent_models() {
        let prepared = prepare_nam_runtime(TINY_WAVENET_48K, "t".into(), 48_000.0, true, true)
            .expect("matching rate must load");
        assert!(prepared.model_r.is_some());
        assert!(prepared.full_rig);
    }

    #[test]
    fn swap_crossfades_without_dropping_on_audio_thread() {
        let mut cap = NamCapture::new(48_000.0);
        cap.configure(0.0, 0.0, 100.0, false);

        let first = prepare_nam_runtime(TINY_WAVENET_48K, "a".into(), 48_000.0, false, false).unwrap();
        cap.submit(Box::new(first));
        cap.begin_block();
        for _ in 0..8 {
            cap.process(0.2, 0.2);
        }

        let second = prepare_nam_runtime(TINY_WAVENET_48K, "b".into(), 48_000.0, false, false).unwrap();
        cap.submit(Box::new(second));
        cap.begin_block(); // adopts `second`, starts fading `first` out
        assert!(cap.fading_out.is_some());

        // Run well past the fade window; every sample must stay finite, and
        // the fade must fully resolve without the audio thread ever calling
        // `retired.take()` (only `begin_block` — called here — does).
        for _ in 0..2_000 {
            let (l, r) = cap.process(0.2, -0.2);
            assert!(l.is_finite() && r.is_finite());
            cap.begin_block();
        }
        assert!(cap.fading_out.is_none(), "fade must resolve and retire");

        // Control thread drains what the audio thread retired.
        cap.poll_garbage();
    }

    #[test]
    fn no_capture_loaded_is_pass_through_at_unity() {
        let mut cap = NamCapture::new(48_000.0);
        cap.configure(0.0, 0.0, 100.0, false);
        let (l, r) = cap.process(0.3, -0.3);
        // DC blocker still runs, so allow a small tolerance rather than exact equality.
        assert!((l - 0.3).abs() < 1.0e-3);
        assert!((r + 0.3).abs() < 1.0e-3);
    }
}
