//! Instrument / voice classification used to suggest a track name.
//!
//! Offline / control-thread only.
//!
//! The default [`HeuristicClassifier`] is a transparent signal-feature
//! classifier — *not* a neural network. It maps [`SpectralFeatures`] to a
//! coarse instrument family using audibly-motivated rules. The [`Classifier`]
//! trait is the extension point for a learned/ONNX model backend (a future
//! slice); such a backend can be dropped in without touching callers.

use serde::{Deserialize, Serialize};

use super::features::SpectralFeatures;

/// Coarse instrument / voice family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentCategory {
    Vocal,
    Bass,
    Drums,
    Keys,
    Guitar,
    Strings,
    Synth,
    Other,
}

impl InstrumentCategory {
    /// All categories in their canonical index order. This order is the
    /// learned-model output contract and matches the FFI integer codes.
    pub const ALL: [InstrumentCategory; 8] = [
        Self::Vocal,
        Self::Bass,
        Self::Drums,
        Self::Keys,
        Self::Guitar,
        Self::Strings,
        Self::Synth,
        Self::Other,
    ];

    /// Category for a canonical index (`0 = Vocal .. 7 = Other`). Out-of-range
    /// indices map to [`InstrumentCategory::Other`].
    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Other)
    }

    /// Human-readable track-name label for this category.
    pub fn track_name(self) -> &'static str {
        match self {
            Self::Vocal => "Vocal",
            Self::Bass => "Bass",
            Self::Drums => "Drums",
            Self::Keys => "Keys",
            Self::Guitar => "Guitar",
            Self::Strings => "Strings",
            Self::Synth => "Synth",
            Self::Other => "Audio",
        }
    }
}

/// Classification result with the winning category and its confidence.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstrumentEstimate {
    pub category: InstrumentCategory,
    /// Winning score margin in `[0, 1]`.
    pub confidence: f32,
    pub features: SpectralFeatures,
}

/// Backend that maps spectral features to an instrument family.
///
/// Implement this to plug in a learned model; the default heuristic backend is
/// always available and requires no model files.
pub trait Classifier {
    fn classify(&self, features: &SpectralFeatures) -> InstrumentEstimate;
}

/// Transparent feature-rule classifier. No model files, deterministic.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeuristicClassifier;

impl Classifier for HeuristicClassifier {
    fn classify(&self, f: &SpectralFeatures) -> InstrumentEstimate {
        // Score each family from audibly-motivated feature evidence, then pick
        // the strongest. Scores are kept in a comparable arbitrary unit range.
        let mut scores = [
            (InstrumentCategory::Drums, 0.0_f32),
            (InstrumentCategory::Bass, 0.0),
            (InstrumentCategory::Vocal, 0.0),
            (InstrumentCategory::Guitar, 0.0),
            (InstrumentCategory::Keys, 0.0),
            (InstrumentCategory::Strings, 0.0),
            (InstrumentCategory::Synth, 0.0),
        ];

        // Drums / percussion: noisy, transient-heavy, high ZCR, broadband.
        scores[0].1 = f.flatness * 2.0
            + f.percussiveness * 2.5
            + f.zero_crossing_rate * 1.5
            + f.high_energy_ratio;

        // Bass: dominant low-band energy, dark centroid, low ZCR, tonal.
        scores[1].1 = f.low_energy_ratio * 3.0
            + darkness(f.centroid_hz, 500.0)
            + (1.0 - f.zero_crossing_rate)
            - f.flatness;

        // Vocal: mid centroid, tonal (low flatness), moderate ZCR, little sub-bass.
        scores[2].1 = mid_band(f.centroid_hz, 900.0, 1200.0) * 2.0
            + (1.0 - f.flatness) * 1.5
            + (1.0 - f.percussiveness)
            + (1.0 - f.low_energy_ratio);

        // Guitar: bright-mid centroid, tonal, some attack.
        scores[3].1 = mid_band(f.centroid_hz, 1500.0, 1500.0) * 1.5
            + (1.0 - f.flatness)
            + f.percussiveness * 0.5;

        // Keys/piano: broad harmonic range, moderate attack, tonal.
        scores[4].1 = mid_band(f.centroid_hz, 1200.0, 1800.0)
            + (1.0 - f.flatness)
            + f.percussiveness * 0.6;

        // Strings: sustained (low percussiveness), tonal, mid-high centroid.
        scores[5].1 = mid_band(f.centroid_hz, 1800.0, 1800.0)
            + (1.0 - f.percussiveness) * 1.5
            + (1.0 - f.flatness);

        // Synth: bright, sustained-to-noisy, wide high-energy content.
        scores[6].1 = f.high_energy_ratio * 1.5
            + brightness(f.centroid_hz, 2500.0)
            + (1.0 - f.percussiveness) * 0.5;

        // Silence / no usable content -> Other.
        if f.rms <= 1e-4 {
            return InstrumentEstimate {
                category: InstrumentCategory::Other,
                confidence: 0.0,
                features: *f,
            };
        }

        let mut best = scores[0];
        let mut second = f32::NEG_INFINITY;
        for &s in &scores[1..] {
            if s.1 > best.1 {
                second = best.1;
                best = s;
            } else if s.1 > second {
                second = s.1;
            }
        }

        let confidence = if best.1 > 0.0 && second.is_finite() {
            ((best.1 - second) / best.1).clamp(0.0, 1.0)
        } else {
            0.0
        };

        InstrumentEstimate {
            category: best.0,
            confidence,
            features: *f,
        }
    }
}

/// 1.0 when `centroid` is far below `pivot`, falling to 0 above it.
fn darkness(centroid: f32, pivot: f32) -> f32 {
    (1.0 - (centroid / pivot)).clamp(0.0, 1.0)
}

/// 1.0 when `centroid` is far above `pivot`, falling to 0 below it.
fn brightness(centroid: f32, pivot: f32) -> f32 {
    ((centroid - pivot) / pivot).clamp(0.0, 1.0)
}

/// Triangular response peaking at `center`, reaching 0 at `center ± width`.
fn mid_band(centroid: f32, center: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    (1.0 - (centroid - center).abs() / width).clamp(0.0, 1.0)
}
