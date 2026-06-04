//! Disk-streaming clip source (Phase F).
//!
//! Large compressed files (MP3 / FLAC / OGG) are too big to fully decode into
//! memory, and unlike PCM WAV they cannot be memory-mapped and read directly.
//! This module streams them: a background decoder thread fills a bounded,
//! preallocated ring buffer ahead of the playhead, and the audio callback reads
//! finished stereo frames from the ring with no allocation, no locks, and no
//! file I/O (see `tasks/native/audio-system-spec.md` §7 and the Phase F plan in
//! `audio-system-checklist.md`).
//!
//! ## Realtime safety
//!
//! The ring stores interleaved stereo samples as `AtomicU32` (f32 bit-casts).
//! Per-sample atomic load/store means there is **no data-race UB** even if the
//! producer/consumer window bookkeeping has an edge case — the worst case is a
//! momentarily stale sample, which the window check below already rejects as an
//! underrun. The callback only does relaxed atomic loads; the decoder thread
//! does the decode, seeks, and throttling.
//!
//! ## Window model (single-producer / single-consumer)
//!
//! * `write_frame` — exclusive upper bound of valid frames (Release by worker).
//! * `reader_frame` — last `floor(pos)` the callback asked for (Release by
//!   callback). The worker keeps the window `[write_frame - capacity,
//!   write_frame)` filled ahead of `reader_frame` and never overwrites a slot
//!   the reader may still touch (throttle keeps `decode_frame < reader +
//!   capacity - margin`). On a seek (reader jumps outside the window) the worker
//!   repositions the decoder and refills.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use std::fs::File;
use std::io;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio_file::probe_audio_file;

/// Ring capacity in stereo frames. 262144 frames ≈ 5.46 s @ 48 kHz and uses
/// ~2 MB (`capacity * 2 * 4` bytes) regardless of how long the source file is.
const RING_FRAMES: usize = 1 << 18;

/// Leave this many frames of head-room so the worker never writes a slot the
/// reader (which samples `idx` and `idx + 1`) could still be reading.
const THROTTLE_MARGIN: u64 = 8192;

/// `FUTUREBOARD_DISK_STREAM_DEBUG=1` enables decoder-thread traces. Cached on
/// first read; never touched from the audio callback.
fn disk_stream_debug_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_DISK_STREAM_DEBUG").is_some())
}

/// Process-wide disk-underrun counter, surfaced in diagnostics. A streaming
/// read that finds its frame outside the buffered window bumps this and returns
/// silence.
static GLOBAL_UNDERRUNS: AtomicU64 = AtomicU64::new(0);

/// Total disk-stream underruns since process start (diagnostics, plan §18).
pub fn total_disk_underruns() -> u64 {
    GLOBAL_UNDERRUNS.load(Ordering::Relaxed)
}

/// Lock-free stereo ring shared between the decoder thread (producer) and the
/// audio callback (consumer).
#[derive(Debug)]
pub struct StreamingRing {
    capacity: usize,
    total_frames: u64,
    sample_rate: u32,
    /// Interleaved stereo f32 bits; length `capacity * 2`.
    data: Box<[AtomicU32]>,
    write_frame: AtomicU64,
    reader_frame: AtomicU64,
    underruns: AtomicU64,
}

impl StreamingRing {
    fn new(total_frames: u64, sample_rate: u32, capacity: usize) -> Self {
        let capacity = capacity.max(2);
        let data = (0..capacity * 2)
            .map(|_| AtomicU32::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            capacity,
            total_frames,
            sample_rate,
            data,
            write_frame: AtomicU64::new(0),
            reader_frame: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        }
    }

    #[inline]
    fn write_frame(&self) -> u64 {
        self.write_frame.load(Ordering::Acquire)
    }

    #[inline]
    fn reader_frame(&self) -> u64 {
        self.reader_frame.load(Ordering::Acquire)
    }

    /// True when frame `idx` lies inside the currently-published window
    /// `[wf - capacity, wf)`.
    #[inline]
    fn frame_in_window(&self, idx: u64, wf: u64) -> bool {
        idx < wf && idx + self.capacity as u64 >= wf
    }

    #[inline]
    fn load_frame(&self, idx: u64) -> (f32, f32) {
        let slot = (idx as usize % self.capacity) * 2;
        (
            f32::from_bits(self.data[slot].load(Ordering::Relaxed)),
            f32::from_bits(self.data[slot + 1].load(Ordering::Relaxed)),
        )
    }

    // ── Producer side (decoder thread) ───────────────────────────────────────

    #[inline]
    fn store_frame(&self, frame: u64, l: f32, r: f32) {
        let slot = (frame as usize % self.capacity) * 2;
        self.data[slot].store(l.to_bits(), Ordering::Relaxed);
        self.data[slot + 1].store(r.to_bits(), Ordering::Relaxed);
    }

