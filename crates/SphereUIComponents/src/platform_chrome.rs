//! Central platform chrome policy for Futureboard Native.
//!
//! All `cfg(target_os = …)` checks for titlebar / menubar / window controls live
//! here. UI code should call [`PlatformChromePolicy::current()`] instead of
//! scattering platform conditionals.

use gpui::{
    point, px, Pixels, Point, TitlebarOptions, WindowDecorations, WindowKind, WindowOptions,
};

/// Product name shared by native window chrome and OS-level window metadata.
pub const APP_WINDOW_TITLE: &str = "Futureboard Studio";

/// Add the product name to a tool or project window title without duplicating it.
pub fn branded_window_title(title: &str) -> String {
    if title == APP_WINDOW_TITLE || title.contains(APP_WINDOW_TITLE) {
        title.to_string()
    } else {
        format!("{title} — {APP_WINDOW_TITLE}")
    }
}

/// Shared titlebar height across platforms (matches GPUI chrome layout).
pub const TITLEBAR_HEIGHT_PX: f32 = 32.0;

/// macOS traffic-light reserved width in the custom titlebar row.
pub const MACOS_TRAFFIC_LIGHT_PADDING_PX: f32 = 72.0;

/// Minimum left inset for external dialog titles (wizard, preferences) on Win/Linux.
pub const EXTERNAL_DIALOG_TITLE_PADDING_PX: f32 = 12.0;

/// Below this width the in-window menubar collapses to a hamburger control.
pub const MENUBAR_COMPACT_BREAKPOINT_PX: f32 = 1400.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformChromeKind {
    Windows,
    Linux,
    MacOS,
}

#[derive(Debug, Clone, Copy)]
pub struct PlatformChromePolicy {
    pub kind: PlatformChromeKind,
    pub show_in_window_menubar: bool,
    pub use_native_macos_menubar: bool,
    pub show_window_controls: bool,
    pub traffic_light_left_padding_px: f32,
    pub titlebar_height_px: f32,
}

impl PlatformChromePolicy {
    pub fn current() -> Self {
        platform_policy()
    }

    /// Chrome for external dialogs (wizard, preferences).
    pub fn external_dialog() -> Self {
        let main = Self::current();
        let traffic_light_left_padding_px = match main.kind {
            PlatformChromeKind::MacOS => MACOS_TRAFFIC_LIGHT_PADDING_PX,
            PlatformChromeKind::Windows | PlatformChromeKind::Linux => {
                EXTERNAL_DIALOG_TITLE_PADDING_PX
            }
        };
        Self {
            show_in_window_menubar: false,
            use_native_macos_menubar: false,
            traffic_light_left_padding_px,
            ..main
        }
    }

    /// Left padding for external dialog titlebars (traffic lights or minimum inset).
    pub fn external_titlebar_left_padding(&self) -> gpui::Pixels {
        self.traffic_light_left_padding()
    }

    /// Use hamburger + picker instead of horizontal top-level menu labels.
    pub fn menubar_compact(viewport_width: f32) -> bool {
        viewport_width < MENUBAR_COMPACT_BREAKPOINT_PX
    }

    pub fn titlebar_height(&self) -> gpui::Pixels {
        px(self.titlebar_height_px)
    }

    pub fn traffic_light_left_padding(&self) -> gpui::Pixels {
        px(self.traffic_light_left_padding_px)
    }

    /// `TitlebarOptions` for the main studio window.
    pub fn studio_titlebar_options() -> TitlebarOptions {
        let policy = Self::current();
        TitlebarOptions {
            title: Some(APP_WINDOW_TITLE.into()),
            // Windows: transparent titlebar + GPUI `WindowControlArea` hit-testing.
            // macOS: blend custom chrome with native traffic lights.
            // Linux: same client chrome path as Windows.
            appears_transparent: true,
            traffic_light_position: policy.native_traffic_light_position(),
        }
    }

    /// `TitlebarOptions` for wizard / settings dialogs.
    pub fn external_dialog_titlebar_options() -> TitlebarOptions {
        let policy = Self::external_dialog();
        TitlebarOptions {
            title: Some(APP_WINDOW_TITLE.into()),
            appears_transparent: true,
            traffic_light_position: policy.native_traffic_light_position(),
        }
    }

    /// Window decorations for external dialogs.
    pub fn external_dialog_window_decorations() -> Option<WindowDecorations> {
        match Self::current().kind {
            PlatformChromeKind::MacOS => None,
            PlatformChromeKind::Windows | PlatformChromeKind::Linux => {
                Some(WindowDecorations::Client)
            }
        }
    }

    /// Whether the OS may draw its own window frame (avoid duplicating GPUI WCO).
    pub fn use_client_window_decorations_for_studio() -> bool {
        matches!(
            Self::current().kind,
            PlatformChromeKind::Windows | PlatformChromeKind::Linux
        )
    }

