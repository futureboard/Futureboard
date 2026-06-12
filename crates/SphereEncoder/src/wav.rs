use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

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
    if samples.len() % config.channels as usize != 0 {
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
