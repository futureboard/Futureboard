//! Stage the shared CEF runtime **flat** beside the application binary.
//!
//! CEF is a single, shared runtime — never one copy per plugin and never a
//! `CEF/` subdirectory. Chromium resolves `libcef`, the `.pak` resources,
//! `icudtl.dat` and `locales/` relative to the host executable's own directory,
//! so every required file is copied directly into the staging root (with
//! `locales/` kept as the one subdirectory CEF requires).
//!
//! The file list is taken from the CEF distribution the repository already uses
//! (`<workspace>/build/cef`, populated by `SphereWebView`'s `install_cef`). We do
//! not download another runtime.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::platform::dynamic_library_extension;
use crate::staging::copy_into;

/// Directory (relative to `build/cef`) holding the runtime binaries.
const RELEASE_DIR: &str = "Release";
/// Directory (relative to `build/cef`) holding the `.pak`/`icu` resources.
const RESOURCES_DIR: &str = "Resources";
const PINNED_CEF_VERSION: &str = "150.0.11";
/// The one CEF subdirectory that must keep its name at the app root.
pub const LOCALES_DIR: &str = "locales";

/// Dev-only or loader artifacts in `Release/` that must never ship.
const SKIP_EXTENSIONS: &[&str] = &["lib", "exp", "pdb", "ilk"];
const SKIP_NAMES: &[&str] = &["bootstrap.exe", "bootstrapc.exe"];

/// Result of staging the CEF runtime.
#[derive(Debug, Clone, Default)]
pub struct CefStageReport {
    /// Flat runtime files copied beside the binary (file names only).
    pub runtime_files: Vec<String>,
    /// Number of locale `.pak` files copied into `locales/`.
    pub locale_count: usize,
}

/// Locate the repository's prepared CEF distribution, if present.
///
/// Returns `<workspace>/build/cef` only when it actually contains the runtime
/// (`Release/`), so a workspace without CEF installed simply reports `None` and
/// packaging can warn instead of failing a developer build.
pub fn locate_cef_dist(workspace_root: &Path, triple: &str) -> Option<PathBuf> {
    let platform_dir = match triple {
        "x86_64-pc-windows-msvc" => "cef_windows_x86_64",
        "x86_64-unknown-linux-gnu" => "cef_linux_x86_64",
        "x86_64-apple-darwin" => "cef_macos_x86_64",
        "aarch64-apple-darwin" => "cef_macos_aarch64",
        _ => return None,
    };
    let dist = workspace_root
        .join("build")
        .join("cef")
        .join(PINNED_CEF_VERSION)
        .join(platform_dir);
    validate_pinned_distribution(&dist).ok().map(|()| dist)
}

fn validate_pinned_distribution(dist: &Path) -> Result<()> {
    let version_header = dist.join("include/cef_version.h");
    let version = fs::read_to_string(&version_header)
        .with_context(|| format!("cannot read {}", version_header.display()))?;
    let expected = format!("#define CEF_VERSION \"{PINNED_CEF_VERSION}+");
    if !version.contains(&expected) {
        bail!(
            "CEF SDK version mismatch in {}: expected {PINNED_CEF_VERSION}",
            version_header.display()
        );
    }

    let archive_path = dist.join("archive.json");
    let archive = fs::read_to_string(&archive_path)
        .with_context(|| format!("cannot read {}", archive_path.display()))?;
    let expected_archive = format!("cef_binary_{PINNED_CEF_VERSION}+");
    if !archive.contains(&expected_archive) {
        bail!(
            "CEF archive mismatch in {}: expected {PINNED_CEF_VERSION}",
            archive_path.display()
        );
    }
    Ok(())
}

/// The runtime files the flat layout must contain for `triple` — used both to
/// assert staging succeeded and by package validation. Resource files are
/// platform-independent; only the shared library name differs.
pub fn required_runtime_files(triple: &str) -> Vec<String> {
    let mut files = vec![
        "resources.pak".to_string(),
        "chrome_100_percent.pak".to_string(),
        "chrome_200_percent.pak".to_string(),
        "icudtl.dat".to_string(),
        "v8_context_snapshot.bin".to_string(),
    ];
    match dynamic_library_extension(triple) {
        "dll" => {
            files.push("libcef.dll".to_string());
            files.push("chrome_elf.dll".to_string());
        }
        "dylib" => files.push("libcef.dylib".to_string()),
        _ => files.push("libcef.so".to_string()),
    }
    files
}