    fn native_traffic_light_position(&self) -> Option<Point<Pixels>> {
        if self.kind != PlatformChromeKind::MacOS {
            return None;
        }
        // Position native traffic lights in the titlebar band (GPUI macOS API).
        Some(point(px(12.0), px(10.0)))
    }
}

#[cfg(target_os = "windows")]
fn platform_policy() -> PlatformChromePolicy {
    PlatformChromePolicy {
        kind: PlatformChromeKind::Windows,
        show_in_window_menubar: true,
        use_native_macos_menubar: false,
        show_window_controls: true,
        traffic_light_left_padding_px: 0.0,
        titlebar_height_px: TITLEBAR_HEIGHT_PX,
    }
}

#[cfg(target_os = "linux")]
fn platform_policy() -> PlatformChromePolicy {
    PlatformChromePolicy {
        kind: PlatformChromeKind::Linux,
        show_in_window_menubar: true,
        use_native_macos_menubar: false,
        show_window_controls: true,
        traffic_light_left_padding_px: 0.0,
        titlebar_height_px: TITLEBAR_HEIGHT_PX,
    }
}

#[cfg(target_os = "macos")]
fn platform_policy() -> PlatformChromePolicy {
    PlatformChromePolicy {
        kind: PlatformChromeKind::MacOS,
        show_in_window_menubar: false,
        use_native_macos_menubar: true,
        show_window_controls: false,
        traffic_light_left_padding_px: MACOS_TRAFFIC_LIGHT_PADDING_PX,
        titlebar_height_px: TITLEBAR_HEIGHT_PX,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn platform_policy() -> PlatformChromePolicy {
    PlatformChromePolicy {
        kind: PlatformChromeKind::Linux,
        show_in_window_menubar: true,
        use_native_macos_menubar: false,
        show_window_controls: true,
        traffic_light_left_padding_px: 0.0,
        titlebar_height_px: TITLEBAR_HEIGHT_PX,
    }
}

/// Studio window options (main Futureboard window).
pub fn studio_window_options() -> WindowOptions {
    WindowOptions {
        titlebar: Some(PlatformChromePolicy::studio_titlebar_options()),
        focus: true,
        // Open the studio window hidden. The OS otherwise shows an empty black
        // client area while StudioLayout's heavy first layout / workspace install
        // runs (black screen at init). The mount path reveals it after the first
        // frame paints via `window.activate_window()` (which applies the stored
        // initial placement and shows the window). The Welcome window opts back
        // into `show: true` in `welcome_window_options`.
        show: false,
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        window_decorations: if PlatformChromePolicy::use_client_window_decorations_for_studio() {
            Some(WindowDecorations::Client)
        } else {
            None
        },
        ..Default::default()
    }
}

/// Wire macOS native menubar command dispatch to the studio layout entity.
pub fn register_studio_menu_dispatcher(
    studio: gpui::Entity<crate::layout::StudioLayout>,
    cx: &mut gpui::Context<crate::layout::StudioLayout>,
) {
    use std::sync::Arc;

    crate::native_macos_menu::set_command_dispatcher(Arc::new(move |command_id, app| {
        let owner_bounds = app
            .active_window()
            .and_then(|handle| handle.update(app, |_, window, _| window.bounds()).ok());
        let _ = studio.update(app, |this, cx| {
            this.dispatch_command_id_from_bounds(command_id, owner_bounds, cx);
            cx.notify();
        });
    }));
    crate::native_macos_menu::install_native_macos_menu(cx);
}

/// Partial options shared by GPUI-backed native dialogs. On Windows,
/// [`WindowKind::Dialog`] is hosted by a real Win32 dialog while GPUI continues
/// to render the complete client surface.
pub fn external_dialog_window_options_partial() -> WindowOptions {
    WindowOptions {
        titlebar: Some(PlatformChromePolicy::external_dialog_titlebar_options()),
        focus: true,
        show: true,
        kind: WindowKind::Dialog,
        is_movable: true,
        is_resizable: false,
        is_minimizable: false,
        window_decorations: PlatformChromePolicy::external_dialog_window_decorations(),
        ..Default::default()
    }
}

/// Top-level external tool window. Unlike [`external_dialog_window_options_partial`],
/// this is an independent application window: it is not modal/owned by the
/// Studio HWND and receives normal taskbar, minimize, maximize, and resize
/// behavior from the platform.
pub fn external_window_options_partial() -> WindowOptions {
    WindowOptions {
        titlebar: Some(PlatformChromePolicy::external_dialog_titlebar_options()),
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        dialog_parenting: false,
        is_movable: true,
        is_resizable: true,
        is_minimizable: true,
        window_decorations: PlatformChromePolicy::external_dialog_window_decorations(),
        ..Default::default()
    }
}
