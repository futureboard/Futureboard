//! Rodhareist — flagship guitar multi-effect DSP core.
//!
//! A realtime-safe stereo pedalboard/amp chain, engine-agnostic like the other
//! `BuiltinAudioPlugins` cores. The signal path mirrors the React editor's
//! signal chain exactly:
//!
//! ```text
//! Gate → Drive → Amp → Chorus → Tape Delay → Plate Reverb → Cabinet
//! ```
//!
//! ## Realtime rules
//!
//! Every buffer (delay lines, reverb combs/allpasses) is preallocated in
//! [`Dsp::new`] / [`Dsp::set_sample_rate`]. [`StereoEffect::process_stereo`]
//! performs no allocation, logging, or locking. Control-thread updates go through
//! [`Dsp::set_params`] / [`Dsp::apply_ui_param`], which recompute coefficients
//! outside the audio callback.
//!
//! Only the MIT/Apache [`biquad`] crate is used for filtering; every waveshaper,
//! delay and reverb is hand-written here (no extra third-party runtime deps).

mod amp;
mod cab;
mod chorus;
mod delay;
mod drive;
mod gate;
mod handoff;
mod nam;
mod reverb;
mod tone_stage;

use builtin_dsp_core::{
    ParamDescriptor, PluginCategory, PluginDescriptor, StereoEffect, clamp, db_to_linear,
    time_constant,
};

use cab::Cabinet;
use chorus::Chorus;
use delay::TapeDelay;
use drive::Drive;
use gate::NoiseGate;
pub use nam::{NamCaptureInfo, NamLoadError};
use reverb::PlateReverb;
pub use tone_stage::ToneEngineKind;
use tone_stage::ToneStage;

pub const PLUGIN_ID: &str = "futureboard.rodharerist";

/// One slot in the Helix-style signal path. Order is user-editable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StageKind {
    Gate = 0,
    Drive = 1,
    Amp = 2,
    Mod = 3,
    Delay = 4,
    Reverb = 5,
    Cab = 6,
}

impl StageKind {
    pub const ALL: &'static [Self] = &[
        Self::Gate,
        Self::Drive,
        Self::Amp,
        Self::Mod,
        Self::Delay,
        Self::Reverb,
        Self::Cab,
    ];

    pub fn from_index(i: i32) -> Option<Self> {
        match i {
            0 => Some(Self::Gate),
            1 => Some(Self::Drive),
            2 => Some(Self::Amp),
            3 => Some(Self::Mod),
            4 => Some(Self::Delay),
            5 => Some(Self::Reverb),
            6 => Some(Self::Cab),
            _ => None,
        }
    }

    pub fn index(self) -> u8 {
        self as u8
    }

    /// Default factory path order.
    pub fn default_path() -> [Option<Self>; 7] {
        [
            Some(Self::Gate),
            Some(Self::Drive),
            Some(Self::Amp),
            Some(Self::Mod),
            Some(Self::Delay),
            Some(Self::Reverb),
            Some(Self::Cab),
        ]
    }
}

/// Overdrive/boost voicing, matching the editor's `dist` models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveModel {
    /// "Green Screamer" — mid-boosted tube-screamer style overdrive.
    Screamer,
    /// "Minotaur Boost" — cleaner, near-transparent analog boost.
    Minotaur,
    /// "Rats Nest" — hard-clipping filthy distortion.
    Rat,
    /// "Breaker Blues" — soft low-gain overdrive.
    Breaker,
    /// "Face Fuzz" — gated asymmetric fuzz.
    Fuzz,
    /// "Centurion" — transparent mid-forward OD.
    Centurion,
}

impl DriveModel {
    pub const ALL: &'static [Self] = &[
        Self::Screamer,
        Self::Minotaur,
        Self::Rat,
        Self::Breaker,
        Self::Fuzz,
        Self::Centurion,
    ];

    /// Map the editor model id.
    pub fn from_model_id(id: &str) -> Option<Self> {
        match id {
            "screamer" => Some(Self::Screamer),
            "minotaur" => Some(Self::Minotaur),
            "rat" => Some(Self::Rat),
            "breaker" => Some(Self::Breaker),
            "fuzz" => Some(Self::Fuzz),
            "centurion" => Some(Self::Centurion),
            _ => None,
        }
    }

    pub fn from_index(i: u32) -> Self {
        Self::ALL
            .get(i as usize)
            .copied()
            .unwrap_or(Self::Screamer)
    }
}

/// Amplifier voicing, matching the editor's `amp` models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpModel {
    /// "Mandarin 80" — warm, mid-forward British tube head.
    Mandarin,
    /// "Brit Plexi 100" — bright, open plexiglass Super Lead.
    Plexi,
    /// "Twin Clean" — high-headroom American clean.
    Twin,
    /// "Top Boost" — chiming British combo.
    TopBoost,
    /// "Recto Modern" — tight high-gain modern.
    Recto,
    /// "JCM Crunch" — classic British crunch.
    Jcm,
    /// "Lead Slate" — saturated hot-rodded lead.
    Slate,
    /// "Bassman" — loose American bass-heavy.
    Bassman,
}

impl AmpModel {
    pub const ALL: &'static [Self] = &[
        Self::Mandarin,
        Self::Plexi,
        Self::Twin,
        Self::TopBoost,
        Self::Recto,
        Self::Jcm,
        Self::Slate,
        Self::Bassman,
    ];

    pub fn from_model_id(id: &str) -> Option<Self> {
        match id {
            "mandarin" => Some(Self::Mandarin),
            "plexi" => Some(Self::Plexi),
            "twin" => Some(Self::Twin),
            "topboost" => Some(Self::TopBoost),
            "recto" => Some(Self::Recto),
            "jcm" => Some(Self::Jcm),
            "slate" => Some(Self::Slate),
            "bassman" => Some(Self::Bassman),
            _ => None,
        }
    }

    pub fn from_index(i: u32) -> Self {
        Self::ALL
            .get(i as usize)
            .copied()
            .unwrap_or(Self::Mandarin)
    }
}

