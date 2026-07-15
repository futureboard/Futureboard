//! Bridge to the ignored Exclusive Edition implementation.
//!
//! This tracked module deliberately uses `include!` instead of a `#[path]`
//! module. Rustfmt resolves `#[path]` modules even when their feature is
//! disabled, which made Community Edition checks require the private source
//! tree. The include macros are expanded only when this feature-gated module is
//! compiled by an Exclusive Edition build.
//!
//! Compiling with `--features exclusive` grants nothing on its own. Providers
//! below install only after a signed license token verifies for this machine,
//! so an Exclusive build on an unlicensed machine behaves like Community.

mod license {
    include!(concat!(
        env!("OUT_DIR"),
        "/futureboard-exclusive/license.rs"
    ));
}

mod license_activation_dialog {
    include!(concat!(
        env!("OUT_DIR"),
        "/futureboard-exclusive/license_activation_dialog.rs"
    ));
}

#[cfg(target_os = "windows")]
mod asio {
    include!(concat!(env!("OUT_DIR"), "/futureboard-exclusive/asio.rs"));
}

pub use license_activation_dialog::{configured_license_activator, open_license_activation_window};

/// Install the Exclusive Edition runtime providers that a verified license
/// grants. Safe to call more than once: providers already installed in this
/// process are left alone, so activating mid-session takes effect without a
/// restart.
///
/// An unlicensed machine is not an error — it installs nothing, and the audio
/// backend list never offers ASIO.
pub fn install_licensed_providers() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if DirectAudio::asio_support_enabled() {
            return Ok(());
        }
        let Some(license) = license::active_license() else {
            return Ok(());
        };
        if !license.grants(license::ENTITLEMENT_ASIO) {
            return Ok(());
        }
        DirectAudio::backend::register_asio_host_factory(asio::host)?;
    }

    Ok(())
}

/// Install Exclusive Edition runtime providers before the application starts.
///
/// Providers install from the stored token with no network involved, so this
/// stays off the critical path. Renewal is kicked onto a background thread: a
/// slow or unreachable licensing service must never delay the DAW opening.
pub fn install() -> Result<(), String> {
    install_licensed_providers()?;
    license::spawn_renewal_if_due();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::license;

    /// ASIO registration must track the verified license exactly — never the
    /// `exclusive` feature, which only decides whether the code is compiled in.
    ///
    /// Asserting both directions against the same machine keeps this honest on
    /// a developer box either way: unlicensed, ASIO must stay off; licensed, it
    /// must come on.
    #[test]
    fn asio_registration_tracks_the_verified_license() {
        let licensed = license::active_license()
            .is_some_and(|license| license.grants(license::ENTITLEMENT_ASIO));
        eprintln!("[license-e2e] machine holds an ASIO entitlement: {licensed}");

        super::install_licensed_providers().expect("installing must not fail");

        assert_eq!(
            DirectAudio::asio_support_enabled(),
            licensed,
            "an ASIO host must be registered if and only if a verified license grants it"
        );
        assert_eq!(
            DirectAudio::backend::BackendKind::allowed_for_current_platform()
                .contains(&DirectAudio::backend::BackendKind::Asio),
            licensed,
            "the backend list must offer ASIO if and only if a verified license grants it"
        );
    }
}
