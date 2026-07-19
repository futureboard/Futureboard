//! Validate a staged application before it is published.
//!
//! Every check runs against the staging directory only. If any check fails the
//! caller aborts before the atomic swap, so the previously published package
//! stays intact.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::staging::{BUILD_INFO_FILE, PLUGINS_DIR, RESOURCES_DIR, SYMBOLS_DIR};

/// File extensions that must never appear in a distributable package — Cargo
/// intermediates and dev-only artifacts.
const FORBIDDEN_EXTENSIONS: &[&str] = &["pdb", "lib", "exp", "d", "rlib", "rmeta"];

/// Directory names that indicate the Cargo target tree leaked into staging.
const FORBIDDEN_DIRS: &[&str] = &["incremental", "deps", "examples", "build"];

/// Run all pre-publish checks on `staging_dir`.
pub fn validate_staging(
    staging_dir: &Path,
    binary_name: &str,
    sidecars: &[String],
    symbols_enabled: bool,
) -> Result<()> {
    check_executable(staging_dir, binary_name)?;
    for sidecar in sidecars {
        check_executable(staging_dir, sidecar)
            .with_context(|| format!("required sidecar `{sidecar}` missing or empty"))?;
    }
    check_required_dirs(staging_dir)?;
    check_build_info(staging_dir)?;
    check_no_forbidden_artifacts(staging_dir, symbols_enabled)?;
    check_all_within_root(staging_dir)?;
    Ok(())
}

/// The main executable exists and is non-empty.
fn check_executable(staging_dir: &Path, binary_name: &str) -> Result<()> {
    let exe = staging_dir.join(binary_name);
    let meta = fs::metadata(&exe)
        .with_context(|| format!("staged executable missing: {}", exe.display()))?;
    if !meta.is_file() {
        bail!("staged executable is not a file: {}", exe.display());
    }
    if meta.len() == 0 {
        bail!("staged executable is empty: {}", exe.display());
    }
    Ok(())
}

/// The expected application directories exist.
fn check_required_dirs(staging_dir: &Path) -> Result<()> {
    for dir in [PLUGINS_DIR, RESOURCES_DIR] {
        let path = staging_dir.join(dir);
        if !path.is_dir() {
            bail!("required directory missing from package: {}", path.display());
        }
    }
    Ok(())
}

/// `build-info.json` exists and parses as JSON.
fn check_build_info(staging_dir: &Path) -> Result<()> {
    let path = staging_dir.join(BUILD_INFO_FILE);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("build metadata missing: {}", path.display()))?;
    serde_json::from_str::<serde_json::Value>(&text)
        .with_context(|| format!("build metadata is not valid JSON: {}", path.display()))?;
    Ok(())
}

/// No Cargo intermediates were copied into the package. The optional `symbols/`
/// directory (populated only via `--symbols`) is skipped so its `.pdb` is not
/// flagged.
fn check_no_forbidden_artifacts(staging_dir: &Path, symbols_enabled: bool) -> Result<()> {
    for entry in walk(staging_dir)? {
        if symbols_enabled && entry.starts_with(staging_dir.join(SYMBOLS_DIR)) {
            continue;
        }
        let name = entry
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if is_forbidden_artifact(name, entry.is_dir()) {
            bail!(
                "forbidden Cargo/dev artifact found in package: {}",
                entry.display()
            );
        }
    }
    Ok(())
}

