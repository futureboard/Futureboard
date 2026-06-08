//! DWM window chrome effects: immersive dark mode, rounded corners, themed
//! border / caption color, and non-client rendering policy.
//!
//! Every attribute is best-effort and runtime-guarded — unsupported attributes
//! on older Windows are simply ignored. Colors are raw Win32 `COLORREF` values
//! so the caller decides where they come from (theme, system, etc.); this
//! module stays free of any specific palette.

use core::ffi::c_void;

use windows::core::BOOL;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};

// Documented attribute ids — used directly to avoid depending on specific
// named-constant exports across windows-crate / SDK versions.
const DWMWA_NCRENDERING_POLICY: i32 = 2;
const DWMWA_USE_IMMERSIVE_DARK_MODE: i32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: i32 = 33;
const DWMWA_BORDER_COLOR: i32 = 34;
const DWMWA_CAPTION_COLOR: i32 = 35;
const DWMNCRP_DISABLED: i32 = 1;

/// Rounded-corner preference (`DWMWA_WINDOW_CORNER_PREFERENCE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerPreference {
    /// Let the system decide.
    Default,
    /// Never round.
    DoNotRound,
    /// Round with the standard radius.
    Round,
    /// Round with a small radius.
    RoundSmall,
}

impl CornerPreference {
    fn raw(self) -> i32 {
        match self {
            CornerPreference::Default => 0,
            CornerPreference::DoNotRound => 1,
            CornerPreference::Round => 2,
            CornerPreference::RoundSmall => 3,
        }
    }

    fn rounds(self) -> bool {
        matches!(self, CornerPreference::Round | CornerPreference::RoundSmall)
    }
}

/// Requested DWM chrome attributes. `None` color fields are left untouched.
#[derive(Debug, Clone, Copy)]
pub struct DwmChromeOptions {
    pub dark_mode: bool,
    pub corner: CornerPreference,
    /// Themed window border color (`COLORREF`), or `None` to leave default.
    pub border_color: Option<u32>,
    /// Caption color (`COLORREF`), or `None` to leave default.
    pub caption_color: Option<u32>,
    /// Disable classic non-client rendering (suppresses the OS frame).
    pub disable_nc_rendering: bool,
}

impl Default for DwmChromeOptions {
    fn default() -> Self {
        Self {
            dark_mode: true,
            corner: CornerPreference::Round,
            border_color: None,
            caption_color: None,
            disable_nc_rendering: true,
        }
    }
}

/// Which attributes the OS actually accepted. Useful for diagnostics/logging.
#[derive(Debug, Clone, Copy, Default)]
pub struct DwmApplyResult {
    pub dark_ok: bool,
    pub rounded: bool,
    /// True if any polish attribute was accepted (dark mode or rounding).
    pub available: bool,
}

/// Zero-sized namespace for DWM window effects.
pub struct DwmWindowEffects;

impl DwmWindowEffects {
    /// Apply the requested chrome attributes to `hwnd`. Best-effort; returns
    /// which attributes the OS accepted.
    pub fn apply(hwnd: HWND, opts: &DwmChromeOptions) -> DwmApplyResult {
        unsafe {
            if opts.disable_nc_rendering {
                let nc_policy = DWMNCRP_DISABLED;
                let _ = set_attr(hwnd, DWMWA_NCRENDERING_POLICY, &nc_policy);
            }

            let dark: BOOL = opts.dark_mode.into();
            let dark_ok = set_attr(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark).is_ok();

            let corner = opts.corner.raw();
            let corner_ok = set_attr(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner).is_ok();
            let rounded = corner_ok && opts.corner.rounds();

            if let Some(border) = opts.border_color {
                let border = COLORREF(border);
                let _ = set_attr(hwnd, DWMWA_BORDER_COLOR, &border);
            }
            if let Some(caption) = opts.caption_color {
                let caption = COLORREF(caption);
                let _ = set_attr(hwnd, DWMWA_CAPTION_COLOR, &caption);
            }

            DwmApplyResult {
                dark_ok,
                rounded,
                available: dark_ok || corner_ok,
            }
        }
    }
}

/// Set a single DWM window attribute from a `Copy` value.
unsafe fn set_attr<T: Copy>(hwnd: HWND, attr: i32, value: &T) -> windows::core::Result<()> {
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(attr),
            value as *const T as *const c_void,
            std::mem::size_of::<T>() as u32,
        )
    }
}
