//! Process-global edition / license status surfaced to shared UI (the About
//! page in Settings).
//!
//! This module holds **no** license logic and **no** signing keys. It is a
//! dependency-free hand-off point: the shared crate must never depend on the
//! private Exclusive Edition crate, so the Exclusive build's app layer installs
//! a provider here and the shared About panel reads from it. A Community build
//! installs nothing and the About panel falls back to its plain edition info.
//!
//! The provider is a closure so it stays fresh: the About panel re-reads license
//! state each time it is shown, which is exactly what lets an activation — or a
//! background renewal on a later launch — appear without special refresh wiring.
//! It is called only from the (rare, user-driven) Settings render, never a hot
//! path.

use std::sync::{Arc, OnceLock, RwLock};

/// Whether a bound license is currently usable or has lapsed. Mirrors the
/// Exclusive Edition's own state enum without depending on that crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicenseDisplayState {
    Active,
    Expired,
}

/// The parts of a verified license worth showing the owner. Never carries the
/// token itself or anything secret — only what the owner already knows.
#[derive(Debug, Clone)]
pub struct LicenseDisplay {
    pub state: LicenseDisplayState,
    /// Display name from the verified token, if the service supplied one.
    pub licensee: Option<String>,
    /// Entitlement ids the license grants, e.g. `["asio"]`.
    pub entitlements: Vec<String>,
    /// Unix seconds the license lapses, or `None` for a perpetual license.
    pub expires_at: Option<u64>,
}

/// What the About panel shows about this build.
#[derive(Debug, Clone)]
pub struct EditionInfo {
    /// Human-readable edition name, e.g. `"Exclusive"`.
    pub edition: &'static str,
    /// The application version string the app layer reports.
    pub app_version: String,
    /// The current license, or `None` when this build/machine is not licensed.
    pub license: Option<LicenseDisplay>,
}

type EditionProvider = Arc<dyn Fn() -> EditionInfo + Send + Sync + 'static>;

fn slot() -> &'static RwLock<Option<EditionProvider>> {
    static SLOT: OnceLock<RwLock<Option<EditionProvider>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(None))
}

/// Install the edition/license provider. Called once by the app layer of an
/// Exclusive build during startup. Idempotent: a later call replaces it.
pub fn set_edition_provider(provider: EditionProvider) {
    if let Ok(mut guard) = slot().write() {
        *guard = Some(provider);
    }
}

/// Current edition/license info, or `None` on a build with no provider
/// (Community Edition). The provider is invoked with no lock held so it can
/// safely touch other shared state.
pub fn current_edition_info() -> Option<EditionInfo> {
    let provider = {
        let guard = slot().read().ok()?;
        guard.as_ref()?.clone()
    };
    Some(provider())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_provider_reports_no_edition_info() {
        // A fresh process with no provider installed (the Community case) must
        // report nothing rather than fabricate an edition.
        if slot().read().map(|g| g.is_some()).unwrap_or(false) {
            return;
        }
        assert!(current_edition_info().is_none());
    }

    #[test]
    fn an_installed_provider_is_read_back() {
        set_edition_provider(Arc::new(|| EditionInfo {
            edition: "Test",
            app_version: "9.9.9".to_string(),
            license: Some(LicenseDisplay {
                state: LicenseDisplayState::Active,
                licensee: Some("Jane Doe".to_string()),
                entitlements: vec!["asio".to_string()],
                expires_at: None,
            }),
        }));
        let info = current_edition_info().expect("provider was installed");
        assert_eq!(info.edition, "Test");
        assert_eq!(info.app_version, "9.9.9");
        assert!(matches!(
            info.license,
            Some(LicenseDisplay {
                state: LicenseDisplayState::Active,
                ..
            })
        ));
    }
}