/// Cabinet voicing, matching the editor's `cab` models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CabModel {
    /// "1960v Vintage 4x12" — Celestion vintage cabinet sim.
    Vintage4x12,
    /// "American 2x12" — brighter, tighter open-back combo.
    American2x12,
    /// "Tweed 1x12" — small, boxy, rolled-off single speaker.
    Tweed1x12,
    /// "Modern 4x12" — tight, scooped, extended highs.
    Modern4x12,
}

impl CabModel {
    pub const ALL: &'static [Self] = &[
        Self::Vintage4x12,
        Self::American2x12,
        Self::Tweed1x12,
        Self::Modern4x12,
    ];

    pub fn from_model_id(id: &str) -> Option<Self> {
        match id {
            "vintage_cab" => Some(Self::Vintage4x12),
            "american_2x12" => Some(Self::American2x12),
            "tweed_1x12" => Some(Self::Tweed1x12),
            "modern_412" => Some(Self::Modern4x12),
            _ => None,
        }
    }

    pub fn from_index(i: u32) -> Self {
        Self::ALL
            .get(i as usize)
            .copied()
            .unwrap_or(Self::Vintage4x12)
    }
}

/// Full parameter set. Knob ranges match `editorui/src/data.ts` one-to-one so the
/// React UI and the bridge speak the same units.
#[derive(Debug, Clone)]
pub struct Params {
    pub power: bool,

    /// Global input trim applied before the first stage (dB, -24..24). This is
    /// the level the NAM capture sees, so it changes the model's gain and
    /// tonal response — it is a first-class control, not a hidden utility.
    pub input_trim_db: f32,
    /// Global output trim applied after the last stage (dB, -24..24).
    pub output_trim_db: f32,

    // Per-stage enables (editor "bypass" toggles, inverted).
    pub gate_on: bool,
    pub drive_on: bool,
    pub amp_on: bool,
    pub mod_on: bool,
    pub delay_on: bool,
    pub reverb_on: bool,
    pub cab_on: bool,

    pub drive_model: DriveModel,
    pub amp_model: AmpModel,
    pub cab_model: CabModel,

    /// Which engine the Tone/Amp slot runs: Classic Amp, NAM Capture, or Bypass.
    /// Mutually exclusive — never more than one processes at a time.
    pub tone_engine: ToneEngineKind,

    /// Helix-style ordered path. `None` = empty slot (stage not in path).
    /// Stages absent from this list are not processed.
    pub stage_order: [Option<StageKind>; 7],

    /// Noise gate threshold (dB, -80..0).
    pub gate_thresh_db: f32,

    // Drive (0..10).
    pub drive_gain: f32,
    pub drive_tone: f32,
    pub drive_level: f32,

    // Amp tonestack (0..10).
    pub amp_gain: f32,
    pub amp_bass: f32,
    pub amp_middle: f32,
    pub amp_treble: f32,
    pub amp_presence: f32,
    pub amp_master: f32,

    // Chorus.
    pub chorus_rate: f32,  // 0..10
    pub chorus_depth: f32, // 0..10
    pub chorus_mix: f32,   // 0..100 %

    // Tape delay.
    pub delay_time_ms: f32, // 40..1200
    pub delay_fb: f32,      // 0..100 %
    pub delay_mix: f32,     // 0..100 %

    // Plate reverb.
    pub reverb_decay_s: f32, // 0.5..15
    pub reverb_mix: f32,     // 0..100 %

    // Cabinet.
    pub cab_mic: f32,  // 0..100 % (mic position / brightness)
    pub cab_dist: f32, // 0..100 % (distance / roll-off)

    // NAM Capture (only active while `tone_engine == ToneEngineKind::NamCapture`).
    pub nam_input_trim_db: f32,  // -24..24
    pub nam_output_trim_db: f32, // -24..24
    pub nam_mix: f32,            // 0..100 % wet
    pub nam_loudness_norm: bool,
}

/// Defaults mirror the editor's `parameterDefaults` (Mandarin patch, "06D").
pub fn default_params() -> Params {
    Params {
        power: true,
        input_trim_db: 0.0,
        output_trim_db: 0.0,
        gate_on: true,
        drive_on: true,
        amp_on: true,
        mod_on: true,
        delay_on: true,
        reverb_on: true,
        cab_on: true,
        drive_model: DriveModel::Screamer,
        amp_model: AmpModel::Mandarin,
        cab_model: CabModel::Vintage4x12,
        tone_engine: ToneEngineKind::Classic,
        stage_order: StageKind::default_path(),
        gate_thresh_db: -55.0,
        drive_gain: 6.0,
        drive_tone: 5.5,
        drive_level: 6.5,
        amp_gain: 6.0,
        amp_bass: 5.1,
        amp_middle: 4.8,
        amp_treble: 4.8,
        amp_presence: 5.0,
        amp_master: 3.5,
        chorus_rate: 4.0,
        chorus_depth: 5.5,
        chorus_mix: 40.0,
        delay_time_ms: 420.0,
        delay_fb: 35.0,
        delay_mix: 30.0,
        reverb_decay_s: 8.5,
        reverb_mix: 55.0,
        cab_mic: 20.0,
        cab_dist: 40.0,
        nam_input_trim_db: 0.0,
        nam_output_trim_db: 0.0,
        nam_mix: 100.0,
        nam_loudness_norm: true,
    }
}

