//! Build-time generator for the embedded UI asset table.
//!
//! Runs from a plugin crate's `build.rs` (behind the `ui-generate` feature) to
//! turn a Vite/React `editorui/dist` tree into a deterministic Rust source file
//! that the crate `include!`s. It never hard-codes Vite's hashed filenames — the
//! table is enumerated from `dist/` every build.
//!
//! ## Why bytes go through `OUT_DIR`
//!
//! The generated `.rs` references file contents with
//! `include_bytes!(concat!(env!("OUT_DIR"), "/ui_assets/<path>"))`. Each `dist`
//! file is copied under `OUT_DIR/ui_assets` first. This keeps **absolute
//! developer-machine paths out of the generated source and the plugin binary** —
//! the only path token in the output is `env!("OUT_DIR")`, resolved on the
//! consumer's machine at compile time.
//!
//! ## Typical usage (`build.rs`)
//!
//! ```no_run
//! # #[cfg(feature = "ui-generate")]
//! # fn main() {
//! use std::path::PathBuf;
//! let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
//! let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("editorui/dist");
//! let report = builtin_audio_plugins::ui::generate::generate(
//!     &builtin_audio_plugins::ui::generate::GenerateOptions::from_out_dir(dist, out_dir),
//! )
//! .expect("embed editor UI");
//! eprintln!("embedded {} UI assets", report.asset_count);
//! # }
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{mime_for_path, normalize_request_path};

/// Options controlling generation.
pub struct GenerateOptions {
    /// The built static site directory (`editorui/dist`).
    pub dist_dir: PathBuf,
    /// The `.rs` file to write (usually `OUT_DIR/embedded_ui_assets.rs`).
    pub out_file: PathBuf,
    /// Directory the asset bytes are copied into. **Must** be
    /// `<OUT_DIR>/<assets_subdir>` for the emitted `include_bytes!` paths to
    /// resolve.
    pub assets_dir: PathBuf,
    /// Sub-path (relative to `OUT_DIR`) used both for [`Self::assets_dir`] and the
    /// emitted `concat!(env!("OUT_DIR"), "/<subdir>/...")` include paths.
    pub assets_subdir: String,
    /// Fully-qualified path to the runtime asset type in the generated code.
    pub asset_type_path: String,
    /// Identifier of the generated `static` slice.
    pub table_ident: String,
    /// Emit `cargo:rerun-if-changed` lines to stdout for incremental rebuilds.
    pub emit_rerun: bool,
}

impl GenerateOptions {
    /// Conventional options: write `OUT_DIR/embedded_ui_assets.rs`, stage bytes in
    /// `OUT_DIR/ui_assets`, reference `::builtin_audio_plugins::ui::EmbeddedUiAsset`
    /// and name the slice `EMBEDDED_UI_ASSETS`.
    pub fn from_out_dir(dist_dir: PathBuf, out_dir: PathBuf) -> Self {
        let assets_subdir = "ui_assets".to_string();
        Self {
            dist_dir,
            out_file: out_dir.join("embedded_ui_assets.rs"),
            assets_dir: out_dir.join(&assets_subdir),
            assets_subdir,
            asset_type_path: "::builtin_audio_plugins::ui::EmbeddedUiAsset".to_string(),
            table_ident: "EMBEDDED_UI_ASSETS".to_string(),
            emit_rerun: true,
        }
    }
}

/// Outcome of a generation run.
#[derive(Debug, Clone)]
pub struct GenerateReport {
    /// Number of assets written into the table.
    pub asset_count: usize,
    /// Whether a real `dist/` with an `index.html` was found. `false` means an
    /// **empty** table was emitted so the crate still compiles.
    pub dist_present: bool,
}

