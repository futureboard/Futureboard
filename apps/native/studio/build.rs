//! Embeds Windows icon, application manifest, and version resources from `apps/shared/`.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../../../packages/shared/app/windows/app.rc");
    println!("cargo:rerun-if-changed=../../../packages/shared/app/windows/app.manifest");
    println!("cargo:rerun-if-changed=../../../packages/shared/app/icons/icon.ico");
    println!("cargo:rerun-if-changed=../../../.discordrpcsecret");

    stage_exclusive_sources();

    // Keep the local Discord application id out of source control while still
    // making it available to `option_env!` in the native binary. An explicit
    // build environment value wins for CI/distribution builds.
    if std::env::var_os("FUTUREBOARD_DISCORD_CLIENT_ID").is_none() {
        if let Ok(application_id) = std::fs::read_to_string("../../../.discordrpcsecret") {
            let application_id = application_id.trim();
            if !application_id.is_empty()
                && application_id
                    .chars()
                    .all(|character| character.is_ascii_digit())
            {
                println!("cargo:rustc-env=FUTUREBOARD_DISCORD_CLIENT_ID={application_id}");
            }
        }
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        embed_resource::compile(
            "../../../packages/shared/app/windows/app.rc",
            embed_resource::NONE,
        )
        .manifest_required()
        .unwrap();
    }
}

/// Stage private implementation files only when Cargo is compiling the
/// Exclusive Edition. `include!` cannot accept their crate-level `//!` comments
/// inside the application's bridge module, so those comments become ordinary
/// comments in the generated copies.
fn stage_exclusive_sources() {
    // The license signing key and activation endpoint are baked in via
    // `option_env!`. Rebuild when either changes so a build never keeps a stale
    // key, and never silently ships without one.
    println!("cargo:rerun-if-env-changed=FUTUREBOARD_LICENSE_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=FUTUREBOARD_LICENSE_ACTIVATION_URL");

    if std::env::var_os("CARGO_FEATURE_EXCLUSIVE").is_none() {
        return;
    }

    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo"),
    );
    let source_dir = manifest_dir.join("../../../crates/ExclusiveEdition/src");
    let output_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"))
        .join("futureboard-exclusive");

    std::fs::create_dir_all(&output_dir).expect("failed to create Exclusive Edition output dir");

    stage_exclusive_source(&source_dir, &output_dir, "license.rs");
    stage_exclusive_source(&source_dir, &output_dir, "license_activation_dialog.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        stage_exclusive_source(&source_dir, &output_dir, "asio.rs");
    }
}

fn stage_exclusive_source(source_dir: &Path, output_dir: &Path, file_name: &str) {
    let source_path = source_dir.join(file_name);
    println!("cargo:rerun-if-changed={}", source_path.display());

    let source = std::fs::read_to_string(&source_path).unwrap_or_else(|error| {
        panic!(
            "Exclusive Edition source is required for --features exclusive: {}: {error}",
            source_path.display()
        )
    });
    let staged = source
        .lines()
        .map(|line| {
            line.strip_prefix("//!")
                .map_or_else(|| line.to_owned(), |comment| format!("//{comment}"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(output_dir.join(file_name), staged)
        .unwrap_or_else(|error| panic!("failed to stage {file_name}: {error}"));
}
