//! Similarity-aligned granular time stretcher.
//!
//! stretch_ratio = output_duration / input_duration
//!   2.0 -> output is twice as long.
//!   0.5 -> output is half as long.

use super::resample::resample_linear;
use std::f32::consts::PI;

const CORR_LEN: usize = 128;

pub fn time_stretch_granular(input: &[f32], stretch_ratio: f32, grain_size: usize) -> Vec<f32> {
    let ratio = stretch_ratio.clamp(0.25, 4.0);
    if input.is_empty() {
        return Vec::new();
    }

    let grain_size = grain_size.clamp(64, 16384);
    if input.len() < grain_size {
        return resample_linear(input, 1.0 / ratio);
    }

    let hop_in = (grain_size / 4).max(1);
    let hop_out = ((hop_in as f32) * ratio).round().max(1.0) as usize;
    let out_len = ((input.len() as f32 * ratio).ceil() as usize).max(1);
    let search_range = hop_in.min(grain_size / 8).max(1);
    let search_step = (search_range / 8).max(1);
    let corr_len = CORR_LEN.min(grain_size / 4).max(1);
    let ref_offset = hop_in.min(grain_size.saturating_sub(corr_len));

    let mut output = vec![0.0_f32; out_len];
    let mut window_sum = vec![0.0_f32; out_len];
    let win = hann_window(grain_size);

    let mut expected_in_pos = 0_usize;
    let mut out_pos = 0_usize;
    let mut prev_pos: Option<usize> = None;

    while out_pos < out_len {
        let max_pos = input.len() - grain_size;
        let mut best_pos = expected_in_pos.min(max_pos);

        if let Some(prev) = prev_pos {
            let mut best_score = f32::NEG_INFINITY;
            let lo = expected_in_pos.saturating_sub(search_range);
            let hi = (expected_in_pos + search_range).min(max_pos);
            let mut pos = lo;

            while pos <= hi {
                let score = normalized_xcorr(input, pos, prev + ref_offset, corr_len);
                if score > best_score {
                    best_score = score;
                    best_pos = pos;
                }
                pos = pos.saturating_add(search_step);
            }
        }

        let copy_len = grain_size.min(out_len - out_pos);
        for i in 0..copy_len {
            let w = win[i];
            output[out_pos + i] += input[best_pos + i] * w;
            window_sum[out_pos + i] += w;
        }

        prev_pos = Some(best_pos);
        expected_in_pos = expected_in_pos.saturating_add(hop_in);
        out_pos = out_pos.saturating_add(hop_out);
    }

    for i in 0..out_len {
        if window_sum[i] > 1e-6 {
            output[i] /= window_sum[i];
        } else {
            let src_pos = ((i as f32 / ratio).floor() as usize).min(input.len() - 1);
            output[i] = input[src_pos];
        }
    }

    output
}

fn normalized_xcorr(signal: &[f32], pos: usize, ref_pos: usize, len: usize) -> f32 {
    let n = len
        .min(signal.len().saturating_sub(pos))
        .min(signal.len().saturating_sub(ref_pos));
    if n == 0 {
        return 0.0;
    }

    let mut sum = 0.0_f32;
    let mut e_sig = 0.0_f32;
    let mut e_ref = 0.0_f32;

    for i in 0..n {
        let s = signal[pos + i];
        let r = signal[ref_pos + i];
        sum += s * r;
        e_sig += s * s;
        e_ref += r * r;
    }

    let denom = (e_sig * e_ref).sqrt();
    if denom > 1e-8 { sum / denom } else { 0.0 }
}

fn hann_window(size: usize) -> Vec<f32> {
    let n1 = (size - 1) as f32;
    (0..size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / n1).cos()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_safe() {
        assert!(time_stretch_granular(&[], 2.0, 2048).is_empty());
    }

    #[test]
    fn stretch_2_roughly_doubles_length() {
        let input: Vec<f32> = (0..4096).map(|i| (i as f32 * 0.01).sin()).collect();
        let out = time_stretch_granular(&input, 2.0, 512);
        let expected = (input.len() as f32 * 2.0) as usize;
        let tolerance = (expected as f32 * 0.1) as usize;
        assert!(
            out.len().abs_diff(expected) <= tolerance,
            "expected ~{expected}, got {}",
            out.len()
        );
    }

    #[test]
    fn no_nan_or_inf() {
        let input: Vec<f32> = (0..2048).map(|i| (i as f32).sin()).collect();
        for val in time_stretch_granular(&input, 1.5, 512) {
            assert!(val.is_finite(), "output contains non-finite value");
        }
    }

    #[test]
    fn does_not_leave_silent_tail_on_constant_input() {
        let input = vec![0.5_f32; 4096];
        let out = time_stretch_granular(&input, 1.8, 512);
        assert!(out.iter().rev().take(64).all(|v| v.abs() > 0.1));
    }
}
