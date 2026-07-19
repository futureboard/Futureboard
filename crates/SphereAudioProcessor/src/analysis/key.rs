//! Musical key estimation via chromagram + Krumhansl-Schmuckler key profiles.
//!
//! Offline / control-thread only.

use serde::{Deserialize, Serialize};

use super::spectrum::{bin_frequency, magnitude_frames};

/// Pitch class (0 = C, 1 = C#/Db, ... 11 = B).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PitchClass {
    C,
    Cs,
    D,
    Ds,
    E,
    F,
    Fs,
    G,
    Gs,
    A,
    As,
    B,
}

impl PitchClass {
    pub fn from_index(index: i32) -> Self {
        match index.rem_euclid(12) {
            0 => Self::C,
            1 => Self::Cs,
            2 => Self::D,
            3 => Self::Ds,
            4 => Self::E,
            5 => Self::F,
            6 => Self::Fs,
            7 => Self::G,
            8 => Self::Gs,
            9 => Self::A,
            10 => Self::As,
            _ => Self::B,
        }
    }

    /// Sharp-spelled name, e.g. `"C#"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::Cs => "C#",
            Self::D => "D",
            Self::Ds => "D#",
            Self::E => "E",
            Self::F => "F",
            Self::Fs => "F#",
            Self::G => "G",
            Self::Gs => "G#",
            Self::A => "A",
            Self::As => "A#",
            Self::B => "B",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyMode {
    Major,
    Minor,
}

impl KeyMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Major => "maj",
            Self::Minor => "min",
        }
    }
}

/// Estimated musical key.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyEstimate {
    pub tonic: PitchClass,
    pub mode: KeyMode,
    /// Correlation of the best key vs. the next best, in `[0, 1]`.
    pub confidence: f32,
}

impl KeyEstimate {
    /// Compact label, e.g. `"A min"`.
    pub fn label(&self) -> String {
        format!("{} {}", self.tonic.name(), self.mode.name())
    }
}

// Krumhansl-Kessler tonal hierarchy profiles (major and minor).
const MAJOR_PROFILE: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

const FRAME_SIZE: usize = 4096;
const HOP: usize = 2048;
const MIN_HZ: f32 = 55.0; // ~A1
const MAX_HZ: f32 = 5000.0;

/// Estimate the musical key of a mono buffer. Returns `None` if the signal is
/// too short or carries no pitched energy.
pub fn estimate_key(samples: &[f32], sample_rate: f32) -> Option<KeyEstimate> {
    if sample_rate <= 0.0 || !sample_rate.is_finite() {
        return None;
    }

    let frames = magnitude_frames(samples, FRAME_SIZE, HOP);
    if frames.is_empty() {
        return None;
    }

    let mut chroma = [0.0_f32; 12];
    for frame in &frames {
        for (bin, &mag) in frame.iter().enumerate().skip(1) {
            let freq = bin_frequency(bin, FRAME_SIZE, sample_rate);
            if freq < MIN_HZ || freq > MAX_HZ {
                continue;
            }
            let midi = 69.0 + 12.0 * (freq / 440.0).log2();
            let pc = (midi.round() as i32).rem_euclid(12) as usize;
            chroma[pc] += mag;
        }
    }

    let total: f32 = chroma.iter().sum();
    if total <= f32::EPSILON {
        return None;
    }
    for c in &mut chroma {
        *c /= total;
    }

    // Correlate chroma against all 24 rotated major/minor profiles.
    let mut best = (0.0_f32, PitchClass::C, KeyMode::Major);
    let mut second_best = f32::NEG_INFINITY;
    for tonic in 0..12 {
        for (mode, profile) in [
            (KeyMode::Major, &MAJOR_PROFILE),
            (KeyMode::Minor, &MINOR_PROFILE),
        ] {
            let score = correlation(&chroma, profile, tonic);
            if score > best.0 {
                second_best = best.0;
                best = (score, PitchClass::from_index(tonic as i32), mode);
            } else if score > second_best {
                second_best = score;
            }
        }
    }

    let confidence = if best.0 > 0.0 && second_best.is_finite() {
        ((best.0 - second_best) / best.0).clamp(0.0, 1.0)
    } else {
        0.0
    };

    Some(KeyEstimate {
        tonic: best.1,
        mode: best.2,
        confidence,
    })
}

/// Pearson correlation between the chroma vector and a profile rotated so that
/// `tonic` aligns with the profile's tonic (index 0).
fn correlation(chroma: &[f32; 12], profile: &[f32; 12], tonic: usize) -> f32 {
    let mut rotated = [0.0_f32; 12];
    for i in 0..12 {
        rotated[i] = profile[(i + 12 - tonic) % 12];
    }

    let mean_c = chroma.iter().sum::<f32>() / 12.0;
    let mean_p = rotated.iter().sum::<f32>() / 12.0;

    let mut num = 0.0_f32;
    let mut den_c = 0.0_f32;
    let mut den_p = 0.0_f32;
    for i in 0..12 {
        let dc = chroma[i] - mean_c;
        let dp = rotated[i] - mean_p;
        num += dc * dp;
        den_c += dc * dc;
        den_p += dp * dp;
    }

    let den = (den_c * den_p).sqrt();
    if den <= f32::EPSILON { 0.0 } else { num / den }
}
