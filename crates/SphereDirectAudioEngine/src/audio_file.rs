//! Audio file decoder for the native playback engine.
//!
//! **WAV/WAVE** — decoded by an inline RIFF/WAVE parser (fast, zero extra deps).
//! **Everything else** — decoded via `symphonia` (MP3, FLAC, OGG Vorbis, AIFF).
//!
//! The result is always interleaved `f32` samples normalised to `−1.0 … 1.0`.
//! Decoding happens on the control thread; the audio callback only reads the
//! finished `AudioFileBuffer` through an `Arc` — no allocation at runtime.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

// ── Public API ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AudioFileBuffer {
    pub sample_rate: u32,
    pub channels: usize,
    pub frames: usize,
    /// Interleaved PCM samples, normalised to `−1.0 … 1.0`.
    pub samples: Vec<f32>,
}

/// Load an audio file from `path` into a decoded `AudioFileBuffer`.
///
/// Supported extensions: `wav`, `wave`, `mp3`, `flac`, `ogg`, `oga`,
/// `aiff`, `aif`.
///
/// Returns an error string on failure; the caller logs it and skips the clip.
pub fn load_audio_file(path: &str) -> Result<AudioFileBuffer, String> {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        // Fast path — hand-written RIFF/WAVE parser (no symphonia overhead).
        "wav" | "wave" => load_wav(p),

        // Symphonia handles everything else.
        "mp3" | "flac" | "ogg" | "oga" | "aiff" | "aif" => load_via_symphonia(p),

        other => Err(format!("unsupported native audio format '{other}'")),
    }
}

#[derive(Debug, Clone)]
pub struct WavPeakResult {
    pub sample_rate: u32,
    pub channel_count: u32,
    pub duration: f64,
    pub samples_per_peak: u32,
    pub peak_count: u32,
    pub peaks: Vec<i32>,
}

pub fn generate_wav_peaks_from_path(
    path: &str,
    samples_per_peak: u32,
) -> Result<WavPeakResult, String> {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "wav" && ext != "wave" {
        return Err("Rust peak generation currently supports PCM WAV only".to_string());
    }

    let mut file = File::open(p).map_err(|e| format!("Cannot open '{}': {e}", p.display()))?;
    let (fmt, data_start, data_len) = read_wav_header(&mut file)?;
    if fmt.audio_format != 1 || !matches!(fmt.bits_per_sample, 16 | 24 | 32) {
        return Err(format!(
            "unsupported WAV format for peak scan: format={} bits={}",
            fmt.audio_format, fmt.bits_per_sample
        ));
    }

    let bytes_per_sample = (fmt.bits_per_sample / 8) as usize;
    let bytes_per_frame = fmt.channels * bytes_per_sample;
    if bytes_per_frame == 0 || data_len < bytes_per_frame as u64 {
        return Err("empty WAV data".to_string());
    }

    let frames = (data_len / bytes_per_frame as u64) as usize;
    let spp = samples_per_peak.max(1) as usize;
    let peak_count = frames.div_ceil(spp);
    let mut peaks = vec![0i32; peak_count * fmt.channels * 2];
    let mut min = vec![1.0f32; fmt.channels];
    let mut max = vec![-1.0f32; fmt.channels];

    file.seek(SeekFrom::Start(data_start))
        .map_err(|e| format!("seek failed: {e}"))?;
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut remaining = data_len;
    let mut frame_index = 0usize;
    let mut current_peak = 0usize;

    while remaining > 0 {
        let wanted = buffer.len().min(remaining as usize);
        let aligned = if remaining as usize <= buffer.len() {
            wanted
        } else {
            (wanted / bytes_per_frame).max(1) * bytes_per_frame
        };
        let read = file
            .read(&mut buffer[..aligned])
            .map_err(|e| format!("read failed: {e}"))?;
        if read == 0 {
            break;
        }

        let frame_count = read / bytes_per_frame;
        for frame in 0..frame_count {
            let frame_byte = frame * bytes_per_frame;
            for ch in 0..fmt.channels {
                let sample_byte = frame_byte + ch * bytes_per_sample;
                let value = read_wav_pcm_sample(&buffer, sample_byte, fmt.bits_per_sample)?;
                if value < min[ch] {
                    min[ch] = value;
                }
                if value > max[ch] {
                    max[ch] = value;
                }
            }

            frame_index += 1;
            if frame_index % spp == 0 {
                write_i16_peak_i32(&mut peaks, current_peak, fmt.channels, &min, &max);
                current_peak += 1;
                reset_peak_min_max(&mut min, &mut max);
            }
        }

        remaining = remaining.saturating_sub((frame_count * bytes_per_frame) as u64);
    }

    if current_peak < peak_count {
        write_i16_peak_i32(&mut peaks, current_peak, fmt.channels, &min, &max);
    }

    Ok(WavPeakResult {
        sample_rate: fmt.sample_rate,
        channel_count: fmt.channels as u32,
        duration: frames as f64 / fmt.sample_rate as f64,
        samples_per_peak: spp as u32,
        peak_count: peak_count as u32,
        peaks,
    })
}

