//! Full-duplex ASIO session (Windows, Exclusive Edition builds only).
//!
//! One ASIO driver is one duplex device with one buffer set and one
//! buffer-switch lifecycle. This module opens the selected driver **once**,
//! builds the input and output streams from the **same** device instance (so
//! CPAL prepares one union buffer set instead of disposing the other side's
//! buffers), and keeps both running for the life of the session:
//!
//! ```text
//! ASIO buffer switch
//!   ├─ input callback  → monitor ring + meters + preview bins + record sink
//!   └─ output callback → drain_commands + fill_output_f32 (normal DAW graph)
//! ```
//!
//! Everything that used to require opening a second stream — selecting a track
//! input, enabling monitoring, arming, starting a take — is now either an
//! atomic store (`SharedState::monitor_src_l/r`, ring active flags) or a
//! bounded command to the input callback ([`AsioInputCommand`]). The driver
//! and its buffers are never touched after open, which is what keeps playback
//! running while inputs are used.
//!
//! Realtime rules in the input callback: atomics, a bounded `try_recv` command
//! drain, and preallocated block-pool sends only. Old record sinks are pushed
//! to a trash channel and dropped on the control thread, never deallocated on
//! the audio thread.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use cpal::platform::{AsioDevice, AsioDriverEvent, AsioDriverEventGuard};
use cpal::traits::{DeviceTrait, StreamTrait};
use crossbeam_channel::{bounded, Receiver, Sender};

use crate::backend::cpal_backend::build_typed_stream;
use crate::backend::{AsioSessionCaps, DauxDeviceConfig};
use crate::command::EngineCommand;
use crate::engine::{f32_store, SharedState};
use crate::error::SphereAudioError;
use crate::runtime::RuntimeProject;

/// Commands for the persistent input callback. Bounded and lock-free; the
/// callback drains them at block start.
pub enum AsioInputCommand {
    /// Install the record fan-out for a take. A previously installed sink is
    /// pushed to the trash channel for control-thread disposal.
    SetRecordSink(Box<RecordSink>),
    /// Remove the record fan-out (take stopped/cancelled).
    ClearRecordSink,
}

/// Everything the input callback needs to feed one recording take. Created by
/// `start_recording`, carried into the callback via [`AsioInputCommand`], and
/// returned through the trash channel when replaced/cleared so the `Sender`
/// disconnect (which finalizes the disk writer) happens off the audio thread.
pub struct RecordSink {
    pub audio_tx: Sender<Vec<i32>>,
    pub free_rx: Receiver<Vec<i32>>,
    pub free_tx: Sender<Vec<i32>>,
    /// Preview-waveform bin width in frames (from the take's sample rate).
    pub samples_per_bin: usize,
    /// Blocks dropped because the pool/queue was exhausted (shared with the
    /// recording session for the end-of-take report).
    pub dropped_blocks: Arc<AtomicU64>,
}

/// Driver lifecycle notifications, converted to flags the control thread
/// polls. Duplicate reset requests coalesce into one pending flag.
#[derive(Default)]
pub struct AsioDriverEventFlags {
    pub reset_requested: AtomicBool,
    pub latencies_changed: AtomicBool,
    pub resync_count: AtomicU64,
}

/// The open duplex session. Dropping it tears the session down in order:
/// streams first (their `Drop` deregisters the buffer callbacks), then the
/// device — whose last driver handle runs `ASIOStop`/`ASIODisposeBuffers`/
/// `ASIOExit` and fully unloads the driver.
pub struct AsioDuplexHandle {
    pub cmd_tx: Sender<EngineCommand>,
    pub input_cmd_tx: Sender<AsioInputCommand>,
    /// Control-thread side of record-sink disposal (see [`RecordSink`]).
    pub trash_rx: Receiver<RecordSink>,
    pub events: Arc<AsioDriverEventFlags>,
    pub caps: AsioSessionCaps,
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_size: u32,
    /// Set when the driver has inputs but the input stream could not open —
    /// playback proceeds, and this is surfaced as a status-bar diagnostic.
    pub input_warning: Option<String>,
    output_stream: cpal::Stream,
    input_stream: Option<cpal::Stream>,
    _event_guard: AsioDriverEventGuard,
    device: AsioDevice,
}

