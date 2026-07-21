//! BuildInHelper — embedded UI asset infrastructure for Built-in Plugins.
//!
//! Each Built-in Plugin dynamic library embeds its compiled React/Vite static UI
//! (`editorui/dist`) as immutable `&'static [u8]` slices. This crate provides the
//! **runtime** side: a normalized lookup table over those bytes plus the
//! [`EmbeddedPluginUi`] trait the shared CEF UI host resolves requests through.
//!
//! CEF is deliberately **not** referenced here. This crate only owns asset data
//! and safe path lookup; request/response handling belongs to `FutureboardNative`
//! or the shared Built-in Plugin UI host. The generator that produces the static
//! tables lives in [`generate`] (build-time only, behind the `generate` feature).
//!
//! ## Why this is its own crate
//!
//! Plugin crates use it from `build.rs`. Cargo compiles build-dependencies as a
//! separate unit, so every crate reachable from here is built twice. While this
//! code lived inside `BuiltinAudioPlugins`, that second unit also rebuilt the
//! eight plugin DSP crates — which are `crate-type = ["rlib", "cdylib"]` and
//! whose `.dll` outputs carry no metadata hash. Both units then raced to write
//! the same `target/debug/deps/<name>.dll`, giving intermittent LNK1104 link
//! failures. This crate is therefore kept dependency-free on purpose.
//!
//! ## Guarantees
//!
//! * Returned bytes are `&'static` — valid for the lifetime of the loaded library.
//!   The host must never free them.
//! * Path lookup normalizes slash direction, strips query/fragment, rejects
//!   parent traversal, maps `""`/`"/"` to `/index.html`, decodes `%xx` escapes and
//!   never panics on malformed input.

/// A single embedded UI file (one entry of a plugin's compiled `dist/`).
///
/// All fields are `'static`: the table is generated at build time and lives in
/// the plugin's read-only data segment.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedUiAsset {
    /// Normalized, absolute request path, e.g. `"/index.html"` or
    /// `"/assets/index-Dh82Ks.js"`. Always begins with `/` and uses `/` separators.
    pub path: &'static str,
    /// MIME type including charset for text formats.
    pub mime_type: &'static str,
    /// Immutable file contents.
    pub bytes: &'static [u8],
    /// Optional strong ETag / content hash for HTTP-style caching.
    pub etag: Option<&'static str>,
}

impl EmbeddedUiAsset {
    /// Byte length of the asset (useful for `Content-Length`).
    pub const fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether the asset has no bytes.
    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// An immutable, path-sorted table of a plugin's embedded UI assets.
///
/// The generator emits the slice sorted by [`EmbeddedUiAsset::path`], enabling a
/// binary search here. Construct with [`EmbeddedUiAssetTable::new`] from the
/// generated `&'static [EmbeddedUiAsset]`.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedUiAssetTable {
    assets: &'static [EmbeddedUiAsset],
}

impl EmbeddedUiAssetTable {
    /// Wrap a generated, path-sorted asset slice.
    pub const fn new(assets: &'static [EmbeddedUiAsset]) -> Self {
        Self { assets }
    }

    /// All assets, in deterministic path order.
    pub const fn assets(&self) -> &'static [EmbeddedUiAsset] {
        self.assets
    }

    /// Number of embedded assets.
    pub const fn len(&self) -> usize {
        self.assets.len()
    }

    /// Whether the plugin embeds no UI assets at all.
    pub const fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Strict lookup for a request path.
    ///
    /// Normalizes the path (see [`normalize_request_path`]), maps `""`/`"/"` to
    /// `/index.html`, then returns the exact asset or `None`. Never falls back to
    /// `index.html` for a missing *file* — use [`Self::resolve`] for SPA routing.
    pub fn get(&self, request_path: &str) -> Option<&'static EmbeddedUiAsset> {
        let normalized = normalize_request_path(request_path)?;
        self.lookup_exact(&normalized)
    }

    /// SPA-aware lookup.
    ///
    /// Returns the exact asset when present. Otherwise, for a client-side route
    /// (a request whose final segment has no file extension, e.g. `/settings`),
    /// falls back to `/index.html` so the React router can take over. A request
    /// for a missing *file* (with an extension, e.g. `/assets/missing.js`) still
    /// returns `None` so the host can emit a real 404.
    pub fn resolve(&self, request_path: &str) -> Option<&'static EmbeddedUiAsset> {
        let normalized = normalize_request_path(request_path)?;
        if let Some(asset) = self.lookup_exact(&normalized) {
            return Some(asset);
        }
        if looks_like_client_route(&normalized) {
            return self.lookup_exact("/index.html");
        }
        None
    }

    /// The `/index.html` entry, if the plugin embeds one.
    pub fn index(&self) -> Option<&'static EmbeddedUiAsset> {
        self.lookup_exact("/index.html")
    }

    fn lookup_exact(&self, normalized_path: &str) -> Option<&'static EmbeddedUiAsset> {
        match self
            .assets
            .binary_search_by(|asset| asset.path.cmp(normalized_path))
        {
            Ok(index) => Some(&self.assets[index]),
            // The table may not be sorted if hand-authored; fall back to a linear
            // scan so a mis-ordered slice degrades to correct-but-slower, not wrong.
            Err(_) => self.assets.iter().find(|asset| asset.path == normalized_path),
        }
    }
}