/// Whether a staged entry name is a forbidden intermediate artifact.
pub fn is_forbidden_artifact(name: &str, is_dir: bool) -> bool {
    if is_dir {
        return FORBIDDEN_DIRS.contains(&name);
    }
    match name.rsplit_once('.') {
        Some((_, ext)) => FORBIDDEN_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// Every staged path resolves to somewhere inside the staging root — a final
/// guard against symlink/traversal escapes.
fn check_all_within_root(staging_dir: &Path) -> Result<()> {
    let root = staging_dir
        .canonicalize()
        .with_context(|| format!("cannot canonicalize staging root {}", staging_dir.display()))?;
    for entry in walk(staging_dir)? {
        let resolved = entry
            .canonicalize()
            .with_context(|| format!("cannot canonicalize {}", entry.display()))?;
        if !resolved.starts_with(&root) {
            bail!("staged path escapes package root: {}", entry.display());
        }
    }
    Ok(())
}

/// Recursively list every file and directory under `root` (excluding `root`).
fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("cannot read directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            out.push(path);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staging;

    /// Build a minimally valid staged package for the happy-path checks.
    fn valid_package(dir: &Path) {
        fs::write(dir.join("FutureboardNative.exe"), b"MZ binary").unwrap();
        fs::create_dir_all(dir.join(PLUGINS_DIR)).unwrap();
        fs::create_dir_all(dir.join(RESOURCES_DIR)).unwrap();
        fs::write(dir.join(BUILD_INFO_FILE), "{\"schemaVersion\":1}").unwrap();
    }

    #[test]
    fn forbidden_artifact_detection() {
        assert!(is_forbidden_artifact("libcore.rlib", false));
        assert!(is_forbidden_artifact("FutureboardNative.pdb", false));
        assert!(is_forbidden_artifact("thing.d", false));
        assert!(is_forbidden_artifact("mod.rmeta", false));
        assert!(is_forbidden_artifact("deps", true));
        assert!(is_forbidden_artifact("incremental", true));

        assert!(!is_forbidden_artifact("FutureboardNative.exe", false));
        assert!(!is_forbidden_artifact("onnxruntime.dll", false));
        assert!(!is_forbidden_artifact("Plugins", true));
        assert!(!is_forbidden_artifact("build-info.json", false));
    }

    #[test]
    fn valid_package_passes() {
        let temp = tempfile::tempdir().unwrap();
        valid_package(temp.path());
        validate_staging(temp.path(), "FutureboardNative.exe", &[], false).unwrap();
    }

    #[test]
    fn valid_package_with_sidecars_passes() {
        let temp = tempfile::tempdir().unwrap();
        valid_package(temp.path());
        fs::write(temp.path().join("FutureboardPluginHostX64.exe"), b"MZ").unwrap();
        fs::write(temp.path().join("FutureboardPluginScanner.exe"), b"MZ").unwrap();
        let sidecars = [
            "FutureboardPluginHostX64.exe".to_string(),
            "FutureboardPluginScanner.exe".to_string(),
        ];
        validate_staging(temp.path(), "FutureboardNative.exe", &sidecars, false).unwrap();
    }

    #[test]
    fn missing_required_sidecar_fails() {
        let temp = tempfile::tempdir().unwrap();
        valid_package(temp.path());
        // Only one of the two required sidecars is present.
        fs::write(temp.path().join("FutureboardPluginHostX64.exe"), b"MZ").unwrap();
        let sidecars = [
            "FutureboardPluginHostX64.exe".to_string(),
            "FutureboardPluginScanner.exe".to_string(),
        ];
        assert!(validate_staging(temp.path(), "FutureboardNative.exe", &sidecars, false).is_err());
    }

    #[test]
    fn empty_executable_fails() {
        let temp = tempfile::tempdir().unwrap();
        valid_package(temp.path());
        fs::write(temp.path().join("FutureboardNative.exe"), b"").unwrap();
        assert!(validate_staging(temp.path(), "FutureboardNative.exe", &[], false).is_err());
    }

    #[test]
    fn missing_required_dir_fails() {
        let temp = tempfile::tempdir().unwrap();
        valid_package(temp.path());
        fs::remove_dir_all(temp.path().join(PLUGINS_DIR)).unwrap();
        assert!(validate_staging(temp.path(), "FutureboardNative.exe", &[], false).is_err());
    }

    #[test]
    fn invalid_build_info_fails() {
        let temp = tempfile::tempdir().unwrap();
        valid_package(temp.path());
        fs::write(temp.path().join(BUILD_INFO_FILE), "not json {").unwrap();
        assert!(validate_staging(temp.path(), "FutureboardNative.exe", &[], false).is_err());
    }

    #[test]
    fn leaked_cargo_artifact_fails() {
        let temp = tempfile::tempdir().unwrap();
        valid_package(temp.path());
        fs::write(temp.path().join("libjunk.rlib"), b"junk").unwrap();
        assert!(validate_staging(temp.path(), "FutureboardNative.exe", &[], false).is_err());
    }

    #[test]
    fn symbols_pdb_allowed_only_under_symbols_dir() {
        let temp = tempfile::tempdir().unwrap();
        valid_package(temp.path());
        let sym = temp.path().join(staging::SYMBOLS_DIR);
        fs::create_dir_all(&sym).unwrap();
        fs::write(sym.join("FutureboardNative.pdb"), b"pdb").unwrap();
        // Allowed when symbols were requested...
        validate_staging(temp.path(), "FutureboardNative.exe", &[], true).unwrap();
        // ...but flagged otherwise.
        assert!(validate_staging(temp.path(), "FutureboardNative.exe", &[], false).is_err());
    }
}
