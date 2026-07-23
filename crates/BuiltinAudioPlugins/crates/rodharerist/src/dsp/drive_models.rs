//! Dedicated processing topologies for the four modern drive models
//! (`ds_one`, `super_drive`, `metal_core`, `tight_rift`).
//!
//! The six legacy voicings share `Drive`'s single generic path; these four
//! each own a full chain — DC block → model pre-EQ → envelope/sag →
//! oversampled nonlinear stage(s) with interstage EQ → post-EQ → fizz control
//! → partial gain compensation → equal-power dry/wet — so their character
//! comes from topology, not just gain and a low-pass.
//!
//! Realtime rules: every filter/oversampler/envelope is preallocated; the
//! per-sample path is arithmetic + biquads only. `configure` (control thread)
//! maps the three editor knobs onto many internal targets; continuous values
//! glide through [`Smoothed`], filter coefficients swap state-preservingly.
//! Interstage filters that run inside the oversampled domain are configured
//! at the oversampled rate.

use builtin_dsp_core::{db_to_linear, make_eq_biquad, time_constant};

use super::StereoBiquad;
use super::smooth::{Oversampler4x, Oversampler8x, Smoothed};

/// Glide time for all smoothed drive internals.
const SMOOTH_SECONDS: f32 = 0.010;

/// Perceptual drive taper: more resolution in the low half of the knob,
/// faster growth up top.
#[inline]
fn drive_curve(g01: f32) -> f32 {
    g01.clamp(0.0, 1.0).powf(1.6)
}

/// Flush a possibly-denormal/non-finite intermediate back to safe territory.
#[inline]
fn sanitize(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

// ---------------------------------------------------------------------------
// Shared primitives
// ---------------------------------------------------------------------------

/// One-pole DC blocker (~18 Hz), stereo. `y = x - x1 + r·y1`.
#[derive(Debug, Clone)]
pub(super) struct DcBlock {
    r: f32,
    x1_l: f32,
    y1_l: f32,
    x1_r: f32,
    y1_r: f32,
}

impl DcBlock {
    pub(super) fn new(sample_rate: f32) -> Self {
        let mut s = Self {
            r: 0.0,
            x1_l: 0.0,
            y1_l: 0.0,
            x1_r: 0.0,
            y1_r: 0.0,
        };
        s.set_sample_rate(sample_rate);
        s
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        self.r = (1.0 - (2.0 * std::f32::consts::PI * 18.0) / sr).clamp(0.9, 0.999_99);
    }

    pub(super) fn reset(&mut self) {
        self.x1_l = 0.0;
        self.y1_l = 0.0;
        self.x1_r = 0.0;
        self.y1_r = 0.0;
    }

    #[inline]
    pub(super) fn run(&mut self, l: f32, r: f32) -> (f32, f32) {
        let yl = l - self.x1_l + self.r * self.y1_l;
        let yr = r - self.x1_r + self.r * self.y1_r;
        self.x1_l = l;
        self.y1_l = sanitize(yl);
        self.x1_r = r;
        self.y1_r = sanitize(yr);
        (self.y1_l, self.y1_r)
    }
}

/// Per-channel envelope follower with independent attack/release, for sag and
/// dynamic asymmetry. Stable for silence, impulses, DC and hostile input:
/// the state is sanitized every tick and can only decay toward the rectified
/// input.
#[derive(Debug, Clone)]
pub(super) struct EnvFollower {
    attack_secs: f32,
    release_secs: f32,
    attack: f32,
    release: f32,
    env_l: f32,
    env_r: f32,
}

impl EnvFollower {
    pub(super) fn new(sample_rate: f32, attack_secs: f32, release_secs: f32) -> Self {
        let mut e = Self {
            attack_secs,
            release_secs,
            attack: 0.0,
            release: 0.0,
            env_l: 0.0,
            env_r: 0.0,
        };
        e.set_sample_rate(sample_rate);
        e
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        self.attack = time_constant(sr, self.attack_secs);
        self.release = time_constant(sr, self.release_secs);
    }

    pub(super) fn reset(&mut self) {
        self.env_l = 0.0;
        self.env_r = 0.0;
    }

    #[inline]
    fn follow(env: f32, x: f32, attack: f32, release: f32) -> f32 {
        let a = x.abs();
        let coeff = if a > env { attack } else { release };
        let next = a + (env - a) * coeff;
        if next.is_finite() && next > 1.0e-20 {
            next
        } else {
            0.0
        }
    }

    #[inline]
    pub(super) fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        self.env_l = Self::follow(self.env_l, l, self.attack, self.release);
        self.env_r = Self::follow(self.env_r, r, self.attack, self.release);
        (self.env_l, self.env_r)
    }
}