/// Implemented by each plugin (or its generated module) to expose its embedded UI
/// to the shared host without the host knowing the concrete table type.
pub trait EmbeddedPluginUi {
    /// Strict asset lookup (see [`EmbeddedUiAssetTable::get`]).
    fn get_ui_asset(path: &str) -> Option<EmbeddedUiAsset>;

    /// SPA-aware asset lookup (see [`EmbeddedUiAssetTable::resolve`]).
    fn resolve_ui_asset(path: &str) -> Option<EmbeddedUiAsset> {
        Self::get_ui_asset(path)
    }
}

/// Whether a normalized path looks like a client-side route rather than a static
/// file: no `.` in its final path segment (so `/`, `/library`, `/a/b` route to
/// the SPA shell, while `/favicon.ico` or `/assets/x.js` do not).
fn looks_like_client_route(normalized_path: &str) -> bool {
    let last = normalized_path.rsplit('/').next().unwrap_or_default();
    !last.contains('.')
}

/// Normalize an incoming request path to a canonical absolute asset key.
///
/// * strips a `?query` and `#fragment`
/// * decodes `%xx` percent-escapes (lenient: invalid escapes are kept verbatim)
/// * converts `\` to `/`
/// * drops `.` and empty segments, **rejects** any `..` (returns `None`)
/// * maps an empty result to `index.html`
/// * returns a `/`-prefixed, `/`-separated path
///
/// Never panics. Returns `None` only for traversal attempts, so callers can treat
/// `None` as "reject" and a returned string as "safe to look up".
pub fn normalize_request_path(raw: &str) -> Option<String> {
    // Strip query and fragment (whichever comes first).
    let end = raw.find(['?', '#']).unwrap_or(raw.len());
    let path = &raw[..end];

    let decoded = percent_decode(path);
    let unified = decoded.replace('\\', "/");

    let mut segments: Vec<&str> = Vec::new();
    for segment in unified.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            other => segments.push(other),
        }
    }

    if segments.is_empty() {
        return Some("/index.html".to_string());
    }

    let mut out = String::with_capacity(unified.len() + 1);
    for segment in &segments {
        out.push('/');
        out.push_str(segment);
    }
    Some(out)
}

/// Custom URL scheme the shared CEF host uses to serve embedded plugin UIs.
/// A plugin editor loads `mikoplugin://<plugin>/index.html`; the host maps
/// `<plugin>` to the loaded library's asset provider (one origin per plugin, so
/// one plugin can never read another's assets).
pub const PLUGIN_URL_SCHEME: &str = "mikoplugin";

/// A parsed `mikoplugin://<plugin>/<path>` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRequest {
    /// The plugin origin (URL host), e.g. `"rodharerist"`.
    pub plugin: String,
    /// The normalized asset path, e.g. `"/index.html"`.
    pub path: String,
}