/// One discovered asset, pre-sorted for deterministic output.
struct Entry {
    /// Normalized URL path (`/index.html`, `/assets/x.js`).
    url_path: String,
    /// Path relative to `dist_dir`, using `/` separators (used for the copied
    /// file location and the `include_bytes!` suffix).
    rel_path: String,
    /// Absolute source path in `dist/`.
    source: PathBuf,
    /// FNV-1a content hash, hex, used as the ETag.
    etag: String,
}

/// Generate the embedded asset table.
///
/// If `dist_dir` (or its `index.html`) is missing, an **empty** table is written
/// and `dist_present` is `false`; this never fails a build for a plugin whose UI
/// has not been built. Returns an error only on real I/O failures.
pub fn generate(options: &GenerateOptions) -> io::Result<GenerateReport> {
    if options.emit_rerun {
        // Re-run when the built site changes (added/removed/edited files).
        println!("cargo:rerun-if-changed={}", options.dist_dir.display());
    }

    let index = options.dist_dir.join("index.html");
    if !options.dist_dir.is_dir() || !index.is_file() {
        write_source(options, &[])?;
        return Ok(GenerateReport {
            asset_count: 0,
            dist_present: false,
        });
    }

    let mut entries = Vec::new();
    collect(&options.dist_dir, &options.dist_dir, &mut entries)?;
    // Deterministic ordering by URL path (matches the runtime binary search).
    entries.sort_by(|a, b| a.url_path.cmp(&b.url_path));

    // Stage bytes under OUT_DIR and (optionally) track each source for rebuilds.
    if options.assets_dir.exists() {
        fs::remove_dir_all(&options.assets_dir)?;
    }
    for entry in &entries {
        let dest = options.assets_dir.join(&entry.rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&entry.source, &dest)?;
        if options.emit_rerun {
            println!("cargo:rerun-if-changed={}", entry.source.display());
        }
    }

    write_source(options, &entries)?;
    Ok(GenerateReport {
        asset_count: entries.len(),
        dist_present: true,
    })
}

/// Recursively gather files under `dir`, computing URL/rel paths against `root`.
fn collect(root: &Path, dir: &Path, out: &mut Vec<Entry>) -> io::Result<()> {
    let mut children: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<io::Result<_>>()?;
    // Sort for deterministic traversal (final ordering is by URL path anyway).
    children.sort();

    for path in children {
        if path.is_dir() {
            collect(root, &path, out)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let rel_path = to_forward_slashes(relative);
        // Normalize into a URL key (leading slash, no traversal). `dist` output is
        // always well-formed, so normalization should not reject anything.
        let url_path = normalize_request_path(&rel_path)
            .unwrap_or_else(|| format!("/{rel_path}"));
        let bytes = fs::read(&path)?;
        out.push(Entry {
            url_path,
            rel_path,
            source: path,
            etag: fnv1a_hex(&bytes),
        });
    }
    Ok(())
}

/// Write the generated Rust source (an empty slice when `entries` is empty).
fn write_source(options: &GenerateOptions, entries: &[Entry]) -> io::Result<()> {
    let type_path = &options.asset_type_path;
    let mut source = String::new();
    source.push_str("// @generated by builtin_audio_plugins::ui::generate — do not edit.\n");
    source.push_str(&format!(
        "pub static {}: &[{}] = &[\n",
        options.table_ident, type_path
    ));

    for entry in entries {
        let include = format!(
            "concat!(env!(\"OUT_DIR\"), \"/{}/{}\")",
            options.assets_subdir, entry.rel_path
        );
        source.push_str(&format!("    {type_path} {{\n"));
        source.push_str(&format!("        path: {},\n", rust_str(&entry.url_path)));
        source.push_str(&format!(
            "        mime_type: {},\n",
            rust_str(mime_for_path(&entry.url_path))
        ));
        source.push_str(&format!("        bytes: include_bytes!({include}),\n"));
        source.push_str(&format!("        etag: Some({}),\n", rust_str(&entry.etag)));
        source.push_str("    },\n");
    }

    source.push_str("];\n");
    if let Some(parent) = options.out_file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.out_file, source)
}