// ── Symphonia decoder ──────────────────────────────────────────────────────────

fn load_via_symphonia(path: &Path) -> Result<AudioFileBuffer, String> {
    let src = File::open(path).map_err(|e| format!("Cannot open '{}': {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
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
        .map_err(|e| format!("Format probe failed: {e}"))?;

    let mut format = probed.format;

    // Pick the first decodable audio track.
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| "No decodable audio track found".to_string())?
        .clone();

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "Track has no sample rate".to_string())?;
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Failed to create codec decoder: {e}"))?;

    let mut all_samples: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // Clean EOF.
            Err(SymphoniaError::IoError(ref e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                break;
            }
            // The codec / format needs a reset (e.g. after a seek or stream error).
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(format!("Packet read error: {e}")),
        };

        // Skip packets that belong to other tracks (e.g. album art).
        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf_ref) => {
                // Initialise the sample buffer on first decoded block.
                if sample_buf.is_none() {
                    let spec = *audio_buf_ref.spec();
                    sample_buf = Some(SampleBuffer::<f32>::new(
                        audio_buf_ref.capacity() as u64,
                        spec,
                    ));
                }
                if let Some(buf) = &mut sample_buf {
                    buf.copy_interleaved_ref(audio_buf_ref);
                    all_samples.extend_from_slice(buf.samples());
                }
            }
            // Benign decode errors — skip the packet and keep going.
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("Decode error: {e}")),
        }
    }

    let frames = if channels > 0 {
        all_samples.len() / channels
    } else {
        0
    };
    Ok(AudioFileBuffer {
        sample_rate,
        channels,
        frames,
        samples: all_samples,
    })
}

// ── Hand-written RIFF/WAVE parser ─────────────────────────────────────────────
//
// Supports PCM 8 / 16 / 24 / 32-bit integer and IEEE float 32-bit.
// Used instead of symphonia for WAV to avoid the extra dependency overhead on
// the most common format.

fn load_wav(path: &Path) -> Result<AudioFileBuffer, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    if bytes.len() < 44 {
        return Err("file too small for WAV".to_string());
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".to_string());
    }

    let mut cursor = 12usize;
    let mut fmt: Option<WavFmt> = None;
    let mut data_range: Option<(usize, usize)> = None;

    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let len = read_u32_le(&bytes, cursor + 4)? as usize;
        let body = cursor + 8;
        let end = body.saturating_add(len);
        if end > bytes.len() {
            return Err("truncated WAV chunk".to_string());
        }

        match id {
            b"fmt " => {
                if len < 16 {
                    return Err("invalid fmt chunk".to_string());
                }
                fmt = Some(WavFmt {
                    audio_format: read_u16_le(&bytes, body)?,
                    channels: read_u16_le(&bytes, body + 2)? as usize,
                    sample_rate: read_u32_le(&bytes, body + 4)?,
                    bits_per_sample: read_u16_le(&bytes, body + 14)?,
                });
            }
            b"data" => {
                data_range = Some((body, len));
            }
            _ => {}
        }

        cursor = end + (len & 1); // skip padding byte for odd-length chunks
    }

    let fmt = fmt.ok_or_else(|| "missing fmt chunk".to_string())?;
    let (data_start, data_len) = data_range.ok_or_else(|| "missing data chunk".to_string())?;
    if fmt.channels == 0 || fmt.sample_rate == 0 {
        return Err("invalid channel count or sample rate".to_string());
    }

    let bytes_per_sample = match fmt.bits_per_sample {
        8 => 1usize,
        16 => 2,
        24 => 3,
        32 => 4,
        bits => return Err(format!("unsupported WAV bit depth: {bits}")),
    };
    let bytes_per_frame = fmt.channels * bytes_per_sample;
    if bytes_per_frame == 0 || data_len < bytes_per_frame {
        return Err("empty WAV data".to_string());
    }

    let frames = data_len / bytes_per_frame;
    let sample_count = frames * fmt.channels;
    let mut samples = Vec::with_capacity(sample_count);

    let mut offset = data_start;
    for _ in 0..sample_count {
        let value = match (fmt.audio_format, fmt.bits_per_sample) {
            // PCM integer
            (1, 8) => (bytes[offset] as f32 - 128.0) / 128.0,
            (1, 16) => read_i16_le(&bytes, offset)? as f32 / 32_768.0,
            (1, 24) => read_i24_le(&bytes, offset)? as f32 / 8_388_608.0,
            (1, 32) => read_i32_le(&bytes, offset)? as f32 / 2_147_483_648.0,
            // IEEE float
            (3, 32) => f32::from_le_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ]),
            (format, _) => return Err(format!("unsupported WAV format code: {format}")),
        };
        samples.push(value.clamp(-1.0, 1.0));
        offset += bytes_per_sample;
    }

    Ok(AudioFileBuffer {
        sample_rate: fmt.sample_rate,
        channels: fmt.channels,
        frames,
        samples,
    })
}

