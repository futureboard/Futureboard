//! Parameter system for device params.
//!
//! ParamValue is the runtime value type.
//! ParamInfo describes metadata for UI mapping.

use serde::{Deserialize, Serialize};

/// Runtime parameter value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
}

impl ParamValue {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            ParamValue::Float(v) => Some(*v),
            ParamValue::Int(v) => Some(*v as f32),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(v) => Some(*v),
            _ => None,
        }
    }
}

/// Parameter metadata for UI and validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamInfo {
    pub id: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: String,
}

impl ParamInfo {
    /// Clamp a float value to [min, max].
    pub fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_value_conversions() {
        let f = ParamValue::Float(0.5);
        assert_eq!(f.as_f32(), Some(0.5));
        assert_eq!(f.as_bool(), None);

        let b = ParamValue::Bool(true);
        assert_eq!(b.as_bool(), Some(true));
        assert_eq!(b.as_f32(), None);
    }

    #[test]
    fn param_info_clamp() {
        let info = ParamInfo {
            id: "gain".into(),
            name: "Gain".into(),
            min: 0.0,
            max: 2.0,
            default: 1.0,
            unit: "x".into(),
        };
        assert_eq!(info.clamp(-1.0), 0.0);
        assert_eq!(info.clamp(1.5), 1.5);
        assert_eq!(info.clamp(5.0), 2.0);
    }
}
