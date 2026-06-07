//! Main-app-owned **content child HWND** for separated-process plugin editor
//! hosting (`FUTUREBOARD_PLUGIN_EDITOR_OWNERSHIP=host_process`).
//!
//! Spec Part 2: when the editor lives in `FutureboardPluginHost-x64.exe`, the
//! *window* still belongs to the GPUI main app. The GPUI plugin-editor window
//! supplies its top HWND; this module creates a real `WS_CHILD` content HWND
//! under it and hands the content HWND's handle to the host process over IPC.
//! The host attaches the VST3 `IPlugView` to that handle from its own COM STA
//! thread (cross-process embedding).
//!
//! VST3 editor hosting follows public.sdk/samples/vst-hosting/editorhost
//! lifecycle: the difference here is only *which process* owns the window
//! (main app) versus the view (host process).
//!
//! Hard requirements enforced here:
//! - `content_hwnd != top_hwnd` (a dedicated child, never the top window).
//! - content child styles: `WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS`.
//! - the child's parent is the supplied top HWND.
//!
//! On non-Windows targets every entry point is a no-op stub returning `None`,
//! so the crate still compiles cross-platform. (macOS NSView hosting is a later
//! slice.)

/// Physical-pixel rect (relative to the parent client area) for the content
/// child window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContentRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

fn debug_enabled() -> bool {
    std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some()
}

#[cfg(target_os = "windows")]
mod imp {
    use super::{debug_enabled, ContentRect};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetParent, IsChild, IsWindow, SetWindowPos, HMENU,
        SWP_NOACTIVATE, SWP_NOZORDER, WINDOW_EX_STYLE, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        WS_VISIBLE,
    };

    fn hwnd_from(handle: u64) -> HWND {
        HWND(handle as *mut core::ffi::c_void)
    }

    /// A real `WS_CHILD` content window parented to a main-app top HWND. Drop
    /// destroys it. The handle ([`Self::hwnd`]) is what travels to the host
    /// process via `HostCommand::OpenEditorWithParentHwnd`.
    pub struct ContentChildHwnd {
        top_hwnd: u64,
        content_hwnd: u64,
    }

    impl ContentChildHwnd {
        /// Create the content child window under `top_hwnd`. Returns `None` if
        /// `top_hwnd` is not a window or window creation fails.
        pub fn create(top_hwnd: u64, rect: ContentRect) -> Option<Self> {
            if top_hwnd == 0 {
                return None;
            }
            let top = hwnd_from(top_hwnd);
            // Safety: all args are validated; `STATIC` is a predefined window
            // class so no registration/WndProc is required. The plugin paints
            // its own child into this HWND after the host attaches the view.
            unsafe {
                if !IsWindow(Some(top)).as_bool() {
                    return None;
                }
                let content = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    windows::core::w!("STATIC"),
                    PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                    rect.x,
                    rect.y,
                    rect.width.max(1),
                    rect.height.max(1),
                    Some(top),
                    None::<HMENU>,
                    None,
                    None,
                )
                .ok()?;

                let content_u64 = content.0 as u64;
                if content_u64 == top_hwnd {
                    // Must never happen with a child window; bail rather than
                    // letting the host attach to the top window.
                    let _ = DestroyWindow(content);
                    return None;
                }

                if debug_enabled() {
                    let parent = GetParent(content).map(|p| p.0 as u64).unwrap_or(0);
                    let is_child = IsChild(top, content).as_bool();
                    eprintln!("[PluginEditorWindow] top_hwnd=0x{top_hwnd:x}");
                    eprintln!("[PluginEditorWindow] content_hwnd=0x{content_u64:x}");
                    eprintln!("[PluginEditorWindow] content_hwnd != top_hwnd");
                    eprintln!("[PluginEditorWindow] content_parent=0x{parent:x}");
                    eprintln!(
                        "[PluginEditorWindow] content_rect=({},{},{}x{})",
                        rect.x, rect.y, rect.width, rect.height
                    );
                    eprintln!("[PluginEditorWindow] content_is_child={is_child} owner=main_app");
                }

                Some(Self {
                    top_hwnd,
                    content_hwnd: content_u64,
                })
            }
        }

        /// The content child HWND as a `u64` — the value passed to the host.
        pub fn hwnd(&self) -> u64 {
            self.content_hwnd
        }

        pub fn top_hwnd(&self) -> u64 {
            self.top_hwnd
        }

        /// Resize/reposition the content child (main app owns the geometry; it
        /// then tells the host to re-issue `onSize`).
        pub fn set_bounds(&self, rect: ContentRect) {
            unsafe {
                let _ = SetWindowPos(
                    hwnd_from(self.content_hwnd),
                    None,
                    rect.x,
                    rect.y,
                    rect.width.max(1),
                    rect.height.max(1),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }

        /// True while the content HWND is still a valid window.
        pub fn is_valid(&self) -> bool {
            unsafe { IsWindow(Some(hwnd_from(self.content_hwnd))).as_bool() }
        }
    }

    impl Drop for ContentChildHwnd {
        fn drop(&mut self) {
            unsafe {
                if self.content_hwnd != 0 && IsWindow(Some(hwnd_from(self.content_hwnd))).as_bool() {
                    let _ = DestroyWindow(hwnd_from(self.content_hwnd));
                }
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::ContentRect;

    /// Non-Windows stub. Host-process editor embedding via NSView/X11 is a later
    /// slice; this keeps the crate compiling everywhere.
    pub struct ContentChildHwnd {
        _private: (),
    }

    impl ContentChildHwnd {
        pub fn create(_top_hwnd: u64, _rect: ContentRect) -> Option<Self> {
            None
        }
        pub fn hwnd(&self) -> u64 {
            0
        }
        pub fn top_hwnd(&self) -> u64 {
            0
        }
        pub fn set_bounds(&self, _rect: ContentRect) {}
        pub fn is_valid(&self) -> bool {
            false
        }
    }
}

pub use imp::ContentChildHwnd;
