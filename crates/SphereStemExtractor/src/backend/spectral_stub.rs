//! Deterministic spectral / mid-side stem split.
//!
//! This is an offline, allocation-friendly approximation used to exercise the
//! Stem Extractor UI and Audio Processor surface until real MDX-NET ONNX
//! weights are wired. It is **not** claimed to be MDX-NET quality.

use super::{InferBackendKind, SeparatedStem, StemInferBackend};
use crate::device::InferDevice;
use crate::error::StemExtractError;
use crate::model::StemModel;
use crate::params::StemExtractParams;
use crate::progress::{StemExtractCancelToken, StemExtractProgress, StemExtractStage};
use crate::stems::StemKind;

pub struct SpectralStubBackend {
    model: StemModel,
    device: InferDevice,
}

impl SpectralStubBackend {
    pub fn new(model: StemModel, device: InferDevice) -> Self {
        Self { model, device }
    }
}

impl StemInferBackend for SpectralStubBackend {
    fn kind(&self) -> InferBackendKind {
        InferBackendKind::SpectralStub
    }

    fn model(&self) -> StemModel {
        self.model
    }

    fn device(&self) -> InferDevice {
        self.device
    }

    fn separate(
        &self,
        interleaved: &[f32],
        channels: usize,
        _sample_rate: u32,
        params: &StemExtractParams,
        cancel: &StemExtractCancelToken,
        on_progress: &mut dyn FnMut(StemExtractProgress),
    ) -> Result<Vec<SeparatedStem>, StemExtractError> {
        if channels == 0 || channels > 2 {
            return Err(StemExtractError::UnsupportedChannels(channels));
        }
        if interleaved.is_empty() {
            return Err(StemExtractError::EmptyInput);
        }

        on_progress(StemExtractProgress::new(
            StemExtractStage::LoadingModel,
            5.0,
            format!(
                "Preparing {} on {}",
                self.model.label(),
                self.device.label()
            ),
        ));
        if cancel.is_cancelled() {
            return Err(StemExtractError::Cancelled);
        }

        let frames = interleaved.len() / channels;
        let selected: Vec<StemKind> = params.stems.iter().collect();
        let total = selected.len().max(1);
        let mut outputs = Vec::with_capacity(selected.len());

        for (index, stem) in selected.into_iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(StemExtractError::Cancelled);
            }
            let percent = 10.0 + (index as f32 / total as f32) * 80.0;
            on_progress(
                StemExtractProgress::new(
                    StemExtractStage::Separating,
                    percent,
                    format!("Separating {}", stem.label()),
                )
                .with_stem(stem),
            );

            let samples = match stem {
                StemKind::Vocals => mid_channel(interleaved, channels, frames, 1.0, 0.15),
                StemKind::Instrumental => side_channel(interleaved, channels, frames, 0.85),
                StemKind::Drums => band_emphasis(interleaved, channels, frames, 0.55, 0.35),
                StemKind::Bass => low_emphasis(interleaved, channels, frames),
                StemKind::Other => residual_other(interleaved, channels, frames),
            };
            outputs.push(SeparatedStem { kind: stem, samples });
        }

        on_progress(StemExtractProgress::new(
            StemExtractStage::Separating,
            95.0,
            "Separation complete",
        ));
        Ok(outputs)
    }
}

fn sample_at(interleaved: &[f32], channels: usize, frame: usize, ch: usize) -> f32 {
    interleaved[frame * channels + ch.min(channels - 1)]
}

fn mid_channel(
    interleaved: &[f32],
    channels: usize,
    frames: usize,
    mid_gain: f32,
    side_leak: f32,
) -> Vec<f32> {
    let mut out = vec![0.0; frames * channels];
    for frame in 0..frames {
        let l = sample_at(interleaved, channels, frame, 0);
        let r = sample_at(interleaved, channels, frame, 1);
        let mid = 0.5 * (l + r) * mid_gain;
        let side = 0.5 * (l - r) * side_leak;
        let left = (mid + side).clamp(-1.0, 1.0);
        let right = (mid - side).clamp(-1.0, 1.0);
        out[frame * channels] = left;
        if channels > 1 {
            out[frame * channels + 1] = right;
        }
    }
    out
}

fn side_channel(interleaved: &[f32], channels: usize, frames: usize, gain: f32) -> Vec<f32> {
    let mut out = vec![0.0; frames * channels];
    for frame in 0..frames {
        let l = sample_at(interleaved, channels, frame, 0);
        let r = sample_at(interleaved, channels, frame, 1);
        let mid = 0.5 * (l + r);
        let side = 0.5 * (l - r);
        // Instrumental ≈ original minus center vocal emphasis.
        let left = ((l - mid * 0.65) + side * 0.25) * gain;
        let right = ((r - mid * 0.65) - side * 0.25) * gain;
        out[frame * channels] = left.clamp(-1.0, 1.0);
        if channels > 1 {
            out[frame * channels + 1] = right.clamp(-1.0, 1.0);
        }
    }
    out
}

fn band_emphasis(
    interleaved: &[f32],
    channels: usize,
    frames: usize,
    high: f32,
    low: f32,
) -> Vec<f32> {
    let mut out = vec![0.0; frames * channels];
    let mut prev = [0.0_f32; 2];
    for frame in 0..frames {
        for ch in 0..channels {
            let x = sample_at(interleaved, channels, frame, ch);
            let hp = x - prev[ch];
            prev[ch] = x;
            let y = (hp * high + x * low).clamp(-1.0, 1.0);
            out[frame * channels + ch] = y;
        }
    }
    out
}

fn low_emphasis(interleaved: &[f32], channels: usize, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * channels];
    let mut state = [0.0_f32; 2];
    let alpha = 0.05_f32;
    for frame in 0..frames {
        for ch in 0..channels {
            let x = sample_at(interleaved, channels, frame, ch);
            state[ch] += alpha * (x - state[ch]);
            out[frame * channels + ch] = (state[ch] * 1.35).clamp(-1.0, 1.0);
        }
    }
    out
}

fn residual_other(interleaved: &[f32], channels: usize, frames: usize) -> Vec<f32> {
    // Mild high-shelf residual so "other" is distinct from vocals/drums/bass.
    let mut out = vec![0.0; frames * channels];
    let mut prev = [0.0_f32; 2];
    for frame in 0..frames {
        for ch in 0..channels {
            let x = sample_at(interleaved, channels, frame, ch);
            let hp = x - prev[ch];
            prev[ch] = x;
            out[frame * channels + ch] = (x * 0.45 + hp * 0.35).clamp(-1.0, 1.0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::StemExtractParams;
    use crate::stems::StemSet;

    #[test]
    fn stub_produces_requested_mdx_net_stems() {
        let backend = SpectralStubBackend::new(StemModel::MdxNet, InferDevice::Cpu);
        let mut params = StemExtractParams::mdx_net_cpu();
        params.stems = StemSet::new([StemKind::Vocals, StemKind::Bass]);
        let input = vec![0.2_f32, -0.1, 0.3, -0.2, 0.0, 0.1, -0.05, 0.05];
        let mut last_pct = 0.0;
        let stems = backend
            .separate(
                &input,
                2,
                48_000,
                &params,
                &StemExtractCancelToken::new(),
                &mut |p| {
                    assert!(p.percent >= last_pct);
                    last_pct = p.percent;
                },
            )
            .unwrap();
        assert_eq!(stems.len(), 2);
        assert_eq!(stems[0].samples.len(), input.len());
        assert_eq!(stems[1].samples.len(), input.len());
    }
}
