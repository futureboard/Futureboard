//! Linear-interpolation resampler.
//!
//! Convention (matches the TypeScript resample.ts):
//!   speed_ratio 2.0 → output has half the samples → plays twice as fast.
//!   speed_ratio 0.5 → output has double the samples → plays at half speed.
//!
//! out_len = ceil(in_len / speed_ratio)

pub fn resample_linear(input: &[f32], speed_ratio: f32) -> Vec<f32> {
    let ratio = speed_ratio.clamp(0.25, 4.0);
    if input.is_empty() {
        return Vec::new();
    }
    if (ratio - 1.0).abs() < 1e-6 {
        return input.to_vec();
    }

    let out_len = ((input.len() as f32) / ratio).ceil() as usize;
    let out_len = out_len.max(1);
    let mut output = vec![0.0_f32; out_len];
    let last_idx = input.len() - 1;

    for i in 0..out_len {
        let src_pos = i as f32 * ratio;
        let lo = (src_pos.floor() as usize).min(last_idx);
        let hi = (lo + 1).min(last_idx);
        let frac = src_pos - lo as f32;
        output[i] = input[lo] + (input[hi] - input[lo]) * frac;
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_safe() {
        assert!(resample_linear(&[], 2.0).is_empty());
    }

    #[test]
    fn speed_2_halves_length() {
        let input: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let out = resample_linear(&input, 2.0);
        assert_eq!(out.len(), 50);
    }

    #[test]
    fn speed_half_doubles_length() {
        let input: Vec<f32> = (0..100).map(|i| i as f32).collect();
        let out = resample_linear(&input, 0.5);
        assert_eq!(out.len(), 200);
    }

    #[test]
    fn no_nan_or_inf() {
        let input: Vec<f32> = (0..256).map(|i| (i as f32).sin()).collect();
        for val in resample_linear(&input, 1.5) {
            assert!(val.is_finite(), "output contains non-finite value");
        }
    }
}
