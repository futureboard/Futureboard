mod spectral_stub;

#[cfg(feature = "onnx")]
mod dsp;
#[cfg(feature = "onnx")]
mod mdx_params;
#[cfg(feature = "onnx")]
mod onnx_htdemucs;
#[cfg(feature = "onnx")]
mod onnx_mdx;

use crate::device::InferDevice;
use crate::error::StemExtractError;
use crate::model::StemModel;
use crate::params::StemExtractParams;
use crate::progress::{StemExtractCancelToken, StemExtractProgress};
use crate::stems::StemKind;

pub use spectral_stub::SpectralStubBackend;

#[cfg(feature = "onnx")]
pub use onnx_htdemucs::OnnxHtDemucsBackend;
#[cfg(feature = "onnx")]
pub use onnx_mdx::OnnxMdxBackend;

/// Which concrete inference backend will run the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferBackendKind {
    /// Real MDX-NET inference via ONNX Runtime (CPU or GPU).
    OnnxMdxNet,
    /// Real HT-Demucs (single-file, time-domain) inference via ONNX Runtime.
    OnnxHtDemucs,
    /// Deterministic offline spectral split used when ONNX weights or the
    /// ONNX Runtime are unavailable.
    SpectralStub,
}

impl InferBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnnxMdxNet => "onnx-mdx-net",
            Self::OnnxHtDemucs => "onnx-htdemucs",
            Self::SpectralStub => "spectral-stub",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OnnxMdxNet => "MDX-NET (ONNX Runtime)",
            Self::OnnxHtDemucs => "HT-Demucs (ONNX Runtime)",
            Self::SpectralStub => "spectral stub",
        }
    }
}

/// One separated stem buffer (interleaved f32, same layout as the input).
#[derive(Clone, Debug)]
pub struct SeparatedStem {
    pub kind: StemKind,
    pub samples: Vec<f32>,
}

pub trait StemInferBackend: Send {
    fn kind(&self) -> InferBackendKind;
    fn model(&self) -> StemModel;
    fn device(&self) -> InferDevice;

    fn separate(
        &self,
        interleaved: &[f32],
        channels: usize,
        sample_rate: u32,
        params: &StemExtractParams,
        cancel: &StemExtractCancelToken,
        on_progress: &mut dyn FnMut(StemExtractProgress),
    ) -> Result<Vec<SeparatedStem>, StemExtractError>;
}

/// Build the MDX-NET backend for the resolved device.
///
/// Prefers the real ONNX Runtime backend when the crate is built with the
/// `onnx` feature and the model's ONNX weights are installed; otherwise falls
/// back to the deterministic spectral stub so the UI/job pipeline still runs.
pub fn create_mdx_net_backend(
    model: StemModel,
    device: InferDevice,
) -> Result<Box<dyn StemInferBackend>, StemExtractError> {
    #[cfg(feature = "onnx")]
    {
        if resolve_ort_dylib().is_none() {
            log::info!(
                "ONNX Runtime library not found next to the app or via ORT_DYLIB_PATH; \
                 using spectral stub for {}",
                model.label()
            );
        } else {
            ensure_ort_dylib_env();
            let models_dir = resolved_models_dir();
            if let Some(files) = crate::download::resolve_installed_model_files(model, &models_dir)
            {
                if model == StemModel::HtDemucs {
                    match OnnxHtDemucsBackend::new(model, device, files) {
                        Ok(backend) => return Ok(Box::new(backend)),
                        Err(err) => {
                            log::warn!(
                                "ONNX HT-Demucs backend unavailable for {} ({err}); \
                                 using spectral stub",
                                model.label()
                            );
                        }
                    }
                } else {
                    match OnnxMdxBackend::new(model, device, files) {
                        Ok(backend) => return Ok(Box::new(backend)),
                        Err(err) => {
                            log::warn!(
                                "ONNX MDX-NET backend unavailable for {} ({err}); \
                                 using spectral stub",
                                model.label()
                            );
                        }
                    }
                }
            } else {
                log::info!(
                    "Weights for {} not installed; using spectral stub",
                    model.label()
                );
            }
        }
    }
    Ok(Box::new(SpectralStubBackend::new(model, device)))
}

/// Directory the ONNX backend looks in for installed weights. Honors
/// `FUTUREBOARD_STEM_MODELS_DIR`, else the default Utilities/Models folder.
#[cfg(feature = "onnx")]
fn resolved_models_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("FUTUREBOARD_STEM_MODELS_DIR") {
        return std::path::PathBuf::from(dir);
    }
    crate::download::default_models_dir()
}

/// Candidate ONNX Runtime library locations shipped next to the app binary.
///
/// Layout by platform (relative to the executable directory):
/// - Windows: `onnxruntime.dll`
/// - Linux:   `libonnxruntime.so`
/// - macOS:   `libonnxruntime.dylib`, then `../Frameworks/libonnxruntime.dylib`
///   (the `.app` bundle `Contents/Frameworks` location).
#[cfg(feature = "onnx")]
fn candidate_ort_dylibs() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            #[cfg(target_os = "windows")]
            out.push(dir.join("onnxruntime.dll"));
            #[cfg(target_os = "macos")]
            {
                out.push(dir.join("libonnxruntime.dylib"));
                out.push(dir.join("../Frameworks/libonnxruntime.dylib"));
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            out.push(dir.join("libonnxruntime.so"));
        }
    }
    out
}

/// Resolve the ONNX Runtime library: an explicit valid `ORT_DYLIB_PATH` wins,
/// otherwise the first existing app-adjacent candidate.
///
/// `ort` is built with `load-dynamic`, so the first API call panics (not a
/// recoverable `Result`) when no `onnxruntime` library can be found — and the
/// workspace builds with `panic = "abort"`. We therefore refuse to touch `ort`
/// unless a library is actually present, keeping the real backend panic-free.
#[cfg(feature = "onnx")]
fn resolve_ort_dylib() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("ORT_DYLIB_PATH") {
        if !path.is_empty() {
            let path = std::path::PathBuf::from(path);
            return path.is_file().then_some(path);
        }
    }
    candidate_ort_dylibs().into_iter().find(|p| p.is_file())
}

/// Point `ort` at the app-adjacent library when `ORT_DYLIB_PATH` is unset, so
/// the runtime resolves reliably on every platform (not just via the OS loader
/// search path).
#[cfg(feature = "onnx")]
fn ensure_ort_dylib_env() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("ORT_DYLIB_PATH").is_some() {
            return;
        }
        if let Some(path) = candidate_ort_dylibs().into_iter().find(|p| p.is_file()) {
            // SAFETY: single `Once`-guarded setter on the offline worker path,
            // executed before the first `ort` API call reads the variable.
            unsafe { std::env::set_var("ORT_DYLIB_PATH", &path) };
            log::info!("ORT_DYLIB_PATH set to {}", path.display());
        }
    });
}