fn read_wav_header(file: &mut File) -> Result<(WavFmt, u64, u64), String> {
    let mut header = [0u8; 12];
    file.read_exact(&mut header)
        .map_err(|e| format!("read WAV header failed: {e}"))?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".to_string());
    }

    let mut fmt: Option<WavFmt> = None;
    let mut data_range: Option<(u64, u64)> = None;
    let mut cursor = 12u64;

    loop {
        let mut chunk_header = [0u8; 8];
        match file.read_exact(&mut chunk_header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("read WAV chunk header failed: {e}")),
        }
        let id = &chunk_header[0..4];
        let len = u32::from_le_bytes([
            chunk_header[4],
            chunk_header[5],
            chunk_header[6],
            chunk_header[7],
        ]) as u64;
        let body = cursor + 8;

        match id {
            b"fmt " => {
                let mut buf = vec![0u8; len as usize];
                file.read_exact(&mut buf)
                    .map_err(|e| format!("read fmt chunk failed: {e}"))?;
                if buf.len() < 16 {
                    return Err("invalid fmt chunk".to_string());
                }
                fmt = Some(WavFmt {
                    audio_format: u16::from_le_bytes([buf[0], buf[1]]),
                    channels: u16::from_le_bytes([buf[2], buf[3]]) as usize,
                    sample_rate: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
                    bits_per_sample: u16::from_le_bytes([buf[14], buf[15]]),
                });
            }
            b"data" => {
                data_range = Some((body, len));
                break;
            }
            _ => {
                file.seek(SeekFrom::Current(len as i64))
                    .map_err(|e| format!("skip WAV chunk failed: {e}"))?;
            }
        }

        if len % 2 == 1 {
            file.seek(SeekFrom::Current(1))
                .map_err(|e| format!("skip WAV padding failed: {e}"))?;
        }
        cursor = body + len + (len % 2);
    }

    let fmt = fmt.ok_or_else(|| "missing fmt chunk".to_string())?;
    let (data_start, data_len) = data_range.ok_or_else(|| "missing data chunk".to_string())?;
    if fmt.channels == 0 || fmt.sample_rate == 0 {
        return Err("invalid channel count or sample rate".to_string());
    }
    Ok((fmt, data_start, data_len))
}

// ── Byte-level helpers ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct WavFmt {
    audio_format: u16,
    channels: usize,
    sample_rate: u32,
    bits_per_sample: u16,
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let b = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "unexpected EOF reading u16".to_string())?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let b = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "unexpected EOF reading u32".to_string())?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_i16_le(bytes: &[u8], offset: usize) -> Result<i16, String> {
    let b = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "unexpected EOF reading i16".to_string())?;
    Ok(i16::from_le_bytes([b[0], b[1]]))
}

fn read_i24_le(bytes: &[u8], offset: usize) -> Result<i32, String> {
    let b = bytes
        .get(offset..offset + 3)
        .ok_or_else(|| "unexpected EOF reading i24".to_string())?;
    let raw = ((b[2] as i32) << 16) | ((b[1] as i32) << 8) | b[0] as i32;
    Ok((raw << 8) >> 8)
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Result<i32, String> {
    let b = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "unexpected EOF reading i32".to_string())?;
    Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_wav_pcm_sample(bytes: &[u8], offset: usize, bits_per_sample: u16) -> Result<f32, String> {
    match bits_per_sample {
        16 => Ok(read_i16_le(bytes, offset)? as f32 / 32_768.0),
        24 => Ok(read_i24_le(bytes, offset)? as f32 / 8_388_608.0),
        32 => Ok(read_i32_le(bytes, offset)? as f32 / 2_147_483_648.0),
        bits => Err(format!("unsupported WAV bit depth: {bits}")),
    }
}

fn reset_peak_min_max(min: &mut [f32], max: &mut [f32]) {
    for i in 0..min.len() {
        min[i] = 1.0;
        max[i] = -1.0;
    }
}

fn write_i16_peak_i32(
    peaks: &mut [i32],
    peak_index: usize,
    channels: usize,
    min: &[f32],
    max: &[f32],
) {
    for ch in 0..channels {
        let base = (peak_index * channels + ch) * 2;
        peaks[base] = clamp_i16_as_i32(min[ch]);
        peaks[base + 1] = clamp_i16_as_i32(max[ch]);
    }
}

fn clamp_i16_as_i32(value: f32) -> i32 {
    (value.clamp(-1.0, 1.0) * 32767.0)
        .round()
        .clamp(-32768.0, 32767.0) as i32
}
