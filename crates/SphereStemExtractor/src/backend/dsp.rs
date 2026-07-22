//! Shared offline DSP helpers for the ONNX stem backends.
//!
//! Classification: scanner/offline path (worker thread only). These allocate
//! freely and must never be called from the realtime audio callback.

use crate::error::StemExtractError;

/// Planar stereo buffer (`[left, right]`) used by the ONNX backends.
pub(super) type PlanarStereo = [Vec<f32>; 2];

pub(super) fn backend_err(msg: impl Into<String>) -> StemExtractError {
    StemExtractError::Backend(msg.into())
}

/// Split interleaved PCM into planar stereo, duplicating mono into both sides.
pub(super) fn deinterleave_stereo(
    interleaved: &[f32],
    channels: usize,
    frames: usize,
) -> PlanarStereo {
    let mut left = vec![0.0f32; frames];
    let mut right = vec![0.0f32; frames];
    for f in 0..frames {
        let l = interleaved[f * channels];
        left[f] = l;
        right[f] = if channels > 1 {
            interleaved[f * channels + 1]
        } else {
            l
        };
    }
    [left, right]
}

/// Re-interleave a planar-stereo buffer to `channels`, folding to mono when
/// `channels == 1`, clamped to `frames`.
pub(super) fn interleave_stereo(planar: &PlanarStereo, channels: usize, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * channels];
    for f in 0..frames {
        let l = planar[0].get(f).copied().unwrap_or(0.0);
        let r = planar[1].get(f).copied().unwrap_or(0.0);
        if channels == 1 {
            out[f] = 0.5 * (l + r);
        } else {
            out[f * channels] = l;
            out[f * channels + 1] = r;
        }
    }
    out
}

/// Resample a planar-stereo buffer from `from` to `to` Hz with a sinc
/// resampler. A no-op when the rates already match.
pub(super) fn resample_planar(
    input: &PlanarStereo,
    from: u32,
    to: u32,
) -> Result<PlanarStereo, StemExtractError> {
    if from == to || input[0].is_empty() {
        return Ok(input.clone());
    }
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Linear,
        window: WindowFunction::BlackmanHarris2,
    };
    let ratio = to as f64 / from as f64;
    let chunk = 1024usize;
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, chunk, 2)
        .map_err(|e| backend_err(format!("resampler init failed: {e}")))?;

    let total = input[0].len();
    let mut out: PlanarStereo = [Vec::new(), Vec::new()];
    let mut pos = 0usize;
    loop {
        let need = resampler.input_frames_next();
        if pos + need > total {
            break;
        }
        let frame: Vec<&[f32]> = vec![&input[0][pos..pos + need], &input[1][pos..pos + need]];
        let processed = resampler
            .process(&frame, None)
            .map_err(|e| backend_err(format!("resample failed: {e}")))?;
        out[0].extend_from_slice(&processed[0]);
        out[1].extend_from_slice(&processed[1]);
        pos += need;
    }
    if pos < total {
        let tail: Vec<Vec<f32>> = vec![input[0][pos..].to_vec(), input[1][pos..].to_vec()];
        let processed = resampler
            .process_partial(Some(&tail), None)
            .map_err(|e| backend_err(format!("resample tail failed: {e}")))?;
        out[0].extend_from_slice(&processed[0]);
        out[1].extend_from_slice(&processed[1]);
    }

    // Normalize to the exact expected length so downstream length math is stable.
    let expected = ((total as f64) * ratio).round() as usize;
    for ch in out.iter_mut() {
        ch.resize(expected, 0.0);
    }
    Ok(out)
}
