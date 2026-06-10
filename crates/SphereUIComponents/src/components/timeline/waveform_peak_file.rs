//! On-disk waveform peak cache (`FBPEAKS1`) stored under
//! `<project>/Cache/Waveforms/<asset_id>.peaks`.

use std::path::Path;

use super::waveform_cache::{WaveformLod, WaveformPeak, WaveformPreview, WAVEFORM_ALGORITHM_VERSION};

pub const PEAK_FILE_MAGIC: &[u8; 8] = b"FBPEAKS1";
pub const PEAK_FILE_VERSION: u32 = 1;

#[derive(Debug)]
pub enum PeakFileError {
    Io(std::io::Error),
    Truncated { field: &'static str },
    BadMagic,
    UnsupportedVersion(u32),
    AlgorithmMismatch { expected: u32, found: u32 },
    AssetMismatch { expected: String, found: String },
    InvalidPeakCount,
}

impl std::fmt::Display for PeakFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Truncated { field } => write!(f, "truncated peak file at {field}"),
            Self::BadMagic => write!(f, "invalid peak file magic"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported peak file version {v}"),
            Self::AlgorithmMismatch { expected, found } => {
                write!(f, "peak algorithm mismatch expected={expected} found={found}")
            }
            Self::AssetMismatch { expected, found } => {
                write!(f, "peak asset mismatch expected={expected} found={found}")
            }
            Self::InvalidPeakCount => write!(f, "invalid peak count in peak file"),
        }
    }
}

impl From<std::io::Error> for PeakFileError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Project-relative path for an asset's peak cache file.
pub fn waveform_peak_relative_path_for_asset(asset_id: &str) -> String {
    let safe = asset_id.replace('/', "__").replace('\\', "__");
    format!("Cache/Waveforms/{safe}.peaks")
}

/// Write peaks to `path`, creating parent directories as needed.
pub fn write_peak_file(
    path: &Path,
    asset_id: &str,
    preview: &WaveformPreview,
) -> Result<usize, PeakFileError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(PEAK_FILE_MAGIC);
    bytes.extend_from_slice(&PEAK_FILE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&WAVEFORM_ALGORITHM_VERSION.to_le_bytes());
    let asset_bytes = asset_id.as_bytes();
    if asset_bytes.len() > u32::MAX as usize {
        return Err(PeakFileError::InvalidPeakCount);
    }
    bytes.extend_from_slice(&(asset_bytes.len() as u32).to_le_bytes());
    bytes.extend_from_slice(asset_bytes);
    bytes.extend_from_slice(&preview.sample_rate.to_le_bytes());
    bytes.extend_from_slice(&preview.channels.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&preview.total_frames.to_le_bytes());
    bytes.extend_from_slice(&preview.duration_seconds.to_le_bytes());
    bytes.extend_from_slice(&(preview.lods.len() as u32).to_le_bytes());
    for lod in &preview.lods {
        if lod.peaks.len() > u32::MAX as usize {
            return Err(PeakFileError::InvalidPeakCount);
        }
        bytes.extend_from_slice(&(lod.samples_per_peak as u32).to_le_bytes());
        bytes.extend_from_slice(&(lod.peaks.len() as u32).to_le_bytes());
        for peak in &lod.peaks {
            bytes.extend_from_slice(&peak.min.to_le_bytes());
            bytes.extend_from_slice(&peak.max.to_le_bytes());
        }
    }
    let size = bytes.len();
    std::fs::write(path, &bytes)?;
    Ok(size)
}

/// Load and validate a peak file. When `expected_asset_id` is set, the stored
/// asset id must match.
pub fn read_peak_file(
    path: &Path,
    expected_asset_id: Option<&str>,
) -> Result<WaveformPreview, PeakFileError> {
    let bytes = std::fs::read(path)?;
    read_peak_bytes(&bytes, expected_asset_id)
}

