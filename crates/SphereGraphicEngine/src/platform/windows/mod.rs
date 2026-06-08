//! Windows native graphics backend: DirectWrite text + DWM window effects.

pub mod dwm_window;
pub mod dwrite_font_manager;
pub mod dwrite_text_renderer;

pub use dwm_window::{CornerPreference, DwmApplyResult, DwmChromeOptions, DwmWindowEffects};
pub use dwrite_font_manager::{DWriteFontManager, FontConfig, FontDiagnostics};
pub use dwrite_text_renderer::DWriteTextRenderer;
