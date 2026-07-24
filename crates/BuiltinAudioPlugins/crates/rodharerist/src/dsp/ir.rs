//! Impulse-response cabinet engine — a real convolution alternative to the
//! modeled [`super::cab::Cabinet`], selectable as [`super::CabModel::Ir`].
//!
//! Loading a `.wav` IR decodes, resamples, trims and FFTs it into partition
//! spectra — allocation-heavy control-thread work, exactly like a NAM capture.
//! [`prepare_ir_runtime`] does that off the audio thread and hands back a
//! [`PreparedIrRuntime`] the caller boxes and pushes into [`IrLoader::submit`].
//! The audio thread adopts it at a block boundary
//! ([`IrConvolver::begin_block`]), briefly ducking through the swap, and the
//! retired runtime goes back to the control thread to be dropped — never
//! inside [`IrConvolver::process`].
//!
//! ## Algorithm
//!
//! Uniform-partitioned overlap-save: the IR is cut into [`PARTITION`]-sample
//! chunks, each zero-padded to [`FFT_SIZE`] and transformed once at load time.
//! Every [`PARTITION`] input samples the convolver transforms one
//! double-length input block, pushes it into a frequency-delay line, and
//! accumulates the FDL against the partition spectra. Latency is exactly
//! [`PARTITION`] samples, reported to the host for delay compensation.
//!
//! Cost scales with IR length, which is why [`MAX_IR_SECONDS`] caps it: this is
//! a *cabinet* IR engine, and a uniform partitioning is the wrong shape for a
//! multi-second reverb tail.
//!
//! ## Realtime rules
//!
//! Every buffer — partition spectra, FDL, FFT scratch, block accumulators —
//! is allocated in [`prepare_ir_runtime`] or [`IrConvolver::new`].
//! [`IrConvolver::process`] performs no allocation, locking or logging.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use super::handoff::HandoffCell;
use super::wav::{WavError, parse_wav};

/// Convolution partition size, in samples — also the engine's exact latency.
/// 128 samples is 2.7 ms at 48 kHz: short enough to play through, long enough
/// that the per-block FFT cost stays modest.
pub const PARTITION: usize = 128;

/// Overlap-save transform length: twice the partition.
const FFT_SIZE: usize = PARTITION * 2;

/// Longest IR the engine keeps, in seconds. A guitar cabinet IR is tens of
/// milliseconds of useful content; anything past this is truncated (with a
/// fade so the cut is not a click) rather than paying uniform-partition cost
/// for a tail this engine is not shaped to run.
pub const MAX_IR_SECONDS: f32 = 0.5;

/// Samples of raised-cosine fade applied at a truncation point.
const TRUNCATE_FADE: usize = 64;

/// Shortest IR worth loading — below this a file is a click, not a cabinet.
const MIN_IR_FRAMES: usize = 8;

/// Duck time either side of a runtime swap, in milliseconds.
const SWAP_DUCK_MS: f32 = 3.0;

/// Bounds on the unit-energy normalization gain, so a pathological file cannot
/// produce a silent or explosive cabinet.
const MIN_NORM_GAIN: f32 = 1.0e-3;
const MAX_NORM_GAIN: f32 = 1.0e3;

/// An IR file failed to load.
#[derive(Debug)]
pub enum IrLoadError {
    /// The container itself could not be decoded.
    Wav(WavError),
    /// Decoded fine but holds too few frames to be an impulse response.
    TooShort,
    /// Decoded fine but is entirely silent — convolving with it would mute the
    /// cabinet, which is never what the user meant.
    Silent,
    /// The file's rate is too far from the engine's to resample sensibly.
    UnusableSampleRate { file: f64, engine: f64 },
}

impl std::fmt::Display for IrLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrLoadError::Wav(e) => write!(f, "IR failed to load: {e}"),
            IrLoadError::TooShort => write!(f, "IR is too short to be an impulse response"),
            IrLoadError::Silent => write!(f, "IR contains only silence"),
            IrLoadError::UnusableSampleRate { file, engine } => write!(
                f,
                "IR was captured at {file} Hz, which is too far from the engine's {engine} Hz"
            ),
        }
    }
}

impl std::error::Error for IrLoadError {}

