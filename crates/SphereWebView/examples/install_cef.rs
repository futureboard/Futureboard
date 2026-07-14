fn main() {
    let force = std::env::args().skip(1).any(|arg| arg == "--force");
    match sphere_webview::install_cef(force) {
        Ok(path) => println!("CEF installed at {}", path.display()),
        Err(error) => {
            eprintln!("CEF installation failed: {error}");
            std::process::exit(1);
        }
    }
}
