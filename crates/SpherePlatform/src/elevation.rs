//! Privilege / elevation detection.
//!
//! Detection never relies on environment variables alone. Env vars such as
//! `SUDO_USER` / `PKEXEC_UID` are supplemental logging hints only.

use crate::dialog::show_elevated_privileges_dialog;

/// Kind of elevation detected, used for console messaging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationKind {
    None,
    /// Windows administrator / elevated UAC token.
    Administrator,
    /// Unix root (euid == 0), including sudo / pkexec / doas sessions.
    Root,
}

impl ElevationKind {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Administrator => "Administrator",
            Self::Root => "Root",
        }
    }
}

/// Structured result of an elevation probe (for logging and tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevationProbe {
    pub platform: &'static str,
    pub elevated: bool,
    pub kind: ElevationKind,
    /// Effective user id on Unix; unused on Windows (`None`).
    pub uid: Option<u32>,
    /// Primary detection method that decided elevation (or confirmed non-elevated).
    pub method: &'static str,
}

impl ElevationProbe {
    pub fn log_lines(&self) -> String {
        let uid = self
            .uid
            .map(|u| u.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        format!(
            "Platform: {}\nUID: {}\nElevated: {}\nMethod: {}",
            self.platform, uid, self.elevated, self.method
        )
    }
}

/// Returns `true` when the current process is running with elevated privileges.
pub fn is_running_elevated() -> bool {
    elevation_probe().elevated
}

/// Probe elevation state with platform / method metadata for logging.
pub fn elevation_probe() -> ElevationProbe {
    #[cfg(windows)]
    {
        windows::probe()
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::probe()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        ElevationProbe {
            platform: std::env::consts::OS,
            elevated: false,
            kind: ElevationKind::None,
            uid: None,
            method: "unsupported",
        }
    }
}

/// `true` when the optional developer override feature is compiled in.
#[inline]
pub fn testing_override_enabled() -> bool {
    cfg!(feature = "allow_elevated_for_testing")
}

fn privilege_debug_logging() -> bool {
    std::env::var_os("FUTUREBOARD_PRIVILEGE_DEBUG").is_some()
        || std::env::var_os("FUTUREBOARD_BOOT_DEBUG").is_some()
}

/// Fail immediately if the process is elevated.
///
/// Logs detection details, shows a native error dialog, and exits with
/// `EXIT_FAILURE`. Does not initialize audio, plugins, settings, or projects.
///
/// When built with `allow_elevated_for_testing`, logs a warning and returns.
pub fn abort_if_elevated() {
    let probe = elevation_probe();

    if probe.elevated || privilege_debug_logging() {
        eprintln!("{}", probe.log_lines());
    }

    if !probe.elevated {
        return;
    }

    if testing_override_enabled() {
        eprintln!(
            "WARNING: elevated privileges detected but allow_elevated_for_testing is enabled; continuing."
        );
        return;
    }

    eprintln!("ERROR:");
    eprintln!("Futureboard Studio cannot run with elevated privileges.");
    eprintln!();
    eprintln!("Detected:");
    eprintln!("{}", probe.kind.as_label());

    show_elevated_privileges_dialog();

    // EXIT_FAILURE — do not continue Studio initialization.
    std::process::exit(1);
}

/// Pure Unix elevation rule (euid == 0). Env vars are not consulted.
#[cfg_attr(
    not(any(test, target_os = "linux", target_os = "macos")),
    allow(dead_code)
)]
pub(crate) fn unix_euid_is_elevated(euid: u32) -> bool {
    euid == 0
}

/// Supplemental hint only — never used as the sole elevation signal.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn unix_elevation_env_hint() -> Option<&'static str> {
    if std::env::var_os("SUDO_USER").is_some() || std::env::var_os("SUDO_UID").is_some() {
        Some("sudo (env hint)")
    } else if std::env::var_os("PKEXEC_UID").is_some() {
        Some("pkexec (env hint)")
    } else if std::env::var_os("DOAS_USER").is_some() {
        Some("doas (env hint)")
    } else {
        None
    }
}

#[cfg(windows)]
mod windows {
    use super::{ElevationKind, ElevationProbe};

    pub(super) fn probe() -> ElevationProbe {
        match probe_windows() {
            Ok(probe) => probe,
            Err(_) => ElevationProbe {
                platform: "Windows",
                elevated: false,
                kind: ElevationKind::None,
                uid: None,
                // Fail closed for detection errors would brick legitimate users
                // if token APIs flake; treat as not elevated but record the miss.
                method: "GetTokenInformation (query failed)",
            },
        }
    }

