mod spectral_stub;

use crate::device::InferDevice;
use crate::error::StemExtractError;
use crate::model::StemModel;
use crate::params::StemExtractParams;
use crate::progress::{StemExtractCancelToken, StemExtractProgress};
use crate::stems::StemKind;

pub use spectral_stub::SpectralStubBackend;

/// Which concrete inference backend will run the job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferBackendKind {
    /// Deterministic offline spectral split used until ONNX MDX-NET weights land.
    SpectralStub,
}

impl InferBackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SpectralStub => "spectral-stub",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SpectralStub => "MDX-NET spectral stub",
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
/// Today this always returns the spectral stub. When ONNX weights are present
/// this factory will select the real MDX-NET runtime for CPU/GPU.
pub fn create_mdx_net_backend(
    model: StemModel,
    device: InferDevice,
) -> Result<Box<dyn StemInferBackend>, StemExtractError> {
    Ok(Box::new(SpectralStubBackend::new(model, device)))
}
