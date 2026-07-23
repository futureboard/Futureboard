//! Serialized per-insert state: what the shared built-in editor's
//! `SelectInstanceMsg.state` carries (see `builtin_plugin_editor_window.rs`
//! in `SphereUIComponents`), and what the project file persists per insert.
//!
//! `Params` already mirrors the DSP one-to-one (`dsp::Params`); this module
//! only adds a schema version so an older saved project doesn't silently
//! misparse against a `Params` shape that has grown fields since.

use crate::Params;

/// Bump when a field is added, removed, or changes meaning in a way that
/// would misparse against an older save. Purely additive changes (a new
/// `Option<T>` defaulting via `#[serde(default)]`) don't require a bump.
///
/// v2: `stage_order` grew from 7 to 9 slots (Comp/Eq stages) — a fixed-size
/// array change that misparses v1 blobs. New comp/eq scalar fields use
/// `#[serde(default)]` and would not have required a bump on their own.
///
/// v3: `stage_order` grew from 9 to 10 slots (Wah stage). The new
/// `mod_model`/`wah_*` fields use `#[serde(default)]` and would not have
/// required a bump on their own.
pub const SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RodhareistState {
    pub schema_version: u32,
    pub params: Params,
}

impl RodhareistState {
    pub fn new(params: Params) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            params,
        }
    }

    /// Serialize for project persistence / the bridge snapshot. `Params`
    /// contains only plain numbers/bools/small enums — this never allocates
    /// more than a few hundred bytes and is only ever called from the
    /// control/UI thread, never the audio callback.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Parse a project-file or bridge-delivered blob. `None` schema_version
    /// mismatches are not rejected here — the caller (project load) decides
    /// whether to fall back to defaults; this only reports the parse itself.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_params;

    #[test]
    fn round_trips_through_json() {
        let state = RodhareistState::new(default_params());
        let json = state.to_json().expect("serialize");
        let restored = RodhareistState::from_json(&json).expect("deserialize");
        assert_eq!(restored.schema_version, SCHEMA_VERSION);
        assert_eq!(restored.params.amp_gain, state.params.amp_gain);
        assert_eq!(restored.params.drive_model, state.params.drive_model);
        assert_eq!(restored.params.stage_order, state.params.stage_order);
    }

    #[test]
    fn malformed_json_is_a_clean_error_not_a_panic() {
        assert!(RodhareistState::from_json("not json").is_err());
        assert!(RodhareistState::from_json("{}").is_err());
    }
}
