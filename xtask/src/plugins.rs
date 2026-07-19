//! Discover, build and stage Built-in Plugin dynamic libraries into `Plugins/`.
//!
//! Each Built-in Plugin ships as one dynamic library (`<plugin>.dll` /
//! `lib<plugin>.so` / `lib<plugin>.dylib`) containing its DSP, metadata, C entry
//! points and embedded React UI. This module never copies the Cargo target tree:
//! it parses `compiler-artifact` JSON to find the exact `cdylib`/`dylib` outputs
//! and stages only those.
//!
//! CEF is intentionally absent here — plugins embed passive UI bytes only.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use cargo_metadata::{Artifact, Message};

use crate::platform::{Edition, dynamic_library_extension, dynamic_library_file_name};
use crate::staging::{PLUGINS_DIR, copy_into};

/// Directory (relative to the workspace root) that holds the plugin crates.
pub const PLUGIN_CRATES_DIR: &str = "crates/BuiltinAudioPlugins/crates";

/// A plugin dynamic library produced by Cargo.
#[derive(Debug, Clone)]
pub struct PluginArtifact {
    /// The plugin crate/library base name (Cargo `[lib] name`).
    pub name: String,
    /// Absolute path to the built dynamic library.
    pub library: PathBuf,
}

/// Whether a plugin crate directory ships an embeddable editor UI, i.e. a
/// `editorui/package.json` exists. Plugins without one build normally and expose
/// no embedded UI — the whole workspace must not fail because a plugin has no UI.
pub fn has_editor_ui(crate_dir: &Path) -> bool {
    crate_dir.join("editorui").join("package.json").is_file()
}

/// Whether a built site is present (`editorui/dist/index.html`) — the precondition
/// for embedding. Missing dist before the UI build is a normal, recoverable state.
pub fn editor_ui_built(crate_dir: &Path) -> bool {
    crate_dir.join("editorui").join("dist").join("index.html").is_file()
}

/// Classify a Cargo artifact: returns `(name, path)` when it is a plugin dynamic
/// library (`cdylib` or `dylib` kind) with an emitted file.
pub fn plugin_dylib_artifact(artifact: &Artifact) -> Option<(String, PathBuf)> {
    let is_dylib = artifact
        .target
        .kind
        .iter()
        .any(|kind| matches!(kind.as_str(), "cdylib" | "dylib"));
    if !is_dylib {
        return None;
    }
    // A cdylib/dylib emits its shared object as one of the artifact filenames.
    artifact
        .filenames
        .iter()
        .find(|path| {
            path.extension()
                .map(|ext| matches!(ext, "dll" | "so" | "dylib"))
                .unwrap_or(false)
        })
        .map(|path| {
            (
                artifact.target.name.clone(),
                PathBuf::from(path.as_std_path()),
            )
        })
}

