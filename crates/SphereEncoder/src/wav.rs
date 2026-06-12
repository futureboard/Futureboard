use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::format::{
    AudioEncodeSpec, AudioEncodeSummary, AudioEncoder, AudioFileFormat, AudioSampleFormat,
    EncodeError, WavEncodeOptions, check_interleaved_len, f32_to_i16, f32_to_i24, f32_to_i32,
    i32_to_f32,
};
use crate::rauf::{RAUF_FLAG_FINALIZED, RaufError, RaufReader, RaufSampleFormat};

pub type Result<T> = std::result::Result<T, WavError>;

#[derive(Debug)]
pub enum WavError {
    Io(std::io::Error),
    Rauf(RaufError),
    UnsupportedFormat(String),
    TooLarge(String),
}

impl std::fmt::Display for WavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Rauf(error) => write!(f, "{error}"),
            Self::UnsupportedFormat(message) => write!(f, "unsupported WAV conversion: {message}"),
            Self::TooLarge(message) => write!(f, "WAV file too large: {message}"),
        }
    }
}

impl std::error::Error for WavError {}

impl From<std::io::Error> for WavError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<RaufError> for WavError {
    fn from(value: RaufError) -> Self {
        Self::Rauf(value)
    }
}

#[derive(Debug, Clone)]
pub struct WavPcmConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
}

#[derive(Debug, Clone)]
pub struct WavReport {
    pub frames_written: u64,
    pub data_bytes: u64,
}

pub fn write_s32le_wav(
    path: impl AsRef<Path>,
    config: WavPcmConfig,
    samples: &[i32],
) -> Result<WavReport> {
    validate_s32_config(&config)?;
    if !samples.len().is_multiple_of(config.channels as usize) {
        return Err(WavError::UnsupportedFormat(format!(
            "sample count {} is not divisible by channels {}",
            samples.len(),
            config.channels
        )));
    }
    let data_bytes = (samples.len() as u64)
        .checked_mul(4)
        .ok_or_else(|| WavError::TooLarge("sample byte count overflow".to_string()))?;
    let mut file = File::create(path)?;
    write_pcm_header(&mut file, &config, data_bytes)?;
    for sample in samples {
        file.write_all(&sample.to_le_bytes())?;
    }
    file.flush()?;
    Ok(WavReport {
        frames_written: samples.len() as u64 / config.channels as u64,
        data_bytes,
    })
}

pub fn convert_rauf_to_wav(
    rauf_path: impl AsRef<Path>,
    wav_path: impl AsRef<Path>,
) -> Result<WavReport> {
    let rauf_path = rauf_path.as_ref();
    let reader = RaufReader::open(rauf_path)?;
    let header = reader.header().clone();
    if header.sample_format != RaufSampleFormat::S32 {
        return Err(WavError::UnsupportedFormat(
            "only RAUF s32le can be exported to WAV in this path".to_string(),
        ));
    }
    if !header.interleaved {
        return Err(WavError::UnsupportedFormat(
            "non-interleaved RAUF is not supported".to_string(),
        ));
    }

    let frames = if header.flags & RAUF_FLAG_FINALIZED != 0 {
        header.frames_written
    } else {
        reader.recover_frames_from_size()?
    };
    let data_bytes = frames
        .checked_mul(header.channels as u64)
        .and_then(|samples| samples.checked_mul(4))
        .ok_or_else(|| WavError::TooLarge("RAUF data byte count overflow".to_string()))?;

    let config = WavPcmConfig {
        sample_rate: header.sample_rate,
        channels: header.channels,
        bits_per_sample: 32,
    };
    validate_s32_config(&config)?;

    let mut input = File::open(rauf_path)?;
    input.seek(SeekFrom::Start(header.data_offset))?;
    let mut output = File::create(wav_path)?;
    write_pcm_header(&mut output, &config, data_bytes)?;
    copy_exact_bytes(&mut input, &mut output, data_bytes)?;
    output.flush()?;
    output.sync_all()?;

    Ok(WavReport {
        frames_written: frames,
        data_bytes,
    })
}

fn validate_s32_config(config: &WavPcmConfig) -> Result<()> {
    if config.sample_rate == 0 {
        return Err(WavError::UnsupportedFormat(
            "sample_rate must be non-zero".to_string(),
        ));
    }
    if config.channels == 0 {
        return Err(WavError::UnsupportedFormat(
            "channels must be non-zero".to_string(),
        ));
    }
    if config.bits_per_sample != 32 {
        return Err(WavError::UnsupportedFormat(format!(
            "expected 32-bit PCM, got {}",
            config.bits_per_sample
        )));
    }
    Ok(())
}

