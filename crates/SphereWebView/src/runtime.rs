//! Feature-gated native-window CEF runtime.

use std::marker::PhantomData;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread::{self, ThreadId};

use cef::{ImplBrowser, ImplBrowserHost, ImplFrame};
use thiserror::Error;

pub use cef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessDispatch {
    BrowserProcess,
    SubprocessExit(i32),
}

/// Dispatch CEF subprocess command lines before starting the native UI.
pub fn execute_subprocess(application: Option<&mut cef::App>) -> ProcessDispatch {
    let args = cef::args::Args::new();
    let exit_code =
        cef::execute_process(Some(args.as_main_args()), application, std::ptr::null_mut());
    if exit_code < 0 {
        ProcessDispatch::BrowserProcess
    } else {
        ProcessDispatch::SubprocessExit(exit_code)
    }
}

/// Browser-process configuration. The runtime uses a portable, integrated
/// message pump and must be driven from the creating UI thread.
#[derive(Debug, Clone, Default)]
pub struct CefRuntimeConfig {
    pub cache_path: Option<PathBuf>,
    pub root_cache_path: Option<PathBuf>,
    pub browser_subprocess_path: Option<PathBuf>,
    pub locale: Option<String>,
    pub user_agent: Option<String>,
    pub remote_debugging_port: Option<u16>,
}

