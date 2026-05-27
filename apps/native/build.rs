//! Embeds Windows icon, application manifest, and version resources from `apps/shared/`.

fn main() {
    println!("cargo:rerun-if-changed=../shared/app.rc");
    println!("cargo:rerun-if-changed=../shared/app.manifest");
    println!("cargo:rerun-if-changed=../shared/icon.ico");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        embed_resource::compile("../shared/app.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}
