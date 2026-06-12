//! FLAC encoding via the pure-Rust `flacenc` crate. No FFmpeg/libav, no native
//! dependencies, no external processes.
//!
//! Limitation: `flacenc` consumes a whole [`MemSource`] and encodes the entire
//! stream in one shot, so this encoder buffers integer samples in memory and
//! emits the file on [`finalize`](AudioEncoder::finalize). For very long
//! arrangement renders this trades memory for a pure-Rust dependency; a true
//! block-streaming FLAC path is a future improvement (the engine can chunk the
//! render across multiple files if memory ever becomes a concern).

use std::path::{Path, PathBuf};

use flacenc::component::BitRepr;
use flacenc::error::Verify;
use flacenc::source::MemSource;

use crate::format::{
    AudioEncodeSpec, AudioEncodeSummary, AudioEncoder, AudioFileFormat, AudioSampleFormat,
    EncodeError, FlacEncodeOptions, check_interleaved_len, f32_to_i16, f32_to_i24,
};
use crate::metadata::AudioMetadata;

/// Supported FLAC sample rates (Futureboard's working rates).
const SUPPORTED_RATES: [u32; 4] = [44_100, 48_000, 88_200, 96_000];

pub struct FlacEncoder {
    path: PathBuf,
    spec: AudioEncodeSpec,
    bits: usize,
    block_size: usize,
    /// Interleaved integer samples at `bits` depth, awaiting finalize.
    samples: Vec<i32>,
    metadata: AudioMetadata,
    finalized: bool,
}

impl FlacEncoder {
    pub fn create(
        path: impl AsRef<Path>,
        spec: AudioEncodeSpec,
        options: FlacEncodeOptions,
        metadata: AudioMetadata,
    ) -> Result<Self, EncodeError> {
        spec.validate()?;
        if !SUPPORTED_RATES.contains(&spec.sample_rate) {
            return Err(EncodeError::UnsupportedSpec(format!(
                "FLAC sample rate {} not supported (expected one of {:?})",
                spec.sample_rate, SUPPORTED_RATES
            )));
        }
        let bits = match spec.sample_format {
            AudioSampleFormat::I16 => 16,
            AudioSampleFormat::I24 => 24,
            AudioSampleFormat::I32 | AudioSampleFormat::F32 => {
                return Err(EncodeError::UnsupportedSpec(
                    "FLAC export supports 16-bit or 24-bit integer depth only".to_string(),
                ));
            }
        };
        let block_size = if options.block_size == 0 {
            4096
        } else {
            options.block_size.clamp(64, 32_768)
        };
        // Validate the path is creatable up front so the UI fails fast.
        let _ = std::fs::File::create(path.as_ref())?;
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            spec,
            bits,
            block_size,
            samples: Vec::new(),
            metadata,
            finalized: false,
        })
    }

    fn ensure_open(&self) -> Result<(), EncodeError> {
        if self.finalized {
            return Err(EncodeError::InvalidInput(
                "write after finalize".to_string(),
            ));
        }
        Ok(())
    }

    fn push_int(&mut self, value: i32) {
        self.samples.push(value);
    }
}

impl AudioEncoder for FlacEncoder {
    fn format(&self) -> AudioFileFormat {
        AudioFileFormat::Flac
    }

    fn spec(&self) -> AudioEncodeSpec {
        self.spec.clone()
    }

    fn write_interleaved_f32(&mut self, frames: &[f32]) -> Result<(), EncodeError> {
        self.ensure_open()?;
        check_interleaved_len(frames.len(), self.spec.channels)?;
        self.samples.reserve(frames.len());
        for &x in frames {
            let v = if self.bits == 16 {
                f32_to_i16(x) as i32
            } else {
                f32_to_i24(x)
            };
            self.push_int(v);
        }
        Ok(())
    }