    #[inline]
    fn publish_write(&self, next_frame: u64) {
        self.write_frame.store(next_frame, Ordering::Release);
    }

    /// Reset the window to start at `frame` (used after a seek reposition). The
    /// reader sees underruns until the worker refills from here.
    #[inline]
    fn reset_window(&self, frame: u64) {
        self.write_frame.store(frame, Ordering::Release);
    }

    // ── Consumer side (audio callback) ───────────────────────────────────────

    /// Read a linearly-interpolated stereo sample at fractional frame `pos`.
    /// Realtime-safe: relaxed atomic loads only. Out-of-window reads return
    /// silence and bump the underrun counters.
    #[inline]
    pub fn read_interp(&self, pos: f64) -> (f32, f32) {
        if pos < 0.0 || self.total_frames == 0 {
            return (0.0, 0.0);
        }
        let idx = pos.floor() as u64;
        if idx >= self.total_frames {
            return (0.0, 0.0);
        }
        // Tell the worker where playback is so it can prefetch / reposition.
        self.reader_frame.store(idx, Ordering::Release);

        let wf = self.write_frame();
        if !self.frame_in_window(idx, wf) {
            self.bump_underrun();
            return (0.0, 0.0);
        }

        // Next frame for interpolation, clamped to the last real frame.
        let next = (idx + 1).min(self.total_frames - 1);
        let (l0, r0) = self.load_frame(idx);
        if next == idx || !self.frame_in_window(next, wf) {
            // At EOF, or the next frame is not buffered yet: hold this frame
            // rather than counting an underrun for the interpolation partner.
            return (l0, r0);
        }
        let frac = (pos - idx as f64) as f32;
        let (l1, r1) = self.load_frame(next);
        (l0 + (l1 - l0) * frac, r0 + (r1 - r0) * frac)
    }

    #[inline]
    fn bump_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
        GLOBAL_UNDERRUNS.fetch_add(1, Ordering::Relaxed);
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
}

/// A streaming clip source: owns the ring and the decoder thread. Dropping it
/// stops and joins the thread (this happens on the control / graveyard thread,
/// never the audio callback).
pub struct StreamingSource {
    ring: Arc<StreamingRing>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for StreamingSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamingSource")
            .field("sample_rate", &self.ring.sample_rate)
            .field("total_frames", &self.ring.total_frames)
            .field("underruns", &self.ring.underruns())
            .finish()
    }
}

impl StreamingSource {
    /// Probe `path`, allocate the ring, and spawn the decoder thread.
    pub fn open(path: &str) -> Result<Self, String> {
        let info = probe_audio_file(path).map_err(|e| format!("probe failed: {e}"))?;
        if info.total_frames == 0 || info.sample_rate == 0 {
            return Err(format!("streaming source has no frames: '{path}'"));
        }

        let ring = Arc::new(StreamingRing::new(
            info.total_frames,
            info.sample_rate,
            RING_FRAMES,
        ));
        let stop = Arc::new(AtomicBool::new(false));

        let worker_ring = Arc::clone(&ring);
        let worker_stop = Arc::clone(&stop);
        let worker_path = path.to_string();
        let worker = std::thread::Builder::new()
            .name("daux-stream-decoder".to_string())
            .spawn(move || {
                if let Err(e) = decode_loop(&worker_path, &worker_ring, &worker_stop) {
                    if disk_stream_debug_enabled() {
                        eprintln!("[DAUx stream] decoder exited: {e}");
                    }
                }
            })
            .map_err(|e| format!("failed to spawn stream decoder: {e}"))?;

        if disk_stream_debug_enabled() {
            eprintln!(
                "[DAUx stream] streaming '{}': {} frames @ {}Hz (ring={} frames)",
                path, info.total_frames, info.sample_rate, RING_FRAMES
            );
        }

        Ok(Self {
            ring,
            stop,
            worker: Some(worker),
        })
    }

    #[inline]
    pub fn sample_rate(&self) -> u32 {
        self.ring.sample_rate
    }

    #[inline]
    pub fn frames(&self) -> usize {
        self.ring.total_frames as usize
    }

    #[inline]
    pub fn read_interp(&self, pos: f64) -> (f32, f32) {
        self.ring.read_interp(pos)
    }

    #[inline]
    pub fn read_frame_stereo(&self, frame: usize) -> (f32, f32) {
        self.ring.read_interp(frame as f64)
    }
}

impl Drop for StreamingSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