/// Host-facing descriptor. Exposes the headline parameters (ids match `data.ts`).
pub fn descriptor() -> PluginDescriptor {
    PluginDescriptor {
        id: PLUGIN_ID,
        name: "Rodhareist",
        vendor: "Futureboard",
        category: PluginCategory::Effect,
        version: env!("CARGO_PKG_VERSION"),
        params: &[
            ParamDescriptor { id: "power", name: "Power", default_value: 1.0, min: 0.0, max: 1.0, unit: "bool" },
            ParamDescriptor { id: "input_trim", name: "Input Trim", default_value: 0.0, min: -24.0, max: 24.0, unit: "dB" },
            ParamDescriptor { id: "output_trim", name: "Output Trim", default_value: 0.0, min: -24.0, max: 24.0, unit: "dB" },
            ParamDescriptor { id: "gate_thresh", name: "Gate", default_value: -55.0, min: -80.0, max: 0.0, unit: "dB" },
            ParamDescriptor { id: "drive_gain", name: "Drive", default_value: 6.0, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "drive_tone", name: "Drive Tone", default_value: 5.5, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "drive_level", name: "Drive Level", default_value: 6.5, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "amp_gain", name: "Amp Drive", default_value: 6.0, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "amp_bass", name: "Bass", default_value: 5.1, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "amp_middle", name: "Middle", default_value: 4.8, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "amp_treble", name: "Treble", default_value: 4.8, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "amp_presence", name: "Presence", default_value: 5.0, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "amp_master", name: "Master", default_value: 3.5, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "chorus_rate", name: "Chorus Rate", default_value: 4.0, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "chorus_depth", name: "Chorus Depth", default_value: 5.5, min: 0.0, max: 10.0, unit: "" },
            ParamDescriptor { id: "chorus_mix", name: "Chorus Mix", default_value: 40.0, min: 0.0, max: 100.0, unit: "%" },
            ParamDescriptor { id: "delay_time", name: "Delay Time", default_value: 420.0, min: 40.0, max: 1200.0, unit: "ms" },
            ParamDescriptor { id: "delay_fb", name: "Delay FB", default_value: 35.0, min: 0.0, max: 100.0, unit: "%" },
            ParamDescriptor { id: "delay_mix", name: "Delay Mix", default_value: 30.0, min: 0.0, max: 100.0, unit: "%" },
            ParamDescriptor { id: "reverb_decay", name: "Reverb Decay", default_value: 8.5, min: 0.5, max: 15.0, unit: "s" },
            ParamDescriptor { id: "reverb_mix", name: "Reverb Mix", default_value: 55.0, min: 0.0, max: 100.0, unit: "%" },
            ParamDescriptor { id: "cab_mic", name: "Cab Mic", default_value: 20.0, min: 0.0, max: 100.0, unit: "%" },
            ParamDescriptor { id: "cab_dist", name: "Cab Distance", default_value: 40.0, min: 0.0, max: 100.0, unit: "%" },
        ],
    }
}

/// The full guitar chain.
pub struct Dsp {
    sample_rate: f32,
    params: Params,
    gate: NoiseGate,
    drive: Drive,
    amp_stage: ToneStage,
    chorus: Chorus,
    delay: TapeDelay,
    reverb: PlateReverb,
    cab: Cabinet,
    /// Linear gains derived from `input_trim_db` / `output_trim_db`, recomputed
    /// on the control thread so the audio path only multiplies.
    in_gain: f32,
    out_gain: f32,
    meters: Meters,
}

/// Input/output telemetry for the editor. Written from the audio thread with
/// plain scalar arithmetic (no allocation, no locking); read opportunistically
/// by the control thread, which tolerates a torn read of a stale frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct MeterFrame {
    pub in_peak: f32,
    pub in_rms: f32,
    pub out_peak: f32,
    pub out_rms: f32,
    /// Set when a post-trim input sample reached full scale. Sticky until the
    /// editor calls [`Dsp::clear_clip`] (click-to-reset).
    pub in_clip: bool,
    /// Set when a post-trim output sample reached full scale. Sticky.
    pub out_clip: bool,
}

#[derive(Debug, Clone)]
struct Meters {
    in_peak: f32,
    out_peak: f32,
    /// Mean-square running averages; `sqrt` is deferred to the reader so the
    /// audio path stays a multiply-add.
    in_ms: f32,
    out_ms: f32,
    /// One-pole coefficient for a ~300 ms RMS window.
    rms_coeff: f32,
    in_clip: bool,
    out_clip: bool,
}

impl Meters {
    fn new(sample_rate: f32) -> Self {
        Self {
            in_peak: 0.0,
            out_peak: 0.0,
            in_ms: 0.0,
            out_ms: 0.0,
            rms_coeff: time_constant(sample_rate, 0.300),
            in_clip: false,
            out_clip: false,
        }
    }

    fn reset(&mut self) {
        self.in_peak = 0.0;
        self.out_peak = 0.0;
        self.in_ms = 0.0;
        self.out_ms = 0.0;
        self.in_clip = false;
        self.out_clip = false;
    }
}

/// Full scale. A sample at or beyond this is reported as a clip.
const CLIP_THRESHOLD: f32 = 1.0;

