//! Offline MDX-NET / UVR model package download.
//!
//! Classification: scanner/offline path (worker thread only). Downloads ONNX
//! weights into `Documents/Futureboard Studio/Utilities/Models/`.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::StemExtractError;
use crate::model::{StemModel, StemModelPackage};
use crate::progress::StemExtractCancelToken;

/// Public UVR model release hosting MDX-NET ONNX weights.
pub const UVR_MODEL_RELEASE_BASE: &str =
    "https://github.com/TRvlvr/model_repo/releases/download/all_public_uvr_models";

/// Progress event while downloading one or more ONNX files for a model.
#[derive(Clone, Debug)]
pub struct StemModelDownloadProgress {
    pub model: StemModel,
    pub file_name: String,
    pub file_index: usize,
    pub file_count: usize,
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub percent: f32,
    pub detail: String,
}

impl StemModelDownloadProgress {
    pub fn new(
        model: StemModel,
        file_name: impl Into<String>,
        file_index: usize,
        file_count: usize,
        bytes_downloaded: u64,
        bytes_total: Option<u64>,
        percent: f32,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            model,
            file_name: file_name.into(),
            file_index,
            file_count,
            bytes_downloaded,
            bytes_total,
            percent: percent.clamp(0.0, 100.0),
            detail: detail.into(),
        }
    }
}

/// Default install directory: `~/Documents/Futureboard Studio/Utilities/Models/`.
pub fn default_models_dir() -> PathBuf {
    let doc = dirs::document_dir().unwrap_or_else(|| PathBuf::from("."));
    doc.join("Futureboard Studio")
        .join("Utilities")
        .join("Models")
}

/// Ensure the models directory exists.
pub fn ensure_models_dir(models_dir: &Path) -> Result<(), StemExtractError> {
    fs::create_dir_all(models_dir).map_err(|e| {
        StemExtractError::Backend(format!(
            "could not create model folder {}: {e}",
            models_dir.display()
        ))
    })
}

/// True when every ONNX file for `model` is present under `models_dir`.
pub fn model_installed(model: StemModel, models_dir: &Path) -> bool {
    let package = model.package();
    package.files.iter().all(|file| {
        let path = models_dir.join(file.file_name);
        path.is_file()
            && fs::metadata(&path)
                .map(|meta| meta.len() > 1_024)
                .unwrap_or(false)
    })
}

/// Absolute paths for installed model files, or `None` if incomplete.
pub fn resolve_installed_model_files(
    model: StemModel,
    models_dir: &Path,
) -> Option<Vec<PathBuf>> {
    if !model_installed(model, models_dir) {
        return None;
    }
    Some(
        model
            .package()
            .files
            .iter()
            .map(|file| models_dir.join(file.file_name))
            .collect(),
    )
}

fn download_url(file_name: &str) -> String {
    format!("{UVR_MODEL_RELEASE_BASE}/{file_name}")
}

/// Download every ONNX file for `model` into `models_dir`.
///
/// Existing complete installs are a no-op. Partial files are written to
/// `*.partial` then atomically renamed on success.
pub fn download_model(
    model: StemModel,
    models_dir: &Path,
    cancel: &StemExtractCancelToken,
    on_progress: &mut dyn FnMut(StemModelDownloadProgress),
) -> Result<(), StemExtractError> {
    ensure_models_dir(models_dir)?;
    let package = model.package();
    if model_installed(model, models_dir) {
        on_progress(StemModelDownloadProgress::new(
            model,
            package.files.first().map(|f| f.file_name).unwrap_or("model"),
            package.file_count().saturating_sub(1),
            package.file_count().max(1),
            package.approx_bytes,
            Some(package.approx_bytes),
            100.0,
            format!("{} already installed", model.label()),
        ));
        return Ok(());
    }

    let file_count = package.file_count().max(1);
    for (file_index, file) in package.files.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(StemExtractError::Cancelled);
        }
        let dest = models_dir.join(file.file_name);
        if dest.is_file()
            && fs::metadata(&dest)
                .map(|meta| meta.len() > 1_024)
                .unwrap_or(false)
        {
            let base = (file_index as f32 / file_count as f32) * 100.0;
            on_progress(StemModelDownloadProgress::new(
                model,
                file.file_name,
                file_index,
                file_count,
                0,
                None,
                base,
                format!("Skipping existing {}", file.file_name),
            ));
            continue;
        }
        download_one_file(
            model,
            package,
            file.file_name,
            file_index,
            file_count,
            &dest,
            cancel,
            on_progress,
        )?;
    }

    if !model_installed(model, models_dir) {
        return Err(StemExtractError::Backend(format!(
            "download finished but {} is still incomplete",
            model.label()
        )));
    }
    on_progress(StemModelDownloadProgress::new(
        model,
        package
            .files
            .last()
            .map(|f| f.file_name)
            .unwrap_or("model"),
        file_count.saturating_sub(1),
        file_count,
        package.approx_bytes,
        Some(package.approx_bytes),
        100.0,
        format!("{} installed", model.label()),
    ));
    Ok(())
}