/// Hard clip with a small tanh knee and independent positive/negative
/// thresholds (both given as positive numbers). `knee = 0` degenerates to a
/// pure clamp.
#[inline]
fn hard_clip_asym(x: f32, t_pos: f32, t_neg: f32, knee: f32) -> f32 {
    let (sign, mag, th) = if x >= 0.0 {
        (1.0, x, t_pos)
    } else {
        (-1.0, -x, t_neg)
    };
    if knee <= 1.0e-6 {
        return sign * mag.min(th);
    }
    let knee_start = (th - knee).max(0.0);
    if mag <= knee_start {
        x
    } else {
        sign * (knee_start + knee * ((mag - knee_start) / knee).tanh())
    }
}

/// Soft asymmetric saturation: separate drive per half-cycle, normalized so
/// small signals pass at ~unity.
#[inline]
fn soft_asym(x: f32, k_pos: f32, k_neg: f32) -> f32 {
    if x >= 0.0 {
        (x * k_pos).tanh() / k_pos.max(1.0e-6)
    } else {
        (x * k_neg).tanh() / k_neg.max(1.0e-6)
    }
}

/// Equal-power dry/wet: keeps perceived level steady across the mix knob
/// (the legacy models' linear crossfade dips in the middle).
#[inline]
fn equal_power_mix(dry: f32, wet: f32, mix: f32) -> f32 {
    let m = mix.clamp(0.0, 1.0);
    dry * (1.0 - m).sqrt() + wet * m.sqrt()
}

// ---------------------------------------------------------------------------
// DS Classic — raw orange-box hard clipper (4×)
// ---------------------------------------------------------------------------

/// Dry, rude, compressed grit: pre-emphasized upper mids into an asymmetric
/// hard clip with a small knee, then a resonant edge and a firm low-pass.
#[derive(Debug, Clone)]
pub(super) struct DsClassic {
    sample_rate: f32,
    dc: DcBlock,
    dc_out: DcBlock,
    input_hpf: StereoBiquad,
    pre_emph: StereoBiquad,
    edge: StereoBiquad,
    post_lpf: StereoBiquad,
    os: Oversampler4x,
    pre_gain: Smoothed,
    t_pos: Smoothed,
    t_neg: Smoothed,
    knee: Smoothed,
    out_gain: Smoothed,
    mix: Smoothed,
}