/// What the host/UI shows about a loaded IR. Serde-serializable so it can
/// travel over the plugin-host IPC as-is, like [`super::NamCaptureInfo`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IrInfo {
    pub name: String,
    /// Rate the file declared, before any resampling.
    pub source_sample_rate: f64,
    /// Frames actually convolved, at the engine's rate.
    pub frames: usize,
    /// True when the file carried two distinct channels (true-stereo IR).
    pub stereo: bool,
    /// True when the file was longer than [`MAX_IR_SECONDS`] and got cut.
    pub truncated: bool,
    /// Engine samples of latency the convolution adds — always [`PARTITION`].
    pub latency_samples: usize,
}

/// A fully-built, ready-to-run IR: partition spectra plus every buffer the
/// audio thread will need, so adopting it allocates nothing.
pub struct PreparedIrRuntime {
    info: IrInfo,
    fft: Arc<dyn Fft<f32>>,
    ifft: Arc<dyn Fft<f32>>,
    /// Partition spectra, `partitions * FFT_SIZE` complex values per channel.
    /// Already scaled by `1/FFT_SIZE` so the round trip comes back at unity.
    h_l: Vec<Complex<f32>>,
    h_r: Vec<Complex<f32>>,
    /// Frequency-delay lines of past input blocks, same layout as `h_*`.
    fdl_l: Vec<Complex<f32>>,
    fdl_r: Vec<Complex<f32>>,
    /// Partition slot holding the newest input block.
    fdl_head: usize,
    partitions: usize,
    /// The previous block's input samples (the overlap in overlap-save).
    prev_l: Vec<f32>,
    prev_r: Vec<f32>,
    /// Transform/accumulation scratch, all `FFT_SIZE` long.
    block: Vec<Complex<f32>>,
    accum: Vec<Complex<f32>>,
    fft_scratch: Vec<Complex<f32>>,
}

impl std::fmt::Debug for PreparedIrRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedIrRuntime")
            .field("info", &self.info)
            .field("partitions", &self.partitions)
            .finish_non_exhaustive()
    }
}

impl PreparedIrRuntime {
    pub fn info(&self) -> IrInfo {
        self.info.clone()
    }

    fn clear(&mut self) {
        self.fdl_l.fill(Complex::new(0.0, 0.0));
        self.fdl_r.fill(Complex::new(0.0, 0.0));
        self.fdl_head = 0;
        self.prev_l.fill(0.0);
        self.prev_r.fill(0.0);
    }

    /// Audio thread: convolve one [`PARTITION`]-sample block. Input and output
    /// buffers are the caller's fixed accumulation buffers; nothing here
    /// allocates.
    fn process_block(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        self.fdl_head = (self.fdl_head + 1) % self.partitions;
        let head = self.fdl_head;
        let n = FFT_SIZE;
        let p = self.partitions;

        for channel in 0..2 {
            let (input, prev, output, h, fdl) = if channel == 0 {
                (
                    in_l,
                    &mut self.prev_l,
                    &mut *out_l,
                    &self.h_l,
                    &mut self.fdl_l,
                )
            } else {
                (
                    in_r,
                    &mut self.prev_r,
                    &mut *out_r,
                    &self.h_r,
                    &mut self.fdl_r,
                )
            };

            // Overlap-save input block: the previous partition then this one.
            for i in 0..PARTITION {
                self.block[i] = Complex::new(prev[i], 0.0);
                self.block[PARTITION + i] = Complex::new(input[i], 0.0);
            }
            self.fft
                .process_with_scratch(&mut self.block, &mut self.fft_scratch);
            fdl[head * n..head * n + n].copy_from_slice(&self.block);

            // Accumulate every partition against the matching past block.
            self.accum.fill(Complex::new(0.0, 0.0));
            for part in 0..p {
                let slot = (head + p - part) % p;
                let x = &fdl[slot * n..slot * n + n];
                let hp = &h[part * n..part * n + n];
                for k in 0..n {
                    self.accum[k] += x[k] * hp[k];
                }
            }
            self.ifft
                .process_with_scratch(&mut self.accum, &mut self.fft_scratch);

            // Overlap-save: only the second half of the block is valid output.
            for i in 0..PARTITION {
                output[i] = self.accum[PARTITION + i].re;
            }
            prev.copy_from_slice(input);
        }
    }
}

