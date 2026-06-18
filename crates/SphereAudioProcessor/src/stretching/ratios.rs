use super::params::{StretchMode, StretchParams};

const MIN_RATIO: f32 = 0.01;
const MAX_RATIO: f32 = 100.0;

pub fn effective_time_ratio(params: &StretchParams, project_bpm: Option<f32>) -> f32 {
    if params.mode == StretchMode::Off {
        return 1.0;
    }

    match params.mode {
        StretchMode::Off => 1.0,
        StretchMode::Manual | StretchMode::Warp => sanitize_ratio(params.time_ratio),
        StretchMode::TempoSync => {
            let Some(source_bpm) = params.source_bpm.filter(|v| valid_positive(*v)) else {
                return 1.0;
            };
            let target_bpm = params
                .target_bpm
                .or(project_bpm)
                .filter(|v| valid_positive(*v))
                .unwrap_or(source_bpm);
            sanitize_ratio(source_bpm / target_bpm)
        }
    }
}

pub fn effective_pitch_ratio(params: &StretchParams) -> f32 {
    sanitize_ratio(params.pitch_ratio)
}

pub fn source_read_rate_for_repitch(params: &StretchParams, project_bpm: Option<f32>) -> f32 {
    let time_ratio = effective_time_ratio(params, project_bpm);
    sanitize_ratio(1.0 / time_ratio)
}

pub fn stretched_duration_samples(
    source_len: u64,
    params: &StretchParams,
    project_bpm: Option<f32>,
) -> u64 {
    if source_len == 0 {
        return 0;
    }

    let duration = source_len as f64 * effective_time_ratio(params, project_bpm) as f64;
    duration.round().clamp(0.0, u64::MAX as f64) as u64
}

fn valid_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn sanitize_ratio(value: f32) -> f32 {
    if valid_positive(value) {
        value.clamp(MIN_RATIO, MAX_RATIO)
    } else {
        1.0
    }
}
