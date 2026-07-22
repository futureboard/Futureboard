//! Custom-scheme hosting for embedded plugin editor UIs.
//!
//! A built-in plugin's compiled React editor lives in its own library as a
//! `&'static [u8]` table (see `builtin_audio_plugins::ui`). This module serves
//! that table to CEF under a custom scheme so the editor loads as a real
//! document with a proper origin, rather than through a `file://` path or a
//! `data:` URL.
//!
//! ## Registration order matters
//!
//! CEF requires a custom scheme to be declared in *every* process, from
//! `cef_app_t::on_register_custom_schemes`. That means the same [`App`] must be
//! handed to both [`crate::runtime::execute_subprocess`] and
//! [`crate::runtime::CefRuntime::initialize`] — a factory registered only in
//! the browser process will not make the scheme resolvable in the renderer.
//! [`plugin_scheme_app`] builds that app; [`register_plugin_scheme_factory`]
//! installs the handler and must be called *after* initialize succeeds.
//!
//! ## Threading
//!
//! Factory and handler methods are invoked on CEF's IO thread, not the UI
//! thread. The resolver is therefore `Send + Sync` and must not block: it is
//! only ever expected to index a static table.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cef::rc::Rc as _;
// The `wrap_*!` macros expand to `impl Impl<T> for` / `impl Wrap<T> for`
// blocks, so every one of these traits has to be nameable here.
use cef::{
    App, CefString, CefStringUtf16, ImplApp, ImplPostData, ImplPostDataElement, ImplRequest,
    ImplResourceHandler, ImplResponse, ImplSchemeHandlerFactory, ImplSchemeRegistrar,
    ResourceHandler, SchemeHandlerFactory, SchemeOptions, WrapApp, WrapResourceHandler,
    WrapSchemeHandlerFactory, wrap_app, wrap_resource_handler, wrap_scheme_handler_factory,
};

/// Scheme built-in plugin editors are served under. Must match
/// `builtin_audio_plugins::ui::PLUGIN_URL_SCHEME`.
pub const PLUGIN_SCHEME: &str = "mikoplugin";

/// Reserved path React's bridge client POSTs JSON envelopes to
/// (`futureboard.requestSelectInstance`, `instanceReady`, `bridgeReady`,
/// `rodhareist.setParam`, ...). Not a real asset path — intercepted before it
/// ever reaches the plugin's resolver, so no plugin's asset table needs to
/// know about it or can shadow it.
const BRIDGE_PATH: &str = "/__bridge";

const BRIDGE_ACK: SchemeAsset = SchemeAsset {
    bytes: b"{\"ok\":true}",
    mime_type: "application/json; charset=utf-8",
};

/// One resolved asset. Bytes are `'static` because they live in a loaded
/// library's read-only data segment — nothing is copied to serve a request.
#[derive(Debug, Clone, Copy)]
pub struct SchemeAsset {
    pub bytes: &'static [u8],
    pub mime_type: &'static str,
}

/// Resolves `mikoplugin://<plugin>/<path>` to embedded bytes.
///
/// Returning `None` produces a 404. The resolver is responsible for rejecting
/// unknown plugin origins, which is what keeps one plugin's editor from reading
/// another's assets.
pub type SchemeResolver = Arc<dyn Fn(&str, &str) -> Option<SchemeAsset> + Send + Sync>;

/// Delivers one React->native bridge message: the raw JSON body POSTed to
/// `mikoplugin://<plugin>/__bridge`. Runs on CEF's IO thread like the
/// resolver — must not block, and is expected to just enqueue the bytes for a
/// later, non-realtime drain (see `builtin_plugin_editor::take_inbound`).
pub type BridgeSink = Arc<dyn Fn(&str, Vec<u8>) + Send + Sync>;

// Declares the plugin scheme in every CEF process. (`wrap_app!` matches on
// `$vis:vis struct`, so the description cannot be a doc comment here.)
wrap_app! {
    pub struct PluginSchemeApp;

    impl App {
        fn on_register_custom_schemes(&self, registrar: Option<&mut cef::SchemeRegistrar>) {
            let Some(registrar) = registrar else { return };
            // STANDARD gives the scheme real origin semantics (so the editor
            // gets a stable origin and relative URLs resolve). SECURE keeps it
            // out of Chromium's mixed-content and insecure-origin penalty box,
            // which otherwise blocks APIs the editor relies on. CORS_ENABLED
            // lets the document fetch its own sibling assets. FETCH_ENABLED is
            // separate from CORS_ENABLED and required for `fetch()` itself to
            // be allowed against this scheme at all — without it, the React
            // bridge's `fetch("__bridge", ...)` (its only way to reach native)
            // fails silently, native never sees `bridgeReady`, and the editor
            // is stuck showing its own empty state forever.
            let options = SchemeOptions::STANDARD.get_raw()
                | SchemeOptions::SECURE.get_raw()
                | SchemeOptions::CORS_ENABLED.get_raw()
                | SchemeOptions::FETCH_ENABLED.get_raw();
            registrar.add_custom_scheme(Some(&CefString::from(PLUGIN_SCHEME)), options);
        }
    }
}