/// Build the given plugin packages and return their dynamic libraries, discovered
/// from Cargo's JSON artifact stream (never guessed from `target/<profile>`).
pub fn build_plugins(
    packages: &[String],
    profile: &str,
    target: Option<&str>,
    edition: Edition,
) -> Result<Vec<PluginArtifact>> {
    if packages.is_empty() {
        return Ok(Vec::new());
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut command = Command::new(&cargo);
    command
        .arg("build")
        .arg("--message-format=json-render-diagnostics")
        .args(["--profile", profile])
        .args(["--target-dir", edition.target_dir()]);
    for package in packages {
        command.args(["--package", package]);
    }
    if let Some(target) = target {
        command.args(["--target", target]);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());

    eprintln!("[xtask] building {} plugin cdylib(s)", packages.len());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn `{cargo} build` for plugins"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("cargo produced no stdout stream"))?;

    let mut libraries: BTreeMap<String, PathBuf> = BTreeMap::new();
    for message in Message::parse_stream(BufReader::new(stdout)) {
        if let Message::CompilerArtifact(artifact) =
            message.context("failed to parse a cargo JSON message")?
        {
            if let Some((name, path)) = plugin_dylib_artifact(&artifact) {
                libraries.insert(name, path);
            }
        }
    }

    let status = child.wait().context("failed to wait on cargo plugin build")?;
    if !status.success() {
        bail!("cargo plugin build failed with {status}");
    }

    Ok(libraries
        .into_iter()
        .map(|(name, library)| PluginArtifact { name, library })
        .collect())
}

/// Copy the built plugin libraries into `staging_dir/Plugins/`, verifying each
/// carries the platform-correct dynamic-library extension. Returns the staged
/// file names (sorted).
pub fn stage_plugins(
    staging_dir: &Path,
    plugins: &[PluginArtifact],
    triple: &str,
) -> Result<Vec<String>> {
    let expected_ext = dynamic_library_extension(triple);
    let mut staged = Vec::with_capacity(plugins.len());
    for plugin in plugins {
        let file_name = plugin
            .library
            .file_name()
            .and_then(|n| n.to_str())
            .with_context(|| format!("plugin library has no file name: {}", plugin.library.display()))?;
        let actual_ext = plugin
            .library
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if actual_ext != expected_ext {
            bail!(
                "plugin `{}` produced `.{actual_ext}` but target {triple} expects `.{expected_ext}`",
                plugin.name
            );
        }
        // Defense: the produced file must match the platform-canonical name Cargo
        // is expected to emit for this library (`<name>.dll` / `lib<name>.so`).
        let expected_name = dynamic_library_file_name(&plugin.name, triple);
        if file_name != expected_name {
            bail!(
                "plugin `{}` produced `{file_name}` but target {triple} expects `{expected_name}`",
                plugin.name
            );
        }
        let relative = format!("{PLUGINS_DIR}/{expected_name}");
        copy_into(staging_dir, &relative, &plugin.library)?;
        staged.push(expected_name);
    }
    staged.sort();
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_editor_ui_presence() {
        let temp = tempfile::tempdir().unwrap();
        let with_ui = temp.path().join("rodharerist");
        fs::create_dir_all(with_ui.join("editorui")).unwrap();
        fs::write(with_ui.join("editorui/package.json"), "{}").unwrap();
        assert!(has_editor_ui(&with_ui));
        assert!(!editor_ui_built(&with_ui));

        fs::create_dir_all(with_ui.join("editorui/dist")).unwrap();
        fs::write(with_ui.join("editorui/dist/index.html"), "<html>").unwrap();
        assert!(editor_ui_built(&with_ui));

        let without_ui = temp.path().join("equz8");
        fs::create_dir_all(&without_ui).unwrap();
        assert!(!has_editor_ui(&without_ui));
    }

    #[test]
    fn staging_rejects_wrong_extension_for_platform() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("stage");
        fs::create_dir_all(&staging).unwrap();
        let lib = temp.path().join("librodharerist.so");
        fs::write(&lib, b"ELF").unwrap();
        let plugins = vec![PluginArtifact {
            name: "rodharerist".to_string(),
            library: lib,
        }];
        // A `.so` cannot be staged for a Windows target.
        assert!(stage_plugins(&staging, &plugins, "x86_64-pc-windows-msvc").is_err());
    }

    #[test]
    fn staging_places_libraries_under_plugins_dir() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().join("stage");
        fs::create_dir_all(&staging).unwrap();
        let lib = temp.path().join("rodharerist.dll");
        fs::write(&lib, b"MZ").unwrap();
        let plugins = vec![PluginArtifact {
            name: "rodharerist".to_string(),
            library: lib,
        }];
        let staged = stage_plugins(&staging, &plugins, "x86_64-pc-windows-msvc").unwrap();
        assert_eq!(staged, vec!["rodharerist.dll".to_string()]);
        assert!(staging.join("Plugins/rodharerist.dll").is_file());
    }
}