/// Offline (control-thread) resample of a whole buffer, cubic-Hermite
/// interpolated. The streaming [`super::rate::RateAdapter`] solves a different
/// problem — this one has the entire signal in hand and no realtime budget.
fn resample_offline(input: &[f32], from_rate: f64, to_rate: f64) -> Vec<f32> {
    if input.is_empty() || (from_rate - to_rate).abs() < 0.5 {
        return input.to_vec();
    }
    let ratio = to_rate / from_rate;
    let out_len = ((input.len() as f64) * ratio).round().max(1.0) as usize;
    let step = 1.0 / ratio;
    let at = |i: isize| -> f32 {
        let clamped = i.clamp(0, input.len() as isize - 1) as usize;
        input[clamped]
    };
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let base = pos.floor();
            let f = (pos - base) as f32;
            let idx = base as isize;
            let p0 = at(idx - 1);
            let p1 = at(idx);
            let p2 = at(idx + 1);
            let p3 = at(idx + 2);
            let c1 = 0.5 * (p2 - p0);
            let c2 = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
            let c3 = 0.5 * (p3 - p0) + 1.5 * (p1 - p2);
            ((c3 * f + c2) * f + c1) * f + p1
        })
        .collect()
}

/// Cut to `max_frames`, fading the last [`TRUNCATE_FADE`] samples so the cut
/// is a decay, not a step. Returns whether anything was actually removed.
fn truncate_with_fade(samples: &mut Vec<f32>, max_frames: usize) -> bool {
    if samples.len() <= max_frames {
        return false;
    }
    samples.truncate(max_frames);
    let fade = TRUNCATE_FADE.min(samples.len());
    let start = samples.len() - fade;
    for (i, sample) in samples[start..].iter_mut().enumerate() {
        let t = (i as f32 + 0.5) / fade as f32;
        // Raised cosine from 1 to 0.
        *sample *= 0.5 * (1.0 + (std::f32::consts::PI * t).cos());
    }
    true
}