impl DsClassic {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            dc: DcBlock::new(sr),
            dc_out: DcBlock::new(sr),
            input_hpf: StereoBiquad::none(),
            pre_emph: StereoBiquad::none(),
            edge: StereoBiquad::none(),
            post_lpf: StereoBiquad::none(),
            os: Oversampler4x::new(),
            pre_gain: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
            t_pos: Smoothed::new(sr, SMOOTH_SECONDS, 0.6),
            t_neg: Smoothed::new(sr, SMOOTH_SECONDS, 0.75),
            knee: Smoothed::new(sr, SMOOTH_SECONDS, 0.12),
            out_gain: Smoothed::new(sr, SMOOTH_SECONDS, 0.5),
            mix: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.dc.set_sample_rate(self.sample_rate);
        self.dc_out.set_sample_rate(self.sample_rate);
        for s in [
            &mut self.pre_gain,
            &mut self.t_pos,
            &mut self.t_neg,
            &mut self.knee,
            &mut self.out_gain,
            &mut self.mix,
        ] {
            s.set_time(self.sample_rate, SMOOTH_SECONDS);
        }
    }

    pub(super) fn reset(&mut self) {
        self.dc.reset();
        self.dc_out.reset();
        self.input_hpf.reset();
        self.pre_emph.reset();
        self.edge.reset();
        self.post_lpf.reset();
        self.os.reset();
        for s in [
            &mut self.pre_gain,
            &mut self.t_pos,
            &mut self.t_neg,
            &mut self.knee,
            &mut self.out_gain,
            &mut self.mix,
        ] {
            s.snap();
        }
    }

    /// Editor knobs 0..10. Drive maps onto gain, thresholds, knee hardness,
    /// pre-emphasis and compensation together; Tone rides the resonant edge
    /// and the post low-pass as one gesture (dark mid-grind ↔ sharp bite).
    pub(super) fn configure(&mut self, gain: f32, tone: f32, level: f32) {
        let d = drive_curve(gain / 10.0);
        let t = (tone / 10.0).clamp(0.0, 1.0);
        let lvl = (level / 10.0).clamp(0.0, 1.0);
        let sr = self.sample_rate;

        let gain_db = 8.0 + d * 24.0; // 8..32 dB into the clipper
        self.pre_gain.set_target(db_to_linear(gain_db));
        // Thresholds close in with drive; halves stay deliberately unequal.
        self.t_pos.set_target(0.62 - d * 0.14);
        self.t_neg.set_target(0.78 - d * 0.12);
        // Knee hardens as drive rises — full drive is nearly a bare clamp.
        self.knee.set_target(0.16 - d * 0.13);
        // ~70% compensation of the added gain, then the level knob.
        let comp_db = -(gain_db - 8.0) * 0.7;
        self.out_gain
            .set_target(db_to_linear(comp_db) * (0.5 + lvl * 1.6));
        self.mix.set_target(1.0);

        self.dc.set_sample_rate(sr);
        self.dc_out.set_sample_rate(sr);
        self.input_hpf
            .set(make_eq_biquad("highpass", 85.0, 0.0, 0.707, sr));
        // Pre-emphasis grows with drive: what screams is chosen before it clips.
        let emph_hz = 750.0 + t * 750.0; // 750..1500
        self.pre_emph
            .set(make_eq_biquad("bell", emph_hz, 4.0 + d * 5.0, 0.9, sr));
        // Tone: resonant edge sweeps 1.2..1.8 kHz while the ceiling opens.
        self.edge.set(make_eq_biquad(
            "bell",
            1_200.0 + t * 600.0,
            2.5 + t * 2.0,
            1.4,
            sr,
        ));
        let lpf = (5_500.0 + t * 2_500.0).min(sr * 0.45); // 5.5..8 kHz
        self.post_lpf
            .set(make_eq_biquad("lowpass", lpf, 0.0, 0.707, sr));
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let pre = self.pre_gain.tick();
        let t_pos = self.t_pos.tick();
        let t_neg = self.t_neg.tick();
        let knee = self.knee.tick().max(0.0);
        let out = self.out_gain.tick();
        let mix = self.mix.tick();

        let (l, r) = self.dc.run(left, right);
        let (l, r) = self.input_hpf.run(l, r);
        let (l, r) = self.pre_emph.run(l, r);
        let (mut wl, mut wr) = self.os.process_stereo(l * pre, r * pre, |a, b| {
            (
                hard_clip_asym(a, t_pos, t_neg, knee),
                hard_clip_asym(b, t_pos, t_neg, knee),
            )
        });
        (wl, wr) = self.edge.run(wl, wr);
        (wl, wr) = self.post_lpf.run(wl, wr);
        // Asymmetric clipping generates DC — strip it before it eats
        // headroom or thumps the mix.
        (wl, wr) = self.dc_out.run(wl, wr);
        wl = sanitize(wl) * out;
        wr = sanitize(wr) * out;
        (
            equal_power_mix(left, wl, mix),
            equal_power_mix(right, wr, mix),
        )
    }
}

// ---------------------------------------------------------------------------
// Super Drive — dynamic asymmetric overdrive (4×)
// ---------------------------------------------------------------------------