// Creates one `ResourceHandler` per `mikoplugin://` request.
wrap_scheme_handler_factory! {
    pub struct PluginSchemeFactory {
        resolver: SchemeResolver,
        bridge: Option<BridgeSink>,
    }

    impl SchemeHandlerFactory {
        fn create(
            &self,
            _browser: Option<&mut cef::Browser>,
            _frame: Option<&mut cef::Frame>,
            _scheme_name: Option<&CefString>,
            request: Option<&mut cef::Request>,
        ) -> Option<ResourceHandler> {
            let request = request?;
            // `url()` hands back a CEF-owned userfree string; copy it into an
            // owned Rust string before the borrow ends.
            let url = CefStringUtf16::from(&request.url()).to_string();
            let (plugin, path) = split_plugin_url(&url)?;

            if path == BRIDGE_PATH {
                let method = CefStringUtf16::from(&request.method()).to_string();
                if method.eq_ignore_ascii_case("POST") {
                    if let Some(sink) = &self.bridge {
                        let body = read_post_data(request);
                        if std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some() {
                            eprintln!(
                                "[plugin-bridge] inbound plugin={plugin} bytes={}",
                                body.len()
                            );
                        }
                        sink(&plugin, body);
                    }
                }
                return Some(PluginAssetHandler::new(
                    Some(BRIDGE_ACK),
                    AtomicUsize::new(0).into(),
                ));
            }

            // A miss still gets a handler, so the renderer sees a clean 404
            // instead of a failed-to-load network error.
            let asset = (self.resolver)(&plugin, &path);
            if std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some() {
                eprintln!(
                    "[plugin-scheme] request url={url} plugin={plugin} path={path} resolved={}",
                    asset.is_some()
                );
            }
            Some(PluginAssetHandler::new(asset, AtomicUsize::new(0).into()))
        }
    }
}

/// Concatenate every `PostDataElement`'s bytes. Bounded by whatever the page
/// actually POSTs (one JSON envelope) — never audio-rate, never large.
fn read_post_data(request: &mut cef::Request) -> Vec<u8> {
    let Some(post_data) = request.post_data() else {
        return Vec::new();
    };
    // `PostData::elements` uses the *current length* of the passed-in `Vec`
    // as the output buffer size (it does not grow it) — an empty vec means
    // "give me zero elements", not "give me however many there are". Must
    // pre-size to `element_count()` first.
    let count = post_data.element_count();
    if count == 0 {
        return Vec::new();
    }
    let mut elements: Vec<Option<cef::PostDataElement>> = (0..count).map(|_| None).collect();
    post_data.elements(Some(&mut elements));
    let mut out = Vec::new();
    for element in elements.into_iter().flatten() {
        let count = element.bytes_count();
        if count == 0 {
            continue;
        }
        let mut buf = vec![0u8; count];
        let read = element.bytes(count, buf.as_mut_ptr());
        buf.truncate(read);
        out.extend_from_slice(&buf);
    }
    out
}