    fn write_interleaved_i32(&mut self, frames: &[i32]) -> Result<(), EncodeError> {
        self.ensure_open()?;
        check_interleaved_len(frames.len(), self.spec.channels)?;
        self.samples.reserve(frames.len());
        for &x in frames {
            // Input is full-scale signed 32-bit; reduce to target depth.
            let v = if self.bits == 16 { x >> 16 } else { x >> 8 };
            self.push_int(v);
        }
        Ok(())
    }

    fn finalize(&mut self) -> Result<AudioEncodeSummary, EncodeError> {
        if self.finalized {
            return Err(EncodeError::FinalizeFailed(
                "encoder already finalized".to_string(),
            ));
        }
        self.finalized = true;

        if !self.metadata.is_empty() {
            tracing::debug!("FLAC metadata tags are not yet written (TODO Vorbis comments)");
        }

        let channels = self.spec.channels as usize;
        let frames_written = (self.samples.len() / channels.max(1)) as u64;

        let mut config = flacenc::config::Encoder::default();
        config.block_size = self.block_size;
        let config = config.into_verified().map_err(|(_, err)| {
            EncodeError::UnsupportedSpec(format!("invalid FLAC encoder config: {err}"))
        })?;

        let source = MemSource::from_samples(
            &self.samples,
            channels,
            self.bits,
            self.spec.sample_rate as usize,
        );
        let stream = flacenc::encode_with_fixed_block_size(&config, source, self.block_size)
            .map_err(|err| EncodeError::FinalizeFailed(format!("FLAC encode failed: {err:?}")))?;

        let mut sink = flacenc::bitsink::ByteSink::new();
        stream.write(&mut sink).map_err(|err| {
            EncodeError::FinalizeFailed(format!("FLAC serialize failed: {err:?}"))
        })?;
        let bytes = sink.as_slice();
        std::fs::write(&self.path, bytes)?;

        Ok(AudioEncodeSummary {
            format: AudioFileFormat::Flac,
            sample_rate: self.spec.sample_rate,
            channels: self.spec.channels,
            sample_format: self.spec.sample_format,
            frames_written,
            bytes_written: bytes.len() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "futureboard-{name}-{}-{}.flac",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }

    fn spec(rate: u32, ch: u16, fmt: AudioSampleFormat) -> AudioEncodeSpec {
        AudioEncodeSpec {
            sample_rate: rate,
            channels: ch,
            sample_format: fmt,
        }
    }

    #[test]
    fn encodes_stereo_48k_16bit_sine_with_flac_magic() {
        let path = temp_path("sine");
        let mut enc = FlacEncoder::create(
            &path,
            spec(48_000, 2, AudioSampleFormat::I16),
            FlacEncodeOptions::default(),
            AudioMetadata::default(),
        )
        .unwrap();

        // One second of a 440 Hz sine, stereo.
        let mut block = Vec::new();
        for n in 0..48_000u32 {
            let t = n as f32 / 48_000.0;
            let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
            block.push(s);
            block.push(s);
        }
        enc.write_interleaved_f32(&block).unwrap();
        let summary = enc.finalize().unwrap();
        assert_eq!(summary.frames_written, 48_000);
        assert!(summary.bytes_written > 0);

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"fLaC", "missing FLAC stream marker");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_unsupported_sample_format() {
        let path = temp_path("badfmt");
        let result = FlacEncoder::create(
            &path,
            spec(48_000, 2, AudioSampleFormat::F32),
            FlacEncodeOptions::default(),
            AudioMetadata::default(),
        );
        assert!(matches!(result, Err(EncodeError::UnsupportedSpec(_))));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_input_finalizes_without_panic() {
        let path = temp_path("empty");
        let mut enc = FlacEncoder::create(
            &path,
            spec(44_100, 1, AudioSampleFormat::I24),
            FlacEncodeOptions::default(),
            AudioMetadata::default(),
        )
        .unwrap();
        let summary = enc.finalize().unwrap();
        assert_eq!(summary.frames_written, 0);
        let _ = std::fs::remove_file(path);
    }
}