fn download_one_file(
    model: StemModel,
    package: StemModelPackage,
    file_name: &str,
    file_index: usize,
    file_count: usize,
    dest: &Path,
    cancel: &StemExtractCancelToken,
    on_progress: &mut dyn FnMut(StemModelDownloadProgress),
) -> Result<(), StemExtractError> {
    let url = download_url(file_name);
    let partial = dest.with_extension("onnx.partial");
    let _ = fs::remove_file(&partial);

    on_progress(StemModelDownloadProgress::new(
        model,
        file_name,
        file_index,
        file_count,
        0,
        None,
        (file_index as f32 / file_count as f32) * 100.0,
        format!("Connecting to {}", package.source_label),
    ));

    let response = ureq::get(&url).call().map_err(|e| {
        StemExtractError::Backend(format!("download failed for {file_name}: {e}"))
    })?;

    let content_length = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|n| *n > 0);

    let mut reader = response.into_body().into_reader();
    let mut file = File::create(&partial).map_err(|e| {
        StemExtractError::Backend(format!("could not create {}: {e}", partial.display()))
    })?;

    let mut buf = [0u8; 64 * 1024];
    let mut downloaded = 0u64;
    let file_span = 100.0 / file_count as f32;
    let file_base = file_index as f32 * file_span;

    loop {
        if cancel.is_cancelled() {
            let _ = fs::remove_file(&partial);
            return Err(StemExtractError::Cancelled);
        }
        let n = reader.read(&mut buf).map_err(|e| {
            StemExtractError::Backend(format!("download read failed for {file_name}: {e}"))
        })?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| {
            StemExtractError::Backend(format!("download write failed for {file_name}: {e}"))
        })?;
        downloaded += n as u64;

        let within = match content_length {
            Some(total) if total > 0 => (downloaded as f32 / total as f32).clamp(0.0, 1.0),
            _ => ((downloaded as f32) / (package.approx_bytes.max(1) as f32 / file_count as f32))
                .clamp(0.0, 0.95),
        };
        let percent = file_base + within * file_span;
        on_progress(StemModelDownloadProgress::new(
            model,
            file_name,
            file_index,
            file_count,
            downloaded,
            content_length,
            percent,
            format!("Downloading {file_name}"),
        ));
    }

    file.flush().map_err(|e| {
        StemExtractError::Backend(format!("download flush failed for {file_name}: {e}"))
    })?;
    drop(file);

    if downloaded < 1_024 {
        let _ = fs::remove_file(&partial);
        return Err(StemExtractError::Backend(format!(
            "download for {file_name} was too small ({downloaded} bytes)"
        )));
    }

    fs::rename(&partial, dest).map_err(|e| {
        StemExtractError::Backend(format!(
            "could not finalize {}: {e}",
            dest.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_requires_every_package_file() {
        let dir = std::env::temp_dir().join(format!(
            "fb-stem-models-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        assert!(!model_installed(StemModel::MdxNetKaraoke, &dir));
        let path = dir.join("UVR_MDXNET_KARA.onnx");
        fs::write(&path, vec![0u8; 2048]).unwrap();
        assert!(model_installed(StemModel::MdxNetKaraoke, &dir));
        assert!(!model_installed(StemModel::MdxNet, &dir));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn default_models_dir_ends_with_utilities_models() {
        let dir = default_models_dir();
        assert!(dir.ends_with(Path::new("Utilities").join("Models")));
    }
}