/// Copy the CEF runtime flat into `staging_dir`.
///
/// * every non-dev file directly under `Release/` → staging root
/// * every file directly under `Resources/` (paks, `icudtl.dat`) → staging root
/// * `Resources/locales/*` → `staging_dir/locales/`
///
/// Fails if a required file (see [`required_runtime_files`]) is absent afterward,
/// so a broken/partial CEF distribution is caught before publishing.
pub fn stage_cef(staging_dir: &Path, dist_dir: &Path, triple: &str) -> Result<CefStageReport> {
    let mut report = CefStageReport::default();

    validate_pinned_distribution(dist_dir)?;
    let release = if dist_dir.join(RELEASE_DIR).is_dir() {
        dist_dir.join(RELEASE_DIR)
    } else {
        dist_dir.to_path_buf()
    };
    for name in flat_files(&release)? {
        if should_skip(&name) {
            continue;
        }
        copy_into(staging_dir, &name, &release.join(&name))?;
        report.runtime_files.push(name);
    }

    let resources = if dist_dir.join(RESOURCES_DIR).is_dir() {
        dist_dir.join(RESOURCES_DIR)
    } else {
        dist_dir.to_path_buf()
    };
    if resources.is_dir() {
        for name in flat_files(&resources)? {
            if should_skip(&name) {
                continue;
            }
            copy_into(staging_dir, &name, &resources.join(&name))?;
            report.runtime_files.push(name);
        }
        // locales/ is the single required CEF subdirectory.
        let locales = resources.join(LOCALES_DIR);
        if locales.is_dir() {
            for name in flat_files(&locales)? {
                let relative = format!("{LOCALES_DIR}/{name}");
                copy_into(staging_dir, &relative, &locales.join(&name))?;
                report.locale_count += 1;
            }
        }
    }

    report.runtime_files.sort();
    report.runtime_files.dedup();

    // Guard: the flat layout must be complete for CEF to boot.
    for required in required_runtime_files(triple) {
        if !staging_dir.join(&required).is_file() {
            bail!(
                "CEF runtime is incomplete: `{required}` was not staged from {}",
                dist_dir.display()
            );
        }
    }
    if report.locale_count == 0 {
        bail!(
            "CEF `locales/` is empty or missing under {}",
            resources.display()
        );
    }

    Ok(report)
}