// Safety: same contract as `CpalStreamHandle` — the handle lives inside
// `EngineInner` behind a parking_lot Mutex and is only touched from the
// control thread. The streams' callbacks communicate exclusively through
// `Arc<SharedState>` atomics and bounded channels.
unsafe impl Send for AsioDuplexHandle {}
unsafe impl Sync for AsioDuplexHandle {}

impl AsioDuplexHandle {
    /// Resume the output side (transport-level start). The input side always
    /// runs so meters and recording work while the engine is "stopped".
    pub fn play(&self) -> Result<(), String> {
        self.output_stream.play().map_err(|e| e.to_string())
    }

    /// Pause the output side. The paused CPAL callback writes silence into the
    /// hardware buffers; input keeps running.
    pub fn pause(&self) -> Result<(), String> {
        self.output_stream.pause().map_err(|e| e.to_string())
    }

    pub fn has_input(&self) -> bool {
        self.input_stream.is_some()
    }

    /// Open the driver's control panel (control thread only).
    pub fn open_control_panel(&self) -> Result<(), String> {
        self.device.asio_open_control_panel()
    }

    /// Re-query driver latencies (e.g. after a `LatenciesChanged` message).
    pub fn refresh_latencies(&self) -> Option<(u32, u32)> {
        self.device
            .asio_latencies()
            .map(|latencies| (latencies.input, latencies.output))
    }

    /// Consume a pending driver reset request (coalesced).
    pub fn take_reset_request(&self) -> bool {
        self.events.reset_requested.swap(false, Ordering::SeqCst)
    }

    pub fn take_latencies_changed(&self) -> bool {
        self.events.latencies_changed.swap(false, Ordering::SeqCst)
    }

    /// Drain record sinks the input callback discarded, dropping them here on
    /// the control thread (disconnects their disk writers).
    pub fn drain_trashed_sinks(&self) -> usize {
        let mut drained = 0;
        while self.trash_rx.try_recv().is_ok() {
            drained += 1;
        }
        drained
    }
}

/// Snap a requested buffer size into the driver's constraints: clamp to
/// `[min, max]`, then honour granularity (`-1` = powers of two, `> 0` = steps
/// from `min`). `None` picks the driver-preferred size.
pub(crate) fn snap_buffer_size(
    requested: Option<u32>,
    info: &cpal::platform::AsioBufferSizeInfo,
) -> u32 {
    let preferred = info.preferred.max(1);
    let Some(requested) = requested.filter(|&frames| frames > 0) else {
        return preferred;
    };
    let min = info.min.max(1);
    let max = info.max.max(min);
    let clamped = requested.clamp(min, max);
    match info.granularity {
        0 => preferred,
        -1 => {
            // Powers of two between min and max, choosing the nearest.
            let mut best = min.next_power_of_two().clamp(min, max);
            let mut candidate = best;
            while candidate <= max {
                if candidate.abs_diff(clamped) < best.abs_diff(clamped) {
                    best = candidate;
                }
                match candidate.checked_mul(2) {
                    Some(next) => candidate = next,
                    None => break,
                }
            }
            best
        }
        granularity if granularity > 0 => {
            let step = granularity as u32;
            let offset = clamped.saturating_sub(min);
            let snapped = min + (offset + step / 2) / step * step;
            snapped.clamp(min, max)
        }
        _ => clamped,
    }
}

