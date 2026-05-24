//! Audio buffer abstraction.
//!
//! Non-interleaved f32 buffers. Pre-allocated to avoid heap allocation in
//! the realtime process path.

/// A multi-channel audio buffer using non-interleaved f32 samples.
///
/// Channels are stored as separate Vec<f32> internally but sliced during
/// processing to avoid allocation.
pub struct AudioBuffer {
    /// One Vec per channel, each holding `max_frames` samples.
    channels: Vec<Vec<f32>>,
    /// Current active frame count (≤ capacity).
    frames: usize,
}

impl AudioBuffer {
    /// Create a new AudioBuffer with the given channel count and max frame size.
    /// All samples are initialized to 0.0.
    pub fn new(channel_count: usize, max_frames: usize) -> Self {
        Self {
            channels: (0..channel_count)
                .map(|_| vec![0.0_f32; max_frames])
                .collect(),
            frames: max_frames,
        }
    }

    /// Number of channels.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Current frame count.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Set the active frame count for this block.
    /// Must be ≤ the capacity allocated at creation.
    pub fn set_frames(&mut self, frames: usize) {
        assert!(
            frames <= self.capacity(),
            "frames ({frames}) exceeds capacity ({})",
            self.capacity()
        );
        self.frames = frames;
    }

    /// Maximum frame capacity.
    pub fn capacity(&self) -> usize {
        if self.channels.is_empty() {
            0
        } else {
            self.channels[0].len()
        }
    }

    /// Get an immutable slice for a channel.
    pub fn channel(&self, ch: usize) -> &[f32] {
        &self.channels[ch][..self.frames]
    }

    /// Get a mutable slice for a channel.
    pub fn channel_mut(&mut self, ch: usize) -> &mut [f32] {
        let frames = self.frames;
        &mut self.channels[ch][..frames]
    }

    /// Clear all channels (fill with 0.0). No allocation.
    pub fn clear(&mut self) {
        for ch in &mut self.channels {
            for sample in ch[..self.frames].iter_mut() {
                *sample = 0.0;
            }
        }
    }

    /// Mix (add) another buffer into this one. Channels are matched by index.
    /// If source has fewer channels, extra channels in self are untouched.
    /// If source has more channels, extra source channels are ignored.
    pub fn mix_from(&mut self, other: &AudioBuffer, gain: f32) {
        let ch_count = self.channel_count().min(other.channel_count());
        let frame_count = self.frames.min(other.frames);
        for ch in 0..ch_count {
            let dst = &mut self.channels[ch][..frame_count];
            let src = &other.channels[ch][..frame_count];
            for i in 0..frame_count {
                dst[i] += src[i] * gain;
            }
        }
    }

    /// Copy samples from another buffer into this one, overwriting.
    pub fn copy_from(&mut self, other: &AudioBuffer) {
        let ch_count = self.channel_count().min(other.channel_count());
        let frame_count = self.frames.min(other.frames);
        for ch in 0..ch_count {
            self.channels[ch][..frame_count].copy_from_slice(&other.channels[ch][..frame_count]);
        }
    }

    /// Apply a gain to all channels.
    pub fn apply_gain(&mut self, gain: f32) {
        for ch in &mut self.channels {
            for sample in ch[..self.frames].iter_mut() {
                *sample *= gain;
            }
        }
    }

    /// Apply equal-power stereo pan. Only meaningful for 2-channel buffers.
    /// pan: -1.0 (full left) to 1.0 (full right).
    pub fn apply_pan(&mut self, pan: f32) {
        if self.channel_count() < 2 {
            return;
        }
        let pan_clamped = pan.clamp(-1.0, 1.0);
        // Equal-power panning: left = cos(θ), right = sin(θ)
        // where θ = (pan + 1) * π/4  (maps -1..1 to 0..π/2)
        let angle = (pan_clamped + 1.0) * std::f32::consts::FRAC_PI_4;
        let gain_l = angle.cos();
        let gain_r = angle.sin();

        let frames = self.frames;
        let (left, right) = self.channels.split_at_mut(1);
        let left = &mut left[0][..frames];
        let right = &mut right[0][..frames];
        for i in 0..frames {
            left[i] *= gain_l;
            right[i] *= gain_r;
        }
    }

    /// Get the peak absolute value across all channels.
    pub fn peak(&self) -> f32 {
        let mut peak = 0.0_f32;
        for ch in &self.channels {
            for &sample in &ch[..self.frames] {
                let abs = sample.abs();
                if abs > peak {
                    peak = abs;
                }
            }
        }
        peak
    }

    /// Get per-channel peak values.
    pub fn channel_peaks(&self) -> Vec<f32> {
        self.channels
            .iter()
            .map(|ch| {
                ch[..self.frames]
                    .iter()
                    .fold(0.0_f32, |max, &s| max.max(s.abs()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_silent() {
        let buf = AudioBuffer::new(2, 128);
        assert_eq!(buf.channel_count(), 2);
        assert_eq!(buf.frames(), 128);
        assert_eq!(buf.peak(), 0.0);
    }

    #[test]
    fn apply_gain_works() {
        let mut buf = AudioBuffer::new(2, 4);
        buf.channel_mut(0).copy_from_slice(&[1.0, 0.5, -0.5, -1.0]);
        buf.channel_mut(1)
            .copy_from_slice(&[0.5, 0.25, -0.25, -0.5]);
        buf.apply_gain(0.5);
        assert_eq!(buf.channel(0), &[0.5, 0.25, -0.25, -0.5]);
        assert_eq!(buf.channel(1), &[0.25, 0.125, -0.125, -0.25]);
    }

    #[test]
    fn mix_from_adds() {
        let mut dst = AudioBuffer::new(2, 4);
        dst.channel_mut(0).copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);

        let mut src = AudioBuffer::new(2, 4);
        src.channel_mut(0).copy_from_slice(&[0.5, 0.5, 0.5, 0.5]);

        dst.mix_from(&src, 1.0);
        assert_eq!(dst.channel(0), &[1.5, 1.5, 1.5, 1.5]);
    }

    #[test]
    fn pan_center_is_equal() {
        let mut buf = AudioBuffer::new(2, 4);
        buf.channel_mut(0).copy_from_slice(&[1.0; 4]);
        buf.channel_mut(1).copy_from_slice(&[1.0; 4]);
        buf.apply_pan(0.0);
        // At center pan, both channels should get ~0.707
        let l = buf.channel(0)[0];
        let r = buf.channel(1)[0];
        assert!(
            (l - r).abs() < 0.001,
            "L={l} R={r} should be equal at center"
        );
        assert!((l - 0.707).abs() < 0.01, "Expected ~0.707, got {l}");
    }
}