/// Decoder thread body: open the file, then loop decoding and filling the ring,
/// repositioning on seek and throttling so the buffer stays ahead of the
/// playhead without overrunning unread frames.
fn decode_loop(
    path: &str,
    ring: &Arc<StreamingRing>,
    stop: &Arc<AtomicBool>,
) -> Result<(), String> {
    let file = File::open(Path::new(path)).map_err(|e| format!("open failed: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("format probe failed: {e}"))?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| "no decodable audio track".to_string())?
        .clone();
    let track_id = track.id;
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("decoder create failed: {e}"))?;

    let capacity = ring.capacity as u64;
    // Next absolute frame index to write into the ring.
    let mut decode_frame: u64 = 0;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }

        let reader = ring.reader_frame();
        let wf = ring.write_frame();
        let window_start = wf.saturating_sub(capacity);

        // Seek reposition: the playhead moved outside what we can serve —
        // either before the buffered window or so far ahead that catching up by
        // decoding would stall playback.
        if reader + 1 < window_start || reader >= wf + capacity {
            let seeked = format.seek(
                SeekMode::Accurate,
                SeekTo::TimeStamp {
                    ts: reader,
                    track_id,
                },
            );
            decoder.reset();
            decode_frame = match seeked {
                Ok(to) => to.actual_ts,
                Err(_) => reader, // best-effort: some formats seek approximately
            };
            ring.reset_window(decode_frame);
            if disk_stream_debug_enabled() {
                eprintln!("[DAUx stream] reposition -> frame {decode_frame} (reader={reader})");
            }
            continue;
        }

        // Throttle: keep the buffer full ahead of the reader but never start a
        // packet that could overwrite a frame the reader might still touch.
        if decode_frame + THROTTLE_MARGIN >= reader + capacity {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }

        // Decode one packet for our track.
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(ref e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // EOF — nothing more to decode; idle until a seek wakes us.
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(format!("packet read error: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_ref) => {
                if sample_buf.is_none() {
                    sample_buf = Some(SampleBuffer::<f32>::new(
                        audio_ref.capacity() as u64,
                        *audio_ref.spec(),
                    ));
                }
                if let Some(buf) = &mut sample_buf {
                    buf.copy_interleaved_ref(audio_ref);
                    let samples = buf.samples();
                    let ch = channels.max(1);
                    let frame_count = samples.len() / ch;
                    for frame in 0..frame_count {
                        let base = frame * ch;
                        let (l, r) = if ch == 1 {
                            (samples[base], samples[base])
                        } else {
                            (samples[base], samples[base + 1])
                        };
                        ring.store_frame(decode_frame, l, r);
                        decode_frame += 1;
                    }
                    ring.publish_write(decode_frame);
                }
            }
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("decode error: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the ring's producer side by hand (no decoder thread) to validate
    /// the window + underrun logic deterministically.
    fn ring(capacity: usize, total: u64) -> StreamingRing {
        StreamingRing::new(total, 48_000, capacity)
    }

    fn fill(r: &StreamingRing, from: u64, to: u64) {
        for f in from..to {
            r.store_frame(f, f as f32, -(f as f32));
        }
        r.publish_write(to);
    }

    #[test]
    fn read_inside_window_returns_buffered_sample() {
        let r = ring(16, 1000);
        fill(&r, 0, 10);
        let (l, _r) = r.read_interp(5.0);
        assert_eq!(l, 5.0);
        assert_eq!(r.underruns(), 0);
    }

    #[test]
    fn read_ahead_of_written_is_underrun() {
        let r = ring(16, 1000);
        fill(&r, 0, 4);
        let (l, rr) = r.read_interp(8.0);
        assert_eq!((l, rr), (0.0, 0.0));
        assert_eq!(r.underruns(), 1);
    }

    #[test]
    fn read_behind_window_start_is_underrun() {
        // capacity 16: after writing up to frame 40 the window is [24, 40).
        let r = ring(16, 1000);
        fill(&r, 0, 40);
        let (l, rr) = r.read_interp(5.0);
        assert_eq!((l, rr), (0.0, 0.0));
        assert_eq!(r.underruns(), 1);
        // A frame inside the window still reads fine.
        let (l2, _) = r.read_interp(30.0);
        assert_eq!(l2, 30.0);
    }

    #[test]
    fn interpolates_between_frames() {
        let r = ring(16, 1000);
        fill(&r, 0, 10);
        // Halfway between frame 2 (=2.0) and 3 (=3.0).
        let (l, _) = r.read_interp(2.5);
        assert!((l - 2.5).abs() < 1e-6);
    }

    #[test]
    fn past_end_is_silent_not_underrun() {
        let r = ring(16, 4);
        fill(&r, 0, 4);
        let (l, rr) = r.read_interp(10.0);
        assert_eq!((l, rr), (0.0, 0.0));
        assert_eq!(r.underruns(), 0);
    }

    #[test]
    fn last_frame_holds_without_underrun() {
        // Reading the final frame must not count its (nonexistent) interp
        // partner as an underrun.
        let r = ring(16, 4);
        fill(&r, 0, 4);
        let (l, _) = r.read_interp(3.0);
        assert_eq!(l, 3.0);
        assert_eq!(r.underruns(), 0);
    }
}
