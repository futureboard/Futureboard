//! Spectral / temporal feature extraction used by instrument classification.
//!
//! Offline / control-thread only.

use serde::{Deserialize, Serialize};

use super::spectrum::{bin_frequency, magnitude_frames};

/// Compact timbre descriptor for a mono buffer. All ratios are in `[0, 1]`
/// unless noted; frequencies are in Hz.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpectralFeatures {
    /// Brightness: energy-weighted mean frequency (Hz).
    pub centroid_hz: f32,
    /// Frequency below which 85% of spectral energy lies (Hz).
    pub rolloff_hz: f32,
    /// Spectral flatness (geometric/arithmetic mean of magnitudes); high = noisy.
    pub flatness: f32,
    /// Zero-crossing rate of the time-domain signal.
    pub zero_crossing_rate: f32,
    /// Fraction of spectral energy below 250 Hz.
    pub low_energy_ratio: f32,
    /// Fraction of spectral energy above 4 kHz.
    pub high_energy_ratio: f32,
    /// Onset density proxy: normalised variance of frame-to-frame flux.
    pub percussiveness: f32,
    /// RMS level of the buffer.
    pub rms: f32,
}

/// Number of scalar features in the [`SpectralFeatures::to_feature_vector`]
/// contract. A learned classifier consuming these features must accept an
/// input of shape `[1, FEATURE_VECTOR_LEN]`.
pub const FEATURE_VECTOR_LEN: usize = 8;

impl SpectralFeatures {
    /// Fixed-order feature vector for learned classifier backends.
    ///
    /// The centroid and rolloff frequencies are log-scaled into a roughly
    /// `[0, 1]` range so all features share a comparable magnitude; the rest are
    /// already normalised ratios. The order is stable and part of the model
    /// input contract — do not reorder without versioning the model.
    pub fn to_feature_vector(&self) -> [f32; FEATURE_VECTOR_LEN] {
        [
            log_hz_norm(self.centroid_hz),
            log_hz_norm(self.rolloff_hz),
            self.flatness,
            self.zero_crossing_rate,
            self.low_energy_ratio,
            self.high_energy_ratio,
            self.percussiveness,
            self.rms.clamp(0.0, 1.0),
        ]
    }
}

/// Map a frequency to roughly `[0, 1]` via log scaling (20 Hz..20 kHz).
fn log_hz_norm(hz: f32) -> f32 {
    if hz <= 20.0 {
        return 0.0;
    }
    let lo = 20.0_f32.ln();
    let hi = 20_000.0_f32.ln();
    ((hz.ln() - lo) / (hi - lo)).clamp(0.0, 1.0)
}

const FRAME_SIZE: usize = 2048;
const HOP: usize = 1024;

/// Extract features from a mono buffer. Returns `None` for signals shorter than
/// a single analysis frame.
pub fn extract(samples: &[f32], sample_rate: f32) -> Option<SpectralFeatures> {
    if sample_rate <= 0.0 || !sample_rate.is_finite() {
        return None;
    }

    let frames = magnitude_frames(samples, FRAME_SIZE, HOP);
    if frames.is_empty() {
        return None;
    }

    // Average magnitude spectrum across frames.
    let bins = frames[0].len();
    let mut avg = vec![0.0_f32; bins];
    for frame in &frames {
        for (a, &m) in avg.iter_mut().zip(frame.iter()) {
            *a += m;
        }
    }
    let inv = 1.0 / frames.len() as f32;
    for a in &mut avg {
        *a *= inv;
    }

    let total: f32 = avg.iter().sum();
    if total <= f32::EPSILON {
        return None;
    }

    // Centroid + energy bands.
    let mut centroid_num = 0.0_f32;
    let mut low = 0.0_f32;
    let mut high = 0.0_f32;
    for (bin, &mag) in avg.iter().enumerate() {
        let freq = bin_frequency(bin, FRAME_SIZE, sample_rate);
        centroid_num += freq * mag;
        if freq < 250.0 {
            low += mag;
        }
        if freq > 4000.0 {
            high += mag;
        }
    }
    let centroid_hz = centroid_num / total;

    // Rolloff at 85% of cumulative energy.
    let threshold = total * 0.85;
    let mut cum = 0.0_f32;
    let mut rolloff_hz = 0.0_f32;
    for (bin, &mag) in avg.iter().enumerate() {
        cum += mag;
        if cum >= threshold {
            rolloff_hz = bin_frequency(bin, FRAME_SIZE, sample_rate);
            break;
        }
    }

    // Spectral flatness = geometric mean / arithmetic mean.
    let mut log_sum = 0.0_f32;
    let mut count = 0.0_f32;
    for &mag in avg.iter().skip(1) {
        log_sum += (mag + 1e-9).ln();
        count += 1.0;
    }
    let arith = total / avg.len() as f32;
    let geo = (log_sum / count).exp();
    let flatness = if arith > f32::EPSILON { (geo / arith).clamp(0.0, 1.0) } else { 0.0 };

    // Zero-crossing rate over the time-domain signal.
    let mut crossings = 0.0_f32;
    for w in samples.windows(2) {
        if (w[0] >= 0.0) != (w[1] >= 0.0) {
            crossings += 1.0;
        }
    }
    let zero_crossing_rate = if samples.len() > 1 {
        crossings / (samples.len() - 1) as f32
    } else {
        0.0
    };

    // Percussiveness: normalised variance of positive spectral flux per frame.
    let mut flux_series = Vec::with_capacity(frames.len().saturating_sub(1));
    for pair in frames.windows(2) {
        let (prev, cur) = (&pair[0], &pair[1]);
        let mut sum = 0.0_f32;
        for (a, b) in cur.iter().zip(prev.iter()) {
            let d = a - b;
            if d > 0.0 {
                sum += d;
            }
        }
        flux_series.push(sum);
    }
    let percussiveness = normalised_variance(&flux_series);

    let rms = if samples.is_empty() {
        0.0
    } else {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    };

    Some(SpectralFeatures {
        centroid_hz,
        rolloff_hz,
        flatness,
        zero_crossing_rate,
        low_energy_ratio: (low / total).clamp(0.0, 1.0),
        high_energy_ratio: (high / total).clamp(0.0, 1.0),
        percussiveness,
        rms,
    })
}

fn normalised_variance(series: &[f32]) -> f32 {
    if series.len() < 2 {
        return 0.0;
    }
    let mean = series.iter().sum::<f32>() / series.len() as f32;
    if mean <= f32::EPSILON {
        return 0.0;
    }
    let var = series.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / series.len() as f32;
    // Coefficient of variation, squashed into [0, 1].
    let cv = var.sqrt() / mean;
    (cv / (1.0 + cv)).clamp(0.0, 1.0)
}
