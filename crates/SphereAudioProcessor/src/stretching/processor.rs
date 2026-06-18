use super::error::StretchError;
use super::params::StretchParams;

pub trait StretchProcessor {
    fn reset(&mut self);
    fn set_params(&mut self, params: StretchParams);
    fn latency_samples(&self) -> usize;

    fn process_stereo(
        &mut self,
        input_l: &[f32],
        input_r: &[f32],
        output_l: &mut [f32],
        output_r: &mut [f32],
    ) -> Result<(), StretchError>;
}