impl Dsp {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        let mut dsp = Self {
            sample_rate: sr,
            params: default_params(),
            gate: NoiseGate::new(sr),
            drive: Drive::new(sr),
            amp_stage: ToneStage::new(sr),
            chorus: Chorus::new(sr),
            delay: TapeDelay::new(sr),
            reverb: PlateReverb::new(sr),
            cab: Cabinet::new(sr),
            in_gain: 1.0,
            out_gain: 1.0,
            meters: Meters::new(sr),
        };
        dsp.apply_params();
        dsp
    }

    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Latest input/output peak levels (0..1), for editor VU telemetry. Decays
    /// each block; never blocks the audio thread.
    pub fn meter_levels(&self) -> (f32, f32) {
        (self.meters.in_peak, self.meters.out_peak)
    }

    /// Full input/output telemetry (peak, RMS and sticky clip flags) for the
    /// editor's gain-staging display. Both levels are measured **after** the
    /// corresponding trim, so the numbers match what the chain and the host
    /// actually see.
    pub fn meter_frame(&self) -> MeterFrame {
        MeterFrame {
            in_peak: self.meters.in_peak,
            in_rms: self.meters.in_ms.max(0.0).sqrt(),
            out_peak: self.meters.out_peak,
            out_rms: self.meters.out_ms.max(0.0).sqrt(),
            in_clip: self.meters.in_clip,
            out_clip: self.meters.out_clip,
        }
    }

    /// Clear the sticky clip indicators (editor click-to-reset).
    pub fn clear_clip(&mut self) {
        self.meters.in_clip = false;
        self.meters.out_clip = false;
    }

    /// Replace the whole parameter set (clamps to legal ranges) and recompute.
    pub fn set_params(&mut self, params: Params) {
        self.params = Params {
            power: params.power,
            input_trim_db: clamp(params.input_trim_db, -24.0, 24.0),
            output_trim_db: clamp(params.output_trim_db, -24.0, 24.0),
            gate_on: params.gate_on,
            drive_on: params.drive_on,
            amp_on: params.amp_on,
            mod_on: params.mod_on,
            delay_on: params.delay_on,
            reverb_on: params.reverb_on,
            cab_on: params.cab_on,
            drive_model: params.drive_model,
            amp_model: params.amp_model,
            cab_model: params.cab_model,
            tone_engine: params.tone_engine,
            stage_order: sanitize_stage_order(params.stage_order),
            gate_thresh_db: clamp(params.gate_thresh_db, -80.0, 0.0),
            drive_gain: clamp(params.drive_gain, 0.0, 10.0),
            drive_tone: clamp(params.drive_tone, 0.0, 10.0),
            drive_level: clamp(params.drive_level, 0.0, 10.0),
            amp_gain: clamp(params.amp_gain, 0.0, 10.0),
            amp_bass: clamp(params.amp_bass, 0.0, 10.0),
            amp_middle: clamp(params.amp_middle, 0.0, 10.0),
            amp_treble: clamp(params.amp_treble, 0.0, 10.0),
            amp_presence: clamp(params.amp_presence, 0.0, 10.0),
            amp_master: clamp(params.amp_master, 0.0, 10.0),
            chorus_rate: clamp(params.chorus_rate, 0.0, 10.0),
            chorus_depth: clamp(params.chorus_depth, 0.0, 10.0),
            chorus_mix: clamp(params.chorus_mix, 0.0, 100.0),
            delay_time_ms: clamp(params.delay_time_ms, 40.0, 1200.0),
            delay_fb: clamp(params.delay_fb, 0.0, 100.0),
            delay_mix: clamp(params.delay_mix, 0.0, 100.0),
            reverb_decay_s: clamp(params.reverb_decay_s, 0.5, 15.0),
            reverb_mix: clamp(params.reverb_mix, 0.0, 100.0),
            cab_mic: clamp(params.cab_mic, 0.0, 100.0),
            cab_dist: clamp(params.cab_dist, 0.0, 100.0),
            nam_input_trim_db: clamp(params.nam_input_trim_db, -24.0, 24.0),
            nam_output_trim_db: clamp(params.nam_output_trim_db, -24.0, 24.0),
            nam_mix: clamp(params.nam_mix, 0.0, 100.0),
            nam_loudness_norm: params.nam_loudness_norm,
        };
        self.apply_params();
    }

    /// Route a single editor parameter (`data.ts` id) to the matching field, then
    /// recompute. Also accepts stage enable ids (`gate_on`, `drive_on`, …) and
    /// model selects (`drive_model`=0/1, `amp_model`=0/1). Returns `false` for an
    /// unknown id so a bridge can flag mismatches. Control-thread only.
    pub fn apply_ui_param(&mut self, id: &str, value: f32) -> bool {
        let mut p = self.params.clone();
        let on = value >= 0.5;
        match id {
            "power" => p.power = on,
            "input_trim" => p.input_trim_db = value,
            "output_trim" => p.output_trim_db = value,
            "gate_on" => p.gate_on = on,
            "drive_on" => p.drive_on = on,
            "amp_on" => p.amp_on = on,
            "mod_on" => p.mod_on = on,
            "delay_on" => p.delay_on = on,
            "reverb_on" => p.reverb_on = on,
            "cab_on" => p.cab_on = on,
            "drive_model" => p.drive_model = DriveModel::from_index(value.round() as u32),
            "amp_model" => {
                p.amp_model = AmpModel::from_index(value.round() as u32);
                p.tone_engine = ToneEngineKind::Classic;
            }
            "cab_model" => p.cab_model = CabModel::from_index(value.round() as u32),
            "tone_engine" => p.tone_engine = ToneEngineKind::from_index(value.round() as u32),
            "path_slot_0" => p.stage_order[0] = StageKind::from_index(value.round() as i32),
            "path_slot_1" => p.stage_order[1] = StageKind::from_index(value.round() as i32),
            "path_slot_2" => p.stage_order[2] = StageKind::from_index(value.round() as i32),
            "path_slot_3" => p.stage_order[3] = StageKind::from_index(value.round() as i32),
            "path_slot_4" => p.stage_order[4] = StageKind::from_index(value.round() as i32),
            "path_slot_5" => p.stage_order[5] = StageKind::from_index(value.round() as i32),
            "path_slot_6" => p.stage_order[6] = StageKind::from_index(value.round() as i32),
            "gate_thresh" => p.gate_thresh_db = value,
            "drive_gain" => p.drive_gain = value,
            "drive_tone" => p.drive_tone = value,
            "drive_level" => p.drive_level = value,
            "amp_gain" => p.amp_gain = value,
            "amp_bass" => p.amp_bass = value,
            "amp_middle" => p.amp_middle = value,
            "amp_treble" => p.amp_treble = value,
            "amp_presence" => p.amp_presence = value,
            "amp_master" => p.amp_master = value,
            "chorus_rate" => p.chorus_rate = value,
            "chorus_depth" => p.chorus_depth = value,
            "chorus_mix" => p.chorus_mix = value,
            "delay_time" => p.delay_time_ms = value,
            "delay_fb" => p.delay_fb = value,
            "delay_mix" => p.delay_mix = value,
            "reverb_decay" => p.reverb_decay_s = value,
            "reverb_mix" => p.reverb_mix = value,
            "cab_mic" => p.cab_mic = value,
            "cab_dist" => p.cab_dist = value,
            "nam_input_trim" => p.nam_input_trim_db = value,
            "nam_output_trim" => p.nam_output_trim_db = value,
            "nam_mix" => p.nam_mix = value,
            "nam_loudness_norm" => p.nam_loudness_norm = on,
            _ => return false,
        }
        self.set_params(p);
        true
    }

    /// Push clamped params into each stage (control-thread only).
    /// Select a stage model by editor id (`mandarin`, `screamer`, …).
    /// Control-thread only.
    pub fn select_model(&mut self, category: &str, model_id: &str) -> bool {
        let mut p = self.params.clone();
        match category {
            "amp" => match model_id {
                "bypass" => p.tone_engine = ToneEngineKind::Bypass,
                "nam_capture" => p.tone_engine = ToneEngineKind::NamCapture,
                _ => {
                    let Some(m) = AmpModel::from_model_id(model_id) else {
                        return false;
                    };
                    p.amp_model = m;
                    p.tone_engine = ToneEngineKind::Classic;
                }
            },
            "drive" => {
                let Some(m) = DriveModel::from_model_id(model_id) else {
                    return false;
                };
                p.drive_model = m;
            }
            // Single-algorithm stages: accept their canonical model ids.
            "gate" if model_id == "gate" => {}
            "mod" if model_id == "chorus" => {}
            "delay" if model_id == "tape" => {}
            "reverb" if model_id == "plate" => {}
            "cab" => {
                let Some(m) = CabModel::from_model_id(model_id) else {
                    return false;
                };
                p.cab_model = m;
            }
            _ => return false,
        }
        self.params = p;
        self.apply_params();
        true
    }

    fn apply_params(&mut self) {
        let p = &self.params;
        self.in_gain = db_to_linear(p.input_trim_db);
        self.out_gain = db_to_linear(p.output_trim_db);
        self.gate.set_threshold_db(p.gate_thresh_db);
        self.drive.configure(p.drive_model, p.drive_gain, p.drive_tone, p.drive_level);
        self.amp_stage.set_engine(p.tone_engine);
        self.amp_stage.configure_classic(
            p.amp_model,
            p.amp_gain,
            p.amp_bass,
            p.amp_middle,
            p.amp_treble,
            p.amp_presence,
            p.amp_master,
        );
        self.amp_stage.configure_nam(
            p.nam_input_trim_db,
            p.nam_output_trim_db,
            p.nam_mix,
            p.nam_loudness_norm,
        );
        self.chorus.configure(p.chorus_rate, p.chorus_depth, p.chorus_mix);
        self.delay.configure(p.delay_time_ms, p.delay_fb, p.delay_mix);
        self.reverb.configure(p.reverb_decay_s, p.reverb_mix);
        self.cab.configure(p.cab_model, p.cab_mic, p.cab_dist);
    }

    /// Replace the Helix path order (control thread).
    pub fn set_path_order(&mut self, order: [Option<StageKind>; 7]) {
        self.params.stage_order = sanitize_stage_order(order);
    }

    /// Audio thread: call once per audio block, before the block's per-sample
    /// [`StereoEffect::process_stereo`] calls. This is the only place a
    /// pending NAM capture swap (queued by [`Dsp::load_nam_capture_json`] on
    /// the control thread) is adopted — never mid-block, never per-sample.
    pub fn begin_block(&mut self) {
        self.amp_stage.begin_block();
    }

    /// Control thread: parse and build a `.nam` capture, then queue it for the
    /// audio thread to adopt at the next [`Dsp::begin_block`]. Rejects a
    /// sample-rate mismatch (nam-rs does not resample) rather than silently
    /// mis-running. `stereo` builds two independent models (true stereo
    /// width); otherwise one model's output is mirrored to both channels.
    /// `full_rig` marks the capture as already modeling amp + cab + mic, so
    /// the host/UI can offer an explicit "Bypass Cab" action.
    pub fn load_nam_capture_json(
        &mut self,
        json: &str,
        name: impl Into<String>,
        stereo: bool,
        full_rig: bool,
    ) -> Result<NamCaptureInfo, NamLoadError> {
        let prepared = nam::prepare_nam_runtime(
            json,
            name.into(),
            self.sample_rate as f64,
            stereo,
            full_rig,
        )?;
        let info = prepared.info();
        // Opportunistic sweep: drop whatever the audio thread has already
        // retired before handing off the new one.
        self.amp_stage.poll_nam_garbage();
        self.amp_stage.submit_nam_runtime(Box::new(prepared));
        Ok(info)
    }

    /// Control thread: drop any NAM capture the audio thread has retired.
    /// Safe to call periodically (e.g. an idle/UI timer/poll) even when
    /// nothing is pending.
    pub fn poll_nam_garbage(&mut self) {
        self.amp_stage.poll_nam_garbage();
    }

    /// Info about the currently active NAM capture, if the Tone/Amp slot has
    /// one loaded (regardless of whether `NamCapture` is the active engine).
    pub fn nam_capture_info(&self) -> Option<NamCaptureInfo> {
        self.amp_stage.nam_capture_info()
    }

    /// Latency contributed by the active NAM capture's receptive field, in
    /// samples (0 if none loaded). A preallocated sample-rate adapter and
    /// full plugin-latency reporting are follow-up work; this exposes the raw
    /// number the capture itself already computes.
    pub fn nam_latency_samples(&self) -> usize {
        self.amp_stage.nam_latency_samples()
    }
}