/// Open the full-duplex session. Control thread only; the engine must have
/// closed any previous stream first (the driver loader refuses to open a
/// second session while one is active).
pub(crate) fn open_duplex(
    host: &cpal::Host,
    config: &DauxDeviceConfig,
    shared: Arc<SharedState>,
    initial_runtime: RuntimeProject,
    glitch_counter: Arc<AtomicU64>,
) -> Result<AsioDuplexHandle, SphereAudioError> {
    let fail = |stage: &str, detail: String| {
        SphereAudioError::StreamOpenFailed(format!("ASIO [{stage}]: {detail}"))
    };

    let asio_host = host.as_asio().ok_or_else(|| {
        fail(
            "host",
            "internal error: non-ASIO host passed to the ASIO session opener".into(),
        )
    })?;

    // ── Driver + capability queries ───────────────────────────────────────
    let device = asio_host
        .asio_device_by_name(config.output_device_id.as_deref())
        .map_err(|error| fail("driver-load", error))?;
    let driver_name = device
        .name()
        .unwrap_or_else(|_| "Unknown ASIO driver".to_string());

    let (in_channels, out_channels) = device
        .asio_channel_counts()
        .map_err(|error| fail("channel-query", error))?;
    if out_channels == 0 {
        return Err(fail(
            "channel-query",
            format!("driver '{driver_name}' reports no output channels"),
        ));
    }

    let buffer_info = device
        .asio_buffer_size_info()
        .map_err(|error| fail("buffer-query", error))?;
    let requested_buffer = config.buffer_size.filter(|&frames| frames > 0);
    let buffer_frames = snap_buffer_size(requested_buffer, &buffer_info);
    if let Some(requested) = requested_buffer {
        if requested != buffer_frames {
            eprintln!(
                "[DAUx ASIO] requested buffer {requested} is outside driver constraints \
                 (min={} max={} preferred={} granularity={}); using {buffer_frames}",
                buffer_info.min, buffer_info.max, buffer_info.preferred, buffer_info.granularity
            );
        }
    }

    let driver_rate = device
        .asio_current_sample_rate()
        .map_err(|error| fail("sample-rate-query", error))?;
    let sample_rate = config
        .sample_rate
        .filter(|&rate| rate > 0)
        .unwrap_or(driver_rate);

    let output_format = device
        .default_output_config()
        .map_err(|error| {
            fail(
                "format-query",
                format!("driver '{driver_name}' output format unsupported: {error}"),
            )
        })?
        .sample_format();

    // ── Streams (input first, then output; both from the same device) ─────
    //
    // CPAL's second build re-creates the ASIO buffers as the union of both
    // directions; passing identical rate/size keeps the two sides coherent.
    shared.sample_rate.store(sample_rate, Ordering::Relaxed);
    let (cmd_tx, cmd_rx) = bounded::<EngineCommand>(512);
    let (input_cmd_tx, input_cmd_rx) = bounded::<AsioInputCommand>(8);
    let (trash_tx, trash_rx) = bounded::<RecordSink>(8);

    let platform_device: cpal::Device = device.clone().into();

    let mut input_warning = None;
    let input_stream = if in_channels > 0 {
        let input_result = device
            .default_input_config()
            .map_err(|error| format!("input format unsupported: {error}"))
            .and_then(|input_config| {
                let stream_config = cpal::StreamConfig {
                    channels: in_channels.min(u16::MAX as u32) as u16,
                    sample_rate: cpal::SampleRate(sample_rate),
                    buffer_size: cpal::BufferSize::Fixed(buffer_frames),
                };
                build_input_fanout_stream(
                    &platform_device,
                    &stream_config,
                    input_config.sample_format(),
                    Arc::clone(&shared),
                    input_cmd_rx,
                    trash_tx,
                )
            });
        match input_result {
            Ok(stream) => Some(stream),
            Err(error) => {
                // Playback must not die because inputs are unavailable, but the
                // degradation is loud: status-bar diagnostic + recording will
                // refuse to start with the same reason.
                let warning = format!(
                    "ASIO input unavailable on '{driver_name}' \
                     ({in_channels} channel(s) reported): {error}"
                );
                eprintln!("[DAUx ASIO] {warning}");
                input_warning = Some(warning);
                None
            }
        }
    } else {
        None
    };

    let output_config = cpal::StreamConfig {
        channels: out_channels.min(2) as u16,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Fixed(buffer_frames),
    };
    let output_stream = build_typed_stream(
        &platform_device,
        &output_config,
        output_format,
        cmd_rx,
        Arc::clone(&shared),
        initial_runtime,
        glitch_counter,
        config.mmcss_priority,
    )
    .map_err(|error| fail("output-open", error))?;

    // Input runs for the life of the session (meters/recording work while the
    // transport is stopped); output starts paused until `EngineInner::start`.
    if let Some(stream) = &input_stream {
        stream
            .play()
            .map_err(|error| fail("input-start", error.to_string()))?;
    }

    // ── Post-open queries (valid once buffers exist) ──────────────────────
    let (input_latency, output_latency) = device
        .asio_latencies()
        .map(|latencies| (latencies.input, latencies.output))
        .unwrap_or((0, 0));

    let effective_in_channels = if input_stream.is_some() { in_channels } else { 0 };
    let channel_names = |is_input: bool, count: u32| -> Vec<String> {
        (0..count)
            .map(|index| {
                device
                    .asio_channel_description(is_input, index)
                    .map(|desc| desc.name)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| {
                        format!(
                            "{} {}",
                            if is_input { "Input" } else { "Output" },
                            index + 1
                        )
                    })
            })
            .collect()
    };

    let caps = AsioSessionCaps {
        driver: driver_name.clone(),
        sample_rate,
        buffer_size: buffer_frames,
        input_channels: effective_in_channels,
        output_channels: out_channels,
        input_channel_names: channel_names(true, effective_in_channels),
        output_channel_names: channel_names(false, out_channels),
        input_latency_samples: input_latency,
        output_latency_samples: output_latency,
    };

    // ── Driver lifecycle messages → coalesced control flags ───────────────
    let events = Arc::new(AsioDriverEventFlags::default());
    let event_flags = Arc::clone(&events);
    let event_guard = device.asio_on_driver_event(move |event| match event {
        AsioDriverEvent::ResetRequest => {
            event_flags.reset_requested.store(true, Ordering::SeqCst);
        }
        AsioDriverEvent::LatenciesChanged => {
            event_flags.latencies_changed.store(true, Ordering::SeqCst);
        }
        AsioDriverEvent::ResyncRequest => {
            event_flags.resync_count.fetch_add(1, Ordering::SeqCst);
        }
        AsioDriverEvent::Other => {}
    });

    eprintln!(
        "[DAUx ASIO] session open: driver='{driver_name}' sr={sample_rate} buffer={buffer_frames} \
         in={effective_in_channels} out={out_channels} latency_in={input_latency} \
         latency_out={output_latency}"
    );

    Ok(AsioDuplexHandle {
        cmd_tx,
        input_cmd_tx,
        trash_rx,
        events,
        caps,
        device_name: driver_name,
        sample_rate,
        buffer_size: buffer_frames,
        input_warning,
        output_stream,
        input_stream,
        _event_guard: event_guard,
        device,
    })
}