/// Render a `&str` as a Rust string literal, escaping the few metacharacters that
/// can appear in a normalized path or generated hash.
fn rust_str(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn to_forward_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// FNV-1a 64-bit, rendered as lowercase hex. Deterministic and dependency-free —
/// enough for an ETag / cache key (not a security hash).
fn fnv1a_hex(bytes: &[u8]) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn options(dist: PathBuf, out: PathBuf) -> GenerateOptions {
        let mut opts = GenerateOptions::from_out_dir(dist, out);
        opts.emit_rerun = false; // keep test output clean
        opts
    }

    #[test]
    fn missing_dist_emits_empty_table() {
        let temp = tempfile::tempdir().unwrap();
        let dist = temp.path().join("editorui/dist"); // never created
        let out = temp.path().join("out");
        fs::create_dir_all(&out).unwrap();

        let report = generate(&options(dist, out.clone())).unwrap();
        assert!(!report.dist_present);
        assert_eq!(report.asset_count, 0);

        let generated = fs::read_to_string(out.join("embedded_ui_assets.rs")).unwrap();
        assert!(generated.contains("EMBEDDED_UI_ASSETS: &[::builtin_audio_plugins::ui::EmbeddedUiAsset] = &[\n];"));
    }

    #[test]
    fn nested_assets_are_enumerated_sorted_and_pathless() {
        let temp = tempfile::tempdir().unwrap();
        let dist = temp.path().join("dist");
        write(&dist.join("index.html"), b"<!doctype html>");
        write(&dist.join("assets/index-Dh82Ks.js"), b"console.log(1)");
        write(&dist.join("assets/index-A91kLm.css"), b".x{}");
        let out = temp.path().join("out");
        fs::create_dir_all(&out).unwrap();

        let report = generate(&options(dist, out.clone())).unwrap();
        assert!(report.dist_present);
        assert_eq!(report.asset_count, 3);

        let generated = fs::read_to_string(out.join("embedded_ui_assets.rs")).unwrap();
        // Deterministic order: css, js, index.html (by URL path).
        let css = generated.find("/assets/index-A91kLm.css").unwrap();
        let js = generated.find("/assets/index-Dh82Ks.js").unwrap();
        let idx = generated.find("/index.html").unwrap();
        assert!(css < js && js < idx, "assets must be path-sorted");

        // MIME detection wired through.
        assert!(generated.contains("text/javascript; charset=utf-8"));
        assert!(generated.contains("text/css; charset=utf-8"));
        assert!(generated.contains("text/html; charset=utf-8"));

        // No absolute developer path leaked; only OUT_DIR is referenced.
        assert!(generated.contains("env!(\"OUT_DIR\")"));
        assert!(!generated.contains(temp.path().to_string_lossy().as_ref()));

        // Bytes were staged under OUT_DIR/ui_assets for include_bytes!.
        assert!(out.join("ui_assets/index.html").is_file());
        assert!(out.join("ui_assets/assets/index-Dh82Ks.js").is_file());
    }

    #[test]
    fn output_is_deterministic_across_runs() {
        let temp = tempfile::tempdir().unwrap();
        let dist = temp.path().join("dist");
        write(&dist.join("index.html"), b"<!doctype html>");
        write(&dist.join("assets/app.js"), b"1");
        let out = temp.path().join("out");
        fs::create_dir_all(&out).unwrap();

        generate(&options(dist.clone(), out.clone())).unwrap();
        let first = fs::read_to_string(out.join("embedded_ui_assets.rs")).unwrap();
        generate(&options(dist, out.clone())).unwrap();
        let second = fs::read_to_string(out.join("embedded_ui_assets.rs")).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn etag_reflects_content() {
        let a = fnv1a_hex(b"hello");
        let b = fnv1a_hex(b"hello");
        let c = fnv1a_hex(b"world");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
    }
}