/// Touch-sensitive: an envelope modulates the clip asymmetry (more even
/// harmonics as you dig in) and a sag term compresses hard playing; a clean
/// low band bypasses the clipper to keep the body.
#[derive(Debug, Clone)]
pub(super) struct SuperDrive {
    sample_rate: f32,
    dc: DcBlock,
    dc_out: DcBlock,
    input_hpf: StereoBiquad,
    mid_hump: StereoBiquad,
    low_keep: StereoBiquad,
    post_lpf: StereoBiquad,
    os: Oversampler4x,
    env: EnvFollower,
    pre_gain: Smoothed,
    sag_amount: Smoothed,
    low_blend: Smoothed,
    out_gain: Smoothed,
    mix: Smoothed,
}

impl SuperDrive {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            dc: DcBlock::new(sr),
            dc_out: DcBlock::new(sr),
            input_hpf: StereoBiquad::none(),
            mid_hump: StereoBiquad::none(),
            low_keep: StereoBiquad::none(),
            post_lpf: StereoBiquad::none(),
            os: Oversampler4x::new(),
            env: EnvFollower::new(sr, 0.005, 0.090),
            pre_gain: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
            sag_amount: Smoothed::new(sr, SMOOTH_SECONDS, 0.0),
            low_blend: Smoothed::new(sr, SMOOTH_SECONDS, 0.18),
            out_gain: Smoothed::new(sr, SMOOTH_SECONDS, 0.6),
            mix: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.dc.set_sample_rate(self.sample_rate);
        self.dc_out.set_sample_rate(self.sample_rate);
        self.env.set_sample_rate(self.sample_rate);
        for s in [
            &mut self.pre_gain,
            &mut self.sag_amount,
            &mut self.low_blend,
            &mut self.out_gain,
            &mut self.mix,
        ] {
            s.set_time(self.sample_rate, SMOOTH_SECONDS);
        }
    }

    pub(super) fn reset(&mut self) {
        self.dc.reset();
        self.dc_out.reset();
        self.input_hpf.reset();
        self.mid_hump.reset();
        self.low_keep.reset();
        self.post_lpf.reset();
        self.os.reset();
        self.env.reset();
        for s in [
            &mut self.pre_gain,
            &mut self.sag_amount,
            &mut self.low_blend,
            &mut self.out_gain,
            &mut self.mix,
        ] {
            s.snap();
        }
    }

    /// Tone shifts the mid hump up and opens the top together.
    pub(super) fn configure(&mut self, gain: f32, tone: f32, level: f32) {
        let d = drive_curve(gain / 10.0);
        let t = (tone / 10.0).clamp(0.0, 1.0);
        let lvl = (level / 10.0).clamp(0.0, 1.0);
        let sr = self.sample_rate;

        let gain_db = 4.0 + d * 22.0; // 4..26 dB
        self.pre_gain.set_target(db_to_linear(gain_db));
        self.sag_amount.set_target(0.15 + d * 0.55);
        // Clean-low blend stays subtle and eases off as drive saturates.
        self.low_blend.set_target(0.22 - d * 0.10);
        let comp_db = -(gain_db - 4.0) * 0.65;
        self.out_gain
            .set_target(db_to_linear(comp_db) * (0.45 + lvl * 1.3));
        self.mix.set_target(1.0);

        self.dc.set_sample_rate(sr);
        self.dc_out.set_sample_rate(sr);
        self.input_hpf
            .set(make_eq_biquad("highpass", 115.0, 0.0, 0.707, sr));
        let hump_hz = 650.0 + t * 300.0; // 650..950
        self.mid_hump
            .set(make_eq_biquad("bell", hump_hz, 4.0 + d * 2.5, 0.8, sr));
        // The clean body band that skips the clipper entirely.
        self.low_keep
            .set(make_eq_biquad("lowpass", 190.0, 0.0, 0.707, sr));
        let lpf = (7_000.0 + t * 3_000.0).min(sr * 0.45); // 7..10 kHz
        self.post_lpf
            .set(make_eq_biquad("lowpass", lpf, 0.0, 0.707, sr));
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let pre = self.pre_gain.tick();
        let sag = self.sag_amount.tick();
        let low_amt = self.low_blend.tick();
        let out = self.out_gain.tick();
        let mix = self.mix.tick();

        let (l, r) = self.dc.run(left, right);
        let (bl, br) = self.low_keep.run(l, r);
        let (l, r) = self.input_hpf.run(l, r);
        let (l, r) = self.mid_hump.run(l, r);

        // Envelope drives both the sag and the asymmetry: dig in → more
        // compression, more even harmonics.
        let (el, er) = self.env.tick(l * pre, r * pre);
        let sag_l = 1.0 / (1.0 + el * sag);
        let sag_r = 1.0 / (1.0 + er * sag);
        let asym_l = (0.75 - el * 0.35).clamp(0.35, 0.9);
        let asym_r = (0.75 - er * 0.35).clamp(0.35, 0.9);

        let (mut wl, mut wr) = self
            .os
            .process_stereo(l * pre * sag_l, r * pre * sag_r, |a, b| {
                (soft_asym(a, 1.35, asym_l), soft_asym(b, 1.35, asym_r))
            });
        (wl, wr) = self.post_lpf.run(wl, wr);
        wl = sanitize(wl + bl * low_amt);
        wr = sanitize(wr + br * low_amt);
        // Asymmetric clipping generates DC — strip it before it eats
        // headroom or thumps the mix.
        (wl, wr) = self.dc_out.run(wl, wr);
        wl *= out;
        wr *= out;
        (
            equal_power_mix(left, wl, mix),
            equal_power_mix(right, wr, mix),
        )
    }
}

