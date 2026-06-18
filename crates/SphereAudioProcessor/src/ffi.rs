use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use crate::stretching::{
    StretchAlgorithm, StretchBackend, StretchError, StretchMode, StretchParams, StretchProcessor,
    create_stretch_processor, resolve_backend,
};

const VERSION: u32 = 0x0001_0000;

#[repr(C)]
pub struct SphereStretchHandle {
    _private: [u8; 0],
}

struct StretchHandleState {
    sample_rate: f32,
    channels: u32,
    params: StretchParams,
    backend: StretchBackend,
    processor: Box<dyn StretchProcessor + Send>,
}

fn state_from_handle(handle: *mut SphereStretchHandle) -> Option<&'static mut StretchHandleState> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: `SphereStretchHandle` is an opaque FFI newtype for `StretchHandleState`.
    Some(unsafe { &mut *(handle.cast::<StretchHandleState>()) })
}

#[unsafe(no_mangle)]
pub extern "C" fn sphere_audio_processor_version() -> u32 {
    VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn sphere_stretch_create(sample_rate: f32, channels: u32) -> *mut SphereStretchHandle {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if !sample_rate.is_finite() || sample_rate <= 0.0 || channels == 0 || channels > 2 {
            return ptr::null_mut();
        }

        let params = StretchParams::default();
        let backend = resolve_backend(&params);
        let processor = match create_stretch_processor(
            backend,
            sample_rate,
            channels as usize,
            params.clone(),
        ) {
            Ok(processor) => processor,
            Err(_) => return ptr::null_mut(),
        };

        let state = StretchHandleState {
            sample_rate,
            channels,
            params,
            backend,
            processor,
        };
        Box::into_raw(Box::new(state)).cast::<SphereStretchHandle>()
    }));

    result.unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn sphere_stretch_destroy(handle: *mut SphereStretchHandle) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() {
            return;
        }
        unsafe {
            drop(Box::from_raw(handle.cast::<StretchHandleState>()));
        }
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn sphere_stretch_reset(handle: *mut SphereStretchHandle) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let Some(state) = state_from_handle(handle) else {
            return;
        };
        state.processor.reset();
    }));
}

#[unsafe(no_mangle)]
pub extern "C" fn sphere_stretch_set_params(
    handle: *mut SphereStretchHandle,
    mode: u32,
    algorithm: u32,
    time_ratio: f32,
    pitch_ratio: f32,
    source_bpm: f32,
    target_bpm: f32,
    preserve_pitch: bool,
    quality: f32,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let Some(state) = state_from_handle(handle) else {
            return -1;
        };

        let params = match build_params(
            mode,
            algorithm,
            time_ratio,
            pitch_ratio,
            source_bpm,
            target_bpm,
            preserve_pitch,
            quality,
        ) {
            Ok(params) => params,
            Err(_) => return -2,
        };

        let backend = resolve_backend(&params);
        if backend != state.backend {
            match create_stretch_processor(
                backend,
                state.sample_rate,
                state.channels as usize,
                params.clone(),
            ) {
                Ok(processor) => {
                    state.processor = processor;
                    state.backend = backend;
                }
                Err(_) => return -3,
            }
        }
        state.params = params.clone();
        state.processor.set_params(params);
        0
    }));

    result.unwrap_or(-4)
}

#[unsafe(no_mangle)]
pub extern "C" fn sphere_stretch_process_stereo(
    handle: *mut SphereStretchHandle,
    input_l: *const f32,
    input_r: *const f32,
    output_l: *mut f32,
    output_r: *mut f32,
    frames: usize,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null()
            || input_l.is_null()
            || input_r.is_null()
            || output_l.is_null()
            || output_r.is_null()
            || frames == 0
        {
            return -1;
        }

        let Some(state) = state_from_handle(handle) else {
            return -1;
        };

        let input_l = unsafe { std::slice::from_raw_parts(input_l, frames) };
        let input_r = unsafe { std::slice::from_raw_parts(input_r, frames) };
        let output_l = unsafe { std::slice::from_raw_parts_mut(output_l, frames) };
        let output_r = unsafe { std::slice::from_raw_parts_mut(output_r, frames) };

        match state.processor.process_stereo(input_l, input_r, output_l, output_r) {
            Ok(()) => 0,
            Err(StretchError::BufferLengthMismatch) => -2,
            Err(_) => -3,
        }
    }));

    result.unwrap_or(-4)
}

fn build_params(
    mode: u32,
    algorithm: u32,
    time_ratio: f32,
    pitch_ratio: f32,
    source_bpm: f32,
    target_bpm: f32,
    preserve_pitch: bool,
    quality: f32,
) -> Result<StretchParams, StretchError> {
    let mode = match mode {
        0 => StretchMode::Off,
        1 => StretchMode::Manual,
        2 => StretchMode::TempoSync,
        3 => StretchMode::Warp,
        _ => return Err(StretchError::InvalidParams("invalid stretch mode".into())),
    };
    let algorithm = match algorithm {
        0 => StretchAlgorithm::Off,
        1 => StretchAlgorithm::RePitch,
        2 => StretchAlgorithm::PreservePitch,
        _ => {
            return Err(StretchError::InvalidParams(
                "invalid stretch algorithm".into(),
            ));
        }
    };

    Ok(StretchParams {
        mode,
        algorithm,
        time_ratio,
        pitch_ratio,
        source_bpm: optional_positive(source_bpm),
        target_bpm: optional_positive(target_bpm),
        preserve_pitch,
        quality,
    })
}

fn optional_positive(value: f32) -> Option<f32> {
    if value.is_finite() && value > 0.0 {
        Some(value)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffi_create_process_destroy_roundtrip() {
        let handle = sphere_stretch_create(48_000.0, 2);
        assert!(!handle.is_null());

        assert_eq!(
            sphere_stretch_set_params(
                handle,
                1,
                1,
                2.0,
                1.0,
                f32::NAN,
                f32::NAN,
                false,
                0.75
            ),
            0
        );

        let input_l = [0.0_f32, 0.25, 0.5, 0.75];
        let input_r = [0.0_f32, 0.25, 0.5, 0.75];
        let mut output_l = [0.0; 4];
        let mut output_r = [0.0; 4];

        assert_eq!(
            sphere_stretch_process_stereo(
                handle,
                input_l.as_ptr(),
                input_r.as_ptr(),
                output_l.as_mut_ptr(),
                output_r.as_mut_ptr(),
                4
            ),
            0
        );

        sphere_stretch_reset(handle);
        sphere_stretch_destroy(handle);
        sphere_stretch_destroy(ptr::null_mut());
    }

    #[test]
    fn version_is_non_zero() {
        assert_ne!(sphere_audio_processor_version(), 0);
    }
}
