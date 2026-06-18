use std::ptr::NonNull;

use crate::stretching::error::StretchError;
use crate::stretching::params::StretchParams;
use crate::stretching::processor::StretchProcessor;
use crate::stretching::ratios::effective_pitch_ratio;

#[link(name = "sphere_signalsmith_bridge", kind = "static")]
unsafe extern "C" {
    fn fb_signalsmith_create(sample_rate: f32, channels: i32) -> *mut std::ffi::c_void;
    fn fb_signalsmith_destroy(handle: *mut std::ffi::c_void);
    fn fb_signalsmith_reset(handle: *mut std::ffi::c_void);
    fn fb_signalsmith_process_stereo(
        handle: *mut std::ffi::c_void,
        input_l: *const f32,
        input_r: *const f32,
        output_l: *mut f32,
        output_r: *mut f32,
        input_frames: i32,
        output_frames: i32,
        pitch_ratio: f32,
        quality: f32,
    ) -> i32;
    fn fb_signalsmith_latency_samples(handle: *mut std::ffi::c_void) -> i32;
}

pub struct SignalsmithProcessor {
    handle: NonNull<std::ffi::c_void>,
    _channels: usize,
    params: StretchParams,
}

impl SignalsmithProcessor {
    pub fn new(sample_rate: f32, channels: usize) -> Result<Self, StretchError> {
        let handle = unsafe { fb_signalsmith_create(sample_rate, channels as i32) };
        let handle = NonNull::new(handle).ok_or(StretchError::BackendUnavailable(
            super::super::params::StretchBackend::Signalsmith,
        ))?;

        Ok(Self {
            handle,
            _channels: channels,
            params: StretchParams::default(),
        })
    }

    fn pitch_ratio(&self) -> f32 {
        effective_pitch_ratio(&self.params)
    }
}

impl Drop for SignalsmithProcessor {
    fn drop(&mut self) {
        unsafe {
            fb_signalsmith_destroy(self.handle.as_ptr());
        }
    }
}

impl StretchProcessor for SignalsmithProcessor {
    fn reset(&mut self) {
        unsafe {
            fb_signalsmith_reset(self.handle.as_ptr());
        }
    }

    fn set_params(&mut self, params: StretchParams) {
        self.params = params;
    }

    fn latency_samples(&self) -> usize {
        let latency = unsafe { fb_signalsmith_latency_samples(self.handle.as_ptr()) };
        latency.max(0) as usize
    }

    /// Time-stretch by `output.len() / input.len()` and pitch-shift by the
    /// param transpose factor. Input and output lengths may differ; the caller
    /// supplies exactly the source samples to consume (see [`StretchProcessor`]).
    fn process_stereo(
        &mut self,
        input_l: &[f32],
        input_r: &[f32],
        output_l: &mut [f32],
        output_r: &mut [f32],
    ) -> Result<(), StretchError> {
        if input_l.len() != input_r.len() || output_l.len() != output_r.len() {
            return Err(StretchError::BufferLengthMismatch);
        }
        if input_l.is_empty() || output_l.is_empty() {
            return Err(StretchError::BufferLengthMismatch);
        }

        let input_frames = i32::try_from(input_l.len())
            .map_err(|_| StretchError::InvalidParams("frame count exceeds i32::MAX".to_string()))?;
        let output_frames = i32::try_from(output_l.len())
            .map_err(|_| StretchError::InvalidParams("frame count exceeds i32::MAX".to_string()))?;

        let status = unsafe {
            fb_signalsmith_process_stereo(
                self.handle.as_ptr(),
                input_l.as_ptr(),
                input_r.as_ptr(),
                output_l.as_mut_ptr(),
                output_r.as_mut_ptr(),
                input_frames,
                output_frames,
                self.pitch_ratio(),
                self.params.quality,
            )
        };

        if status == 0 {
            Ok(())
        } else {
            Err(StretchError::BackendFailed(format!(
                "signalsmith process_stereo failed with code {status}"
            )))
        }
    }
}

unsafe impl Send for SignalsmithProcessor {}