// ---------------------------------------------------------------------------
// Metal Core — two-stage scooped metal distortion (8×)
// ---------------------------------------------------------------------------

/// True two-stage topology: asymmetric body stage → interstage bass cut +
/// presence (inside the 8× domain) → harder symmetric stage with a fast-knee
/// ceiling → scoop/resonance/fizz post section. Subtle sag only — tightness
/// wins over pump.
#[derive(Debug, Clone)]
pub(super) struct MetalCore {
    sample_rate: f32,
    dc: DcBlock,
    dc_out: DcBlock,
    input_hpf: StereoBiquad,
    tighten: StereoBiquad,
    // Oversampled-domain filters (configured at 8× rate).
    inter_hpf: StereoBiquad,
    inter_presence: StereoBiquad,
    // Base-rate post section.
    scoop: StereoBiquad,
    resonance: StereoBiquad,
    fizz_lpf: StereoBiquad,
    os: Oversampler8x,
    env: EnvFollower,
    pre_gain: Smoothed,
    inter_gain: Smoothed,
    ceiling: Smoothed,
    out_gain: Smoothed,
    mix: Smoothed,
}

impl MetalCore {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            dc: DcBlock::new(sr),
            dc_out: DcBlock::new(sr),
            input_hpf: StereoBiquad::none(),
            tighten: StereoBiquad::none(),
            inter_hpf: StereoBiquad::none(),
            inter_presence: StereoBiquad::none(),
            scoop: StereoBiquad::none(),
            resonance: StereoBiquad::none(),
            fizz_lpf: StereoBiquad::none(),
            os: Oversampler8x::new(),
            env: EnvFollower::new(sr, 0.002, 0.060),
            pre_gain: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
            inter_gain: Smoothed::new(sr, SMOOTH_SECONDS, 2.0),
            ceiling: Smoothed::new(sr, SMOOTH_SECONDS, 0.85),
            out_gain: Smoothed::new(sr, SMOOTH_SECONDS, 0.4),
            mix: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.dc.set_sample_rate(self.sample_rate);
        self.dc_out.set_sample_rate(self.sample_rate);
        self.env.set_sample_rate(self.sample_rate);
        for s in [
            &mut self.pre_gain,
            &mut self.inter_gain,
            &mut self.ceiling,
            &mut self.out_gain,
            &mut self.mix,
        ] {
            s.set_time(self.sample_rate, SMOOTH_SECONDS);
        }
    }

    pub(super) fn reset(&mut self) {
        self.dc.reset();
        self.dc_out.reset();
        self.input_hpf.reset();
        self.tighten.reset();
        self.inter_hpf.reset();
        self.inter_presence.reset();
        self.scoop.reset();
        self.resonance.reset();
        self.fizz_lpf.reset();
        self.os.reset();
        self.env.reset();
        for s in [
            &mut self.pre_gain,
            &mut self.inter_gain,
            &mut self.ceiling,
            &mut self.out_gain,
            &mut self.mix,
        ] {
            s.snap();
        }
    }

    /// Tone rebalances scoop depth ↔ presence ↔ fizz ceiling in one gesture:
    /// low = cavernous scooped wall, high = tighter, more forward.
    pub(super) fn configure(&mut self, gain: f32, tone: f32, level: f32) {
        let d = drive_curve(gain / 10.0);
        let t = (tone / 10.0).clamp(0.0, 1.0);
        let lvl = (level / 10.0).clamp(0.0, 1.0);
        let sr = self.sample_rate;
        let osr = sr * 8.0; // interstage filters live in the 8× domain

        // Moderate gain into stage 1, more into stage 2 — not one 60× wall.
        let stage1_db = 10.0 + d * 16.0; // 10..26 dB
        let stage2_db = 6.0 + d * 12.0; // 6..18 dB more
        self.pre_gain.set_target(db_to_linear(stage1_db));
        self.inter_gain.set_target(db_to_linear(stage2_db));
        self.ceiling.set_target(0.9 - d * 0.2);
        let comp_db = -(stage1_db + stage2_db - 16.0) * 0.7;
        self.out_gain
            .set_target(db_to_linear(comp_db) * (0.6 + lvl * 1.8));
        self.mix.set_target(1.0);

        self.dc.set_sample_rate(sr);
        self.dc_out.set_sample_rate(sr);
        self.input_hpf
            .set(make_eq_biquad("highpass", 65.0, 0.0, 0.707, sr));
        // Pre-clip low-shelf keeps bass out of the saturation; deepens with drive.
        self.tighten.set(make_eq_biquad(
            "lowshelf",
            150.0,
            -(2.0 + d * 5.0),
            0.707,
            sr,
        ));
        // Interstage: strip low-mud, push presence into stage 2 (8× rate!).
        self.inter_hpf
            .set(make_eq_biquad("highpass", 160.0, 0.0, 0.707, osr));
        self.inter_presence
            .set(make_eq_biquad("bell", 2_200.0, 3.0 + t * 2.0, 0.9, osr));
        // Post: the metal V. Tone trades scoop depth for presence and air.
        self.scoop
            .set(make_eq_biquad("bell", 680.0, -(9.0 - t * 5.0), 0.9, sr));
        self.resonance
            .set(make_eq_biquad("bell", 3_100.0, 1.5 + t * 1.5, 1.2, sr));
        let fizz = (7_000.0 + t * 4_000.0).min(sr * 0.45); // 7..11 kHz
        self.fizz_lpf
            .set(make_eq_biquad("lowpass", fizz, 0.0, 0.707, sr));
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let pre = self.pre_gain.tick();
        let inter = self.inter_gain.tick();
        let ceiling = self.ceiling.tick().max(0.2);
        let out = self.out_gain.tick();
        let mix = self.mix.tick();

        let (l, r) = self.dc.run(left, right);
        let (l, r) = self.input_hpf.run(l, r);
        let (l, r) = self.tighten.run(l, r);

        // Subtle sag only: enough to feel alive, never enough to pump.
        let (el, er) = self.env.tick(l * pre, r * pre);
        let sag_l = 1.0 / (1.0 + el * 0.15);
        let sag_r = 1.0 / (1.0 + er * 0.15);

        let inter_hpf = &mut self.inter_hpf;
        let inter_presence = &mut self.inter_presence;
        let (mut wl, mut wr) = self
            .os
            .process_stereo(l * pre * sag_l, r * pre * sag_r, |a, b| {
                // Stage 1: asymmetric body — keeps transient shape.
                let a1 = soft_asym(a, 1.15, 0.8);
                let b1 = soft_asym(b, 1.15, 0.8);
                // Interstage EQ at 8×: no bass into stage 2, presence in.
                let (a2, b2) = inter_hpf.run(a1, b1);
                let (a3, b3) = inter_presence.run(a2, b2);
                // Stage 2: harder, symmetric, fast-knee ceiling.
                let a4 = hard_clip_asym((a3 * inter).tanh() * 1.25, ceiling, ceiling, 0.05);
                let b4 = hard_clip_asym((b3 * inter).tanh() * 1.25, ceiling, ceiling, 0.05);
                (a4, b4)
            });
        (wl, wr) = self.scoop.run(wl, wr);
        (wl, wr) = self.resonance.run(wl, wr);
        (wl, wr) = self.fizz_lpf.run(wl, wr);
        // Asymmetric clipping generates DC — strip it before it eats
        // headroom or thumps the mix.
        (wl, wr) = self.dc_out.run(wl, wr);
        wl = sanitize(wl) * out;
        wr = sanitize(wr) * out;
        (
            equal_power_mix(left, wl, mix),
            equal_power_mix(right, wr, mix),
        )
    }
}

