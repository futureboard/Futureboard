fn main() {
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .file("src/stretching/backends/signalsmith_bridge.cpp")
        .include("vendor/signalsmith-stretch")
        .include("vendor/signalsmith-linear/include")
        .include("vendor/signalsmith-linear")
        .warnings(false);

    #[cfg(target_env = "msvc")]
    build.flag("/EHsc");

    build.compile("sphere_signalsmith_bridge");

    println!("cargo:rerun-if-changed=src/stretching/backends/signalsmith_bridge.cpp");
    println!("cargo:rerun-if-changed=src/stretching/backends/signalsmith_bridge.h");
    println!("cargo:rerun-if-changed=vendor/signalsmith-stretch/signalsmith-stretch.h");
    println!("cargo:rerun-if-changed=vendor/signalsmith-linear/stft.h");
    println!("cargo:rerun-if-changed=vendor/signalsmith-linear/fft.h");
    println!("cargo:rerun-if-changed=vendor/signalsmith-linear/linear.h");
}
