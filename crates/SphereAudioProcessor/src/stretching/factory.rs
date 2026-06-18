use super::backends::{repitch::RePitchProcessor, signalsmith::SignalsmithProcessor};
use super::error::StretchError;
use super::params::{StretchAlgorithm, StretchBackend, StretchMode, StretchParams};
use super::processor::StretchProcessor;

pub fn signalsmith_stretch_available() -> bool {
    SignalsmithProcessor::new(48_000.0, 2).is_ok()
}

pub fn resolve_backend(params: &StretchParams) -> StretchBackend {
    if params.mode == StretchMode::Off {
        return StretchBackend::InternalRePitch;
    }

    if params.algorithm == StretchAlgorithm::RePitch || !params.preserve_pitch {
        return StretchBackend::InternalRePitch;
    }

    StretchBackend::Signalsmith
}

pub fn create_stretch_processor(
    backend: StretchBackend,
    sample_rate: f32,
    channels: usize,
    params: StretchParams,
) -> Result<Box<dyn StretchProcessor + Send>, StretchError> {
    if !sample_rate.is_finite() || sample_rate <= 0.0 {
        return Err(StretchError::InvalidParams(format!(
            "invalid sample_rate: {sample_rate}"
        )));
    }
    if channels == 0 || channels > 2 {
        return Err(StretchError::InvalidParams(format!(
            "unsupported channel count: {channels}"
        )));
    }

    match backend {
        StretchBackend::InternalRePitch => {
            let mut processor = RePitchProcessor::new(sample_rate, channels);
            processor.set_params(params);
            Ok(Box::new(processor))
        }
        StretchBackend::Signalsmith => match SignalsmithProcessor::new(sample_rate, channels) {
            Ok(mut processor) => {
                processor.set_params(params);
                Ok(Box::new(processor))
            }
            Err(err) => {
                log::warn!(
                    "Signalsmith stretch init failed, falling back to InternalRePitch: {err}"
                );
                let mut processor = RePitchProcessor::new(sample_rate, channels);
                processor.set_params(params);
                Ok(Box::new(processor))
            }
        },
    }
}
