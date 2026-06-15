//! Embeds Windows icon, application manifest, and version resources from `apps/shared/`.

fn main() {
    println!("cargo:rerun-if-changed=../../packages/shared/app/windows/app.rc");
    println!("cargo:rerun-if-changed=../../packages/shared/app/windows/app.manifest");
    println!("cargo:rerun-if-changed=../../packages/shared/app/icons/icon.ico");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        embed_resource::compile(
            "../../packages/shared/app/windows/app.rc",
            embed_resource::NONE,
        )
        .manifest_required()
        .unwrap();
    }
}
