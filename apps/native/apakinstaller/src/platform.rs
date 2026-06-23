pub const ELEVATED_WARNING_GUI: &str = "You are running APAK Installer as Administrator.\nPackages may be installed into the Administrator profile instead of your normal Futureboard library.\nPlease restart without Administrator privileges.";
pub const ELEVATED_WARNING_CLI: &str = "Warning: APAK is running as Administrator. Per-user paths may resolve to the Administrator profile.";

pub fn show_startup_error(title: &str, message: &str) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        use windows::core::PCWSTR;

        let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let msg_w: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            MessageBoxW(
                None,
                PCWSTR(msg_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    #[cfg(not(windows))]
    {
        eprintln!("{title}: {message}");
    }
}

pub fn is_process_elevated() -> bool {
    #[cfg(windows)]
    {
        is_process_elevated_windows().unwrap_or(false)
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn is_process_elevated_windows() -> windows::core::Result<bool> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = Default::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);

        result?;
        Ok(elevation.TokenIsElevated != 0)
    }
}