fn read_peak_bytes(
    bytes: &[u8],
    expected_asset_id: Option<&str>,
) -> Result<WaveformPreview, PeakFileError> {
    let mut cursor = bytes;
    let mut take = |n: usize, field: &'static str| -> Result<&[u8], PeakFileError> {
        if cursor.len() < n {
            return Err(PeakFileError::Truncated { field });
        }
        let (head, tail) = cursor.split_at(n);
        cursor = tail;
        Ok(head)
    };

    let magic = take(8, "magic")?;
    if magic != PEAK_FILE_MAGIC {
        return Err(PeakFileError::BadMagic);
    }
    let version = u32::from_le_bytes(take(4, "version")?.try_into().unwrap());
    if version != PEAK_FILE_VERSION {
        return Err(PeakFileError::UnsupportedVersion(version));
    }
    let algo = u32::from_le_bytes(take(4, "algorithm")?.try_into().unwrap());
    if algo != WAVEFORM_ALGORITHM_VERSION {
        return Err(PeakFileError::AlgorithmMismatch {
            expected: WAVEFORM_ALGORITHM_VERSION,
            found: algo,
        });
    }
    let asset_len = u32::from_le_bytes(take(4, "asset_id_len")?.try_into().unwrap()) as usize;
    let asset_raw = take(asset_len, "asset_id")?;
    let asset_id = std::str::from_utf8(asset_raw)
        .map_err(|_| PeakFileError::Truncated { field: "asset_id_utf8" })?
        .to_string();
    if let Some(expected) = expected_asset_id {
        if asset_id != expected {
            return Err(PeakFileError::AssetMismatch {
                expected: expected.to_string(),
                found: asset_id,
            });
        }
    }
    let sample_rate = u32::from_le_bytes(take(4, "sample_rate")?.try_into().unwrap());
    let channels = u16::from_le_bytes(take(2, "channels")?.try_into().unwrap());
    let _pad = u16::from_le_bytes(take(2, "pad")?.try_into().unwrap());
    let total_frames = u64::from_le_bytes(take(8, "total_frames")?.try_into().unwrap());
    let duration_seconds = f64::from_le_bytes(take(8, "duration_seconds")?.try_into().unwrap());
    let lod_count = u32::from_le_bytes(take(4, "lod_count")?.try_into().unwrap()) as usize;

    let mut lods = Vec::with_capacity(lod_count);
    for _ in 0..lod_count {
        let samples_per_peak =
            u32::from_le_bytes(take(4, "samples_per_peak")?.try_into().unwrap()) as usize;
        let peak_count =
            u32::from_le_bytes(take(4, "peak_count")?.try_into().unwrap()) as usize;
        let mut peaks = Vec::with_capacity(peak_count);
        for _ in 0..peak_count {
            let min = f32::from_le_bytes(take(4, "peak_min")?.try_into().unwrap());
            let max = f32::from_le_bytes(take(4, "peak_max")?.try_into().unwrap());
            peaks.push(WaveformPeak { min, max });
        }
        lods.push(WaveformLod {
            samples_per_peak,
            peaks,
        });
    }

    Ok(WaveformPreview {
        sample_rate,
        channels,
        duration_seconds,
        total_frames,
        lods,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "fb_peak_{label}_{}_{}",
            std::process::id(),
            nanos
        ))
    }

    fn sample_preview() -> WaveformPreview {
        WaveformPreview {
            sample_rate: 48_000,
            channels: 2,
            duration_seconds: 1.0,
            total_frames: 48_000,
            lods: vec![WaveformLod {
                samples_per_peak: 256,
                peaks: vec![
                    WaveformPeak { min: -0.5, max: 0.5 },
                    WaveformPeak { min: -0.25, max: 0.75 },
                ],
            }],
        }
    }

    #[test]
    fn peak_file_roundtrip() {
        let dir = temp_dir("roundtrip");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.peaks");
        let preview = sample_preview();
        let asset_id = "Assets/Audio/loop.wav";
        let bytes = write_peak_file(&path, asset_id, &preview).unwrap();
        assert!(bytes > 64);
        let loaded = read_peak_file(&path, Some(asset_id)).unwrap();
        assert_eq!(loaded.sample_rate, preview.sample_rate);
        assert_eq!(loaded.lods[0].peaks.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn peak_file_rejects_bad_magic() {
        let err = read_peak_bytes(b"NOTPEAK1", Some("a")).unwrap_err();
        assert!(matches!(err, PeakFileError::BadMagic));
    }

    #[test]
    fn peak_relative_path_sanitizes_slashes() {
        assert_eq!(
            waveform_peak_relative_path_for_asset("Assets/Audio/kick.wav"),
            "Cache/Waveforms/Assets__Audio__kick.wav.peaks"
        );
    }
}