// ---------------------------------------------------------------------------
// Tight Rift — transient-aware modern high-gain (8×)
// ---------------------------------------------------------------------------

/// Djent-style tightness without the old fixed 220 Hz body-ectomy: a gentle
/// real high-pass plus a drive-dependent low shelf, a fast-recovery envelope
/// (detector is high-passed so lows never pump the gain), and a parallel
/// transient path that sharpens pick attack.
#[derive(Debug, Clone)]
pub(super) struct TightRift {
    sample_rate: f32,
    dc: DcBlock,
    dc_out: DcBlock,
    input_hpf: StereoBiquad,
    tighten_shelf: StereoBiquad,
    // Transient path.
    trans_hpf: StereoBiquad,
    // Oversampled-domain interstage (8× rate).
    inter_hpf: StereoBiquad,
    // Post section.
    definition: StereoBiquad,
    pick_bite: StereoBiquad,
    fizz_lpf: StereoBiquad,
    os: Oversampler8x,
    env: EnvFollower,
    pre_gain: Smoothed,
    ceiling: Smoothed,
    trans_amt: Smoothed,
    out_gain: Smoothed,
    mix: Smoothed,
}

impl TightRift {
    pub(super) fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            dc: DcBlock::new(sr),
            dc_out: DcBlock::new(sr),
            input_hpf: StereoBiquad::none(),
            tighten_shelf: StereoBiquad::none(),
            trans_hpf: StereoBiquad::none(),
            inter_hpf: StereoBiquad::none(),
            definition: StereoBiquad::none(),
            pick_bite: StereoBiquad::none(),
            fizz_lpf: StereoBiquad::none(),
            os: Oversampler8x::new(),
            env: EnvFollower::new(sr, 0.001, 0.030),
            pre_gain: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
            ceiling: Smoothed::new(sr, SMOOTH_SECONDS, 0.85),
            trans_amt: Smoothed::new(sr, SMOOTH_SECONDS, 0.12),
            out_gain: Smoothed::new(sr, SMOOTH_SECONDS, 0.4),
            mix: Smoothed::new(sr, SMOOTH_SECONDS, 1.0),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        self.dc.set_sample_rate(self.sample_rate);
        self.dc_out.set_sample_rate(self.sample_rate);
        self.env.set_sample_rate(self.sample_rate);
        for s in [
            &mut self.pre_gain,
            &mut self.ceiling,
            &mut self.trans_amt,
            &mut self.out_gain,
            &mut self.mix,
        ] {
            s.set_time(self.sample_rate, SMOOTH_SECONDS);
        }
    }

    pub(super) fn reset(&mut self) {
        self.dc.reset();
        self.dc_out.reset();
        self.input_hpf.reset();
        self.tighten_shelf.reset();
        self.trans_hpf.reset();
        self.inter_hpf.reset();
        self.definition.reset();
        self.pick_bite.reset();
        self.fizz_lpf.reset();
        self.os.reset();
        self.env.reset();
        for s in [
            &mut self.pre_gain,
            &mut self.ceiling,
            &mut self.trans_amt,
            &mut self.out_gain,
            &mut self.mix,
        ] {
            s.snap();
        }
    }

    /// Tone rebalances definition ↔ pick bite ↔ high damping.
    pub(super) fn configure(&mut self, gain: f32, tone: f32, level: f32) {
        let d = drive_curve(gain / 10.0);
        let t = (tone / 10.0).clamp(0.0, 1.0);
        let lvl = (level / 10.0).clamp(0.0, 1.0);
        let sr = self.sample_rate;
        let osr = sr * 8.0;

        let gain_db = 12.0 + d * 22.0; // 12..34 dB
        self.pre_gain.set_target(db_to_linear(gain_db));
        self.ceiling.set_target(0.9 - d * 0.15);
        self.trans_amt.set_target(0.08 + d * 0.10);
        let comp_db = -(gain_db - 12.0) * 0.7;
        self.out_gain
            .set_target(db_to_linear(comp_db) * (0.5 + lvl * 1.4));
        self.mix.set_target(1.0);

        self.dc.set_sample_rate(sr);
        self.dc_out.set_sample_rate(sr);
        // Real body-preserving high-pass; tightening is the drive-dependent
        // shelf, not a fixed 220 Hz wall.
        self.input_hpf
            .set(make_eq_biquad("highpass", 70.0, 0.0, 0.707, sr));
        let shelf_hz = 150.0 + d * 70.0; // 150..220, only at full drive
        self.tighten_shelf.set(make_eq_biquad(
            "lowshelf",
            shelf_hz,
            -(1.5 + d * 8.5),
            0.707,
            sr,
        ));
        self.trans_hpf
            .set(make_eq_biquad("highpass", 1_200.0, 0.0, 0.707, sr));
        self.inter_hpf
            .set(make_eq_biquad("highpass", 200.0, 0.0, 0.707, osr));
        self.definition
            .set(make_eq_biquad("bell", 1_900.0, 2.0 + t * 3.0, 0.9, sr));
        self.pick_bite
            .set(make_eq_biquad("bell", 3_800.0, 1.5 + t * 2.5, 1.1, sr));
        let fizz = (8_000.0 + t * 4_000.0).min(sr * 0.45); // 8..12 kHz
        self.fizz_lpf
            .set(make_eq_biquad("lowpass", fizz, 0.0, 0.707, sr));
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let pre = self.pre_gain.tick();
        let ceiling = self.ceiling.tick().max(0.2);
        let trans_amt = self.trans_amt.tick();
        let out = self.out_gain.tick();
        let mix = self.mix.tick();

        let (l, r) = self.dc.run(left, right);
        let (l, r) = self.input_hpf.run(l, r);
        // Transient path taps the un-tightened signal for pick attack.
        let (tl, tr) = self.trans_hpf.run(l, r);
        let (l, r) = self.tighten_shelf.run(l, r);

        // Fast-recovery sag on a high-passed detector: palm mutes recover
        // instantly, lows never pump the gain.
        let (el, er) = self.env.tick(tl * pre, tr * pre);
        let sag_l = 1.0 / (1.0 + el * 0.10);
        let sag_r = 1.0 / (1.0 + er * 0.10);

        let inter_hpf = &mut self.inter_hpf;
        let (mut wl, mut wr) = self
            .os
            .process_stereo(l * pre * sag_l, r * pre * sag_r, |a, b| {
                // Stage 1: light asym for body...
                let a1 = soft_asym(a, 1.1, 0.85);
                let b1 = soft_asym(b, 1.1, 0.85);
                // ...then keep stage 2 tight...
                let (a2, b2) = inter_hpf.run(a1, b1);
                // ...into a fast-knee clip with a hard ceiling.
                let a3 = hard_clip_asym((a2 * 2.4).tanh() * 1.15, ceiling, ceiling, 0.04);
                let b3 = hard_clip_asym((b2 * 2.4).tanh() * 1.15, ceiling, ceiling, 0.04);
                (a3, b3)
            });
        // Subtle transient recombination: sharpened attack, not a click layer.
        wl += soft_asym(tl * 2.0, 1.0, 1.0) * trans_amt;
        wr += soft_asym(tr * 2.0, 1.0, 1.0) * trans_amt;
        (wl, wr) = self.definition.run(wl, wr);
        (wl, wr) = self.pick_bite.run(wl, wr);
        (wl, wr) = self.fizz_lpf.run(wl, wr);
        // Asymmetric clipping generates DC — strip it before it eats
        // headroom or thumps the mix.
        (wl, wr) = self.dc_out.run(wl, wr);
        wl = sanitize(wl) * out;
        wr = sanitize(wr) * out;
        (
            equal_power_mix(left, wl, mix),
            equal_power_mix(right, wr, mix),
        )
    }
}