/// Pixel bounds in the native parent's client coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl WindowBounds {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Result<Self, CefRuntimeError> {
        if width <= 0 || height <= 0 {
            return Err(CefRuntimeError::InvalidBounds { width, height });
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    fn as_cef_rect(self) -> cef::Rect {
        cef::Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

/// A native parent HWND (Windows), X11 Window (Linux), or NSView pointer
/// (macOS).
#[derive(Clone, Copy)]
pub struct NativeParent(cef::sys::cef_window_handle_t);

impl NativeParent {
    /// # Safety
    ///
    /// The handle must stay valid for all child [`WebView`] instances and must
    /// belong to the thread that owns [`CefRuntime`].
    pub unsafe fn from_raw(handle: cef::sys::cef_window_handle_t) -> Self {
        Self(handle)
    }

    pub fn as_raw(self) -> cef::sys::cef_window_handle_t {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct WebViewConfig {
    pub url: String,
    pub bounds: WindowBounds,
}

impl WebViewConfig {
    pub fn new(url: impl Into<String>, bounds: WindowBounds) -> Self {
        Self {
            url: url.into(),
            bounds,
        }
    }
}

/// Process-wide CEF state. This is intentionally `!Send` and `!Sync`.
pub struct CefRuntime {
    owner_thread: ThreadId,
    shutdown: bool,
    #[cfg(target_os = "macos")]
    _library: cef::library_loader::LibraryLoader,
    _not_send: PhantomData<Rc<()>>,
}

impl CefRuntime {
    pub fn initialize(
        config: CefRuntimeConfig,
        application: Option<&mut cef::App>,
    ) -> Result<Self, CefRuntimeError> {
        #[cfg(target_os = "macos")]
        let library = load_macos_framework()?;

        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        let args = cef::args::Args::new();
        let settings = cef::Settings {
            no_sandbox: 1,
            multi_threaded_message_loop: 0,
            external_message_pump: 0,
            // This library only creates WindowInfo::set_as_child browsers.
            windowless_rendering_enabled: 0,
            cache_path: cef_path(config.cache_path.as_ref()),
            root_cache_path: cef_path(config.root_cache_path.as_ref()),
            browser_subprocess_path: cef_path(config.browser_subprocess_path.as_ref()),
            locale: cef_string(config.locale.as_deref()),
            user_agent: cef_string(config.user_agent.as_deref()),
            remote_debugging_port: config.remote_debugging_port.unwrap_or(0) as i32,
            ..Default::default()
        };
        if cef::initialize(
            Some(args.as_main_args()),
            Some(&settings),
            application,
            std::ptr::null_mut(),
        ) != 1
        {
            return Err(CefRuntimeError::InitializeFailed);
        }

        Ok(Self {
            owner_thread: thread::current().id(),
            shutdown: false,
            #[cfg(target_os = "macos")]
            _library: library,
            _not_send: PhantomData,
        })
    }

    /// Advance CEF from the native application's UI loop.
    pub fn do_message_loop_work(&self) -> Result<(), CefRuntimeError> {
        self.ensure_thread()?;
        cef::do_message_loop_work();
        Ok(())
    }

    /// Create a real native CEF child window. No render handler, shared texture,
    /// or off-screen rendering path is used.
    pub fn create_webview<'runtime>(
        &'runtime self,
        parent: NativeParent,
        config: WebViewConfig,
    ) -> Result<WebView<'runtime>, CefRuntimeError> {
        self.ensure_thread()?;
        if config.url.trim().is_empty() {
            return Err(CefRuntimeError::EmptyUrl);
        }
        let window_info =
            cef::WindowInfo::default().set_as_child(parent.as_raw(), &config.bounds.as_cef_rect());
        debug_assert_eq!(window_info.windowless_rendering_enabled, 0);
        let browser = cef::browser_host_create_browser_sync(
            Some(&window_info),
            None,
            Some(&cef::CefString::from(config.url.as_str())),
            Some(&cef::BrowserSettings::default()),
            None,
            None,
        )
        .ok_or(CefRuntimeError::CreateBrowserFailed)?;

        Ok(WebView {
            browser,
            owner_thread: self.owner_thread,
            _runtime: PhantomData,
            _not_send: PhantomData,
        })
    }

    pub fn shutdown(mut self) -> Result<(), CefRuntimeError> {
        self.ensure_thread()?;
        if !self.shutdown {
            cef::shutdown();
            self.shutdown = true;
        }
        Ok(())
    }

    fn ensure_thread(&self) -> Result<(), CefRuntimeError> {
        if thread::current().id() != self.owner_thread {
            return Err(CefRuntimeError::WrongThread);
        }
        Ok(())
    }
}

impl Drop for CefRuntime {
    fn drop(&mut self) {
        if !self.shutdown && thread::current().id() == self.owner_thread {
            cef::shutdown();
            self.shutdown = true;
        }
    }
}

pub struct WebView<'runtime> {
    browser: cef::Browser,
    owner_thread: ThreadId,
    _runtime: PhantomData<&'runtime CefRuntime>,
    _not_send: PhantomData<Rc<()>>,
}

impl WebView<'_> {
    pub fn load_url(&self, url: &str) -> Result<(), CefRuntimeError> {
        self.ensure_thread()?;
        if url.trim().is_empty() {
            return Err(CefRuntimeError::EmptyUrl);
        }
        let frame = self
            .browser
            .main_frame()
            .ok_or(CefRuntimeError::MissingMainFrame)?;
        frame.load_url(Some(&cef::CefString::from(url)));
        Ok(())
    }

    pub fn set_bounds(&self, bounds: WindowBounds) -> Result<(), CefRuntimeError> {
        self.ensure_thread()?;
        let host = self
            .browser
            .host()
            .ok_or(CefRuntimeError::MissingBrowserHost)?;
        platform_set_bounds(host.window_handle(), bounds)?;
        host.notify_move_or_resize_started();
        Ok(())
    }

    pub fn native_window_handle(&self) -> Result<cef::sys::cef_window_handle_t, CefRuntimeError> {
        self.ensure_thread()?;
        self.browser
            .host()
            .map(|host| host.window_handle())
            .ok_or(CefRuntimeError::MissingBrowserHost)
    }

    pub fn go_back(&self) -> Result<(), CefRuntimeError> {
        self.ensure_thread()?;
        if self.browser.can_go_back() != 0 {
            self.browser.go_back();
        }
        Ok(())
    }

    pub fn go_forward(&self) -> Result<(), CefRuntimeError> {
        self.ensure_thread()?;
        if self.browser.can_go_forward() != 0 {
            self.browser.go_forward();
        }
        Ok(())
    }

    pub fn reload(&self) -> Result<(), CefRuntimeError> {
        self.ensure_thread()?;
        self.browser.reload();
        Ok(())
    }

    pub fn close(&self, force: bool) -> Result<(), CefRuntimeError> {
        self.ensure_thread()?;
        let host = self
            .browser
            .host()
            .ok_or(CefRuntimeError::MissingBrowserHost)?;
        host.close_browser(i32::from(force));
        Ok(())
    }

    fn ensure_thread(&self) -> Result<(), CefRuntimeError> {
        if thread::current().id() != self.owner_thread {
            return Err(CefRuntimeError::WrongThread);
        }
        Ok(())
    }
}

