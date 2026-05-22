//! Audio device trait and registry.
//!
//! Devices are insert effects that process audio buffers in the track
//! insert chain. Each device has a stable string ID and typed parameters.

pub mod gain;
pub mod pan;

use crate::buffer::AudioBuffer;
use crate::error::EngineResult;
use crate::params::ParamValue;
use crate::transport::Transport;

/// Context passed to devices during processing.
pub struct ProcessContext<'a> {
    pub sample_rate: f32,
    pub bpm: f64,
    pub transport: &'a Transport,
}

/// Trait for all audio processing devices (insert effects).
pub trait AudioDevice: Send {
    /// Unique device type identifier (e.g., "gain", "eq", "compressor").
    fn device_type(&self) -> &str;

    /// Process audio in-place.
    fn process(&mut self, buffer: &mut AudioBuffer, context: &ProcessContext);

    /// Set a named parameter. Returns error for unknown params.
    fn set_param(&mut self, param: &str, value: ParamValue) -> EngineResult<()>;

    /// Reset internal state (e.g., filter history) for the given sample rate.
    fn reset(&mut self, sample_rate: f32);

    /// Whether this device is bypassed.
    fn enabled(&self) -> bool;

    /// Set bypass state.
    fn set_enabled(&mut self, enabled: bool);
}

/// Create a device by type name. Returns None for unknown types.
pub fn create_device(device_type: &str) -> Option<Box<dyn AudioDevice>> {
    match device_type {
        "gain" => Some(Box::new(gain::GainDevice::new())),
        "pan" => Some(Box::new(pan::PanDevice::new())),
        _ => None,
    }
}