// Serves one in-memory asset. No I/O, no allocation of the payload.
wrap_resource_handler! {
    pub struct PluginAssetHandler {
        asset: Option<SchemeAsset>,
        cursor: Arc<AtomicUsize>,
    }

    impl ResourceHandler {
        fn open(
            &self,
            _request: Option<&mut cef::Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut cef::Callback>,
        ) -> ::std::os::raw::c_int {
            // The bytes are already in memory: complete synchronously rather
            // than making CEF wait on a callback.
            if let Some(handle_request) = handle_request {
                *handle_request = 1;
            }
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut cef::Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let Some(response) = response else { return };
            match self.asset {
                Some(asset) => {
                    // `mime_type` is `"text/html; charset=utf-8"`; CEF's mime
                    // field wants the bare type, with charset set separately
                    // so the renderer builds the right `Content-Type` header
                    // instead of embedding the charset in the type token.
                    let (mime, charset) = split_mime(asset.mime_type);
                    let status_text = CefString::from("OK");
                    let mime_string = CefString::from(mime);
                    response.set_status(200);
                    response.set_status_text(Some(&status_text));
                    response.set_mime_type(Some(&mime_string));
                    if let Some(charset) = charset {
                        response.set_charset(Some(&CefString::from(charset)));
                    }
                    // Belt-and-suspenders: some CEF network-service code paths
                    // read the actual `Content-Type` header rather than the
                    // response object's mime/charset fields when building the
                    // document's resource response, so set both.
                    response.set_header_by_name(
                        Some(&CefString::from("Content-Type")),
                        Some(&CefString::from(asset.mime_type)),
                        1,
                    );
                    // Embedded assets are immutable for the life of the
                    // process, so let the renderer cache them freely.
                    response.set_header_by_name(
                        Some(&CefString::from("Cache-Control")),
                        Some(&CefString::from("public, max-age=31536000, immutable")),
                        1,
                    );
                    if let Some(length) = response_length {
                        *length = asset.bytes.len() as i64;
                    }
                    if std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some() {
                        let readback = CefStringUtf16::from(&response.mime_type()).to_string();
                        eprintln!(
                            "[plugin-scheme] status=200 status_text=OK mime_set={mime} \
                             mime_readback={readback} length={} ",
                            asset.bytes.len()
                        );
                    }
                }
                None => {
                    response.set_status(404);
                    response.set_status_text(Some(&CefString::from("Not Found")));
                    response.set_mime_type(Some(&CefString::from("text/plain")));
                    if let Some(length) = response_length {
                        *length = 0;
                    }
                    if std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some() {
                        eprintln!("[plugin-scheme] status=404 (no asset resolved)");
                    }
                }
            }
        }

        // `ImplResourceHandler::read` is a safe trait method taking a raw
        // pointer — the signature is fixed by cef-rs and cannot be marked
        // `unsafe`. CEF guarantees `data_out` points to at least
        // `bytes_to_read` writable bytes; both are validated below before use.
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut cef::ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(bytes_read) = bytes_read else { return 0 };
            *bytes_read = 0;

            let Some(asset) = self.asset else { return 0 };
            if data_out.is_null() || bytes_to_read <= 0 {
                return 0;
            }

            let offset = self.cursor.load(Ordering::Relaxed);
            let remaining = asset.bytes.len().saturating_sub(offset);
            if remaining == 0 {
                // Returning 0 with *bytes_read == 0 signals end of stream.
                return 0;
            }

            let count = remaining.min(bytes_to_read as usize);
            // SAFETY: `data_out` is a CEF-owned buffer of at least
            // `bytes_to_read` bytes; `count <= bytes_to_read` and the source
            // range is inside the static asset.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    asset.bytes[offset..].as_ptr(),
                    data_out,
                    count,
                );
            }
            self.cursor.store(offset + count, Ordering::Relaxed);
            *bytes_read = count as ::std::os::raw::c_int;
            1
        }
    }
}

/// Split a `mime_type; charset=...` string into the bare MIME type and, when
/// present, the charset value. CEF's response mime-type field expects the
/// bare type; a charset embedded in it (e.g. `"text/html; charset=utf-8"`
/// stored as one token) is not parsed apart by the renderer and can leave it
/// unable to sniff the type as HTML.
fn split_mime(mime_type: &str) -> (&str, Option<&str>) {
    match mime_type.split_once(';') {
        Some((mime, rest)) => {
            let charset = rest
                .trim()
                .strip_prefix("charset=")
                .map(str::trim);
            (mime.trim(), charset)
        }
        None => (mime_type.trim(), None),
    }
}

/// Split `mikoplugin://<plugin>/<path>` into its origin and path.
///
/// Deliberately conservative: any `..` segment rejects the whole request, so a
/// traversal can never reach the resolver. An empty path maps to `/index.html`.
/// Returns `None` for a foreign scheme or an empty origin.
pub fn split_plugin_url(url: &str) -> Option<(String, String)> {
    let prefix = format!("{PLUGIN_SCHEME}://");
    if url.len() < prefix.len() || !url[..prefix.len()].eq_ignore_ascii_case(&prefix) {
        return None;
    }
    let rest = &url[prefix.len()..];

    let (host, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let host_end = host.find(['?', '#']).unwrap_or(host.len());
    let plugin = &host[..host_end];
    if plugin.is_empty() || plugin.contains('\\') || plugin == "." || plugin == ".." {
        return None;
    }

    let path_end = path.find(['?', '#']).unwrap_or(path.len());
    let mut segments: Vec<&str> = Vec::new();
    for segment in path[..path_end].split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            other => segments.push(other),
        }
    }
    let normalized = if segments.is_empty() {
        "/index.html".to_string()
    } else {
        let mut out = String::with_capacity(path_end + 1);
        for segment in &segments {
            out.push('/');
            out.push_str(segment);
        }
        out
    };

    Some((plugin.to_ascii_lowercase(), normalized))
}

