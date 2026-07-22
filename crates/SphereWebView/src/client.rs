//! Browser-process CEF client for plugin editor browsers: navigation lockdown
//! plus a renderer-crash signal.
//!
//! `runtime::create_webview*` previously passed `None` for the client
//! parameter to `browser_host_create_browser_sync`, meaning the browser had
//! **zero** navigation policy (any in-page link, `window.open`, or redirect
//! would have gone through) and no way to observe a renderer crash at all.
//! This supplies both via one `RequestHandler`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cef::rc::Rc as _;
use cef::{
    Browser, CefString, CefStringUtf16, Client, Frame, ImplClient, ImplRequest,
    ImplRequestHandler, Request, RequestHandler, TerminationStatus, WrapClient,
    WrapRequestHandler, wrap_client, wrap_request_handler,
};

use crate::scheme::PLUGIN_SCHEME;

/// Set from CEF's `on_render_process_terminated` (browser-process thread,
/// not necessarily the UI thread); polled from the GPUI pump tick. A plain
/// atomic is enough for one crash-happened bit — no lock, no queue.
#[derive(Clone)]
pub struct CrashFlag(Arc<AtomicBool>);

impl CrashFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Read-and-clear. `true` means the renderer for this browser terminated
    /// (crash, OOM-kill, `chrome://kill` in dev) since the last check.
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }

    fn mark(&self) {
        self.0.store(true, Ordering::Release);
    }
}

impl Default for CrashFlag {
    fn default() -> Self {
        Self::new()
    }
}

wrap_request_handler! {
    pub struct PluginRequestHandler {
        crash_flag: CrashFlag,
    }

    impl RequestHandler {
        // Only the plugin's own custom-scheme origin may ever load in this
        // browser. Blocks external navigation, `file://`, and any other
        // scheme a compromised or buggy page might try to reach — the editor
        // has no legitimate reason to navigate anywhere else, ever.
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let Some(request) = request else { return 0 };
            let url = CefStringUtf16::from(&request.url()).to_string();
            if is_allowed_navigation(&url) {
                0 // allow
            } else {
                eprintln!("[plugin-scheme] blocked navigation outside plugin origin url={url}");
                1 // cancel
            }
        }

        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            status: TerminationStatus,
            _error_code: ::std::os::raw::c_int,
            _error_string: Option<&CefString>,
        ) {
            eprintln!("[plugin-scheme] renderer process terminated status={status:?}");
            self.crash_flag.mark();
        }
    }
}

/// Same-origin navigations only (the editor's own `mikoplugin://` document),
/// plus the harmless `about:blank` CEF uses internally before a scheme
/// navigation resolves.
fn is_allowed_navigation(url: &str) -> bool {
    let prefix = format!("{PLUGIN_SCHEME}://");
    url.eq_ignore_ascii_case("about:blank") || url.to_ascii_lowercase().starts_with(&prefix)
}

wrap_client! {
    pub struct PluginBrowserClient {
        request_handler: RequestHandler,
    }

    impl Client {
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.request_handler.clone())
        }
    }
}

/// Build the client for one plugin editor browser and the crash flag its
/// owner should poll. Every browser gets its own — `on_before_browse`/
/// `on_render_process_terminated` fire per-browser, so there is no reason to
/// share one `Client` (and thus one `CrashFlag`) across unrelated windows.
pub fn plugin_browser_client() -> (Client, CrashFlag) {
    let crash_flag = CrashFlag::new();
    let handler = PluginRequestHandler::new(crash_flag.clone());
    (PluginBrowserClient::new(handler), crash_flag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_only_the_plugin_scheme_and_about_blank() {
        assert!(is_allowed_navigation("mikoplugin://rodharerist/index.html"));
        assert!(is_allowed_navigation("MIKOPLUGIN://rodharerist/"));
        assert!(is_allowed_navigation("about:blank"));
    }

    #[test]
    fn blocks_everything_else() {
        for url in [
            "https://example.com",
            "http://localhost:1234",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "chrome://settings",
            "",
        ] {
            assert!(!is_allowed_navigation(url), "should block {url}");
        }
    }

    #[test]
    fn crash_flag_reads_and_clears() {
        let flag = CrashFlag::new();
        assert!(!flag.take());
        flag.mark();
        assert!(flag.take());
        assert!(!flag.take());
    }
}
