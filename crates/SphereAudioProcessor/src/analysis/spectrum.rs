//! Shared STFT helpers for offline analysis.
//!
//! Offline / control-thread only. Allocates freely and runs FFTs over a whole
//! buffer. Never call from a realtime audio callback.

use std::f32::consts::PI;

use rustfft::{FftPlanner, num_complex::Complex};

/// Periodic Hann window (good for overlap-add STFT analysis).
pub fn hann_window(size: usize) -> Vec<f32> {
    if size <= 1 {
        return vec![1.0; size];
    }
    (0..size)
        .map(|n| {
            let s = (PI * n as f32 / size as f32).sin();
            s * s
        })
        .collect()
}

/// Magnitude spectra for successive windowed frames.
///
/// Returns one `Vec<f32>` of length `size / 2` per frame (positive
/// frequencies, DC included). Empty if the signal is shorter than one frame.
pub fn magnitude_frames(samples: &[f32], size: usize, hop: usize) -> Vec<Vec<f32>> {
    debug_assert!(size > 0 && hop > 0);
    if samples.len() < size || size == 0 || hop == 0 {
        return Vec::new();
    }

    let window = hann_window(size);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);

    let half = size / 2;
    let mut buf = vec![Complex::new(0.0_f32, 0.0_f32); size];
    let mut scratch = vec![Complex::new(0.0_f32, 0.0_f32); fft.get_inplace_scratch_len()];
    let mut frames = Vec::new();

    let mut pos = 0;
    while pos + size <= samples.len() {
        for i in 0..size {
            buf[i] = Complex::new(samples[pos + i] * window[i], 0.0);
        }
        fft.process_with_scratch(&mut buf, &mut scratch);
        frames.push(buf[..half].iter().map(|c| c.norm()).collect());
        pos += hop;
    }

    frames
}

/// Frequency (Hz) of FFT bin `bin` for the given frame size and sample rate.
#[inline]
pub fn bin_frequency(bin: usize, size: usize, sample_rate: f32) -> f32 {
    bin as f32 * sample_rate / size as f32
}
