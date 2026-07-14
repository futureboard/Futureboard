//! Embeds Windows icon, application manifest, and version resources from `apps/shared/`.

fn main() {
    println!("cargo:rerun-if-changed=../../../packages/shared/app/windows/app.rc");
    println!("cargo:rerun-if-changed=../../../packages/shared/app/windows/app.manifest");
    println!("cargo:rerun-if-changed=../../../packages/shared/app/icons/icon.ico");
    println!("cargo:rerun-if-changed=../../../.discordrpcsecret");

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