/// File names (not directories) directly inside `dir`.
fn flat_files(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("cannot read CEF directory {}", dir.display()))?
    {
        let entry = entry?;
        if entry.path().is_file() {
            if let Some(name) = entry.file_name().to_str() {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Whether a `Release/` entry is a dev-only/loader artifact to leave behind.
fn should_skip(name: &str) -> bool {
    if SKIP_NAMES.contains(&name) {
        return true;
    }
    match name.rsplit_once('.') {
        Some((_, ext)) => SKIP_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal fake `build/cef` distribution mirroring the real Windows
    /// layout (Release/ + Resources/ + Resources/locales/).
    fn fake_windows_dist(root: &Path) -> PathBuf {
        let dist = root.join("build/cef");
        let release = dist.join(RELEASE_DIR);
        fs::create_dir_all(&release).unwrap();
        fs::create_dir_all(dist.join("include")).unwrap();
        fs::write(
            dist.join("include/cef_version.h"),
            "#define CEF_VERSION \"150.0.11+gtest+chromium-150.0.0.0\"\n",
        )
        .unwrap();
        fs::write(
            dist.join("archive.json"),
            "{\"name\":\"cef_binary_150.0.11+gtest_windows64.tar.bz2\"}",
        )
        .unwrap();
        for f in [
            "libcef.dll",
            "chrome_elf.dll",
            "v8_context_snapshot.bin",
            "libGLESv2.dll",
            // dev-only / loader artifacts that must be filtered out:
            "libcef.lib",
            "bootstrap.exe",
        ] {
            fs::write(release.join(f), b"x").unwrap();
        }
        let resources = dist.join(RESOURCES_DIR);
        fs::create_dir_all(resources.join(LOCALES_DIR)).unwrap();
        for f in [
            "resources.pak",
            "chrome_100_percent.pak",
            "chrome_200_percent.pak",
            "icudtl.dat",
        ] {
            fs::write(resources.join(f), b"x").unwrap();
        }
        fs::write(resources.join(LOCALES_DIR).join("en-US.pak"), b"x").unwrap();
        fs::write(resources.join(LOCALES_DIR).join("fr.pak"), b"x").unwrap();
        dist
    }

    #[test]
    fn locate_returns_none_without_release() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("build/cef")).unwrap();
        assert!(locate_cef_dist(temp.path(), "x86_64-pc-windows-msvc").is_none());
    }

    #[test]
    fn rejects_distribution_version_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let dist = fake_windows_dist(temp.path());
        fs::write(
            dist.join("include/cef_version.h"),
            "#define CEF_VERSION \"150.0.14+gwrong+chromium-150.0.0.0\"\n",
        )
        .unwrap();
        assert!(validate_pinned_distribution(&dist).is_err());
    }

    #[test]
    fn stages_runtime_flat_with_locales_subdir() {
        let temp = tempfile::tempdir().unwrap();
        let dist = fake_windows_dist(temp.path());
        let staging = temp.path().join("stage");
        fs::create_dir_all(&staging).unwrap();

        let report = stage_cef(&staging, &dist, "x86_64-pc-windows-msvc").unwrap();

        // Flat beside the binary — never a CEF/ subdir.
        assert!(staging.join("libcef.dll").is_file());
        assert!(staging.join("chrome_elf.dll").is_file());
        assert!(staging.join("v8_context_snapshot.bin").is_file());
        assert!(staging.join("resources.pak").is_file());
        assert!(staging.join("icudtl.dat").is_file());
        assert!(!staging.join("CEF").exists());

        // locales/ preserved as the one required subdirectory.
        assert!(staging.join("locales/en-US.pak").is_file());
        assert_eq!(report.locale_count, 2);

        // Dev-only / loader artifacts filtered out.
        assert!(!staging.join("libcef.lib").exists());
        assert!(!staging.join("bootstrap.exe").exists());
    }

    #[test]
    fn flat_versioned_distribution_never_stages_import_libraries() {
        let temp = tempfile::tempdir().unwrap();
        let dist = temp.path().join("cef");
        fs::create_dir_all(dist.join("include")).unwrap();
        fs::create_dir_all(dist.join(LOCALES_DIR)).unwrap();
        fs::write(
            dist.join("include/cef_version.h"),
            "#define CEF_VERSION \"150.0.11+gtest+chromium-150.0.0.0\"\n",
        )
        .unwrap();
        fs::write(
            dist.join("archive.json"),
            "{\"name\":\"cef_binary_150.0.11+gtest_windows64.tar.bz2\"}",
        )
        .unwrap();
        for name in required_runtime_files("x86_64-pc-windows-msvc") {
            fs::write(dist.join(name), b"x").unwrap();
        }
        fs::write(dist.join("libcef.lib"), b"forbidden").unwrap();
        fs::write(dist.join(LOCALES_DIR).join("en-US.pak"), b"x").unwrap();

        let staging = temp.path().join("stage");
        fs::create_dir_all(&staging).unwrap();
        stage_cef(&staging, &dist, "x86_64-pc-windows-msvc").unwrap();
        assert!(!staging.join("libcef.lib").exists());
    }

    #[test]
    fn incomplete_distribution_fails() {
        let temp = tempfile::tempdir().unwrap();
        let dist = fake_windows_dist(temp.path());
        // Remove a required file after building the fake dist.
        fs::remove_file(dist.join(RESOURCES_DIR).join("icudtl.dat")).unwrap();
        let staging = temp.path().join("stage");
        fs::create_dir_all(&staging).unwrap();
        assert!(stage_cef(&staging, &dist, "x86_64-pc-windows-msvc").is_err());
    }

    #[test]
    fn required_files_track_platform_library() {
        assert!(
            required_runtime_files("x86_64-pc-windows-msvc").contains(&"libcef.dll".to_string())
        );
        assert!(
            required_runtime_files("x86_64-unknown-linux-gnu").contains(&"libcef.so".to_string())
        );
        assert!(
            required_runtime_files("aarch64-apple-darwin").contains(&"libcef.dylib".to_string())
        );
    }
}
