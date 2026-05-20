extern crate napi_build;

fn main() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let sdk_root = manifest_dir.join("../../external/vst3sdk");
    let backend_root = manifest_dir.join("vst3backend");
    println!("cargo:rerun-if-changed={}", backend_root.join("src/vst3_scanner.cpp").display());
    println!("cargo:rerun-if-changed={}", backend_root.join("include/sphere_plugin_host_vst3.h").display());

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .include(backend_root.join("include"))
        .include(&sdk_root)
        .include(sdk_root.join("pluginterfaces"))
        .include(sdk_root.join("public.sdk/source"))
        .define("SMTG_OS_WINDOWS", Some("1"))
        .file(backend_root.join("src/vst3_scanner.cpp"))
        .file(sdk_root.join("pluginterfaces/base/coreiids.cpp"))
        .file(sdk_root.join("pluginterfaces/base/funknown.cpp"))
        .file(sdk_root.join("pluginterfaces/base/ustring.cpp"))
        .file(sdk_root.join("public.sdk/source/common/commonstringconvert.cpp"))
        .file(sdk_root.join("public.sdk/source/vst/utility/stringconvert.cpp"))
        .file(sdk_root.join("public.sdk/source/vst/vstinitiids.cpp"))
        .file(sdk_root.join("public.sdk/source/vst/hosting/hostclasses.cpp"))
        .file(sdk_root.join("public.sdk/source/vst/hosting/pluginterfacesupport.cpp"))
        .file(sdk_root.join("public.sdk/source/vst/hosting/module.cpp"));

    if cfg!(target_os = "windows") {
        build.file(sdk_root.join("public.sdk/source/vst/hosting/module_win32.cpp"));
        println!("cargo:rustc-link-lib=ole32");
    }

    build.compile("sphere_plugin_host_vst3");
    napi_build::setup();
}