/// Keep first occurrence of each stage; pack non-empty slots left; clear rest.
fn sanitize_stage_order(order: [Option<StageKind>; 7]) -> [Option<StageKind>; 7] {
    let mut out = [None; 7];
    let mut seen = [false; 7];
    let mut w = 0usize;
    for slot in order {
        let Some(stage) = slot else { continue };
        let idx = stage.index() as usize;
        if seen[idx] {
            continue;
        }
        seen[idx] = true;
        if w < 7 {
            out[w] = Some(stage);
            w += 1;
        }
    }
    out
}

impl StereoEffect for Dsp {
    fn reset(&mut self) {
        self.gate.reset();
        self.drive.reset();
        self.amp_stage.reset();
        self.chorus.reset();
        self.delay.reset();
        self.reverb.reset();
        self.cab.reset();
        self.meters.reset();
    }

    fn set_sample_rate(&mut self, sample_rate: f32) {
        let sr = sample_rate.max(1.0);
        self.sample_rate = sr;
        self.gate.set_sample_rate(sr);
        self.drive.set_sample_rate(sr);
        self.amp_stage.set_sample_rate(sr);
        self.chorus.set_sample_rate(sr);
        self.delay.set_sample_rate(sr);
        self.reverb.set_sample_rate(sr);
        self.cab.set_sample_rate(sr);
        self.meters.rms_coeff = time_constant(sr, 0.300);
        self.apply_params();
    }

    fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.params.power {
            return (left, right);
        }

        // Input trim first: everything downstream — including a NAM capture,
        // whose gain and voicing depend on the level it is fed — sees the
        // trimmed signal, so that is also what the input meter must report.
        let (mut l, mut r) = (left * self.in_gain, right * self.in_gain);

        let in_level = l.abs().max(r.abs());
        self.meters.in_peak = in_level.max(self.meters.in_peak - self.meters.in_peak * 0.0005);
        self.meters.in_ms = in_level * in_level
            + self.meters.rms_coeff * (self.meters.in_ms - in_level * in_level);
        if in_level >= CLIP_THRESHOLD {
            self.meters.in_clip = true;
        }

        for slot in self.params.stage_order {
            let Some(stage) = slot else { continue };
            match stage {
                StageKind::Gate if self.params.gate_on => {
                    (l, r) = self.gate.process(l, r);
                }
                StageKind::Drive if self.params.drive_on => {
                    (l, r) = self.drive.process(l, r);
                }
                StageKind::Amp if self.params.amp_on => {
                    (l, r) = self.amp_stage.process(l, r);
                }
                StageKind::Mod if self.params.mod_on => {
                    (l, r) = self.chorus.process(l, r);
                }
                StageKind::Delay if self.params.delay_on => {
                    (l, r) = self.delay.process(l, r);
                }
                StageKind::Reverb if self.params.reverb_on => {
                    (l, r) = self.reverb.process(l, r);
                }
                StageKind::Cab if self.params.cab_on => {
                    (l, r) = self.cab.process(l, r);
                }
                _ => {}
            }
        }

        // Guard against denormals / NaNs escaping into the engine.
        if !l.is_finite() {
            l = 0.0;
        }
        if !r.is_finite() {
            r = 0.0;
        }

        l *= self.out_gain;
        r *= self.out_gain;

        let out_level = l.abs().max(r.abs());
        self.meters.out_peak = out_level.max(self.meters.out_peak - self.meters.out_peak * 0.0005);
        self.meters.out_ms = out_level * out_level
            + self.meters.rms_coeff * (self.meters.out_ms - out_level * out_level);
        if out_level >= CLIP_THRESHOLD {
            self.meters.out_clip = true;
        }
        (l, r)
    }
}

// ---------------------------------------------------------------------------
// Shared realtime-safe building blocks used by the stage modules.
// ---------------------------------------------------------------------------

use biquad::{Biquad, DirectForm1};

/// A biquad with independent left/right state but shared coefficients, so a
/// stereo stage filters each channel correctly (a single instance cannot serve
/// both channels — its state would be stepped at twice the rate).
#[derive(Debug, Clone, Default)]
pub(crate) struct StereoBiquad {
    left: Option<DirectForm1<f32>>,
    right: Option<DirectForm1<f32>>,
}

impl StereoBiquad {
    pub(crate) fn none() -> Self {
        Self { left: None, right: None }
    }

    /// Install (or clear) the filter for both channels. `DirectForm1` is `Copy`,
    /// so both channels start from identical coefficients and state.
    pub(crate) fn set(&mut self, filter: Option<DirectForm1<f32>>) {
        self.left = filter;
        self.right = filter;
    }

    pub(crate) fn reset(&mut self) {
        if let Some(f) = self.left.as_mut() {
            f.reset_state();
        }
        if let Some(f) = self.right.as_mut() {
            f.reset_state();
        }
    }

    #[inline]
    pub(crate) fn run(&mut self, left: f32, right: f32) -> (f32, f32) {
        let l = self.left.as_mut().map(|f| f.run(left)).unwrap_or(left);
        let r = self.right.as_mut().map(|f| f.run(right)).unwrap_or(right);
        (l, r)
    }
}

/// A fractional-read circular delay line (preallocated). Used by chorus and the
/// tape delay for modulated / interpolated taps.
#[derive(Debug, Clone)]
pub(crate) struct InterpDelay {
    buffer: Vec<f32>,
    write: usize,
}

impl InterpDelay {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity.max(2)],
            write: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    #[inline]
    pub(crate) fn write_sample(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write += 1;
        if self.write >= self.buffer.len() {
            self.write = 0;
        }
    }

    /// Linearly-interpolated read `delay_samples` behind the write head.
    #[inline]
    pub(crate) fn read_interp(&self, delay_samples: f32) -> f32 {
        let len = self.buffer.len();
        let max_delay = (len - 2) as f32;
        let d = delay_samples.clamp(1.0, max_delay);
        let mut read_pos = self.write as f32 - d;
        while read_pos < 0.0 {
            read_pos += len as f32;
        }
        let base = read_pos.floor();
        let frac = read_pos - base;
        let i0 = base as usize % len;
        let i1 = (i0 + 1) % len;
        self.buffer[i0] * (1.0 - frac) + self.buffer[i1] * frac
    }
}

/// A minimal sine LFO with a stable per-sample increment.
#[derive(Debug, Clone)]
pub(crate) struct Lfo {
    phase: f32,
    increment: f32,
}

impl Lfo {
    pub(crate) fn new() -> Self {
        Self {
            phase: 0.0,
            increment: 0.0,
        }
    }

    pub(crate) fn set_rate(&mut self, rate_hz: f32, sample_rate: f32) {
        self.increment = (rate_hz.max(0.0) / sample_rate.max(1.0)).min(0.5);
    }

    pub(crate) fn set_phase(&mut self, phase01: f32) {
        self.phase = phase01.rem_euclid(1.0);
    }

