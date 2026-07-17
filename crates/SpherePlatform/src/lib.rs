//! Cross-platform OS helpers for Futureboard Studio and related tools.
//!
//! The main Studio application must never run elevated. Helper processes
//! (installer, updater, driver installer, license service, crash dumper) may
//! call [`is_running_elevated`] later without pulling Studio-specific UX.

mod dialog;
mod elevation;

pub use dialog::{DIALOG_MESSAGE, DIALOG_TITLE, show_elevated_privileges_dialog};
pub use elevation::{
    ElevationKind, ElevationProbe, abort_if_elevated, elevation_probe, is_running_elevated,
    testing_override_enabled,
};

/// Convenience alias matching the `platform::…` call style used by Studio.
pub mod platform {
    pub use crate::{
        ElevationKind, ElevationProbe, abort_if_elevated, elevation_probe, is_running_elevated,
        show_elevated_privileges_dialog, testing_override_enabled,
    };
}
