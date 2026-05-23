fn main() {
    // Only set up Node.js addon linking when the `napi-addon` feature is active.
    // Without it the binary target builds clean without linking against node.lib.
    if std::env::var("CARGO_FEATURE_NAPI_ADDON").is_ok() {
        napi_build::setup();
    }
}
