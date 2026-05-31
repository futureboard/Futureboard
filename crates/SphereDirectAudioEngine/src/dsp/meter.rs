/// Per-block peak and RMS computation — allocation-free, no division by zero.
/// Returns `(peak, rms)` for a mono slice of samples.
#[allow(dead_code)]
#[inline]
pub fn compute_peak_rms(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let mut peak = 0.0f32;
    let mut sq_sum = 0.0f32;
    for &s in samples {
        let abs = s.abs();
        if abs > peak {
            peak = abs;
        }
        sq_sum += s * s;
    }
    let rms = (sq_sum / samples.len() as f32).sqrt();
    (peak, rms)
}

/// Compute peak and RMS for one channel extracted from an interleaved buffer.
///
/// `channel` is zero-based.  Skips bounds-checked access with stride iteration.
#[allow(dead_code)]
#[inline]
pub fn channel_peak_rms(interleaved: &[f32], channel: usize, num_channels: usize) -> (f32, f32) {
    if interleaved.is_empty() || num_channels == 0 || channel >= num_channels {
        return (0.0, 0.0);
    }
    let mut peak = 0.0f32;
    let mut sq_sum = 0.0f32;
    let mut count = 0usize;
    let mut i = channel;
    while i < interleaved.len() {
        let abs = interleaved[i].abs();
        if abs > peak {
            peak = abs;
        }
        sq_sum += interleaved[i] * interleaved[i];
        count += 1;
        i += num_channels;
    }
    let rms = if count > 0 {
        (sq_sum / count as f32).sqrt()
    } else {
        0.0
    };
    (peak, rms)
}

/// Smooth a peak value with exponential decay toward a new measurement.
/// `decay`: values close to 1.0 = slow decay (e.g. 0.998 per block).
#[inline]
pub fn smooth_peak(previous: f32, current_peak: f32, decay: f32) -> f32 {
    (previous * decay).max(current_peak)
}