    fn probe_windows() -> windows::core::Result<ElevationProbe> {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{
            GetTokenInformation, TOKEN_ELEVATION, TOKEN_ELEVATION_TYPE, TOKEN_QUERY,
            TokenElevation, TokenElevationType, TokenElevationTypeFull,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;

            let mut elevation = TOKEN_ELEVATION::default();
            let mut returned = 0u32;
            let elev_ok = GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut returned,
            );

            let mut elev_type = TOKEN_ELEVATION_TYPE::default();
            let mut type_returned = 0u32;
            let type_ok = GetTokenInformation(
                token,
                TokenElevationType,
                Some((&mut elev_type as *mut TOKEN_ELEVATION_TYPE).cast()),
                std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
                &mut type_returned,
            );

            let token_is_elevated = elev_ok.is_ok() && elevation.TokenIsElevated != 0;
            let elevation_type_full = type_ok.is_ok() && elev_type == TokenElevationTypeFull;

            // Administrators group membership alone is not elevation under UAC
            // (filtered admin tokens remain members). Combine with elevation.
            let admin_member = check_administrators_membership(token).unwrap_or(false);

            let _ = CloseHandle(token);

            elev_ok?;

            let elevated = token_is_elevated || elevation_type_full;
            let method = if token_is_elevated {
                "GetTokenInformation(TokenElevation)"
            } else if elevation_type_full {
                "GetTokenInformation(TokenElevationTypeFull)"
            } else if admin_member {
                "CheckTokenMembership(Administrators) — not elevated (filtered token)"
            } else {
                "GetTokenInformation(TokenElevation) — not elevated"
            };

            Ok(ElevationProbe {
                platform: "Windows",
                elevated,
                kind: if elevated {
                    ElevationKind::Administrator
                } else {
                    ElevationKind::None
                },
                uid: None,
                method,
            })
        }
    }

    fn check_administrators_membership(
        token: windows::Win32::Foundation::HANDLE,
    ) -> windows::core::Result<bool> {
        use windows::Win32::Security::{
            AllocateAndInitializeSid, CheckTokenMembership, FreeSid, PSID, SECURITY_NT_AUTHORITY,
        };
        use windows::Win32::System::SystemServices::{
            DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID,
        };

        unsafe {
            let mut sid = PSID::default();
            AllocateAndInitializeSid(
                &SECURITY_NT_AUTHORITY,
                2,
                SECURITY_BUILTIN_DOMAIN_RID as u32,
                DOMAIN_ALIAS_RID_ADMINS as u32,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut sid,
            )?;

            let mut is_member = windows::core::BOOL(0);
            let result = CheckTokenMembership(Some(token), sid, &mut is_member);
            let _ = FreeSid(sid);
            result?;
            Ok(is_member.as_bool())
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::{ElevationKind, ElevationProbe, unix_elevation_env_hint, unix_euid_is_elevated};

    #[cfg(target_os = "linux")]
    const PLATFORM: &str = "Linux";
    #[cfg(target_os = "macos")]
    const PLATFORM: &str = "macOS";

    unsafe extern "C" {
        fn geteuid() -> u32;
    }

    pub(super) fn probe() -> ElevationProbe {
        let euid = unsafe { geteuid() };
        let elevated = unix_euid_is_elevated(euid);
        let env_hint = unix_elevation_env_hint();

        let method = if elevated {
            match env_hint {
                Some("sudo (env hint)") => "geteuid() + sudo env",
                Some("pkexec (env hint)") => "geteuid() + pkexec env",
                Some("doas (env hint)") => "geteuid() + doas env",
                _ => "geteuid()",
            }
        } else {
            "geteuid()"
        };

        ElevationProbe {
            platform: PLATFORM,
            elevated,
            kind: if elevated {
                ElevationKind::Root
            } else {
                ElevationKind::None
            },
            uid: Some(euid),
            method,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_root_uid_is_elevated() {
        assert!(unix_euid_is_elevated(0));
    }

    #[test]
    fn unix_normal_uid_is_not_elevated() {
        assert!(!unix_euid_is_elevated(1000));
        assert!(!unix_euid_is_elevated(501));
        assert!(!unix_euid_is_elevated(1));
    }

    #[test]
    fn elevation_kind_labels() {
        assert_eq!(ElevationKind::Administrator.as_label(), "Administrator");
        assert_eq!(ElevationKind::Root.as_label(), "Root");
        assert_eq!(ElevationKind::None.as_label(), "None");
    }

    #[test]
    fn probe_log_format_contains_required_fields() {
        let probe = ElevationProbe {
            platform: "Linux",
            elevated: true,
            kind: ElevationKind::Root,
            uid: Some(0),
            method: "geteuid()",
        };
        let log = probe.log_lines();
        assert!(log.contains("Platform: Linux"));
        assert!(log.contains("UID: 0"));
        assert!(log.contains("Elevated: true"));
        assert!(log.contains("Method: geteuid()"));
    }

    #[test]
    fn current_process_probe_is_consistent() {
        let probe = elevation_probe();
        assert_eq!(probe.elevated, is_running_elevated());
        if probe.elevated {
            assert_ne!(probe.kind, ElevationKind::None);
        } else {
            assert_eq!(probe.kind, ElevationKind::None);
        }
    }

    /// Opt-in acceptance check for a normal-user launch environment.
    #[test]
    #[ignore = "depends on the test runner privilege level"]
    fn current_process_is_not_elevated_in_normal_ci() {
        if testing_override_enabled() {
            return;
        }
        let probe = elevation_probe();
        assert!(
            !probe.elevated,
            "test process unexpectedly elevated: {}",
            probe.log_lines()
        );
    }

    #[test]
    fn env_hint_does_not_alone_mark_elevated() {
        // Even if sudo-like env is present, elevation is decided by euid only.
        assert!(!unix_euid_is_elevated(1000));
    }

    #[cfg(windows)]
    #[test]
    fn windows_probe_reports_windows_platform() {
        let probe = elevation_probe();
        assert_eq!(probe.platform, "Windows");
        assert!(probe.uid.is_none());
        assert!(probe.method.contains("TokenElevation") || probe.method.contains("query failed"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_reports_linux_platform() {
        let probe = elevation_probe();
        assert_eq!(probe.platform, "Linux");
        assert!(probe.uid.is_some());
        assert!(probe.method.contains("geteuid"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_probe_reports_macos_platform() {
        let probe = elevation_probe();
        assert_eq!(probe.platform, "macOS");
        assert!(probe.uid.is_some());
        assert!(probe.method.contains("geteuid"));
    }
}
