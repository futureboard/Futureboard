use serde::{Deserialize, Serialize};

use crate::device::InferDevice;
use crate::error::StemExtractError;
use crate::model::StemModel;
use crate::stems::{StemKind, StemSet};

/// Offline quality hint for MDX-NET extraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StemExtractQuality {
    Draft,
    #[default]
    Balanced,
    High,
}

impl StemExtractQuality {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Balanced => "balanced",
            Self::High => "high",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Balanced => "Balanced",
            Self::High => "High",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "balanced" => Some(Self::Balanced),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

/// Serializable stem-extraction settings shared by UI, Audio Processor, and jobs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StemExtractParams {
    pub model: StemModel,
    pub device: InferDevice,
    /// When true and GPU is unavailable, fall back to CPU instead of failing.
    pub allow_cpu_fallback: bool,
    pub stems: StemSet,
    pub quality: StemExtractQuality,
}

impl Default for StemExtractParams {
    fn default() -> Self {
        Self {
            model: StemModel::MdxNet,
            device: InferDevice::Cpu,
            allow_cpu_fallback: true,
            stems: StemSet::default(),
            quality: StemExtractQuality::Balanced,
        }
    }
}

impl StemExtractParams {
    pub fn mdx_net_cpu() -> Self {
        Self::default()
    }

    pub fn mdx_net_gpu() -> Self {
        Self {
            device: InferDevice::Gpu,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), StemExtractError> {
        if self.stems.is_empty() {
            return Err(StemExtractError::NoStemsSelected);
        }
        for stem in self.stems.iter() {
            if !self.model.supports_stem(stem) {
                return Err(StemExtractError::StemNotSupported {
                    model: self.model,
                    stem,
                });
            }
        }
        Ok(())
    }

    pub fn set_stem(&mut self, stem: StemKind, enabled: bool) {
        if self.model.supports_stem(stem) {
            self.stems.toggle(stem, enabled);
        }
    }

    pub fn set_model(&mut self, model: StemModel) {
        self.model = model;
        // Model changes reset the stem checklist to that model's full default
        // set so Karaoke ↔ 4-stem switches stay obvious in the dialog.
        self.stems = StemSet::all_for_model(model);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_to_karaoke_drops_unsupported_stems() {
        let mut params = StemExtractParams::default();
        params.set_model(StemModel::MdxNetKaraoke);
        assert!(params.stems.contains(StemKind::Vocals));
        assert!(params.stems.contains(StemKind::Instrumental));
        assert!(!params.stems.contains(StemKind::Drums));
        params.validate().unwrap();
    }

    #[test]
    fn serde_round_trip_defaults_to_mdx_net_cpu() {
        let json = serde_json::to_string(&StemExtractParams::default()).unwrap();
        let parsed: StemExtractParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, StemModel::MdxNet);
        assert_eq!(parsed.device, InferDevice::Cpu);
    }
}
