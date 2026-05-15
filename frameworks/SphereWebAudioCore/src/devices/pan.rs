//! Pan device — equal-power stereo panning.

use crate::buffer::AudioBuffer;
use crate::error::{EngineError, EngineResult};
use crate::params::ParamValue;

use super::{AudioDevice, ProcessContext};

pub struct PanDevice {
    /// Pan position: -1.0 (full left) to 1.0 (full right), 0.0 = center.
    pan: f32,
    enabled: bool,
}

impl PanDevice {
    pub fn new() -> Self {
        Self {
            pan: 0.0,
            enabled: true,
        }
    }
}

impl AudioDevice for PanDevice {
    fn device_type(&self) -> &str {
        "pan"
    }

    fn process(&mut self, buffer: &mut AudioBuffer, _context: &ProcessContext) {
        if !self.enabled || self.pan.abs() < 1e-7 {
            return; // Center pan, skip (equal-power center ≈ 0.707 for both)
        }
        buffer.apply_pan(self.pan);
    }

    fn set_param(&mut self, param: &str, value: ParamValue) -> EngineResult<()> {
        match param {
            "pan" => {
                if let Some(v) = value.as_f32() {
                    self.pan = v.clamp(-1.0, 1.0);
                    Ok(())
                } else {
                    Err(EngineError::InvalidParam {
                        device: "pan".into(),
                        param: param.into(),
                    })
                }
            }
            _ => Err(EngineError::InvalidParam {
                device: "pan".into(),
                param: param.into(),
            }),
        }
    }

    fn reset(&mut self, _sample_rate: f32) {
        // Stateless
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_center_is_noop() {
        let mut dev = PanDevice::new();
        let mut buf = AudioBuffer::new(2, 4);
        buf.channel_mut(0).copy_from_slice(&[1.0; 4]);
        buf.channel_mut(1).copy_from_slice(&[1.0; 4]);

        let transport = crate::transport::Transport::new(44100.0, 120.0);
        let ctx = ProcessContext {
            sample_rate: 44100.0,
            bpm: 120.0,
            transport: &transport,
        };
        dev.process(&mut buf, &ctx);
        // Center pan skips processing, so values should be unchanged
        assert_eq!(buf.channel(0), &[1.0; 4]);
        assert_eq!(buf.channel(1), &[1.0; 4]);
    }

    #[test]
    fn pan_hard_right() {
        let mut dev = PanDevice::new();
        dev.set_param("pan", ParamValue::Float(1.0)).unwrap();

        let mut buf = AudioBuffer::new(2, 4);
        buf.channel_mut(0).copy_from_slice(&[1.0; 4]);
        buf.channel_mut(1).copy_from_slice(&[1.0; 4]);

        let transport = crate::transport::Transport::new(44100.0, 120.0);
        let ctx = ProcessContext {
            sample_rate: 44100.0,
            bpm: 120.0,
            transport: &transport,
        };
        dev.process(&mut buf, &ctx);
        // Hard right: left should be near 0, right should be near 1
        assert!(buf.channel(0)[0] < 0.01, "Left should be ~0: {}", buf.channel(0)[0]);
        assert!(buf.channel(1)[0] > 0.99, "Right should be ~1: {}", buf.channel(1)[0]);
    }
}
