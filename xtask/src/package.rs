//! Package orchestration: build → collect → stage → validate → publish.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use cargo_metadata::MetadataCommand;

use crate::cargo_build::{self, APP_PACKAGE};
use crate::metadata::{BuildInfo, MetadataInputs};
use crate::platform::{Edition, platform_folder};
use crate::staging::{self, StagingPlan};
use crate::validation;

/// Everything the `package` subcommand needs.
pub struct PackageOptions {
    pub profile: String,
    pub target: Option<String>,
    pub edition: Edition,
    /// Root of the distributable tree (default `out`).
    pub out_root: PathBuf,
    /// Also stage debug symbols into `symbols/`.
    pub symbols: bool,
}

/// Run the full package pipeline and return the published directory.
pub fn run(options: &PackageOptions) -> Result<PathBuf> {
    // 1. Build and discover the real executable paths (app + runtime sidecars).
    let build =
        cargo_build::build(&options.profile, options.target.as_deref(), options.edition)?;
    let executable = &build.app_executable;
    eprintln!("[xtask] built executable: {}", executable.display());

    // Resolve the effective target triple (explicit flag or detected host) for
    // platform naming and metadata.
    let target_triple = match &options.target {
        Some(target) => target.clone(),
        None => host_target().context("could not determine host target triple")?,
    };
    let platform = platform_folder(&target_triple);

    // 2. Prepare staging (clean any stale directory first).
    let plan = StagingPlan::new(&options.out_root, &options.profile, options.edition, &platform);
    plan.prepare()?;

    // 3. Copy required runtime files into staging — the app binary, the sidecar
    //    executables it spawns from its own directory, and any runtime libraries
    //    the build placed beside the binary.
    let binary_name = staging::stage_executable(&plan.staging_dir, executable)?;

    let mut sidecar_names = Vec::with_capacity(build.sidecar_executables.len());
    for sidecar in &build.sidecar_executables {
        let name = staging::stage_executable(&plan.staging_dir, sidecar)?;
        eprintln!("[xtask] staged sidecar executable: {name}");
        sidecar_names.push(name);
    }

    let siblings = staging::stage_runtime_siblings(&plan.staging_dir, executable)?;
    for lib in &siblings {
        eprintln!("[xtask] staged runtime library: {lib}");
    }

    // 4. Create expected directories.
    staging::create_layout_dirs(&plan.staging_dir)?;

    // 5. Optional symbols.
    if options.symbols {
        let staged = staging::stage_symbols(&plan.staging_dir, executable)?;
        if staged.is_empty() {
            eprintln!("[xtask] --symbols requested but no .pdb was found beside the binary");
        }
        for path in &staged {
            eprintln!("[xtask] staged symbols: {path}");
        }
    }

    // 6. Write build metadata.
    let version = package_version()?;
    let info = BuildInfo::collect(&MetadataInputs {
        binary: &binary_name,
        sidecars: &sidecar_names,
        edition: options.edition,
        profile: &options.profile,
        target: &target_triple,
        platform: &platform,
        version: &version,
    });
    staging::write_build_info(&plan.staging_dir, &info.to_json()?)?;

    // 7. Validate the staged application before publishing.
    validation::validate_staging(
        &plan.staging_dir,
        &binary_name,
        &sidecar_names,
        options.symbols,
    )
    .context("staged package failed validation; previous output left untouched")?;

    // 8. Optional non-GUI smoke check on Windows (does not launch the GUI).
    #[cfg(windows)]
    smoke_check(&plan.staging_dir.join(&binary_name));

    // 9. Atomically publish, then tidy the (now empty) staging root.
    staging::publish(&plan.staging_dir, &plan.final_dir)?;
    staging::cleanup_staging_root_if_empty(&plan.staging_dir);
    eprintln!("[xtask] published package: {}", plan.final_dir.display());

    Ok(plan.final_dir)
}

/// Read the `futureboard_native` package version from workspace metadata,
/// reusing the existing application versioning source of truth.
fn package_version() -> Result<String> {
    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .context("failed to query cargo metadata for the application version")?;
    metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == APP_PACKAGE)
        .map(|package| package.version.to_string())
        .ok_or_else(|| anyhow!("package `{APP_PACKAGE}` not found in workspace metadata"))
}

/// The host target triple, parsed from `rustc -vV`'s `host:` line.
fn host_target() -> Result<String> {
    let output = std::process::Command::new(
        std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string()),
    )
    .arg("-vV")
    .output()
    .context("failed to run rustc -vV")?;
    let text = String::from_utf8(output.stdout).context("rustc -vV output was not UTF-8")?;
    text.lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|host| host.trim().to_string())
        .ok_or_else(|| anyhow!("rustc -vV did not report a host triple"))
}

/// Best-effort `FutureboardNative.exe --version`. Never fails the package:
/// the binary may not implement `--version`, and packaging must not launch the
/// GUI. Purely informational.
#[cfg(windows)]
fn smoke_check(exe: &Path) {
    use std::process::Command;
    match Command::new(exe).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            eprintln!("[xtask] smoke check `{} --version`: {}", exe.display(), text.trim());
        }
        Ok(_) => eprintln!(
            "[xtask] smoke check skipped: `{} --version` returned non-zero (no --version handler)",
            exe.display()
        ),
        Err(error) => eprintln!("[xtask] smoke check skipped: {error}"),
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn smoke_check(_exe: &Path) {}
