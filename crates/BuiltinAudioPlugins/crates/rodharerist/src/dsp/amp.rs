//! Guitar amplifier — preamp tube stages, passive-style tonestack, presence and
//! master. Mandarin 80 (warm, mid-forward) vs Brit Plexi 100 (bright, open).

use builtin_dsp_core::make_eq_biquad;

use super::{AmpModel, StereoBiquad, tube_stage};

#[derive(Debug, Clone)]
pub(super) struct Amp {
    sample_rate: f32,
    model: AmpModel,
    pre_gain: f32,
    stage_drive: f32,
    master: f32,
    bass: StereoBiquad,
    mid: StereoBiquad,
    treble: StereoBiquad,
    presence: StereoBiquad,
}

impl Amp {
    pub(super) fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            model: AmpModel::Mandarin,
            pre_gain: 1.0,
            stage_drive: 1.0,
            master: 0.5,
            bass: StereoBiquad::none(),
            mid: StereoBiquad::none(),
            treble: StereoBiquad::none(),
            presence: StereoBiquad::none(),
        }
    }

    pub(super) fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
    }

    pub(super) fn reset(&mut self) {
        self.bass.reset();
        self.mid.reset();
        self.treble.reset();
        self.presence.reset();
    }

    /// All tone knobs are 0..10 from the editor.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn configure(
        &mut self,
        model: AmpModel,
        gain: f32,
        bass: f32,
        middle: f32,
        treble: f32,
        presence: f32,
        master: f32,
    ) {
        self.model = model;
        let g = (gain / 10.0).clamp(0.0, 1.0);
        let sr = self.sample_rate;

        // Voicing constants per amp: (pre_scale, stage_drive, mid_hz, treble_hz).
        let (pre_scale, stage_drive, mid_hz, treble_hz) = match model {
            AmpModel::Mandarin => (30.0, 2.2, 620.0, 3_000.0),
            AmpModel::Plexi => (20.0, 1.5, 720.0, 3_400.0),
            AmpModel::Twin => (8.0, 0.9, 800.0, 4_200.0),
            AmpModel::TopBoost => (14.0, 1.3, 900.0, 4_800.0),
            AmpModel::Recto => (42.0, 3.0, 550.0, 2_800.0),
            AmpModel::Jcm => (26.0, 2.0, 680.0, 3_200.0),
            AmpModel::Slate => (48.0, 3.4, 500.0, 2_600.0),
            AmpModel::Bassman => (18.0, 1.4, 480.0, 2_900.0),
        };
        self.pre_gain = 1.0 + g * pre_scale;
        self.stage_drive = stage_drive;
        self.master = (master / 10.0).clamp(0.0, 1.0) * 1.2;

        // Passive-style tonestack: ±dB around each knob's centre (5.0).
        let bass_db = (bass - 5.0) / 5.0 * 12.0;
        let mid_db = (middle - 5.0) / 5.0 * 10.0;
        let treble_db = (treble - 5.0) / 5.0 * 12.0;
        let presence_db = (presence - 5.0) / 5.0 * 8.0;

        self.bass
            .set(make_eq_biquad("lowshelf", 110.0, bass_db, 0.707, sr));
        self.mid
            .set(make_eq_biquad("bell", mid_hz, mid_db, 0.8, sr));
        self.treble
            .set(make_eq_biquad("highshelf", treble_hz, treble_db, 0.707, sr));
        self.presence
            .set(make_eq_biquad("highshelf", 5_200.0, presence_db, 0.707, sr));
    }

    #[inline]
    fn preamp(&self, x: f32) -> f32 {
        // Two cascaded tube stages with a small asymmetric bias for even harmonics.
        let s1 = tube_stage(x * self.pre_gain, 0.15, self.stage_drive);
        tube_stage(s1, 0.08, self.stage_drive * 0.8)
    }

    #[inline]
    pub(super) fn process(&mut self, left: f32, right: f32) -> (f32, f32) {
        let mut l = self.preamp(left);
        let mut r = self.preamp(right);
        (l, r) = self.bass.run(l, r);
        (l, r) = self.mid.run(l, r);
        (l, r) = self.treble.run(l, r);
        (l, r) = self.presence.run(l, r);
        (l * self.master, r * self.master)
    }
}
