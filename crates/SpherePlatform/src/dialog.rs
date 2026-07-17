//! Native elevated-privilege error dialogs (pre-GPUI).

pub const DIALOG_TITLE: &str = "Futureboard Studio";

pub const DIALOG_MESSAGE: &str = "\
Futureboard Studio should not be run with Administrator or root privileges.

Running with elevated permissions may cause:

• Incorrect file ownership
• Plugin permission issues
• Configuration corruption
• Project accessibility problems
• Security risks

Please launch Futureboard Studio as a normal user.";

/// Display the Futureboard Studio elevated-privileges error dialog.
///
/// Blocks until the user dismisses OK. Safe to call before GPUI / audio init.
pub fn show_elevated_privileges_dialog() {
    #[cfg(windows)]
    {
        windows_message_box(DIALOG_TITLE, DIALOG_MESSAGE);
    }

    #[cfg(target_os = "macos")]
    {
        macos_alert(DIALOG_TITLE, DIALOG_MESSAGE);
    }

    #[cfg(target_os = "linux")]
    {
        linux_alert(DIALOG_TITLE, DIALOG_MESSAGE);
    }

    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        eprintln!("{DIALOG_TITLE}\n\n{DIALOG_MESSAGE}");
    }
}

#[cfg(windows)]
fn windows_message_box(title: &str, message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::PCWSTR;

    let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let message_w: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(target_os = "macos")]
fn macos_alert(title: &str, message: &str) {
    // Native pre-AppKit dialog via osascript — no Cocoa/GPUI dependency.
    let script = format!(
        "display dialog \"{}\" with title \"{}\" buttons {{\"OK\"}} default button \"OK\" with icon stop",
        escape_applescript(message),
        escape_applescript(title),
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .status();
}

#[cfg(target_os = "macos")]
fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "linux")]
fn linux_alert(title: &str, message: &str) {
    // Prefer common desktop dialog tools; fall back to stderr if none exist.
    if std::process::Command::new("zenity")
        .args([
            "--error",
            "--title",
            title,
            "--text",
            message,
            "--width=420",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return;
    }

    if std::process::Command::new("kdialog")
        .args(["--title", title, "--error", message])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return;
    }

    eprintln!("{title}\n\n{message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_copy_matches_spec() {
        assert_eq!(DIALOG_TITLE, "Futureboard Studio");
        assert!(DIALOG_MESSAGE.contains("Administrator or root privileges"));
        assert!(DIALOG_MESSAGE.contains("Incorrect file ownership"));
        assert!(DIALOG_MESSAGE.contains("Plugin permission issues"));
        assert!(DIALOG_MESSAGE.contains("Configuration corruption"));
        assert!(DIALOG_MESSAGE.contains("Project accessibility problems"));
        assert!(DIALOG_MESSAGE.contains("Security risks"));
        assert!(DIALOG_MESSAGE.contains("Please launch Futureboard Studio as a normal user."));
    }
}
