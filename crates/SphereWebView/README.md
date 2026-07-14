# SphereWebView

Native, windowed CEF web views for Futureboard Studio on Windows, Linux, and
macOS. The library embeds CEF as a real child window; off-screen rendering is
deliberately disabled.

Normal workspace builds do not download or link CEF. Install the pinned SDK
explicitly:

```powershell
cargo run -p SphereWebView --example install_cef --features installer
```

Pass `-- --force` to replace an existing `build/cef` installation. Consumers
that own the browser-process lifecycle enable `cef-runtime`; `.cargo/config.toml`
then points `cef-dll-sys` at the installed workspace SDK.

Call `runtime::execute_subprocess` before application startup, initialize one
`CefRuntime`, and create native child views with `CefRuntime::create_webview`.