/// Parse a `mikoplugin://<plugin>/<path>` URL into its plugin origin and a
/// normalized asset path.
///
/// * the scheme must be [`PLUGIN_URL_SCHEME`] (case-insensitive)
/// * the host (plugin id) must be non-empty and free of traversal
/// * an empty/`/` path maps to `/index.html`
/// * path normalization/traversal rejection matches [`normalize_request_path`]
///
/// Returns `None` for a wrong scheme, a missing/invalid plugin id, or a traversal
/// attempt. Never panics.
pub fn parse_plugin_url(url: &str) -> Option<PluginRequest> {
    let prefix = format!("{PLUGIN_URL_SCHEME}://");
    // Case-insensitive scheme match without allocating the whole lowercased URL.
    let rest = if url.len() >= prefix.len()
        && url[..prefix.len()].eq_ignore_ascii_case(&prefix)
    {
        &url[prefix.len()..]
    } else {
        return None;
    };

    // Host = up to the first path separator; the remainder (with its leading `/`)
    // is the asset path. Strip any query/fragment stuck to a bare host.
    let (host, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let host_end = host.find(['?', '#']).unwrap_or(host.len());
    let plugin = &host[..host_end];

    if plugin.is_empty() || plugin == "." || plugin == ".." || plugin.contains('\\') {
        return None;
    }

    let normalized = normalize_request_path(path)?;
    Some(PluginRequest {
        plugin: plugin.to_string(),
        path: normalized,
    })
}

/// Build a `mikoplugin://<plugin>/<path>` URL from a plugin id and asset path.
/// The path is normalized first (so `""`/`"/"` become `/index.html`).
pub fn build_plugin_url(plugin: &str, path: &str) -> String {
    let normalized = normalize_request_path(path).unwrap_or_else(|| "/index.html".to_string());
    format!("{PLUGIN_URL_SCHEME}://{plugin}{normalized}")
}

/// Lenient `%xx` percent-decoding. Invalid escapes are left untouched so the
/// function is total and never loses characters. `+` is **not** treated as space
/// (that is form-encoding, not path-encoding).
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hi = hex_value(bytes[index + 1]);
            let lo = hex_value(bytes[index + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi << 4) | lo);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    // Percent-decoded bytes may or may not be valid UTF-8; keep it lossless-ish
    // by replacing invalid sequences rather than failing.
    String::from_utf8_lossy(&out).into_owned()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Best-effort MIME type for a file extension (lower-cased, without the dot).
///
/// Covers the formats a Vite/React bundle produces. Unknown types fall back to
/// `application/octet-stream` rather than failing.
pub fn mime_for_extension(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// MIME type for a full (normalized or raw) path, keyed off its final extension.
pub fn mime_for_path(path: &str) -> &'static str {
    match path.rsplit_once('.') {
        Some((_, ext)) if !ext.contains('/') => mime_for_extension(ext),
        _ => "application/octet-stream",
    }
}

#[cfg(feature = "generate")]
pub mod generate;

#[cfg(test)]
mod tests {
    use super::*;

    // A tiny static table mirroring a Vite `dist/` with nested assets.
    static INDEX: EmbeddedUiAsset = EmbeddedUiAsset {
        path: "/index.html",
        mime_type: "text/html; charset=utf-8",
        bytes: b"<!doctype html>",
        etag: Some("aaaa"),
    };
    static APP_CSS: EmbeddedUiAsset = EmbeddedUiAsset {
        path: "/assets/index-A91kLm.css",
        mime_type: "text/css; charset=utf-8",
        bytes: b".x{}",
        etag: None,
    };
    static APP_JS: EmbeddedUiAsset = EmbeddedUiAsset {
        path: "/assets/index-Dh82Ks.js",
        mime_type: "text/javascript; charset=utf-8",
        bytes: b"console.log(1)",
        etag: None,
    };
    // Sorted by path: /assets/...css, /assets/...js, /index.html.
    static ASSETS: &[EmbeddedUiAsset] = &[APP_CSS, APP_JS, INDEX];

    fn table() -> EmbeddedUiAssetTable {
        EmbeddedUiAssetTable::new(ASSETS)
    }

    #[test]
    fn empty_and_root_map_to_index() {
        assert_eq!(normalize_request_path("").as_deref(), Some("/index.html"));
        assert_eq!(normalize_request_path("/").as_deref(), Some("/index.html"));
        assert_eq!(table().get("").unwrap().path, "/index.html");
        assert_eq!(table().get("/").unwrap().path, "/index.html");
    }

    #[test]
    fn query_string_and_fragment_are_stripped() {
        assert_eq!(
            normalize_request_path("/assets/index-Dh82Ks.js?v=123").as_deref(),
            Some("/assets/index-Dh82Ks.js")
        );
        assert_eq!(
            normalize_request_path("/index.html#/route").as_deref(),
            Some("/index.html")
        );
        assert!(table().get("/assets/index-Dh82Ks.js?v=9").is_some());
    }

    #[test]
    fn backslashes_are_normalized() {
        assert_eq!(
            normalize_request_path("\\assets\\index-A91kLm.css").as_deref(),
            Some("/assets/index-A91kLm.css")
        );
        assert!(table().get("\\assets\\index-A91kLm.css").is_some());
    }

    #[test]
    fn traversal_is_rejected() {
        assert!(normalize_request_path("/../secret").is_none());
        assert!(normalize_request_path("/assets/../../etc/passwd").is_none());
        assert!(normalize_request_path("..%2f..%2fx").is_none());
        assert!(table().get("/assets/../../secret").is_none());
    }

    #[test]
    fn percent_encoding_is_decoded() {
        assert_eq!(
            normalize_request_path("/assets/index%2DA91kLm.css").as_deref(),
            Some("/assets/index-A91kLm.css")
        );
        // Invalid escape is preserved rather than panicking.
        assert_eq!(
            normalize_request_path("/a%zz").as_deref(),
            Some("/a%zz")
        );
    }

    #[test]
    fn missing_file_is_none() {
        assert!(table().get("/assets/missing.js").is_none());
    }

    #[test]
    fn resolve_falls_back_to_index_for_routes_only() {
        // Client route (no extension) -> index.html.
        assert_eq!(table().resolve("/library").unwrap().path, "/index.html");
        assert_eq!(table().resolve("/a/b/c").unwrap().path, "/index.html");
        // Missing file with an extension -> real 404 (None).
        assert!(table().resolve("/assets/missing.js").is_none());
        // Present file -> itself.
        assert_eq!(
            table().resolve("/assets/index-Dh82Ks.js").unwrap().path,
            "/assets/index-Dh82Ks.js"
        );
    }

    #[test]
    fn table_ordering_is_deterministic() {
        let paths: Vec<_> = table().assets().iter().map(|a| a.path).collect();
        let mut sorted = paths.clone();
        sorted.sort_unstable();
        assert_eq!(paths, sorted);
    }

    #[test]
    fn nested_assets_are_found() {
        assert_eq!(
            table().get("/assets/index-A91kLm.css").unwrap().mime_type,
            "text/css; charset=utf-8"
        );
    }

    #[test]
    fn plugin_without_ui_returns_nothing() {
        static EMPTY: &[EmbeddedUiAsset] = &[];
        let empty = EmbeddedUiAssetTable::new(EMPTY);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert!(empty.get("/index.html").is_none());
        assert!(empty.resolve("/anything").is_none());
        assert!(empty.index().is_none());
    }

    #[test]
    fn mime_detection_covers_bundle_formats() {
        assert_eq!(mime_for_extension("html"), "text/html; charset=utf-8");
        assert_eq!(mime_for_extension("JS"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for_extension("mjs"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for_extension("css"), "text/css; charset=utf-8");
        assert_eq!(mime_for_extension("json"), "application/json; charset=utf-8");
        assert_eq!(mime_for_extension("svg"), "image/svg+xml");
        assert_eq!(mime_for_extension("png"), "image/png");
        assert_eq!(mime_for_extension("jpg"), "image/jpeg");
        assert_eq!(mime_for_extension("jpeg"), "image/jpeg");
        assert_eq!(mime_for_extension("webp"), "image/webp");
        assert_eq!(mime_for_extension("gif"), "image/gif");
        assert_eq!(mime_for_extension("ico"), "image/x-icon");
        assert_eq!(mime_for_extension("woff"), "font/woff");
        assert_eq!(mime_for_extension("woff2"), "font/woff2");
        assert_eq!(mime_for_extension("ttf"), "font/ttf");
        assert_eq!(mime_for_extension("wasm"), "application/wasm");
    }

    #[test]
    fn unknown_mime_falls_back_to_octet_stream() {
        assert_eq!(mime_for_extension("xyz"), "application/octet-stream");
        assert_eq!(mime_for_path("/assets/data.bin"), "application/octet-stream");
        assert_eq!(mime_for_path("/no-extension"), "application/octet-stream");
    }

    #[test]
    fn parses_mikoplugin_url() {
        let req = parse_plugin_url("mikoplugin://rodharerist/index.html").unwrap();
        assert_eq!(req.plugin, "rodharerist");
        assert_eq!(req.path, "/index.html");

        // Bare host maps to index.html.
        let root = parse_plugin_url("mikoplugin://rodharerist").unwrap();
        assert_eq!(root.path, "/index.html");
        assert_eq!(parse_plugin_url("mikoplugin://rodharerist/").unwrap().path, "/index.html");

        // Scheme is case-insensitive; query strings are stripped.
        assert_eq!(
            parse_plugin_url("MikoPlugin://rodharerist/index.html?v=1").unwrap().path,
            "/index.html"
        );
    }

    #[test]
    fn rejects_bad_plugin_urls() {
        assert!(parse_plugin_url("https://rodharerist/index.html").is_none());
        assert!(parse_plugin_url("mikoplugin:///index.html").is_none()); // empty host
        assert!(parse_plugin_url("mikoplugin://rodharerist/../secret").is_none());
        assert!(parse_plugin_url("mikoplugin://../index.html").is_none());
    }

    #[test]
    fn builds_and_round_trips_plugin_url() {
        assert_eq!(
            build_plugin_url("rodharerist", "index.html"),
            "mikoplugin://rodharerist/index.html"
        );
        // Empty path defaults to index.html.
        assert_eq!(
            build_plugin_url("rodharerist", ""),
            "mikoplugin://rodharerist/index.html"
        );
        let url = build_plugin_url("rodharerist", "/index.html");
        let req = parse_plugin_url(&url).unwrap();
        assert_eq!(req.plugin, "rodharerist");
        assert_eq!(req.path, "/index.html");
    }
}