fn cef_path(path: Option<&PathBuf>) -> cef::CefString {
    path.map(|path| cef::CefString::from(path.to_string_lossy().as_ref()))
        .unwrap_or_default()
}

fn cef_string(value: Option<&str>) -> cef::CefString {
    value.map(cef::CefString::from).unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn load_macos_framework() -> Result<cef::library_loader::LibraryLoader, CefRuntimeError> {
    let executable = std::env::current_exe()?;
    let loader =
        std::panic::catch_unwind(|| cef::library_loader::LibraryLoader::new(&executable, false))
            .map_err(|_| CefRuntimeError::MacFrameworkNotFound)?;
    if !loader.load() {
        return Err(CefRuntimeError::MacFrameworkNotFound);
    }
    Ok(loader)
}

#[cfg(windows)]
fn platform_set_bounds(
    handle: cef::sys::cef_window_handle_t,
    bounds: WindowBounds,
) -> Result<(), CefRuntimeError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos};
    let ok = unsafe {
        SetWindowPos(
            handle.0.cast(),
            std::ptr::null_mut(),
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
    };
    if ok == 0 {
        return Err(CefRuntimeError::PlatformResizeFailed(
            std::io::Error::last_os_error().to_string(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn platform_set_bounds(
    handle: cef::sys::cef_window_handle_t,
    bounds: WindowBounds,
) -> Result<(), CefRuntimeError> {
    let xlib = x11_dl::xlib::Xlib::open()
        .map_err(|error| CefRuntimeError::PlatformResizeFailed(error.to_string()))?;
    let display = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
    if display.is_null() {
        return Err(CefRuntimeError::PlatformResizeFailed(
            "XOpenDisplay returned null".to_owned(),
        ));
    }
    unsafe {
        (xlib.XMoveResizeWindow)(
            display,
            handle,
            bounds.x,
            bounds.y,
            bounds.width as u32,
            bounds.height as u32,
        );
        (xlib.XFlush)(display);
        (xlib.XCloseDisplay)(display);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_set_bounds(
    handle: cef::sys::cef_window_handle_t,
    bounds: WindowBounds,
) -> Result<(), CefRuntimeError> {
    use objc2::{msg_send, runtime::AnyObject};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    if handle.is_null() {
        return Err(CefRuntimeError::PlatformResizeFailed(
            "CEF returned a null NSView".to_owned(),
        ));
    }
    let view = unsafe { &*handle.cast::<AnyObject>() };
    let frame = NSRect {
        origin: NSPoint {
            x: bounds.x as f64,
            y: bounds.y as f64,
        },
        size: NSSize {
            width: bounds.width as f64,
            height: bounds.height as f64,
        },
    };
    unsafe {
        let _: () = msg_send![view, setFrame: frame];
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum CefRuntimeError {
    #[error("CEF initialization failed")]
    InitializeFailed,
    #[error("CEF browser creation failed")]
    CreateBrowserFailed,
    #[error("CEF operations must run on the runtime's creating thread")]
    WrongThread,
    #[error("web view URL cannot be empty")]
    EmptyUrl,
    #[error("invalid web view bounds {width}x{height}")]
    InvalidBounds { width: i32, height: i32 },
    #[error("CEF browser has no main frame")]
    MissingMainFrame,
    #[error("CEF browser has no host")]
    MissingBrowserHost,
    #[error("native web view resize failed: {0}")]
    PlatformResizeFailed(String),
    #[error("failed to resolve the current executable: {0}")]
    CurrentExecutable(#[from] std::io::Error),
    #[cfg(target_os = "macos")]
    #[error("Chromium Embedded Framework.framework was not found in the application bundle")]
    MacFrameworkNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_positive_bounds() {
        assert!(WindowBounds::new(0, 0, 0, 100).is_err());
        assert!(WindowBounds::new(0, 0, 100, -1).is_err());
    }

    #[test]
    fn accepts_native_child_bounds() {
        assert_eq!(
            WindowBounds::new(4, 8, 1280, 720).unwrap(),
            WindowBounds {
                x: 4,
                y: 8,
                width: 1280,
                height: 720,
            }
        );
    }
}
