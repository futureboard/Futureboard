//! Gain device — utility gain / output trim.

use crate::buffer::AudioBuffer;
use crate::error::{EngineError, EngineResult};
use crate::params::ParamValue;

use super::{AudioDevice, ProcessContext};

pub struct GainDevice {
    /// Linear gain (1.0 = unity).
    gain: f32,
    enabled: bool,
}

impl GainDevice {
    pub fn new() -> Self {
        Self {
            gain: 1.0,
            enabled: true,
        }
    }
}

impl AudioDevice for GainDevice {
    fn device_type(&self) -> &str {
        "gain"
    }

    fn process(&mut self, buffer: &mut AudioBuffer, _context: &ProcessContext) {
        if !self.enabled || (self.gain - 1.0).abs() < 1e-7 {
            return; // Unity gain, skip processing
        }
        buffer.apply_gain(self.gain);
    }

    fn set_param(&mut self, param: &str, value: ParamValue) -> EngineResult<()> {
        match param {
            "gain" => {
                if let Some(v) = value.as_f32() {
                    self.gain = v.clamp(0.0, 10.0);
                    Ok(())
                } else {
                    Err(EngineError::InvalidParam {
                        device: "gain".into(),
                        param: param.into(),
                    })
                }
            }
            _ => Err(EngineError::InvalidParam {
                device: "gain".into(),
                param: param.into(),
            }),
        }
    }

    fn reset(&mut self, _sample_rate: f32) {
        // Stateless device, nothing to reset
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
    fn gain_unity_is_noop() {
        let mut dev = GainDevice::new();
        let mut buf = AudioBuffer::new(2, 4);
        buf.channel_mut(0).copy_from_slice(&[1.0, 0.5, -0.5, -1.0]);
        buf.channel_mut(1)
            .copy_from_slice(&[0.5, 0.25, -0.25, -0.5]);

        let transport = crate::transport::Transport::new(44100.0, 120.0);
        let ctx = ProcessContext {
            sample_rate: 44100.0,
            bpm: 120.0,
            transport: &transport,
        };
        dev.process(&mut buf, &ctx);
        // Unity gain should leave buffer unchanged
        assert_eq!(buf.channel(0), &[1.0, 0.5, -0.5, -1.0]);
    }

    #[test]
    fn gain_half() {
        let mut dev = GainDevice::new();
        dev.set_param("gain", ParamValue::Float(0.5)).unwrap();

        let mut buf = AudioBuffer::new(1, 4);
        buf.channel_mut(0).copy_from_slice(&[1.0, 0.8, 0.4, 0.2]);

        let transport = crate::transport::Transport::new(44100.0, 120.0);
        let ctx = ProcessContext {
            sample_rate: 44100.0,
            bpm: 120.0,
            transport: &transport,
        };
        dev.process(&mut buf, &ctx);
        assert_eq!(buf.channel(0), &[0.5, 0.4, 0.2, 0.1]);
    }

    #[test]
    fn unknown_param_returns_error() {
        let mut dev = GainDevice::new();
        let result = dev.set_param("nonexistent", ParamValue::Float(1.0));
        assert!(result.is_err());
    }
}