/// Parse, resample, trim, normalize and transform a `.wav` IR — all on the
/// control thread. `engine_sample_rate` is the rate the convolution will run
/// at; a file captured at another rate is resampled here, once, rather than
/// per-sample at playback.
pub fn prepare_ir_runtime(
    bytes: &[u8],
    name: String,
    engine_sample_rate: f64,
) -> Result<PreparedIrRuntime, IrLoadError> {
    let audio = parse_wav(bytes).map_err(IrLoadError::Wav)?;
    if !(engine_sample_rate.is_finite() && engine_sample_rate >= 1.0) {
        return Err(IrLoadError::UnusableSampleRate {
            file: audio.sample_rate,
            engine: engine_sample_rate,
        });
    }
    let rate_ratio = audio.sample_rate / engine_sample_rate;
    if !(0.1..=10.0).contains(&rate_ratio) {
        return Err(IrLoadError::UnusableSampleRate {
            file: audio.sample_rate,
            engine: engine_sample_rate,
        });
    }

    let source_sample_rate = audio.sample_rate;
    let stereo = audio.channels >= 2;
    let mut left = Vec::new();
    let mut right = Vec::new();
    audio.channel_into(0, &mut left);
    audio.channel_into(1, &mut right);

    let mut left = resample_offline(&left, source_sample_rate, engine_sample_rate);
    let mut right = if stereo {
        resample_offline(&right, source_sample_rate, engine_sample_rate)
    } else {
        left.clone()
    };

    let max_frames = ((MAX_IR_SECONDS as f64 * engine_sample_rate) as usize).max(PARTITION);
    let truncated =
        truncate_with_fade(&mut left, max_frames) | truncate_with_fade(&mut right, max_frames);
    if left.len() < MIN_IR_FRAMES {
        return Err(IrLoadError::TooShort);
    }

    // Unit-energy normalization keeps different IRs at comparable loudness —
    // IR files in the wild are recorded at wildly different levels.
    let energy: f32 = left
        .iter()
        .zip(right.iter())
        .map(|(l, r)| 0.5 * (l * l + r * r))
        .sum();
    if !(energy.is_finite() && energy > 0.0) {
        return Err(IrLoadError::Silent);
    }
    let gain = (1.0 / energy.sqrt()).clamp(MIN_NORM_GAIN, MAX_NORM_GAIN);
    for sample in left.iter_mut().chain(right.iter_mut()) {
        *sample *= gain;
    }

    let frames = left.len();
    let partitions = frames.div_ceil(PARTITION).max(1);

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let ifft = planner.plan_fft_inverse(FFT_SIZE);
    let scratch_len = fft
        .get_inplace_scratch_len()
        .max(ifft.get_inplace_scratch_len());
    let mut fft_scratch = vec![Complex::new(0.0, 0.0); scratch_len];

    // Fold the inverse transform's 1/N into the IR spectra so the hot path
    // never scales anything.
    let norm = 1.0 / FFT_SIZE as f32;
    let mut build = |channel: &[f32]| -> Vec<Complex<f32>> {
        let mut spectra = vec![Complex::new(0.0, 0.0); partitions * FFT_SIZE];
        let mut scratch_block = vec![Complex::new(0.0, 0.0); FFT_SIZE];
        for part in 0..partitions {
            let start = part * PARTITION;
            let end = (start + PARTITION).min(channel.len());
            scratch_block.fill(Complex::new(0.0, 0.0));
            for (i, &sample) in channel[start..end].iter().enumerate() {
                scratch_block[i] = Complex::new(sample * norm, 0.0);
            }
            fft.process_with_scratch(&mut scratch_block, &mut fft_scratch);
            spectra[part * FFT_SIZE..(part + 1) * FFT_SIZE].copy_from_slice(&scratch_block);
        }
        spectra
    };
    let h_l = build(&left);
    let h_r = build(&right);

    Ok(PreparedIrRuntime {
        info: IrInfo {
            name,
            source_sample_rate,
            frames,
            stereo,
            truncated,
            latency_samples: PARTITION,
        },
        fft,
        ifft,
        h_l,
        h_r,
        fdl_l: vec![Complex::new(0.0, 0.0); partitions * FFT_SIZE],
        fdl_r: vec![Complex::new(0.0, 0.0); partitions * FFT_SIZE],
        fdl_head: 0,
        partitions,
        prev_l: vec![0.0; PARTITION],
        prev_r: vec![0.0; PARTITION],
        block: vec![Complex::new(0.0, 0.0); FFT_SIZE],
        accum: vec![Complex::new(0.0, 0.0); FFT_SIZE],
        fft_scratch,
    })
}

/// The two lock-free hand-off cells between the control side and the audio
/// side — the same pattern [`super::nam::NamChannel`] uses.
pub struct IrChannel {
    /// Control thread → audio thread: a freshly built runtime awaiting adoption.
    pending: HandoffCell<PreparedIrRuntime>,
    /// Audio thread → control thread: a retired runtime awaiting disposal.
    retired: HandoffCell<PreparedIrRuntime>,
}

/// Cloneable control-side handle to an [`IrConvolver`]'s hand-off cells.
///
/// Thread contract: exactly **one** control thread may use the loader at a
/// time (the cells are single-producer/single-consumer per direction).
#[derive(Clone)]
pub struct IrLoader {
    channel: Arc<IrChannel>,
}

impl IrLoader {
    /// Push a freshly-built runtime for the audio thread to adopt at the next
    /// block boundary. A not-yet-adopted runtime already waiting is dropped
    /// here (safe: the audio thread never touched it).
    pub fn submit(&self, runtime: Box<PreparedIrRuntime>) {
        if let Some(bumped) = self.channel.pending.put(runtime) {
            drop(bumped);
        }
    }

    /// Drop any runtime the audio thread has retired.
    pub fn collect_garbage(&self) {
        if let Some(dead) = self.channel.retired.take() {
            drop(dead);
        }
    }

    /// Parse, build and submit a `.wav` IR in one call (control thread —
    /// decoding, resampling and the load-time FFTs all allocate).
    pub fn load_wav(
        &self,
        bytes: &[u8],
        name: impl Into<String>,
        engine_sample_rate: f64,
    ) -> Result<IrInfo, IrLoadError> {
        let prepared = prepare_ir_runtime(bytes, name.into(), engine_sample_rate)?;
        let info = prepared.info();
        self.collect_garbage();
        self.submit(Box::new(prepared));
        Ok(info)
    }