    /// Advance and return a sine in [-1, 1].
    #[inline]
    pub(crate) fn tick(&mut self) -> f32 {
        let value = (self.phase * std::f32::consts::TAU).sin();
        self.phase += self.increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        value
    }

    pub(crate) fn reset(&mut self) {
        self.phase = 0.0;
    }
}

/// Cheap, stable soft clipper (`tanh` approximation is fine here — `tanh` itself
/// is used where accuracy matters).
#[inline]
pub(crate) fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

/// Asymmetric tube-ish saturation: even-harmonic bias plus soft clipping.
#[inline]
pub(crate) fn tube_stage(x: f32, bias: f32, drive: f32) -> f32 {
    let biased = x * drive + bias;
    (biased.tanh() - bias.tanh()) / drive.max(1.0e-6)
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
    fn tone_engine_is_mutually_exclusive_via_select_model_and_ui_param() {
        let mut dsp = Dsp::new(48_000.0);
        assert_eq!(dsp.params().tone_engine, ToneEngineKind::Classic);

        assert!(dsp.select_model("amp", "nam_capture"));
        assert_eq!(dsp.params().tone_engine, ToneEngineKind::NamCapture);

        assert!(dsp.select_model("amp", "bypass"));
        assert_eq!(dsp.params().tone_engine, ToneEngineKind::Bypass);

        // Picking a classic amp model always snaps the engine back to Classic.
        assert!(dsp.select_model("amp", "plexi"));
        assert_eq!(dsp.params().tone_engine, ToneEngineKind::Classic);
        assert_eq!(dsp.params().amp_model, AmpModel::Plexi);

        assert!(dsp.apply_ui_param("tone_engine", 1.0));
        assert_eq!(dsp.params().tone_engine, ToneEngineKind::NamCapture);
    }

    #[test]
    fn nam_capture_loads_and_swaps_at_block_boundary_not_mid_block() {
        let mut dsp = Dsp::new(48_000.0);
        dsp.apply_ui_param("tone_engine", 1.0);

        let info = dsp
            .load_nam_capture_json(TINY_WAVENET_48K, "Test Capture", false, true)
            .expect("matching sample rate must load");
        assert_eq!(info.name, "Test Capture");
        assert!(info.full_rig);
        assert!(dsp.nam_capture_info().is_none(), "not adopted before begin_block");

        dsp.begin_block();
        assert!(dsp.nam_capture_info().is_some(), "adopted at block boundary");

        for _ in 0..256 {
            let (l, r) = dsp.process_stereo(0.2, -0.2);
            assert!(l.is_finite() && r.is_finite());
        }

        dsp.poll_nam_garbage();
    }

    #[test]
    fn nam_capture_rejects_sample_rate_mismatch() {
        let mut dsp = Dsp::new(44_100.0);
        let err = dsp
            .load_nam_capture_json(TINY_WAVENET_48K, "Bad Rate", false, false)
            .expect_err("48kHz capture must be rejected at 44.1kHz engine rate");
        assert!(matches!(err, NamLoadError::SampleRateMismatch { .. }));
    }

    fn silence_tail_is_finite(dsp: &mut Dsp) {
        for _ in 0..8_000 {
            let (l, r) = dsp.process_stereo(0.0, 0.0);
            assert!(l.is_finite() && r.is_finite());
        }
    }

    #[test]
    fn full_chain_stays_finite_and_bounded() {
        let mut dsp = Dsp::new(48_000.0);
        let mut worst = 0.0f32;
        for n in 0..48_000 {
            // A gnarly test signal: decaying pluck + noise-ish alternation.
            let t = n as f32 / 48_000.0;
            let x = (t * 220.0 * std::f32::consts::TAU).sin() * (-t * 3.0).exp()
                + if n % 2 == 0 { 0.05 } else { -0.05 };
            let (l, r) = dsp.process_stereo(x, x);
            assert!(l.is_finite() && r.is_finite());
            worst = worst.max(l.abs()).max(r.abs());
        }
        // The chain must not explode (reverb + delay feedback stay stable).
        assert!(worst < 8.0, "output blew up: peak={worst}");
    }

    #[test]
    fn power_off_is_bit_transparent() {
        let mut dsp = Dsp::new(48_000.0);
        let mut p = default_params();
        p.power = false;
        dsp.set_params(p);
        for n in 0..1_000 {
            let x = (n as f32 * 0.01).sin();
            assert_eq!(dsp.process_stereo(x, -x), (x, -x));
        }
    }

    #[test]
    fn apply_ui_param_covers_data_ts_ids() {
        let mut dsp = Dsp::new(48_000.0);
        // Every id present in editorui/src/data.ts must be routable.
        for id in [
            "gate_thresh",
            "drive_gain",
            "drive_tone",
            "drive_level",
            "amp_gain",
            "amp_bass",
            "amp_middle",
            "amp_treble",
            "amp_presence",
            "amp_master",
            "chorus_rate",
            "chorus_depth",
            "chorus_mix",
            "delay_time",
            "delay_fb",
            "delay_mix",
            "reverb_decay",
            "reverb_mix",
            "cab_mic",
            "cab_dist",
            // Global gain staging (editorui/src/globals.ts).
            "power",
            "input_trim",
            "output_trim",
        ] {
            assert!(dsp.apply_ui_param(id, 5.0), "id `{id}` was not routed");
        }
        assert!(!dsp.apply_ui_param("does_not_exist", 1.0));
    }

    #[test]
    fn model_selection_switches_voicing() {
        let mut dsp = Dsp::new(48_000.0);
        assert!(dsp.apply_ui_param("amp_model", 1.0));
        assert_eq!(dsp.params().amp_model, AmpModel::Plexi);
        assert!(dsp.apply_ui_param("drive_model", 1.0));
        assert_eq!(dsp.params().drive_model, DriveModel::Minotaur);
        assert!(dsp.select_model("amp", "recto"));
        assert_eq!(dsp.params().amp_model, AmpModel::Recto);
        assert!(dsp.select_model("drive", "fuzz"));
        assert_eq!(dsp.params().drive_model, DriveModel::Fuzz);
        assert!(dsp.apply_ui_param("cab_model", 1.0));
        assert_eq!(dsp.params().cab_model, CabModel::American2x12);
        assert!(dsp.select_model("cab", "modern_412"));
        assert_eq!(dsp.params().cab_model, CabModel::Modern4x12);
        assert!(!dsp.select_model("cab", "not_a_cab"));
    }

    #[test]
    fn path_order_reorders_processing_slots() {
        let mut dsp = Dsp::new(48_000.0);
        assert!(dsp.apply_ui_param("path_slot_0", 2.0)); // Amp first
        assert!(dsp.apply_ui_param("path_slot_1", 0.0)); // Gate
        assert!(dsp.apply_ui_param("path_slot_2", -1.0)); // clear
        // Remaining slots still have defaults until overwritten — sanitize packs.
        dsp.set_path_order([
            Some(StageKind::Amp),
            Some(StageKind::Cab),
            None,
            None,
            None,
            None,
            None,
        ]);
        assert_eq!(
            dsp.params().stage_order,
            [
                Some(StageKind::Amp),
                Some(StageKind::Cab),
                None,
                None,
                None,
                None,
                None
            ]
        );
        let (l, r) = dsp.process_stereo(0.2, -0.2);
        assert!(l.is_finite() && r.is_finite());
    }

    #[test]
    fn reverb_tail_decays_to_silence() {
        let mut dsp = Dsp::new(48_000.0);
        // Isolate the reverb so we can assert the tail dies out.
        let mut p = default_params();
        p.gate_on = false;
        p.drive_on = false;
        p.amp_on = false;
        p.mod_on = false;
        p.delay_on = false;
        p.cab_on = false;
        p.reverb_decay_s = 2.0;
        p.reverb_mix = 100.0;
        dsp.set_params(p);
        // Excite, then run a long silent tail.
        for _ in 0..64 {
            let _ = dsp.process_stereo(0.5, 0.5);
        }
        silence_tail_is_finite(&mut dsp);
        let mut tail = 0.0f32;
        for _ in 0..2_000 {
            let (l, r) = dsp.process_stereo(0.0, 0.0);
            tail = tail.max(l.abs()).max(r.abs());
        }
        assert!(tail < 0.2, "reverb tail did not decay: {tail}");
    }

    /// A bare chain (every stage out of the path) must pass a signal through at
    /// exactly the combined trim gain, so the trims are usable for gain staging
    /// rather than being an approximate "level" control.
    fn bare_dsp() -> Dsp {
        let mut dsp = Dsp::new(48_000.0);
        let mut p = default_params();
        p.stage_order = [None; 7];
        dsp.set_params(p);
        dsp
    }

    #[test]
    fn input_trim_scales_the_signal_entering_the_chain() {
        let mut dsp = bare_dsp();
        assert!(dsp.apply_ui_param("input_trim", 6.0));
        let (l, _) = dsp.process_stereo(0.1, 0.1);
        // +6 dB ≈ ×1.9953.
        assert!((l - 0.199_53).abs() < 1.0e-3, "input trim not applied: {l}");
    }

    #[test]
    fn output_trim_scales_the_signal_leaving_the_chain() {
        let mut dsp = bare_dsp();
        assert!(dsp.apply_ui_param("output_trim", -6.0));
        let (l, _) = dsp.process_stereo(0.4, 0.4);
        assert!((l - 0.200_47).abs() < 1.0e-3, "output trim not applied: {l}");
    }

    #[test]
    fn trims_are_clamped_to_their_declared_range() {
        let mut dsp = bare_dsp();
        dsp.apply_ui_param("input_trim", 999.0);
        dsp.apply_ui_param("output_trim", -999.0);
        assert_eq!(dsp.params().input_trim_db, 24.0);
        assert_eq!(dsp.params().output_trim_db, -24.0);
    }

    #[test]
    fn global_bypass_is_a_true_passthrough_ignoring_trims() {
        let mut dsp = bare_dsp();
        dsp.apply_ui_param("input_trim", 12.0);
        dsp.apply_ui_param("power", 0.0);
        let (l, r) = dsp.process_stereo(0.25, -0.25);
        assert_eq!((l, r), (0.25, -0.25));
    }

    #[test]
    fn input_meter_reports_the_post_trim_level() {
        let mut dsp = bare_dsp();
        dsp.apply_ui_param("input_trim", 6.0);
        for _ in 0..64 {
            let _ = dsp.process_stereo(0.5, 0.5);
        }
        let frame = dsp.meter_frame();
        assert!(
            frame.in_peak > 0.9 && frame.in_peak < 1.05,
            "input meter should track post-trim level, got {}",
            frame.in_peak
        );
    }

    #[test]
    fn rms_of_a_dc_level_converges_to_that_level() {
        let mut dsp = bare_dsp();
        // 300 ms window at 48 kHz — run well past it.
        for _ in 0..48_000 {
            let _ = dsp.process_stereo(0.5, 0.5);
        }
        let frame = dsp.meter_frame();
        assert!(
            (frame.in_rms - 0.5).abs() < 0.01,
            "input rms should converge to 0.5, got {}",
            frame.in_rms
        );
        assert!(frame.in_rms <= frame.in_peak + 1.0e-4);
    }

    #[test]
    fn clip_flags_are_sticky_until_cleared() {
        let mut dsp = bare_dsp();
        let _ = dsp.process_stereo(0.2, 0.2);
        assert!(!dsp.meter_frame().in_clip);

        let _ = dsp.process_stereo(1.5, 1.5);
        // Still set several quiet samples later.
        for _ in 0..512 {
            let _ = dsp.process_stereo(0.01, 0.01);
        }
        assert!(dsp.meter_frame().in_clip, "clip flag should latch");
        assert!(dsp.meter_frame().out_clip);

        dsp.clear_clip();
        assert!(!dsp.meter_frame().in_clip);
        assert!(!dsp.meter_frame().out_clip);
    }
}
