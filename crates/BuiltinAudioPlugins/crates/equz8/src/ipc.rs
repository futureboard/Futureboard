//! Stable editor/DSP parameter wire contract for EQ-Z8.
//!
//! String ids live at the UI/control boundary only. The native host resolves
//! them to compact indices before publishing edits to an audio-side bounded
//! queue; [`Dsp::apply_wire_param`](crate::Dsp::apply_wire_param) consumes the
//! numeric form without allocation, serialization, locking, or string lookup.

use serde::{Deserialize, Serialize};

use crate::{BAND_COUNT, BandType, Params, clamp, default_params};

pub const PROTOCOL_VERSION: u32 = 1;
pub const STATE_VERSION: u32 = 1;

pub const POWER_INDEX: u32 = 0;
pub const MIX_INDEX: u32 = 1;
pub const OUTPUT_INDEX: u32 = 2;
pub const BAND_BASE_INDEX: u32 = 3;
pub const BAND_STRIDE: u32 = 5;

pub const BAND_ENABLED: u32 = 0;
pub const BAND_TYPE: u32 = 1;
pub const BAND_FREQ: u32 = 2;
pub const BAND_GAIN: u32 = 3;
pub const BAND_Q: u32 = 4;

pub const PARAM_COUNT: usize = BAND_BASE_INDEX as usize + BAND_COUNT * BAND_STRIDE as usize;

pub const UI_PARAM_IDS: [&str; PARAM_COUNT] = [
    "power",
    "mix",
    "outputDb",
    "band1_enabled",
    "band1_type",
    "band1_freq",
    "band1_gainDb",
    "band1_q",
    "band2_enabled",
    "band2_type",
    "band2_freq",
    "band2_gainDb",
    "band2_q",
    "band3_enabled",
    "band3_type",
    "band3_freq",
    "band3_gainDb",
    "band3_q",
    "band4_enabled",
    "band4_type",
    "band4_freq",
    "band4_gainDb",
    "band4_q",
    "band5_enabled",
    "band5_type",
    "band5_freq",
    "band5_gainDb",
    "band5_q",
    "band6_enabled",
    "band6_type",
    "band6_freq",
    "band6_gainDb",
    "band6_q",
    "band7_enabled",
    "band7_type",
    "band7_freq",
    "band7_gainDb",
    "band7_q",
    "band8_enabled",
    "band8_type",
    "band8_freq",
    "band8_gainDb",
    "band8_q",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Equz8State {
    pub version: u32,
    pub params: Params,
}

impl Equz8State {
    pub fn new(params: Params) -> Self {
        Self {
            version: STATE_VERSION,
            params,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl Default for Equz8State {
    fn default() -> Self {
        Self::new(default_params())
    }
}

pub fn ui_param_index(id: &str) -> Option<u32> {
    UI_PARAM_IDS
        .iter()
        .position(|candidate| *candidate == id)
        .map(|index| index as u32)
}

pub fn ui_param_id(index: u32) -> Option<&'static str> {
    UI_PARAM_IDS.get(index as usize).copied()
}

pub const fn band_wire_index(band: usize, field: u32) -> u32 {
    BAND_BASE_INDEX + band as u32 * BAND_STRIDE + field
}

pub fn decode_band_wire(index: u32) -> Option<(usize, u32)> {
    let offset = index.checked_sub(BAND_BASE_INDEX)?;
    let band = (offset / BAND_STRIDE) as usize;
    let field = offset % BAND_STRIDE;
    (band < BAND_COUNT).then_some((band, field))
}

pub fn sanitize_params(params: &mut Params) {
    params.output_db = clamp(params.output_db, -24.0, 12.0);
    params.mix = clamp(params.mix, 0.0, 100.0);
    for band in &mut params.bands {
        band.freq = clamp(band.freq, 20.0, 20_000.0);
        band.gain_db = clamp(band.gain_db, -18.0, 18.0);
        band.q = clamp(band.q, 0.1, 12.0);
    }
}

/// Apply one compact UI/control update. This is allocation-free and total:
/// invalid indices are rejected and all continuous values are clamped.
pub fn apply_wire_param(params: &mut Params, index: u32, value: f32) -> bool {
    if !value.is_finite() {
        return false;
    }
    match index {
        POWER_INDEX => params.power = value >= 0.5,
        MIX_INDEX => params.mix = clamp(value, 0.0, 100.0),
        OUTPUT_INDEX => params.output_db = clamp(value, -24.0, 12.0),
        _ => {
            let Some((band_index, field)) = decode_band_wire(index) else {
                return false;
            };
            let band = &mut params.bands[band_index];
            match field {
                BAND_ENABLED => band.active = value >= 0.5,
                BAND_TYPE => band.band_type = BandType::from_wire(value),
                BAND_FREQ => band.freq = clamp(value, 20.0, 20_000.0),
                BAND_GAIN => band.gain_db = clamp(value, -18.0, 18.0),
                BAND_Q => band.q = clamp(value, 0.1, 12.0),
                _ => return false,
            }
        }
    }
    true
}

/// Resolve a string id off the realtime path and apply it to a state mirror.
pub fn apply_ui_param(params: &mut Params, id: &str, value: f32) -> bool {
    let Some(index) = ui_param_index(id) else {
        return false;
    };
    apply_wire_param(params, index, value)
}

pub fn ui_values(params: &Params) -> Vec<(&'static str, f32)> {
    let mut values = Vec::with_capacity(PARAM_COUNT);
    values.push(("power", f32::from(params.power)));
    values.push(("mix", params.mix));
    values.push(("outputDb", params.output_db));
    for (index, band) in params.bands.iter().enumerate() {
        let base = BAND_BASE_INDEX as usize + index * BAND_STRIDE as usize;
        values.push((UI_PARAM_IDS[base], f32::from(band.active)));
        values.push((UI_PARAM_IDS[base + 1], band.band_type.to_wire()));
        values.push((UI_PARAM_IDS[base + 2], band.freq));
        values.push((UI_PARAM_IDS[base + 3], band.gain_db));
        values.push((UI_PARAM_IDS[base + 4], band.q));
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_to_wire_indices() {
        assert_eq!(UI_PARAM_IDS.len(), PARAM_COUNT);
        for (index, id) in UI_PARAM_IDS.iter().enumerate() {
            assert_eq!(ui_param_index(id), Some(index as u32));
            assert_eq!(ui_param_id(index as u32), Some(*id));
        }
    }

    #[test]
    fn state_round_trips() {
        let mut params = default_params();
        assert!(apply_ui_param(&mut params, "band4_gainDb", -4.5));
        let json = Equz8State::new(params).to_json().unwrap();
        let decoded = Equz8State::from_json(&json).unwrap();
        assert_eq!(decoded.params.bands[3].gain_db, -4.5);
    }

    #[test]
    fn invalid_and_non_finite_updates_are_rejected() {
        let mut params = default_params();
        assert!(!apply_wire_param(&mut params, u32::MAX, 1.0));
        assert!(!apply_wire_param(&mut params, BAND_BASE_INDEX, f32::NAN));
    }
}