/// Build the [`App`] that declares the plugin scheme. Pass the **same** app to
/// both `execute_subprocess` and `CefRuntime::initialize`.
pub fn plugin_scheme_app() -> App {
    // Constructing a `cef::App` is itself a CEF object creation, so the API
    // version has to be bound *here* — not by whatever consumes the app later.
    // Without this the process aborts on the first C→C++ call with
    // `CefApp_0_CToCpp called with invalid version -1`.
    crate::runtime::ensure_api_version();
    PluginSchemeApp::new()
}

/// Install the handler that serves plugin assets. Call once, after
/// `CefRuntime::initialize` has returned successfully.
///
/// `domain_name` is `None` so the factory serves every origin under the scheme;
/// per-plugin isolation is enforced by the resolver, which only answers for
/// plugin ids it knows.
pub fn register_plugin_scheme_factory(
    resolver: SchemeResolver,
    bridge: Option<BridgeSink>,
) -> Result<(), SchemeError> {
    let mut factory = PluginSchemeFactory::new(resolver, bridge);
    let ok = cef::register_scheme_handler_factory(
        Some(&CefString::from(PLUGIN_SCHEME)),
        None,
        Some(&mut factory),
    );
    if ok == 0 {
        return Err(SchemeError::RegisterFactoryFailed);
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum SchemeError {
    #[error("CEF rejected the {PLUGIN_SCHEME} scheme handler factory")]
    RegisterFactoryFailed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_mime_and_charset() {
        assert_eq!(
            split_mime("text/html; charset=utf-8"),
            ("text/html", Some("utf-8"))
        );
        assert_eq!(split_mime("application/wasm"), ("application/wasm", None));
        assert_eq!(
            split_mime("text/javascript;charset=utf-8"),
            ("text/javascript", Some("utf-8"))
        );
    }

    #[test]
    fn parses_plugin_and_path() {
        assert_eq!(
            split_plugin_url("mikoplugin://rodharerist/index.html"),
            Some(("rodharerist".into(), "/index.html".into()))
        );
        assert_eq!(
            split_plugin_url("mikoplugin://rodharerist/assets/app-A1b2.js"),
            Some(("rodharerist".into(), "/assets/app-A1b2.js".into()))
        );
    }

    #[test]
    fn bare_origin_and_root_map_to_index() {
        for url in [
            "mikoplugin://rodharerist",
            "mikoplugin://rodharerist/",
            "mikoplugin://rodharerist/?v=2",
        ] {
            assert_eq!(
                split_plugin_url(url),
                Some(("rodharerist".into(), "/index.html".into())),
                "{url}"
            );
        }
    }

    #[test]
    fn scheme_match_is_case_insensitive_and_origin_is_normalized() {
        assert_eq!(
            split_plugin_url("MikoPlugin://Rodhareist/index.html"),
            Some(("rodhareist".into(), "/index.html".into()))
        );
    }

    #[test]
    fn query_and_fragment_are_stripped_from_the_path() {
        assert_eq!(
            split_plugin_url("mikoplugin://rod/a.js?v=1#x"),
            Some(("rod".into(), "/a.js".into()))
        );
    }

    #[test]
    fn traversal_is_rejected_before_reaching_the_resolver() {
        for url in [
            "mikoplugin://rodharerist/../secret",
            "mikoplugin://rodharerist/assets/../../secret",
            "mikoplugin://../index.html",
            "mikoplugin://../../index.html",
        ] {
            assert_eq!(split_plugin_url(url), None, "{url}");
        }
    }

    #[test]
    fn foreign_schemes_and_empty_origins_are_rejected() {
        assert_eq!(split_plugin_url("https://example.com/index.html"), None);
        assert_eq!(split_plugin_url("mikoplugin:///index.html"), None);
        assert_eq!(split_plugin_url("mikoplugin://"), None);
        assert_eq!(split_plugin_url(""), None);
    }

    #[test]
    fn never_panics_on_malformed_input() {
        for url in [
            "mikoplugin",
            "mikoplugin:",
            "mikoplugin:/",
            "mikoplugin://%",
            "mikoplugin://a/%zz",
            "mikoplugin://a\\b/c",
        ] {
            let _ = split_plugin_url(url);
        }
    }
}
