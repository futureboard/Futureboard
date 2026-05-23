extern crate napi_build;

fn write_byte_array(out: &mut String, name: &str, bytes: &[u8]) {
    out.push_str(&format!("static const unsigned char {}[] = {{\n", name));
    for (index, byte) in bytes.iter().enumerate() {
        if index % 16 == 0 {
            out.push_str("  ");
        }
        out.push_str(&format!("0x{:02X},", byte));
        if index % 16 == 15 {
            out.push('\n');
        } else {
            out.push(' ');
        }
    }
    out.push_str("\n};\n");
    out.push_str(&format!(
        "static const unsigned int {}_len = {};\n\n",
        name,
        bytes.len()
    ));
}

fn generate_embedded_assets(manifest_dir: &std::path::Path) -> std::path::PathBuf {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let header_path = out_dir.join("sphere_plugin_editor_embedded_assets.h");
    let root = manifest_dir.join("../..");
    let font_root = root.join("packages/shared/fonts");
    let icon_root = root.join("packages/shared/tabler-icons/icons/outline");

    let mut header = String::from("#pragma once\n\n");
    let fonts = [
        ("kInterRegularTtf", font_root.join("Inter-Regular.ttf")),
        ("kInterSemiBoldTtf", font_root.join("Inter-SemiBold.ttf")),
    ];
    for (name, path) in fonts {
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        write_byte_array(&mut header, name, &bytes);
    }

    let icons = [
        ("kIconSettingsSvg", "settings.svg"),
        ("kIconPowerSvg", "power.svg"),
        ("kIconCpuSvg", "cpu.svg"),
        ("kIconBoltSvg", "bolt.svg"),
        ("kIconDatabaseSvg", "database.svg"),
        ("kIconRefreshSvg", "refresh.svg"),
        ("kIconChevronDownSvg", "chevron-down.svg"),
        ("kIconDotsSvg", "dots.svg"),
    ];
    for (name, file) in icons {
        let path = icon_root.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        write_byte_array(&mut header, name, &bytes);
    }

    std::fs::write(&header_path, header).unwrap();
    header_path
}

fn main() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let sdk_root = manifest_dir.join("../../external/vst3sdk");
    let clap_root = manifest_dir.join("../../external/clap");
    let clap_helpers_root = manifest_dir.join("../../external/clap-helpers");
    let yoga_root = manifest_dir.join("../../external/yoga");
    let nanovg_root = manifest_dir.join(
        "../../external/vst3sdk/public.sdk/samples/vst/dataexchange/source/3rdparty/nanovg/src",
    );
    let d3d_nanovg_root = manifest_dir.join(
        "../../external/vst3sdk/public.sdk/samples/vst/dataexchange/source/3rdparty/D3D11NanoVG",
    );
    let embedded_assets_header = generate_embedded_assets(&manifest_dir);
    let embedded_assets_include = embedded_assets_header.parent().unwrap().to_path_buf();
    let backend_root = manifest_dir.join("vst3backend");
    println!(
        "cargo:rerun-if-changed={}",
        backend_root.join("src/vst3_scanner.cpp").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        backend_root.join("src/plugin_editor_window.cpp").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        backend_root
            .join("include/sphere_plugin_host_vst3.h")
            .display()
    );

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++20")
        .flag_if_supported("/Zc:char8_t-")
        .flag_if_supported("/EHsc")
        .include(backend_root.join("include"))
        .include(&sdk_root)
        .include(sdk_root.join("pluginterfaces"))
        .include(sdk_root.join("public.sdk/source"))
        .include(clap_root.join("include"))
        .include(clap_helpers_root.join("include"))
        .include(&yoga_root)
        .include(&nanovg_root)
        .include(&d3d_nanovg_root)
        .include(&embedded_assets_include)
        .define("SMTG_OS_WINDOWS", Some("1"))
        .file(backend_root.join("src/vst3_scanner.cpp"))
        .file(backend_root.join("src/plugin_editor_window.cpp"))
        .file(nanovg_root.join("nanovg.c"))
        .file(yoga_root.join("yoga/YGConfig.cpp"))
        .file(yoga_root.join("yoga/YGEnums.cpp"))
        .file(yoga_root.join("yoga/YGNode.cpp"))
        .file(yoga_root.join("yoga/YGNodeLayout.cpp"))
        .file(yoga_root.join("yoga/YGNodeStyle.cpp"))
        .file(yoga_root.join("yoga/YGPixelGrid.cpp"))
        .file(yoga_root.join("yoga/YGValue.cpp"))
        .file(yoga_root.join("yoga/algorithm/AbsoluteLayout.cpp"))
        .file(yoga_root.join("yoga/algorithm/Baseline.cpp"))
        .file(yoga_root.join("yoga/algorithm/Cache.cpp"))
        .file(yoga_root.join("yoga/algorithm/CalculateLayout.cpp"))
        .file(yoga_root.join("yoga/algorithm/FlexLine.cpp"))
        .file(yoga_root.join("yoga/algorithm/PixelGrid.cpp"))
        .file(yoga_root.join("yoga/config/Config.cpp"))
        .file(yoga_root.join("yoga/debug/AssertFatal.cpp"))
        .file(yoga_root.join("yoga/debug/Log.cpp"))
        .file(yoga_root.join("yoga/event/event.cpp"))
        .file(yoga_root.join("yoga/node/LayoutResults.cpp"))
        .file(yoga_root.join("yoga/node/Node.cpp"))
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
        println!("cargo:rustc-link-lib=user32");
        println!("cargo:rustc-link-lib=gdi32");
        println!("cargo:rustc-link-lib=d3d11");
        println!("cargo:rustc-link-lib=dxgi");
        println!("cargo:rustc-link-lib=dwmapi");
    } else if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-lib=dl");
    }

    build.compile("sphere_plugin_host_vst3");
    napi_build::setup();
}
