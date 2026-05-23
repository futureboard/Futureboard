//! DAUx cpal backend — wraps CPAL for WASAPI Shared / CoreAudio / ALSA.
//!
//! This is the "Auto" / "WasapiShared" / "CoreAudio" / "Alsa" backend.
//! On each platform cpal picks the best native API:
//!   - Windows  → WASAPI Shared event-driven
//!   - macOS    → CoreAudio
//!   - Linux    → ALSA
//!
//! On Windows the audio thread gets MMCSS "Pro Audio" priority if
//! `config.mmcss_priority` is true.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample};
use crossbeam_channel::{bounded, Receiver, Sender};

use crate::backend::render::{drain_commands, fill_output_f32, LocalAudioState};
use crate::backend::DauxDeviceConfig;
use crate::command::EngineCommand;
use crate::engine::SharedState;
use crate::error::SphereAudioError;
use crate::runtime::RuntimeProject;
use crate::types::JsAudioDeviceInfo;

// ─────────────────────────────────────────────────────────────────────────────

pub struct CpalStreamHandle {
    stream: cpal::Stream,
    pub cmd_tx: Sender<EngineCommand>,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub device_name: String,
    pub backend_name: String,
}

// Safety: see engine.rs — stream is only touched on the JS/main thread under Mutex.
unsafe impl Send for CpalStreamHandle {}
unsafe impl Sync for CpalStreamHandle {}