fn write_pcm_header(file: &mut File, config: &WavPcmConfig, data_bytes: u64) -> Result<()> {
    if data_bytes > u32::MAX as u64 {
        return Err(WavError::TooLarge(format!(
            "data chunk is {data_bytes} bytes; RIFF/WAVE v1 limit is {}",
            u32::MAX
        )));
    }
    let data_size = data_bytes as u32;
    let riff_size = data_size
        .checked_add(36)
        .ok_or_else(|| WavError::TooLarge("RIFF size overflow".to_string()))?;
    let block_align = config.channels * 4;
    let byte_rate = config.sample_rate * block_align as u32;

    file.write_all(b"RIFF")?;
    file.write_all(&riff_size.to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&config.channels.to_le_bytes())?;
    file.write_all(&config.sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&config.bits_per_sample.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;
    Ok(())
}

fn copy_exact_bytes(input: &mut File, output: &mut File, bytes: u64) -> Result<()> {
    let mut remaining = bytes;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let wanted = buffer.len().min(remaining as usize);
        input.read_exact(&mut buffer[..wanted])?;
        output.write_all(&buffer[..wanted])?;
        remaining -= wanted as u64;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Streaming WAV encoder implementing the generic AudioEncoder trait.
// ---------------------------------------------------------------------------

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

/// Streaming RIFF/WAVE encoder supporting PCM 16/24/32-bit and Float32.
///
/// The header is written up-front with placeholder sizes and patched on
/// [`finalize`](AudioEncoder::finalize) by seeking back. Samples are streamed
/// straight through a `BufWriter`, so memory use stays bounded regardless of
/// render length.
pub struct WavEncoder {
    writer: BufWriter<File>,
    spec: AudioEncodeSpec,
    data_bytes: u64,
    finalized: bool,
    scratch: Vec<u8>,
}

impl WavEncoder {
    pub fn create(
        path: impl AsRef<Path>,
        spec: AudioEncodeSpec,
        _options: WavEncodeOptions,
    ) -> std::result::Result<Self, EncodeError> {
        spec.validate()?;
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        write_stream_header_placeholder(&mut writer, &spec)?;
        Ok(Self {
            writer,
            spec,
            data_bytes: 0,
            finalized: false,
            scratch: Vec::new(),
        })
    }

    fn push_sample_bytes(
        &mut self,
        encode: impl Fn(&mut Vec<u8>),
    ) -> std::result::Result<(), EncodeError> {
        self.scratch.clear();
        encode(&mut self.scratch);
        self.writer.write_all(&self.scratch)?;
        self.data_bytes += self.scratch.len() as u64;
        Ok(())
    }
}

fn write_stream_header_placeholder(
    writer: &mut BufWriter<File>,
    spec: &AudioEncodeSpec,
) -> std::result::Result<(), EncodeError> {
    let audio_format = if spec.sample_format.is_float() {
        WAVE_FORMAT_IEEE_FLOAT
    } else {
        WAVE_FORMAT_PCM
    };
    let bits = spec.sample_format.bits();
    let block_align = spec.channels * spec.sample_format.bytes_per_sample() as u16;
    let byte_rate = spec.sample_rate * block_align as u32;

    writer.write_all(b"RIFF")?;
    writer.write_all(&0u32.to_le_bytes())?; // patched at finalize
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    writer.write_all(&16u32.to_le_bytes())?;
    writer.write_all(&audio_format.to_le_bytes())?;
    writer.write_all(&spec.channels.to_le_bytes())?;
    writer.write_all(&spec.sample_rate.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&bits.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&0u32.to_le_bytes())?; // patched at finalize
    Ok(())
}

impl AudioEncoder for WavEncoder {
    fn format(&self) -> AudioFileFormat {
        AudioFileFormat::Wav
    }

    fn spec(&self) -> AudioEncodeSpec {
        self.spec.clone()
    }

    fn write_interleaved_f32(&mut self, frames: &[f32]) -> std::result::Result<(), EncodeError> {
        if self.finalized {
            return Err(EncodeError::InvalidInput(
                "write after finalize".to_string(),
            ));
        }
        check_interleaved_len(frames.len(), self.spec.channels)?;
        let fmt = self.spec.sample_format;
        for &x in frames {
            self.push_sample_bytes(|buf| encode_one(buf, fmt, SampleIn::F32(x)))?;
        }
        Ok(())
    }

    fn write_interleaved_i32(&mut self, frames: &[i32]) -> std::result::Result<(), EncodeError> {
        if self.finalized {
            return Err(EncodeError::InvalidInput(
                "write after finalize".to_string(),
            ));
        }
        check_interleaved_len(frames.len(), self.spec.channels)?;
        let fmt = self.spec.sample_format;
        for &x in frames {
            self.push_sample_bytes(|buf| encode_one(buf, fmt, SampleIn::I32(x)))?;
        }
        Ok(())
    }

    fn finalize(&mut self) -> std::result::Result<AudioEncodeSummary, EncodeError> {
        if self.finalized {
            return Err(EncodeError::FinalizeFailed(
                "encoder already finalized".to_string(),
            ));
        }
        self.finalized = true;
        self.writer.flush()?;

        if self.data_bytes > u32::MAX as u64 {
            return Err(EncodeError::FinalizeFailed(format!(
                "data chunk is {} bytes; RIFF/WAVE v1 limit is {}",
                self.data_bytes,
                u32::MAX
            )));
        }
        let data_size = self.data_bytes as u32;
        let riff_size = data_size
            .checked_add(36)
            .ok_or_else(|| EncodeError::FinalizeFailed("RIFF size overflow".to_string()))?;

        let file = self.writer.get_mut();
        file.seek(SeekFrom::Start(4))?;
        file.write_all(&riff_size.to_le_bytes())?;
        file.seek(SeekFrom::Start(40))?;
        file.write_all(&data_size.to_le_bytes())?;
        file.flush()?;
        file.sync_all()?;

        let bytes_per_frame =
            self.spec.channels as u64 * self.spec.sample_format.bytes_per_sample() as u64;
        let frames_written = self.data_bytes.checked_div(bytes_per_frame).unwrap_or(0);
        Ok(AudioEncodeSummary {
            format: AudioFileFormat::Wav,
            sample_rate: self.spec.sample_rate,
            channels: self.spec.channels,
            sample_format: self.spec.sample_format,
            frames_written,
            bytes_written: self.data_bytes,
        })
    }
}

enum SampleIn {
    F32(f32),
    I32(i32),
}

fn encode_one(buf: &mut Vec<u8>, fmt: AudioSampleFormat, sample: SampleIn) {
    match fmt {
        AudioSampleFormat::F32 => {
            let v = match sample {
                SampleIn::F32(x) => x,
                SampleIn::I32(x) => i32_to_f32(x),
            };
            buf.extend_from_slice(&v.to_le_bytes());
        }
        AudioSampleFormat::I32 => {
            let v = match sample {
                SampleIn::F32(x) => f32_to_i32(x),
                SampleIn::I32(x) => x,
            };
            buf.extend_from_slice(&v.to_le_bytes());
        }
        AudioSampleFormat::I16 => {
            let v = match sample {
                SampleIn::F32(x) => f32_to_i16(x),
                SampleIn::I32(x) => (x >> 16) as i16,
            };
            buf.extend_from_slice(&v.to_le_bytes());
        }
        AudioSampleFormat::I24 => {
            let v = match sample {
                SampleIn::F32(x) => f32_to_i24(x),
                SampleIn::I32(x) => x >> 8,
            };
            let bytes = v.to_le_bytes();
            buf.extend_from_slice(&bytes[0..3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rauf::{RaufConfig, RaufSampleFormat, RaufWriter};

    fn temp_path(name: &str, ext: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "futureboard-{name}-{}-{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            ext
        ));
        path
    }

    #[test]
    fn writes_s32le_wav_header_and_payload() {
        let path = temp_path("s32-wav", "wav");
        let report = write_s32le_wav(
            &path,
            WavPcmConfig {
                sample_rate: 48_000,
                channels: 2,
                bits_per_sample: 32,
            },
            &[1, -1, i32::MAX, i32::MIN],
        )
        .unwrap();
        assert_eq!(report.frames_written, 2);
        assert_eq!(report.data_bytes, 16);

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), 1);
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            48_000
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 32);
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 16);
        assert_eq!(i32::from_le_bytes(bytes[44..48].try_into().unwrap()), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn converts_rauf_to_wav_without_ffmpeg() {
        let rauf_path = temp_path("convert", "rauf");
        let wav_path = temp_path("convert", "wav");
        let mut writer = RaufWriter::create(
            &rauf_path,
            RaufConfig {
                sample_rate: 44_100,
                channels: 2,
                sample_format: RaufSampleFormat::S32,
                interleaved: true,
                project_start_sample: 128,
                take_id: [9; 16],
            },
        )
        .unwrap();
        writer
            .write_s32le_interleaved(&[10, 20, 30, 40, 50, 60])
            .unwrap();
        writer.finalize().unwrap();

        let report = convert_rauf_to_wav(&rauf_path, &wav_path).unwrap();
        assert_eq!(report.frames_written, 3);
        assert_eq!(report.data_bytes, 24);

        let bytes = std::fs::read(&wav_path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            44_100
        );
        assert_eq!(i32::from_le_bytes(bytes[44..48].try_into().unwrap()), 10);
        assert_eq!(i32::from_le_bytes(bytes[64..68].try_into().unwrap()), 60);

        let _ = std::fs::remove_file(rauf_path);
        let _ = std::fs::remove_file(wav_path);
    }
}

#[cfg(test)]
mod encoder_tests {
    use super::*;
    use crate::format::AudioEncodeSpec;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "futureboard-{name}-{}-{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }

    #[test]
    fn float32_wav_streams_header_and_payload() {
        let path = temp_path("f32");
        let mut enc = WavEncoder::create(
            &path,
            AudioEncodeSpec {
                sample_rate: 48_000,
                channels: 2,
                sample_format: AudioSampleFormat::F32,
            },
            WavEncodeOptions::default(),
        )
        .unwrap();
        enc.write_interleaved_f32(&[0.0, 1.0, -1.0, 0.5]).unwrap();
        let summary = enc.finalize().unwrap();
        assert_eq!(summary.frames_written, 2);
        assert_eq!(summary.bytes_written, 16);

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        // IEEE float format code.
        assert_eq!(
            u16::from_le_bytes([bytes[20], bytes[21]]),
            WAVE_FORMAT_IEEE_FLOAT
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 32);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 16);
        assert_eq!(f32::from_le_bytes(bytes[44..48].try_into().unwrap()), 0.0);
        assert_eq!(f32::from_le_bytes(bytes[48..52].try_into().unwrap()), 1.0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pcm16_wav_clamps_and_rounds() {
        let path = temp_path("i16");
        let mut enc = WavEncoder::create(
            &path,
            AudioEncodeSpec {
                sample_rate: 44_100,
                channels: 1,
                sample_format: AudioSampleFormat::I16,
            },
            WavEncodeOptions::default(),
        )
        .unwrap();
        enc.write_interleaved_f32(&[1.0, -1.0, 2.0]).unwrap();
        let summary = enc.finalize().unwrap();
        assert_eq!(summary.frames_written, 3);
        assert_eq!(summary.bytes_written, 6);

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u16::from_le_bytes([bytes[20], bytes[21]]), WAVE_FORMAT_PCM);
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
        assert_eq!(i16::from_le_bytes([bytes[44], bytes[45]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([bytes[46], bytes[47]]), -i16::MAX);
        // 2.0 clamps to full-scale, not wraps.
        assert_eq!(i16::from_le_bytes([bytes[48], bytes[49]]), i16::MAX);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_misaligned_interleaved_input() {
        let path = temp_path("misaligned");
        let mut enc = WavEncoder::create(
            &path,
            AudioEncodeSpec {
                sample_rate: 48_000,
                channels: 2,
                sample_format: AudioSampleFormat::I16,
            },
            WavEncodeOptions::default(),
        )
        .unwrap();
        let err = enc.write_interleaved_f32(&[0.1, 0.2, 0.3]).unwrap_err();
        assert!(matches!(err, EncodeError::InvalidInput(_)));
        let _ = enc.finalize();
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn double_finalize_errors_not_panics() {
        let path = temp_path("double");
        let mut enc = WavEncoder::create(
            &path,
            AudioEncodeSpec {
                sample_rate: 48_000,
                channels: 1,
                sample_format: AudioSampleFormat::I32,
            },
            WavEncodeOptions::default(),
        )
        .unwrap();
        enc.finalize().unwrap();
        assert!(matches!(
            enc.finalize(),
            Err(EncodeError::FinalizeFailed(_))
        ));
        let _ = std::fs::remove_file(path);
    }
}
