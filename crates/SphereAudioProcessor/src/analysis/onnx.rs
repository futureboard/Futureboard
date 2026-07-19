//! Learned instrument/voice classifier backend built on `tract-onnx`
//! (pure-Rust ONNX inference, dual MIT / Apache-2.0).
//!
//! Offline / control-thread only. Enabled by the `onnx` crate feature.
//!
//! # Model contract
//!
//! The ONNX model must accept a single `f32` input of shape
//! `[1, FEATURE_VECTOR_LEN]` (see [`SpectralFeatures::to_feature_vector`]) and
//! produce a single `f32` output of shape `[1, 8]` or `[8]`: one score per
//! [`InstrumentCategory`] in canonical order (`0 = Vocal .. 7 = Other`). Scores
//! may be raw logits; confidence is derived from a softmax margin.
//!
//! No model file ships with the crate — the caller supplies one. If inference
//! fails at classify time the backend falls back to the built-in heuristic
//! rather than panicking, so a bad model degrades gracefully.

use std::path::Path;

use tract_onnx::prelude::*;

use super::error::AnalysisError;
use super::features::{FEATURE_VECTOR_LEN, SpectralFeatures};
use super::instrument::{Classifier, HeuristicClassifier, InstrumentCategory, InstrumentEstimate};

type RunnableModel = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

/// Instrument/voice classifier backed by an ONNX model.
pub struct OnnxClassifier {
    model: RunnableModel,
    fallback: HeuristicClassifier,
}

impl OnnxClassifier {
    /// Load a classifier from an ONNX model file. Returns
    /// [`AnalysisError::ModelLoad`] if the file cannot be read, parsed, or
    /// shaped to the `[1, FEATURE_VECTOR_LEN]` input contract.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, AnalysisError> {
        let model = tract_onnx::onnx()
            .model_for_path(path.as_ref())
            .map_err(|e| AnalysisError::ModelLoad(e.to_string()))?
            .with_input_fact(
                0,
                f32::fact([1, FEATURE_VECTOR_LEN]).into(),
            )
            .map_err(|e| AnalysisError::ModelLoad(e.to_string()))?
            .into_optimized()
            .map_err(|e| AnalysisError::ModelLoad(e.to_string()))?
            .into_runnable()
            .map_err(|e| AnalysisError::ModelLoad(e.to_string()))?;

        Ok(Self {
            model,
            fallback: HeuristicClassifier,
        })
    }

    fn run(&self, features: &SpectralFeatures) -> Result<InstrumentEstimate, AnalysisError> {
        let vec = features.to_feature_vector();
        let input = tract_ndarray::Array2::from_shape_vec((1, FEATURE_VECTOR_LEN), vec.to_vec())
            .map_err(|e| AnalysisError::Inference(e.to_string()))?;
        let tensor: Tensor = input.into();

        let outputs = self
            .model
            .run(tvec!(tensor.into()))
            .map_err(|e| AnalysisError::Inference(e.to_string()))?;

        let view = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| AnalysisError::Inference(e.to_string()))?;

        let logits: Vec<f32> = view.iter().copied().collect();
        if logits.len() < InstrumentCategory::ALL.len() {
            return Err(AnalysisError::Inference(format!(
                "model produced {} scores, expected at least {}",
                logits.len(),
                InstrumentCategory::ALL.len()
            )));
        }

        let (index, confidence) = argmax_softmax_margin(&logits[..InstrumentCategory::ALL.len()]);
        Ok(InstrumentEstimate {
            category: InstrumentCategory::from_index(index),
            confidence,
            features: *features,
        })
    }
}

impl Classifier for OnnxClassifier {
    fn classify(&self, features: &SpectralFeatures) -> InstrumentEstimate {
        match self.run(features) {
            Ok(estimate) => estimate,
            Err(err) => {
                log::warn!("ONNX classifier inference failed, using heuristic fallback: {err}");
                self.fallback.classify(features)
            }
        }
    }
}

/// Argmax index plus a confidence taken from the softmax top-vs-second margin.
fn argmax_softmax_margin(logits: &[f32]) -> (usize, f32) {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut probs: Vec<f32> = logits.iter().map(|&l| (l - max).exp()).collect();
    let sum: f32 = probs.iter().sum();
    if sum > 0.0 {
        for p in &mut probs {
            *p /= sum;
        }
    }

    let mut best_idx = 0;
    let mut best = f32::NEG_INFINITY;
    let mut second = f32::NEG_INFINITY;
    for (i, &p) in probs.iter().enumerate() {
        if p > best {
            second = best;
            best = p;
            best_idx = i;
        } else if p > second {
            second = p;
        }
    }

    let confidence = if second.is_finite() {
        (best - second).clamp(0.0, 1.0)
    } else {
        best.clamp(0.0, 1.0)
    };
    (best_idx, confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_reports_load_error() {
        match OnnxClassifier::from_path("does-not-exist.onnx") {
            Ok(_) => panic!("expected a load error for a missing model file"),
            Err(err) => assert!(matches!(err, AnalysisError::ModelLoad(_))),
        }
    }

    #[test]
    fn softmax_margin_prefers_dominant_logit() {
        let (idx, conf) = argmax_softmax_margin(&[0.1, 5.0, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(idx, 1);
        assert!(conf > 0.5, "confidence too low: {conf}");
    }
}
