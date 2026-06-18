use super::error::StretchError;
use super::params::StretchParams;

pub trait StretchProcessor {
    fn reset(&mut self);
    fn set_params(&mut self, params: StretchParams);
    fn latency_samples(&self) -> usize;

    /// Render `output_*.len()` output samples from `input_*.len()` source
    /// samples. For a streaming time-stretch backend the input and output
    /// lengths may differ (the time ratio is `output_len / input_len`); the
    /// caller supplies exactly the source samples to consume this block. Within
    /// a side, the left/right slices must be equal length. Backends that cannot
    /// resample (e.g. `RePitchProcessor`) document a stricter equal-length
    /// requirement.
    fn process_stereo(
        &mut self,
        input_l: &[f32],
        input_r: &[f32],
        output_l: &mut [f32],
        output_r: &mut [f32],
    ) -> Result<(), StretchError>;
}