// ── Input fan-out ─────────────────────────────────────────────────────────────

/// Native-format input sample: convertible to the monitor-path f32 and the
/// record-path full-scale i32 without allocation.
trait AsioInputSample: cpal::SizedSample + Copy + Send + 'static {
    fn to_monitor_f32(self) -> f32;
    fn to_record_i32(self) -> i32;
}

impl AsioInputSample for i16 {
    #[inline]
    fn to_monitor_f32(self) -> f32 {
        self as f32 / 32_768.0
    }
    #[inline]
    fn to_record_i32(self) -> i32 {
        (self as i32) << 16
    }
}

impl AsioInputSample for i32 {
    #[inline]
    fn to_monitor_f32(self) -> f32 {
        self as f32 / 2_147_483_648.0
    }
    #[inline]
    fn to_record_i32(self) -> i32 {
        self
    }
}

impl AsioInputSample for f32 {
    #[inline]
    fn to_monitor_f32(self) -> f32 {
        self
    }
    #[inline]
    fn to_record_i32(self) -> i32 {
        crate::recording::f32_to_s32(self)
    }
}

impl AsioInputSample for f64 {
    #[inline]
    fn to_monitor_f32(self) -> f32 {
        self as f32
    }
    #[inline]
    fn to_record_i32(self) -> i32 {
        crate::recording::f32_to_s32(self as f32)
    }
}

fn build_input_fanout_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sample_format: cpal::SampleFormat,
    shared: Arc<SharedState>,
    input_cmd_rx: Receiver<AsioInputCommand>,
    trash_tx: Sender<RecordSink>,
) -> Result<cpal::Stream, String> {
    match sample_format {
        cpal::SampleFormat::I16 => {
            build_input_fanout_typed::<i16>(device, config, shared, input_cmd_rx, trash_tx)
        }
        cpal::SampleFormat::I32 => {
            build_input_fanout_typed::<i32>(device, config, shared, input_cmd_rx, trash_tx)
        }
        cpal::SampleFormat::F32 => {
            build_input_fanout_typed::<f32>(device, config, shared, input_cmd_rx, trash_tx)
        }
        cpal::SampleFormat::F64 => {
            build_input_fanout_typed::<f64>(device, config, shared, input_cmd_rx, trash_tx)
        }
        other => Err(format!("unsupported ASIO input sample format {other}")),
    }
}