    /// Unload whatever IR is active, returning the cabinet slot to a dry pass
    /// through. Signalled by submitting nothing and clearing on the audio side
    /// — see [`IrConvolver::request_unload`].
    pub fn clear(&self) {
        self.collect_garbage();
    }
}

/// The audio-thread-resident IR engine.
pub(super) struct IrConvolver {
    channel: Arc<IrChannel>,
    active: Option<Box<PreparedIrRuntime>>,
    /// Taken from `pending` and held until the duck reaches silence.
    staged: Option<Box<PreparedIrRuntime>>,
    /// A retiree the control thread hasn't drained yet. Held here — never
    /// dropped on the audio thread — until a later block finds `retired` empty.
    retire_overflow: Option<Box<PreparedIrRuntime>>,

    sample_rate: f32,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    fill: usize,

    /// Output gain, ducked to 0 across a runtime swap so the discontinuity
    /// between two different cabinets is a dip, not a click.
    gain: f32,
    gain_step: f32,
    ducking: bool,
}

impl IrConvolver {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            channel: Arc::new(IrChannel {
                pending: HandoffCell::new(),
                retired: HandoffCell::new(),
            }),
            active: None,
            staged: None,
            retire_overflow: None,
            sample_rate: sr,
            in_l: vec![0.0; PARTITION],
            in_r: vec![0.0; PARTITION],
            out_l: vec![0.0; PARTITION],
            out_r: vec![0.0; PARTITION],
            fill: 0,
            gain: 1.0,
            gain_step: Self::step_for(sr),
            ducking: false,
        }
    }

    fn step_for(sample_rate: f32) -> f32 {
        1.0 / (sample_rate * (SWAP_DUCK_MS / 1_000.0)).max(1.0)
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.gain_step = Self::step_for(self.sample_rate);
        // Spectra are rate-specific: an IR prepared for the old rate would run
        // at the wrong length. Retire it and let the control side reload.
        if let Some(old) = self.active.take() {
            self.retire(old);
        }
        self.reset();
    }

    pub(super) fn reset(&mut self) {
        self.in_l.fill(0.0);
        self.in_r.fill(0.0);
        self.out_l.fill(0.0);
        self.out_r.fill(0.0);
        self.fill = 0;
        self.gain = if self.ducking { 0.0 } else { 1.0 };
        if let Some(rt) = self.active.as_mut() {
            rt.clear();
        }
    }

    /// Clone out a control-side loader handle for this convolver's cells.
    pub(super) fn loader(&self) -> IrLoader {
        IrLoader {
            channel: Arc::clone(&self.channel),
        }
    }

    /// Control thread: push a freshly-built runtime.
    pub(super) fn submit(&self, runtime: Box<PreparedIrRuntime>) {
        if let Some(bumped) = self.channel.pending.put(runtime) {
            drop(bumped);
        }
    }

    /// Control thread: drop any runtime the audio thread has retired.
    pub(super) fn poll_garbage(&mut self) {
        if let Some(dead) = self.channel.retired.take() {
            drop(dead);
        }
    }

    /// Info about the currently active IR, if one is loaded.
    pub(super) fn active_info(&self) -> Option<IrInfo> {
        self.active.as_ref().map(|rt| rt.info())
    }

    /// Latency the convolution adds, in samples — [`PARTITION`] while an IR is
    /// loaded, 0 when the slot is a dry pass through.
    pub(super) fn latency_samples(&self) -> usize {
        if self.active.is_some() { PARTITION } else { 0 }
    }

    fn retire(&mut self, runtime: Box<PreparedIrRuntime>) {
        if let Some(bounced) = self.channel.retired.put(runtime) {
            self.retire_overflow = Some(bounced);
        }
    }

    /// Audio thread: adopt a pending runtime once the duck has reached
    /// silence, and hand a retired one back. Called once per audio block.
    pub(super) fn begin_block(&mut self) {
        if let Some(carry) = self.retire_overflow.take() {
            self.retire(carry);
        }

        if self.staged.is_none() {
            if let Some(new_rt) = self.channel.pending.take() {
                if self.active.is_none() {
                    // Nothing playing through the slot yet — no need to duck.
                    self.active = Some(new_rt);
                    self.gain = 1.0;
                    self.ducking = false;
                } else {
                    self.staged = Some(new_rt);
                    self.ducking = true;
                }
            }
        }

        if self.ducking && self.gain <= 0.0 {
            if let Some(mut new_rt) = self.staged.take() {
                new_rt.clear();
                if let Some(old) = self.active.replace(new_rt) {
                    self.retire(old);
                }
            }
            self.ducking = false;
        }
    }

    /// Audio thread hot path. One sample in, one sample out, delayed by
    /// [`PARTITION`]; the convolution itself runs once every [`PARTITION`]
    /// calls. With no IR loaded the slot is a dry pass through at the same
    /// delay, so the reported latency stays honest either way.
    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let out = (self.out_l[self.fill], self.out_r[self.fill]);
        self.in_l[self.fill] = left;
        self.in_r[self.fill] = right;
        self.fill += 1;
        if self.fill >= PARTITION {
            self.fill = 0;
            match self.active.as_mut() {
                Some(rt) => {
                    rt.process_block(&self.in_l, &self.in_r, &mut self.out_l, &mut self.out_r)
                }
                None => {
                    self.out_l.copy_from_slice(&self.in_l);
                    self.out_r.copy_from_slice(&self.in_r);
                }
            }
        }

        self.gain = if self.ducking {
            (self.gain - self.gain_step).max(0.0)
        } else {
            (self.gain + self.gain_step).min(1.0)
        };
        (out.0 * self.gain, out.1 * self.gain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    /// Build a float-WAV file around raw mono or stereo samples.
    fn wav_file(samples: &[f32], channels: u16, rate: u32) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
        let block_align = channels * 4;
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&(rate * block_align as u32).to_le_bytes());
        fmt.extend_from_slice(&block_align.to_le_bytes());
        fmt.extend_from_slice(&32u16.to_le_bytes());

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(4 + 8 + fmt.len() as u32 + 8 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        out.extend_from_slice(&fmt);
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    /// A short, arbitrary but deterministic IR.
    fn test_ir(len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let t = i as f32 / len as f32;
                ((i as f32 * 0.7).sin() * (1.0 - t) * (1.0 - t)) * 0.5
            })
            .collect()
    }

    /// Push `input` through a convolver and collect the output.
    fn render(conv: &mut IrConvolver, input: &[f32]) -> Vec<f32> {
        input
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                if i % PARTITION == 0 {
                    conv.begin_block();
                }
                conv.process(x, x).0
            })
            .collect()
    }

    /// The engine's whole reason to exist: its output must match a direct
    /// time-domain convolution with the (normalized) IR, delayed by exactly
    /// `PARTITION` samples.
    #[test]
    fn matches_a_direct_convolution_delayed_by_the_partition_size() {
        let ir = test_ir(300);
        let prepared =
            prepare_ir_runtime(&wav_file(&ir, 1, SR as u32), "t".into(), SR).expect("IR must load");

        // Reproduce the load-time normalization for the reference.
        let energy: f32 = ir.iter().map(|h| h * h).sum();
        let gain = 1.0 / energy.sqrt();
        let norm: Vec<f32> = ir.iter().map(|h| h * gain).collect();

        let input: Vec<f32> = (0..2_048)
            .map(|n| {
                if n == 5 {
                    1.0
                } else {
                    (n as f32 * 0.031).sin() * 0.3
                }
            })
            .collect();

        let mut expected = vec![0.0f32; input.len()];
        for (n, out) in expected.iter_mut().enumerate() {
            for (k, &h) in norm.iter().enumerate() {
                if k <= n {
                    *out += input[n - k] * h;
                }
            }
        }

        let mut conv = IrConvolver::new(SR as f32);
        conv.submit(Box::new(prepared));
        let actual = render(&mut conv, &input);

        // The duck ramps in over the first few ms; compare past it, and past
        // the one-partition latency.
        let skip = PARTITION + (SR as usize * 6 / 1_000);
        let mut worst = 0.0f32;
        for n in skip..input.len() {
            worst = worst.max((actual[n] - expected[n - PARTITION]).abs());
        }
        assert!(worst < 1.0e-4, "convolution mismatch, worst error {worst}");
    }

    #[test]
    fn reports_partition_latency_only_while_an_ir_is_loaded() {
        let mut conv = IrConvolver::new(SR as f32);
        assert_eq!(conv.latency_samples(), 0);
        assert!(conv.active_info().is_none());

        let ir = test_ir(200);
        let prepared = prepare_ir_runtime(&wav_file(&ir, 1, SR as u32), "cab".into(), SR).unwrap();
        assert_eq!(prepared.info().latency_samples, PARTITION);
        conv.submit(Box::new(prepared));
        conv.begin_block();
        assert_eq!(conv.latency_samples(), PARTITION);
        assert_eq!(conv.active_info().map(|i| i.name), Some("cab".into()));
    }

    /// With no IR loaded the slot must pass the signal through unchanged, just
    /// delayed — never mute the cabinet.
    #[test]
    fn without_an_ir_the_slot_is_a_delayed_dry_pass_through() {
        let mut conv = IrConvolver::new(SR as f32);
        let input: Vec<f32> = (0..1_024).map(|n| (n as f32 * 0.05).sin() * 0.4).collect();
        let out = render(&mut conv, &input);
        for n in PARTITION..input.len() {
            assert!(
                (out[n] - input[n - PARTITION]).abs() < 1.0e-6,
                "dry path altered the signal at {n}"
            );
        }
    }

    /// A swap must duck through the discontinuity and retire the old runtime
    /// to the control thread rather than dropping it on the audio thread.
    #[test]
    fn swapping_ducks_and_retires_the_previous_runtime() {
        let mut conv = IrConvolver::new(SR as f32);
        conv.submit(Box::new(
            prepare_ir_runtime(&wav_file(&test_ir(200), 1, SR as u32), "a".into(), SR).unwrap(),
        ));
        let input: Vec<f32> = (0..4_096).map(|n| (n as f32 * 0.05).sin() * 0.4).collect();
        let _ = render(&mut conv, &input);
        assert_eq!(conv.active_info().map(|i| i.name), Some("a".into()));

        conv.submit(Box::new(
            prepare_ir_runtime(&wav_file(&test_ir(400), 1, SR as u32), "b".into(), SR).unwrap(),
        ));
        let out = render(&mut conv, &input);
        for (n, y) in out.iter().enumerate() {
            assert!(y.is_finite(), "non-finite through the swap at {n}");
        }
        assert_eq!(conv.active_info().map(|i| i.name), Some("b".into()));
        assert!(!conv.ducking, "the duck must resolve");
        assert!((conv.gain - 1.0).abs() < 1.0e-6, "gain must come back up");

        // The retired runtime is waiting for the control thread, not dropped.
        conv.poll_garbage();
    }

    #[test]
    fn the_loader_handle_reaches_the_same_cells() {
        let mut conv = IrConvolver::new(SR as f32);
        let loader = conv.loader();
        let info = loader
            .load_wav(&wav_file(&test_ir(256), 1, SR as u32), "via-loader", SR)
            .expect("load through loader");
        assert_eq!(info.name, "via-loader");
        assert!(!info.stereo);
        conv.begin_block();
        assert_eq!(
            conv.active_info().map(|i| i.name),
            Some("via-loader".into())
        );
        loader.collect_garbage();
    }

    #[test]
    fn a_stereo_file_keeps_its_channels_distinct() {
        // Left rings, right is a single tap — the two sides must not converge.
        let mut interleaved = Vec::new();
        for i in 0..256 {
            interleaved.push(((i as f32) * 0.4).sin() * 0.5);
            interleaved.push(if i == 0 { 1.0 } else { 0.0 });
        }
        let info = prepare_ir_runtime(&wav_file(&interleaved, 2, SR as u32), "st".into(), SR)
            .expect("stereo IR must load")
            .info();
        assert!(info.stereo);

        let mut conv = IrConvolver::new(SR as f32);
        conv.submit(Box::new(
            prepare_ir_runtime(&wav_file(&interleaved, 2, SR as u32), "st".into(), SR).unwrap(),
        ));
        let mut diff = 0.0f32;
        for i in 0..2_048 {
            if i % PARTITION == 0 {
                conv.begin_block();
            }
            let x = if i == 300 { 1.0 } else { 0.0 };
            let (l, r) = conv.process(x, x);
            diff = diff.max((l - r).abs());
        }
        assert!(diff > 1.0e-3, "stereo channels collapsed together");
    }

    #[test]
    fn a_file_at_another_rate_is_resampled_to_the_engine_rate() {
        let ir = test_ir(441); // 10 ms at 44.1 kHz
        let info = prepare_ir_runtime(&wav_file(&ir, 1, 44_100), "resampled".into(), SR)
            .expect("a 44.1 kHz IR must load into a 48 kHz engine")
            .info();
        assert_eq!(info.source_sample_rate, 44_100.0);
        // 10 ms at 48 kHz is 480 frames, give or take the interpolation edge.
        assert!(
            info.frames.abs_diff(480) < 4,
            "resampled to {} frames",
            info.frames
        );
    }

    #[test]
    fn an_over_long_file_is_truncated_with_a_fade() {
        let long = vec![0.5f32; (SR as usize) * 2]; // two seconds
        let info = prepare_ir_runtime(&wav_file(&long, 1, SR as u32), "long".into(), SR)
            .expect("a long file still loads")
            .info();
        assert!(info.truncated);
        assert_eq!(info.frames, (MAX_IR_SECONDS as f64 * SR) as usize);
    }

    #[test]
    fn rejects_silent_short_and_undecodable_files() {
        assert!(matches!(
            prepare_ir_runtime(&wav_file(&[0.0; 512], 1, SR as u32), "s".into(), SR),
            Err(IrLoadError::Silent)
        ));
        assert!(matches!(
            prepare_ir_runtime(&wav_file(&[0.5; 4], 1, SR as u32), "s".into(), SR),
            Err(IrLoadError::TooShort)
        ));
        assert!(matches!(
            prepare_ir_runtime(b"definitely not a wav", "s".into(), SR),
            Err(IrLoadError::Wav(_))
        ));
        // 8 kHz into a 192 kHz engine is a 24x stretch — refused, not guessed.
        assert!(matches!(
            prepare_ir_runtime(&wav_file(&test_ir(256), 1, 8_000), "s".into(), 192_000.0),
            Err(IrLoadError::UnusableSampleRate { .. })
        ));
    }

    /// Different IRs must land at comparable loudness — that is what the
    /// unit-energy normalization is for.
    #[test]
    fn normalization_evens_out_wildly_different_source_levels() {
        let quiet: Vec<f32> = test_ir(256).iter().map(|h| h * 0.001).collect();
        let loud: Vec<f32> = test_ir(256).iter().map(|h| h * 20.0).collect();

        let rms = |ir: &[f32]| -> f32 {
            let mut conv = IrConvolver::new(SR as f32);
            conv.submit(Box::new(
                prepare_ir_runtime(&wav_file(ir, 1, SR as u32), "n".into(), SR).unwrap(),
            ));
            let input: Vec<f32> = (0..8_192).map(|n| (n as f32 * 0.07).sin() * 0.4).collect();
            let out = render(&mut conv, &input);
            let tail = &out[2_048..];
            (tail.iter().map(|y| y * y).sum::<f32>() / tail.len() as f32).sqrt()
        };
        let a = rms(&quiet);
        let b = rms(&loud);
        assert!(a > 1.0e-4 && b > 1.0e-4, "both must be audible: {a} {b}");
        assert!(
            (a / b - 1.0).abs() < 0.01,
            "levels diverged: {a} vs {b} (ratio {})",
            a / b
        );
    }

    #[test]
    fn stays_finite_across_rates_and_block_alignments() {
        for &sr in &[44_100.0f64, 48_000.0, 96_000.0] {
            let mut conv = IrConvolver::new(sr as f32);
            conv.submit(Box::new(
                prepare_ir_runtime(&wav_file(&test_ir(512), 2, sr as u32), "x".into(), sr).unwrap(),
            ));
            for block in 0..64 {
                conv.begin_block();
                // Deliberately uneven blocks: the convolver's own partition
                // boundary must not depend on the host's.
                for n in 0..(37 + block % 11) {
                    let x = ((block * 100 + n) as f32 * 0.03).sin();
                    let (l, r) = conv.process(x, -x);
                    assert!(l.is_finite() && r.is_finite(), "{sr} Hz went non-finite");
                }
            }
        }
    }
}
