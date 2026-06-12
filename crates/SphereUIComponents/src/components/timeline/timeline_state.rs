//! Backwards-compatibility shim.
//!
//! The timeline arrangement state model now lives in
//! [`crate::components::timeline::state`], split by domain. This flat re-export
//! keeps existing `timeline_state::*` import paths working while call sites are
//! migrated to the new module path.
pub use crate::components::timeline::state::*;
