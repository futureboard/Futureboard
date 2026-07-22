//! Headless probe for off-screen built-in plugin editor hosting.
//!
//! Opens the same windowless CEF browser the editor window opens, with no GPUI
//! window involved, and reports what actually happens:
//!
//! - whether the browser attaches,
//! - whether frames are painted (and how many),
//! - whether the page's React bridge reaches native (`bridgeReady`).
//!
//! Run it from a directory containing the staged CEF runtime, e.g.:
//!
//! ```text
//! cargo build -p sphere_ui_components --features builtin-plugin-editor \
//!     --example osr_editor_probe
//! cp target/debug/examples/osr_editor_probe out/release/community/linux-x64/
//! cd out/release/community/linux-x64 && FUTUREBOARD_PLUGIN_VIEW_DEBUG=1 ./osr_editor_probe
//! ```
//!
//! `FUTUREBOARD_PLUGIN_VIEW_DEBUG=1` turns on page console forwarding, which is
//! how a JavaScript error inside the editor becomes visible.

fn main() {
    #[cfg(not(feature = "builtin-plugin-editor"))]
    {
        eprintln!("build with --features builtin-plugin-editor");
    }

    #[cfg(feature = "builtin-plugin-editor")]
    run();
}

#[cfg(feature = "builtin-plugin-editor")]
fn run() {
    use sphere_ui_components::components::builtin_plugin_editor as host;
    use sphere_webview::runtime::ProcessDispatch;

    sphere_webview::runtime::log_process_entry();

    // CEF re-launches this executable for its helpers; they must exit here.
    let mut scheme_app = sphere_webview::scheme::plugin_scheme_app();
    if let ProcessDispatch::SubprocessExit(code) =
        sphere_webview::runtime::execute_subprocess(Some(&mut scheme_app))
    {
        std::process::exit(code);
    }

    let plugin_id = "rodharerist";
    println!("[probe] availability={}", host::availability(plugin_id));
    if let Err(error) = host::init_at_boot() {
        println!("[probe] FAILED init_at_boot: {error}");
        return;
    }

    let view_id = host::allocate_view_id();
    let rect = host::ViewRect {
        x: 0,
        y: 0,
        width: 972,
        height: 728,
    };
    if let Err(error) = host::open_view(view_id, "probe::instance", plugin_id, 0, rect, 1.0) {
        println!("[probe] FAILED open_view: {error}");
        return;
    }

    let origin = host::origin_for_plugin_id(plugin_id).expect("built-in resolves");
    let mut frames = 0;
    let mut inbound = 0;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut last_generation = 0;

    while std::time::Instant::now() < deadline {
        host::pump();
        std::thread::sleep(std::time::Duration::from_millis(16));

        for event in host::take_view_events(view_id) {
            println!("[probe] view_event={event:?}");
        }

        let generation = host::view_frame_generation(view_id);
        if generation != last_generation {
            last_generation = generation;
            frames += 1;
            if frames <= 3 || frames % 60 == 0 {
                let size = host::with_view_frame(view_id, |bytes, w, h| (bytes.len(), w, h));
                println!("[probe] frame #{frames} generation={generation} size={size:?}");
            }
        }

        for raw in host::take_inbound(origin) {
            inbound += 1;
            println!(
                "[probe] inbound #{inbound}: {}",
                String::from_utf8_lossy(&raw)
            );
        }
    }

    println!("[probe] SUMMARY frames_painted={frames} bridge_messages={inbound}");
    println!(
        "[probe] verdict paint={} bridge={}",
        if frames > 0 { "OK" } else { "NONE" },
        if inbound > 0 { "OK" } else { "NONE" }
    );

    host::close_view(view_id);
    for _ in 0..60 {
        host::pump();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
