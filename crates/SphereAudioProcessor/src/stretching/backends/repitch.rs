use crate::stretching::error::StretchError;
use crate::stretching::params::StretchParams;
use crate::stretching::processor::StretchProcessor;
use crate::stretching::ratios::source_read_rate_for_repitch;

pub struct RePitchProcessor {
    _sample_rate: f32,
    channels: usize,
    params: StretchParams,
    project_bpm: Option<f32>,
}

impl RePitchProcessor {
    pub fn new(sample_rate: f32, channels: usize) -> Self {
        Self {
            _sample_rate: sample_rate,
            channels,
            params: StretchParams::default(),
            project_bpm: None,
        }
    }

    fn read_rate(&self) -> f32 {
        source_read_rate_for_repitch(&self.params, self.project_bpm)
    }

    fn linear_interp(channel: &[f32], position: f32) -> f32 {
        if channel.is_empty() {
            return 0.0;
        }
        if position <= 0.0 {
            return channel[0];
        }

        let max_index = channel.len() - 1;
        if position >= max_index as f32 {
            return channel[max_index];
        }

        let index = position.floor() as usize;
        let frac = position - index as f32;
        let next = (index + 1).min(max_index);
        channel[index] + (channel[next] - channel[index]) * frac
    }
}

impl StretchProcessor for RePitchProcessor {
    fn reset(&mut self) {}

    fn set_params(&mut self, params: StretchParams) {
        self.params = params;
    }

    fn latency_samples(&self) -> usize {
        0
    }

    fn process_stereo(
        &mut self,
        input_l: &[f32],
        input_r: &[f32],
        output_l: &mut [f32],
        output_r: &mut [f32],
    ) -> Result<(), StretchError> {
        if input_l.len() != input_r.len()
            || input_l.len() != output_l.len()
            || input_l.len() != output_r.len()
        {
            return Err(StretchError::BufferLengthMismatch);
        }

        let read_rate = self.read_rate();
        let frames = output_l.len();

        if self.channels == 1 {
            for i in 0..frames {
                let source_pos = i as f32 * read_rate;
                let sample = Self::linear_interp(input_l, source_pos);
                output_l[i] = sample;
                output_r[i] = sample;
            }
            return Ok(());
        }

        for i in 0..frames {
            let source_pos = i as f32 * read_rate;
            output_l[i] = Self::linear_interp(input_l, source_pos);
            output_r[i] = Self::linear_interp(input_r, source_pos);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stretching::params::{StretchAlgorithm, StretchMode};

    #[test]
    fn writes_full_output_buffer() {
        let params = StretchParams {
            mode: StretchMode::Manual,
            algorithm: StretchAlgorithm::RePitch,
            time_ratio: 2.0,
            preserve_pitch: false,
            ..StretchParams::default()
        };
        let mut processor = RePitchProcessor::new(48_000.0, 2);
        processor.set_params(params);

        let input_l = [0.0_f32, 0.5, 1.0, 0.5];
        let input_r = [1.0_f32, 0.5, 0.0, 0.5];
        let mut output_l = [f32::NAN; 4];
        let mut output_r = [f32::NAN; 4];

        processor
            .process_stereo(&input_l, &input_r, &mut output_l, &mut output_r)
            .expect("process");

        assert!(output_l.iter().all(|v| v.is_finite()));
        assert!(output_r.iter().all(|v| v.is_finite()));
        assert_eq!(output_l.len(), 4);
        assert_eq!(output_r.len(), 4);
    }
}