impl CpalStreamHandle {
    pub fn play(&self) -> Result<(), String> {
        self.stream.play().map_err(|e| e.to_string())
    }
    pub fn pause(&self) -> Result<(), String> {
        self.stream.pause().map_err(|e| e.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────

pub fn list_output_devices() -> Vec<JsAudioDeviceInfo> {
    crate::device::list_output_devices()
}

pub fn list_input_devices() -> Vec<JsAudioDeviceInfo> {
    crate::device::list_input_devices()
}

/// Open a cpal output stream with the given `DauxDeviceConfig`.
/// Returns the stream handle on success.
pub fn open(
    config: &DauxDeviceConfig,
    shared: Arc<SharedState>,
    initial_runtime: RuntimeProject,
    glitch_counter: Arc<AtomicU64>,
) -> Result<CpalStreamHandle, SphereAudioError> {
    let (dev, dev_name) = crate::device::resolve_output_device(config.output_device_id.as_deref())
        .map_err(SphereAudioError::DeviceNotFound)?;

    let backend_name = cpal::default_host().id().name().to_string();

    // Build stream config candidates.
    let default_supported = dev
        .default_output_config()
        .map_err(|e| SphereAudioError::StreamOpenFailed(e.to_string()))?;
    let sample_format = default_supported.sample_format();
    let default_cfg = default_supported.config();

    // Apply requested sample rate / buffer size overrides.
    let requested_cfg = if config.sample_rate.is_some() || config.buffer_size.is_some() {
        let mut c = default_cfg.clone();
        if let Some(sr) = config.sample_rate {
            c.sample_rate = cpal::SampleRate(sr);
        }
        if let Some(bs) = config.buffer_size {
            let frames = if config.safe_mode { bs.max(512) } else { bs };
            c.buffer_size = BufferSize::Fixed(frames);
        }
        Some(c)
    } else {
        None
    };

    let candidates: Vec<(&str, cpal::StreamConfig)> = {
        let mut v = Vec::new();
        if let Some(rc) = requested_cfg {
            v.push(("requested", rc));
        }
        v.push(("default", default_cfg));
        v
    };

    let mut last_error = None;

    for (label, stream_config) in &candidates {
        shared
            .sample_rate
            .store(stream_config.sample_rate.0, Ordering::Relaxed);

        let (tx, rx) = bounded::<EngineCommand>(512);

        match build_typed_stream(
            &dev,
            stream_config,
            sample_format,
            rx,
            Arc::clone(&shared),
            initial_runtime.clone(),
            Arc::clone(&glitch_counter),
            config.mmcss_priority,
        ) {
            Ok(stream) => {
                let buf_size = match stream_config.buffer_size {
                    BufferSize::Fixed(f) => f,
                    BufferSize::Default => 0,
                };
                return Ok(CpalStreamHandle {
                    stream,
                    cmd_tx: tx,
                    sample_rate: stream_config.sample_rate.0,
                    buffer_size: buf_size,
                    device_name: dev_name,
                    backend_name,
                });
            }
            Err(e) => {
                last_error = Some(format!("{label} config failed: {e}"));
            }
        }
    }

    Err(SphereAudioError::StreamOpenFailed(
        last_error.unwrap_or_else(|| "no candidates available".into()),
    ))
}

// ── Stream builders ───────────────────────────────────────────────────────────

fn build_typed_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: SampleFormat,
    cmd_rx: Receiver<EngineCommand>,
    shared: Arc<SharedState>,
    initial_runtime: RuntimeProject,
    glitch_counter: Arc<AtomicU64>,
    mmcss_priority: bool,
) -> Result<cpal::Stream, String> {
    macro_rules! build_for {
        ($T:ty) => {
            build_stream_typed::<$T>(
                device,
                config,
                cmd_rx,
                shared,
                initial_runtime,
                glitch_counter,
                mmcss_priority,
            )
        };
    }
    match sample_format {
        SampleFormat::I8 => build_for!(i8),
        SampleFormat::I16 => build_for!(i16),
        SampleFormat::I32 => build_for!(i32),
        SampleFormat::I64 => build_for!(i64),
        SampleFormat::U8 => build_for!(u8),
        SampleFormat::U16 => build_for!(u16),
        SampleFormat::U32 => build_for!(u32),
        SampleFormat::U64 => build_for!(u64),
        SampleFormat::F32 => build_for!(f32),
        SampleFormat::F64 => build_for!(f64),
        fmt => Err(format!("unsupported sample format: {fmt}")),
    }
}

fn build_stream_typed<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    cmd_rx: Receiver<EngineCommand>,
    shared: Arc<SharedState>,
    initial_runtime: RuntimeProject,
    glitch_counter: Arc<AtomicU64>,
    mmcss_priority: bool,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Sample + FromSample<f32>,
{
    let output_sample_rate = config.sample_rate.0;
    let sr = output_sample_rate as f64;
    let ch = config.channels as usize;
    let mut runtime = initial_runtime;
    runtime.sample_rate = output_sample_rate;
    let mut local = LocalAudioState::new(sr);
    let mut mmcss_set = false;
    // f32 scratch buffer for shared render kernel.
    let mut f32_scratch: Vec<f32> = Vec::new();

    let stream = device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
                // ── Set MMCSS on first callback invocation ────────────────────
                #[cfg(target_os = "windows")]
                if mmcss_priority && !mmcss_set {
                    mmcss_set = set_mmcss_pro_audio();
                }
                #[cfg(not(target_os = "windows"))]
                let _ = mmcss_priority; // suppress unused warning

                // ── Drain command queue ───────────────────────────────────────
                drain_commands(
                    &cmd_rx,
                    &mut runtime,
                    &shared,
                    &mut local,
                    output_sample_rate,
                );

                // ── Fill via shared f32 kernel ────────────────────────────────
                let frames_needed = data.len() / ch.max(1);
                let f32_len = frames_needed * ch;
                if f32_scratch.len() < f32_len {
                    f32_scratch.resize(f32_len, 0.0f32);
                }
                let scratch = &mut f32_scratch[..f32_len];
                for s in scratch.iter_mut() {
                    *s = 0.0;
                }

                fill_output_f32(scratch, ch, &mut runtime, &shared, &mut local);

                // ── Convert f32 → T ───────────────────────────────────────────
                for (dst, src) in data.iter_mut().zip(scratch.iter()) {
                    *dst = T::from_sample(*src);
                }
                let _ = mmcss_set; // suppress unused (non-windows)
            },
            move |err| {
                eprintln!("[DAUx cpal] Stream error: {err}");
                glitch_counter.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())?;

    Ok(stream)
}

// ── MMCSS helper (Windows only) ───────────────────────────────────────────────

/// Set MMCSS "Pro Audio" priority on the calling thread.
/// Returns true on success.  Called once per audio thread on first callback.
#[cfg(target_os = "windows")]
fn set_mmcss_pro_audio() -> bool {
    // Use raw extern declaration to avoid windows crate feature-flag issues.
    #[link(name = "avrt")]
    extern "system" {
        fn AvSetMmThreadCharacteristicsW(task_name: *const u16, task_index: *mut u32) -> isize;
    }

    let task: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
    let mut task_index = 0u32;
    unsafe {
        let handle = AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut task_index);
        let ok = handle != 0;
        if ok {
            eprintln!("[DAUx] MMCSS 'Pro Audio' priority set (index={task_index})");
        } else {
            eprintln!("[DAUx] MMCSS set failed (may require elevated privileges)");
        }
        ok
    }
}
