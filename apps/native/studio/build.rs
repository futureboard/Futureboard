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
    // Supabase auth config is baked from the repo `.env` (or the build
    // environment). Only the public URL and the *publishable* anon key are ever
    // read here — never the service secret key.
    println!("cargo:rerun-if-changed=../../../.env");
    println!("cargo:rerun-if-env-changed=FUTUREBOARD_SUPABASE_URL");
    println!("cargo:rerun-if-env-changed=FUTUREBOARD_SUPABASE_ANON_KEY");
    // The EULA is embedded (include_str!) from the staged copies below; rebuild
    // when the source text changes.
    println!("cargo:rerun-if-changed=../../../crates/ExclusiveEdition/assets/EULA.EN.txt");
    println!("cargo:rerun-if-changed=../../../crates/ExclusiveEdition/assets/EULA.TH.txt");

    if std::env::var_os("CARGO_FEATURE_EXCLUSIVE").is_none() {
        return;
    }

    bake_supabase_config();

    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by Cargo"),
    );
    let source_dir = manifest_dir.join("../../../crates/ExclusiveEdition/src");
    let assets_dir = manifest_dir.join("../../../crates/ExclusiveEdition/assets");
    let output_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"))
        .join("futureboard-exclusive");

    std::fs::create_dir_all(&output_dir).expect("failed to create Exclusive Edition output dir");

    stage_exclusive_source(&source_dir, &output_dir, "license.rs");
    stage_exclusive_source(&source_dir, &output_dir, "license_activation_dialog.rs");
    stage_exclusive_source(&source_dir, &output_dir, "auth.rs");
    stage_exclusive_source(&source_dir, &output_dir, "auth_dialog.rs");
    stage_exclusive_source(&source_dir, &output_dir, "eula.rs");
    stage_exclusive_source(&source_dir, &output_dir, "eula_dialog.rs");

    // The EULA text is embedded into the binary. Copy it beside the staged
    // source so `include_str!(concat!(env!("OUT_DIR"), ...))` finds it.
    copy_exclusive_asset(&assets_dir, &output_dir, "EULA.EN.txt");
    copy_exclusive_asset(&assets_dir, &output_dir, "EULA.TH.txt");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        stage_exclusive_source(&source_dir, &output_dir, "asio.rs");
    }
}

/// Copy an Exclusive Edition asset verbatim into the staging directory so the
/// staged source can embed it with `include_str!`.
fn copy_exclusive_asset(assets_dir: &Path, output_dir: &Path, file_name: &str) {
    let source_path = assets_dir.join(file_name);
    println!("cargo:rerun-if-changed={}", source_path.display());
    std::fs::copy(&source_path, output_dir.join(file_name)).unwrap_or_else(|error| {
        panic!(
            "Exclusive Edition asset is required for --features exclusive: {}: {error}",
            source_path.display()
        )
    });
}

/// Bake the Supabase auth endpoint and publishable key for `option_env!` in the
/// staged `auth.rs`. Source precedence: an explicit build-environment value
/// wins (CI/distribution), otherwise the repo `.env` is read.
///
/// SECURITY: the `.env` also contains `SUPABASE_SECRET_KEY` (a service-role
/// secret). It is deliberately never read or emitted here — a service secret in
/// a shipped desktop binary would be a full account-takeover credential. Only
/// the public URL and the publishable anon key belong in the client.
fn bake_supabase_config() {
    let dotenv = read_dotenv("../../../.env");
    let resolve = |build_key: &str, dotenv_key: &str| -> Option<String> {
        std::env::var(build_key)
            .ok()
            .or_else(|| dotenv.iter().find(|(k, _)| k == dotenv_key).map(|(_, v)| v.clone()))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };

    if let Some(url) = resolve("FUTUREBOARD_SUPABASE_URL", "SUPABASE_URL") {
        println!("cargo:rustc-env=FUTUREBOARD_SUPABASE_URL={url}");
    }
    if let Some(anon) = resolve("FUTUREBOARD_SUPABASE_ANON_KEY", "SUPABASE_PUBLISHABLE_KEY") {
        println!("cargo:rustc-env=FUTUREBOARD_SUPABASE_ANON_KEY={anon}");
    }
}

/// Minimal `KEY=VALUE` parser for the repo `.env`. Ignores blanks and `#`
/// comments; does not expand or unquote — the values used here are plain tokens.
fn read_dotenv(relative: &str) -> Vec<(String, String)> {
    let Ok(contents) = std::fs::read_to_string(relative) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
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
