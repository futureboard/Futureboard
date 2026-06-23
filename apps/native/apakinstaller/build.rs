//! Embeds Windows resources for APAK command-line and GUI tools.

fn main() {
    println!("cargo:rerun-if-changed=windows/apak_tools.rc");
    println!("cargo:rerun-if-changed=windows/apakinstaller.manifest");
    println!("cargo:rerun-if-changed=windows/apak.manifest");
    println!("cargo:rerun-if-changed=windows/makeapak.manifest");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        embed_resource::compile("windows/apak_tools.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}
