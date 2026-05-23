/// Apply a linear gain to a sample buffer in-place.
#[inline]
pub fn apply_gain(buffer: &mut [f32], gain: f32) {
    for s in buffer.iter_mut() {
        *s *= gain;
    }
}

/// Convert dBFS to linear amplitude.  Clamps to 0.0 for very negative dB.
#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    if db <= -120.0 {
        0.0
    } else {
        10.0f32.powf(db / 20.0)
    }
}

/// Convert linear amplitude to dBFS.
#[inline]
pub fn linear_to_db(linear: f32) -> f32 {
    if linear <= 1e-6 {
        -120.0
    } else {
        20.0 * linear.log10()
    }
}

/// Constant-power stereo pan.
///
/// `pan`: -1.0 = full left, 0.0 = center, 1.0 = full right.
/// Returns (left_gain, right_gain), both in [0..1].
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4; // 0..π/2
    let l = angle.cos();
    let r = angle.sin();
    (l, r)
}