/// One persistent input callback, four fan-out paths (mirrors the recording
/// stream contract in `recording.rs`):
///   1. monitor  → `shared.input_ring` (drained by the output render callback)
///   2. meters   → peak atomics (+ `session_input_peak` for the Settings test)
///   3. preview  → min/max/rms bins → `shared.preview_ring`
///   4. record   → preallocated block pool → bounded channel → disk writer
///
/// Realtime-safe: no allocation, no locks, no syscalls. Sink swaps arrive over
/// a bounded channel; discarded sinks leave via `trash_tx` so their channel
/// `Sender`s drop on the control thread.
fn build_input_fanout_typed<T: AsioInputSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<SharedState>,
    input_cmd_rx: Receiver<AsioInputCommand>,
    trash_tx: Sender<RecordSink>,
) -> Result<cpal::Stream, String> {
    use crate::engine::atomic_max_f32_bits;
    use crate::input_ring::WaveformPeak;

    let channels = config.channels.max(1) as usize;

    let mut record_sink: Option<RecordSink> = None;
    // Preview accumulator — callback-local state, no allocation per block.
    let mut bin_min = f32::MAX;
    let mut bin_max = f32::MIN;
    let mut bin_sumsq = 0.0f32;
    let mut bin_count = 0usize;

    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _info| {
                // 1. Drain sink-swap commands (bounded, lock-free).
                while let Ok(command) = input_cmd_rx.try_recv() {
                    match command {
                        AsioInputCommand::SetRecordSink(sink) => {
                            if let Some(old) = record_sink.replace(*sink) {
                                let _ = trash_tx.try_send(old);
                            }
                        }
                        AsioInputCommand::ClearRecordSink => {
                            if let Some(old) = record_sink.take() {
                                let _ = trash_tx.try_send(old);
                            }
                        }
                    }
                }

                let frames = data.len() / channels;
                shared.input_cb_count.fetch_add(1, Ordering::Relaxed);
                shared
                    .input_frames_received
                    .fetch_add(frames as u64, Ordering::Relaxed);

                let mon_l_ch = shared.monitor_src_l.load(Ordering::Relaxed) as usize;
                let mon_r_ch = shared.monitor_src_r.load(Ordering::Relaxed) as usize;
                let ring_active = shared.input_ring.is_active();
                let preview_active = shared.recording_preview_active.load(Ordering::Relaxed);
                let samples_per_bin = record_sink
                    .as_ref()
                    .map(|sink| sink.samples_per_bin)
                    .unwrap_or(0);

                let mut raw_peak_l = 0.0f32;
                let mut raw_peak_r = 0.0f32;
                let mut last_l = 0.0f32;
                let mut last_r = 0.0f32;
                let mut session_peak = 0.0f32;

                for frame in data.chunks(channels) {
                    let first = frame
                        .first()
                        .copied()
                        .map(T::to_monitor_f32)
                        .unwrap_or(0.0);
                    let l = frame
                        .get(mon_l_ch)
                        .copied()
                        .map(T::to_monitor_f32)
                        .unwrap_or(first)
                        .clamp(-1.0, 1.0);
                    let r = frame
                        .get(mon_r_ch)
                        .copied()
                        .map(T::to_monitor_f32)
                        .unwrap_or(l)
                        .clamp(-1.0, 1.0);
                    last_l = l;
                    last_r = r;
                    raw_peak_l = raw_peak_l.max(l.abs());
                    raw_peak_r = raw_peak_r.max(r.abs());

                    // Monitor bridge → output render callback. Gated on the
                    // ring's active flag so the consumer's cursor management
                    // matches the producer exactly.
                    if ring_active {
                        shared.input_ring.write_stereo(l, r);
                    }

                    // Preview bins for the in-progress take (mono mix of the
                    // monitored channels), same protocol as recording.rs.
                    if preview_active && samples_per_bin > 0 {
                        let m = (l + r) * 0.5;
                        bin_min = bin_min.min(m);
                        bin_max = bin_max.max(m);
                        bin_sumsq += m * m;
                        bin_count += 1;
                        if bin_count >= samples_per_bin {
                            let rms = (bin_sumsq / bin_count as f32).sqrt();
                            shared.preview_ring.push(WaveformPeak {
                                min: bin_min,
                                max: bin_max,
                                rms,
                            });
                            bin_min = f32::MAX;
                            bin_max = f32::MIN;
                            bin_sumsq = 0.0;
                            bin_count = 0;
                        }
                    } else {
                        bin_min = f32::MAX;
                        bin_max = f32::MIN;
                        bin_sumsq = 0.0;
                        bin_count = 0;
                    }

                    // Session-wide input peak across all channels (Settings
                    // input test + diagnostics).
                    for &sample in frame {
                        session_peak = session_peak.max(T::to_monitor_f32(sample).abs());
                    }
                }

                shared
                    .live_input_l
                    .store(f32_store(last_l), Ordering::Relaxed);
                shared
                    .live_input_r
                    .store(f32_store(last_r), Ordering::Relaxed);
                atomic_max_f32_bits(&shared.live_input_peak_l, raw_peak_l);
                atomic_max_f32_bits(&shared.live_input_peak_r, raw_peak_r);
                atomic_max_f32_bits(&shared.session_input_peak, session_peak);
                if ring_active {
                    shared.live_input_active.store(true, Ordering::Relaxed);
                }

                // 4. Record fan-out → disk writer (only while a take is live).
                if shared.recording_active.load(Ordering::Relaxed) {
                    if let Some(sink) = record_sink.as_ref() {
                        match sink.free_rx.try_recv() {
                            Ok(mut block) => {
                                if block.capacity() < data.len() {
                                    sink.dropped_blocks.fetch_add(1, Ordering::Relaxed);
                                    shared.record_ring_overruns.fetch_add(1, Ordering::Relaxed);
                                    let _ = sink.free_tx.try_send(block);
                                } else {
                                    block.clear();
                                    block.extend(data.iter().copied().map(T::to_record_i32));
                                    if let Err(error) = sink.audio_tx.try_send(block) {
                                        sink.dropped_blocks.fetch_add(1, Ordering::Relaxed);
                                        shared
                                            .record_ring_overruns
                                            .fetch_add(1, Ordering::Relaxed);
                                        let _ = sink.free_tx.try_send(error.into_inner());
                                    }
                                }
                            }
                            Err(_) => {
                                sink.dropped_blocks.fetch_add(1, Ordering::Relaxed);
                                shared.record_ring_overruns.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            },
            |error| eprintln!("[DAUx ASIO] input stream error: {error}"),
            None,
        )
        .map_err(|error| format!("cannot open ASIO input stream: {error}"))
}

#[cfg(test)]
mod tests {
    use super::snap_buffer_size;
    use cpal::platform::AsioBufferSizeInfo;

    fn info(min: u32, max: u32, preferred: u32, granularity: i32) -> AsioBufferSizeInfo {
        AsioBufferSizeInfo {
            min,
            max,
            preferred,
            granularity,
        }
    }

    #[test]
    fn default_uses_preferred() {
        assert_eq!(snap_buffer_size(None, &info(64, 2048, 512, -1)), 512);
        assert_eq!(snap_buffer_size(Some(0), &info(64, 2048, 512, -1)), 512);
    }

    #[test]
    fn clamps_to_driver_range() {
        assert_eq!(snap_buffer_size(Some(16), &info(64, 2048, 512, -1)), 64);
        assert_eq!(snap_buffer_size(Some(1 << 20), &info(64, 2048, 512, -1)), 2048);
    }

    #[test]
    fn power_of_two_granularity_snaps_to_nearest() {
        assert_eq!(snap_buffer_size(Some(200), &info(64, 2048, 512, -1)), 256);
        assert_eq!(snap_buffer_size(Some(96), &info(64, 2048, 512, -1)), 64);
        assert_eq!(snap_buffer_size(Some(97), &info(64, 2048, 512, -1)), 128);
    }

    #[test]
    fn linear_granularity_steps_from_min() {
        // min=48, step=16 → valid sizes 48, 64, 80, ...
        assert_eq!(snap_buffer_size(Some(70), &info(48, 1024, 128, 16)), 64);
        assert_eq!(snap_buffer_size(Some(73), &info(48, 1024, 128, 16)), 80);
    }

    #[test]
    fn fixed_size_driver_always_uses_preferred() {
        assert_eq!(snap_buffer_size(Some(256), &info(512, 512, 512, 0)), 512);
    }
}
