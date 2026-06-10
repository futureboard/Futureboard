//! `FutureboardPluginHost-x64.exe` — the separated VST3 plugin/editor host
//! process (IPC *server*).
//!
//! VST3 editor hosting follows public.sdk/samples/vst-hosting/editorhost
//! lifecycle: the host owns the COM STA thread and the editor message pump, and
//! drives `createView`/`attached`/`onSize`/`removed` via the proven C++ backend
//! (`sphere_plugin_host::native_editor`). What is new here is *where* it runs:
//! out-of-process, so a crashing plugin editor cannot take down the GPUI main
//! app.
//!
//! In `main_owned_window` mode (Slice 1 default) the **visible editor window is
//! owned by the main app** — this process only receives an HWND over IPC and
//! attaches the VST3 view to it. The host therefore never creates a top-level
//! editor window; it only pumps messages so the attached `IPlugView` repaints.
//!
//! Protocol: [`HostCommand`] frames arrive on **stdin**, [`HostEvent`] frames
//! are written to **stdout**, human logs go to **stderr** behind
//! `FUTUREBOARD_PLUGIN_VIEW_DEBUG`. See [`sphere_plugin_host::ipc`].

#![cfg_attr(
    all(windows, not(debug_assertions)),
    windows_subsystem = "windows"
)]

use std::collections::HashMap;
use std::io::{self, BufReader};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sphere_plugin_host::audio_bridge::{SharedAudioRegion, AUDIO_BUF_LEN, MAX_BLOCK_FRAMES};
use sphere_plugin_host::ipc::{self, HostCommand, HostEvent, PROTOCOL_VERSION};
use sphere_plugin_host::native_editor::{self, EmbedRegion};
use sphere_plugin_host::plugin_host_preview::{
    try_start_preview_output, BridgeAudioShared, PluginHostPreviewEngine, SharedPluginHostPreview,
};

fn debug_enabled() -> bool {
    std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some()
}

/// Whether the **temporary** separate-CPAL preview output is allowed.
///
/// Stage 1: this is OFF by default. Plugin DSP output is meant to flow into the
/// main DAW engine (mixer / master / meters), not a second device stream, so we
/// must not "fake success" with a private CPAL stream. Until the shared-memory
/// mix path (Stage 3) lands, preview MIDI is still queued to the VSTi but no
/// audio device is opened and the host logs `dsp_output=pending`. Set
/// `FUTUREBOARD_PLUGIN_HOST_CPAL_PREVIEW=1` to opt into the legacy audition
/// stream for manual testing only.
fn debug_audio_out_enabled() -> bool {
    std::env::var("FUTUREBOARD_PLUGIN_HOST_DEBUG_AUDIO_OUT")
        .or_else(|_| std::env::var("FUTUREBOARD_PLUGIN_HOST_CPAL_PREVIEW"))
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            matches!(v.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

fn log_host_audio_mode() {
    let debug = debug_audio_out_enabled();
    eprintln!("[plugin-host-audio] debug_audio_out={debug}");
    eprintln!(
        "[plugin-host-audio] device_stream={}",
        if debug { "debug_only" } else { "disabled" }
    );
    eprintln!("[plugin-host-dsp] output_to=shared_audio_bridge");
}

macro_rules! hlog {
    ($($arg:tt)*) => {{
        if debug_enabled() {
            eprintln!($($arg)*);
        }
    }};
}

fn parse_parent_pid() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--parent-pid" {
            return args.next().and_then(|v| v.parse().ok());
        }
    }
    None
}

fn main() {
    let selftest = std::env::args().any(|a| a == "--selftest");
    let parent_pid = parse_parent_pid();

    let _log_path = sphere_plugin_host::plugin_host_logging::init_host_logging();
    sphere_plugin_host::plugin_host_logging::log_startup_environment();

    platform::com_init();
    platform::ensure_dpi_awareness();
    let pid = std::process::id();
    let thread_id = platform::current_thread_id();
    hlog!("[PluginHostEditor] start pid={pid} thread_id={thread_id} selftest={selftest}");
    if let Some(parent_pid) = parent_pid {
        eprintln!("[plugin-host] parent_pid={parent_pid}");
    }

    // Confirm the main app stripped its renderer-only environment before
    // spawning us (spec Part 1). The host must run with a clean native
    // environment so plugin GPU/WebView/DirectComposition UI can paint.
    log_renderer_env();
    log_runtime_policy();

    if selftest {
        let code = run_selftest();
        platform::com_uninit();
        std::process::exit(code);
    }

    let mut out = io::stdout();
    let _ = ipc::write_frame(
        &mut out,
        &HostEvent::Ready {
            protocol_version: PROTOCOL_VERSION,
            pid,
        },
    );

    let shutdown = Arc::new(AtomicBool::new(false));
    if let Some(parent_pid) = parent_pid {
        let shutdown_flag = shutdown.clone();
        std::thread::Builder::new()
            .name("plugin-host-parent-watch".into())
            .spawn(move || parent_watchdog(parent_pid, shutdown_flag))
            .expect("spawn parent watchdog");
    }

    run_ipc_loop(out, shutdown);
    platform::com_uninit();
}

/// Announce the runtime-ownership policy this host enforces. The external
/// bridge is always authoritative: there is no in-process VST3 runtime and no
/// legacy editor unless `FUTUREBOARD_LEGACY_PLUGIN_EDITOR` is explicitly set.
fn log_runtime_policy() {
    let legacy_enabled = std::env::var_os("FUTUREBOARD_LEGACY_PLUGIN_EDITOR").is_some();
    eprintln!("[plugin-runtime-policy] external_bridge_forced=true");
    eprintln!("[plugin-runtime-policy] legacy_editor_enabled={legacy_enabled}");
    eprintln!("[plugin-runtime-policy] in_process_runtime_allowed=false");
    if legacy_enabled {
        eprintln!(
            "[plugin-runtime-policy] WARNING legacy plugin editor/runtime enabled by FUTUREBOARD_LEGACY_PLUGIN_EDITOR=1"
        );
    }
}

/// Report whether the main-app-only renderer environment leaked into this
/// process. After the spawn-side `sanitize_child_env` fix these should all read
/// `<unset>`; a `set` here means an env var is still being inherited.
fn log_renderer_env() {
    let role = std::env::var("FUTUREBOARD_PROCESS_ROLE").unwrap_or_else(|_| "<unset>".into());
    eprintln!("[plugin-host] FUTUREBOARD_PROCESS_ROLE={role}");
    let dcomp = if std::env::var_os("GPUI_DISABLE_DIRECT_COMPOSITION").is_some() {
        "set"
    } else {
        "<unset>"
    };
    eprintln!("[plugin-host] GPUI_DISABLE_DIRECT_COMPOSITION={dcomp}");
    let leaked = std::env::vars().any(|(k, _)| {
        k.starts_with("GPUI_")
            || k.starts_with("WGPU_")
            || k == "DXGI_PRESENT_ALLOW_TEARING"
            || k == "LIBGL_ALWAYS_SOFTWARE"
    });
    if leaked {
        eprintln!("[plugin-host-env] sanitized=false");
    } else {
        eprintln!("[plugin-host-env] sanitized=true");
    }
}

/// Editor handles keyed by `plugin_instance_id` — the in-process
/// `PluginEditorRegistry` role, living inside the host process.
type Registry = HashMap<String, u64>;

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LoadedPlugin {
    plugin_path: String,
    class_id: String,
    name: String,
    sample_rate: u32,
    max_block_size: u32,
}

type LoadedRegistry = HashMap<String, LoadedPlugin>;

#[derive(Debug)]
#[allow(dead_code)]
struct PendingEditorPrepare {
    prepare_id: u64,
    preferred_width: u32,
    preferred_height: u32,
}

type PendingPrepareRegistry = HashMap<String, PendingEditorPrepare>;

struct DelayedGpuRedraw {
    instance_id: String,
    deadline: Instant,
}

static IDLE_TICK: AtomicU64 = AtomicU64::new(0);

fn parent_watchdog(parent_pid: u32, shutdown: Arc<AtomicBool>) {
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if !platform::is_process_alive(parent_pid) {
            eprintln!("[plugin-host] parent process gone; shutting down");
            shutdown.store(true, Ordering::SeqCst);
            break;
        }
    }
}

fn shutdown_host(
    registry: &mut Registry,
    loaded: &mut LoadedRegistry,
    pending: &mut PendingPrepareRegistry,
    preview: &SharedPluginHostPreview,
    reason: &str,
) {
    let editor_count = registry.len();
    eprintln!("[plugin-host] closing editors count={editor_count} reason={reason}");
    {
        let mut engine = preview.lock();
        for instance_id in registry.drain().map(|(id, _)| id) {
            engine.embed_detach_for_instance(&instance_id);
            engine.unload_instance(&instance_id);
        }
    }
    pending.clear();
    let plugin_count = loaded.len();
    eprintln!("[plugin-host] unloading plugins count={plugin_count}");
    loaded.clear();
    eprintln!("[plugin-host] process exit");
}

/// Stage 3 (host producer side): if the engine has requested a new block via
/// `request_seq`, drain the shared MIDI ring into the loaded VSTi, render one
/// block, write it to `audio_out`, publish the output meters, and acknowledge
/// with `done_seq`. Wait-free handshake — the host never blocks the engine; the
/// engine reads whatever the host last produced (one-block latency).
///
/// NB Stage 3 cadence: this currently runs on the host's ~120 Hz idle loop, so a
/// fast engine block rate will outrun it. Moving production onto a dedicated
/// host audio thread (and removing the `render_block` Vec allocs) is the next
/// refinement; until the engine drives `request_seq` (Stage 3b) this is a no-op.
fn service_audio_bridge(
    region: &SharedAudioRegion,
    dsp: &BridgeAudioShared,
    plugin_instance_id: &str,
) {
    let bridge = region.bridge();
    let req = bridge.request_seq.load(Ordering::Acquire);
    let done = bridge.done_seq.load(Ordering::Relaxed);
    if req == done {
        return; // no new block requested
    }
    // No engine mutex on the block path: the voice list is an Arc snapshot the
    // engine republishes on load/unload only, so the IPC thread can hold the
    // engine lock across editor attach / plugin load for seconds without
    // starving block production (the old `lock_misses` dropouts).
    let frames = (bridge.block_frames.load(Ordering::Relaxed) as usize).min(MAX_BLOCK_FRAMES);
    // Drain engine-pushed MIDI (Stage 3b fills the ring; empty until then).
    let mut midi_count = 0u32;
    while let Some(ev) = bridge.midi.try_pop() {
        dsp.apply_shared_midi(ev);
        midi_count += 1;
    }
    if midi_count > 0 {
        eprintln!(
            "[plugin-host-midi-consume] seq={req} instance={plugin_instance_id} events={midi_count}"
        );
    }
    let mut in_l = [0.0f32; MAX_BLOCK_FRAMES];
    let mut in_r = [0.0f32; MAX_BLOCK_FRAMES];
    // SAFETY: the engine owns `audio_in` until it bumps `request_seq`.
    unsafe {
        bridge
            .audio_in
            .read_deinterleaved(&mut in_l[..frames], &mut in_r[..frames], frames);
    }
    let mut interleaved = [0.0f32; AUDIO_BUF_LEN];
    let len = (frames * 2).min(AUDIO_BUF_LEN);
    let (mix_l, mix_r) =
        dsp.render_single_voice(plugin_instance_id, frames, &in_l[..frames], &in_r[..frames]);
    let mut peak_l = 0.0f32;
    let mut peak_r = 0.0f32;
    for i in 0..frames {
        let l = mix_l.get(i).copied().unwrap_or(0.0);
        let r = mix_r.get(i).copied().unwrap_or(0.0);
        if let Some(slot) = interleaved.get_mut(i * 2) {
            *slot = l;
        }
        if let Some(slot) = interleaved.get_mut(i * 2 + 1) {
            *slot = r;
        }
        peak_l = peak_l.max(l.abs());
        peak_r = peak_r.max(r.abs());
    }
    let dsp_ready = dsp.dsp_ready() && (dsp.has_loaded_instances() || dsp.continuous_mode());
    // SAFETY: the host owns `audio_out` for this block — the engine waits on
    // `done_seq` (published below) before reading it.
    unsafe {
        bridge.audio_out.write_interleaved(&interleaved[..len]);
    }
    bridge.store_meters(peak_l, peak_r);
    bridge.set_dsp_output_ready(dsp_ready);
    // Throttled: at most one audible-output trace per ~256 blocks so the
    // producer thread never floods stderr while sound is playing.
    static VST3_PROCESS_LOG_BLOCKS: AtomicU64 = AtomicU64::new(0);
    if (peak_l > 0.0001 || peak_r > 0.0001)
        && VST3_PROCESS_LOG_BLOCKS.fetch_add(1, Ordering::Relaxed) % 256 == 0
    {
        eprintln!(
            "[vst3-process] instance={plugin_instance_id} frames={frames} midi_events={midi_count} output_peak_l={peak_l:.6} output_peak_r={peak_r:.6}",
        );
    }
    bridge.done_seq.store(req, Ordering::Release);
}

/// All mapped shared-audio regions, keyed by insert `plugin_instance_id`.
type SharedAudioRegions = Arc<Mutex<HashMap<String, Arc<SharedAudioRegion>>>>;

/// Dedicated host audio producer thread (Stage 3 cadence fix). Once the region
/// is mapped it services blocks on a tight cadence instead of the ~120 Hz idle
/// loop, so it can keep up with the engine's block rate. Servicing is a cheap
/// no-op until the engine drives `request_seq`. VST3 `process()` runs only here
/// (the editor stays on the STA main thread); the per-voice MIDI mutex inside
/// `BridgeAudioShared` serializes it against IPC MIDI so there is never a
/// concurrent `process()` — without coupling block production to the engine
/// mutex held across plugin load / editor attach.
fn run_audio_producer(
    regions: SharedAudioRegions,
    dsp: Arc<BridgeAudioShared>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        let snapshot = regions.lock().map(|map| map.clone()).unwrap_or_default();
        for (instance_id, region) in snapshot {
            service_audio_bridge(region.as_ref(), &dsp, &instance_id);
        }
        // Acknowledge the latest voice-snapshot publish now that any snapshot
        // borrowed for this block has been dropped (lets unload hand the final
        // processor release back to the IPC thread).
        dsp.mark_snapshot_observed();
        // ~4 kHz poll: responsive to the engine's block requests without a full
        // busy-spin. No-op cost is one atomic load when no block is pending.
        std::thread::sleep(Duration::from_micros(250));
    }
}

fn run_ipc_loop(mut out: io::Stdout, shutdown: Arc<AtomicBool>) {
    // Commands are read on a dedicated thread so the STA/message-pump thread
    // never blocks on stdin (spec Part 9). Each received command kicks the UI
    // thread out of its message wait so IPC latency stays low even while the
    // loop idles in MsgWaitForMultipleObjectsEx.
    let ui_thread_id = platform::current_thread_id();
    let (tx, rx) = crossbeam_channel::unbounded::<HostCommand>();
    std::thread::Builder::new()
        .name("plugin-host-stdin".into())
        .spawn(move || {
            let mut reader = BufReader::new(io::stdin());
            loop {
                match ipc::read_frame::<HostCommand, _>(&mut reader) {
                    Ok(Some(cmd)) => {
                        if tx.send(cmd).is_err() {
                            break;
                        }
                        platform::wake_ui_thread(ui_thread_id);
                    }
                    Ok(None) => {
                        eprintln!("[plugin-host] stdin eof; parent likely exited; shutting down");
                        break;
                    }
                    Err(_) => break,
                }
            }
            platform::wake_ui_thread(ui_thread_id);
        })
        .expect("spawn plugin-host stdin reader");

    let mut registry = Registry::new();
    let mut loaded = LoadedRegistry::new();
    let mut pending_prepare = PendingPrepareRegistry::new();
    let mut delayed_redraws: Vec<DelayedGpuRedraw> = Vec::new();
    // Latest requested editor size per instance (coalesced from ResizeEditor
    // commands), applied below with a bounded preview try-lock.
    let mut pending_resizes: HashMap<String, (u32, u32, u32)> = HashMap::new();
    let preview: SharedPluginHostPreview = PluginHostPreviewEngine::shared(48_000, 256);
    let mut preview_output_started = false;
    log_host_audio_mode();
    // Stage 2/3: the mapped shared-memory audio bridge (engine-created), shared
    // with the dedicated audio producer thread. The `Arc` keeps the view mapped
    // for the host's lifetime.
    let region_slots: SharedAudioRegions = Arc::new(Mutex::new(HashMap::new()));
    {
        let slots = region_slots.clone();
        let dsp = preview.lock().bridge_shared();
        let shutdown = shutdown.clone();
        std::thread::Builder::new()
            .name("plugin-host-audio".into())
            .spawn(move || run_audio_producer(slots, dsp, shutdown))
            .expect("spawn plugin-host audio producer");
    }

    eprintln!(
        "[PluginUIThread] loop started thread_id={}",
        platform::current_thread_id()
    );
    if platform::editor_safe_mode() {
        eprintln!(
            "[PluginEditorSafe] FUTUREBOARD_PLUGIN_EDITOR_SAFE=1 — window-tree polling, \
             per-message verbose logs, attach-time re-entrant pumping, and focus hacks disabled"
        );
    }
    // Pump-gap watchdog: if message dispatch stalls >50ms while an editor is
    // open, the plugin UI freezes (cross-process parenting attaches input
    // queues, so a wedged host thread blocks clicks on plugin dialogs too).
    let mut last_pump_done = Instant::now();
    let mut window_tree: std::collections::HashMap<u64, String> =
        std::collections::HashMap::new();
    // Spin watchdog state: consecutive wakes that claimed input but dispatched
    // nothing (the signature of a 100% CPU pump spin).
    let mut spin_iterations: u32 = 0;
    let mut spin_window_start = Instant::now();
    let mut last_wait_mode: &'static str = "";

    loop {
        if shutdown.load(Ordering::SeqCst) {
            eprintln!("[PluginUIThread] loop exited reason=parent_watchdog");
            shutdown_host(
                &mut registry,
                &mut loaded,
                &mut pending_prepare,
                &preview,
                "parent_watchdog",
            );
            return;
        }
        let mut slowest_section: &'static str = "none";
        let mut slowest_section_ms: u128 = 0;
        macro_rules! timed_section {
            ($name:expr, $body:expr) => {{
                let started = Instant::now();
                let result = $body;
                let elapsed = started.elapsed().as_millis();
                if elapsed > slowest_section_ms {
                    slowest_section_ms = elapsed;
                    slowest_section = $name;
                }
                result
            }};
        }

        // 1. Drain and dispatch every queued command. Each dispatch is timed:
        //    long handlers (plugin load, editor attach) block this pump and —
        //    because cross-process parenting attaches input queues — block
        //    clicks on plugin windows too. The watchdog below names them.
        loop {
            match rx.try_recv() {
                Ok(cmd) => {
                    if matches!(cmd, HostCommand::Shutdown) {
                        hlog!("[PluginHostEditor] shutdown requested");
                        eprintln!("[PluginUIThread] loop exited reason=ipc_shutdown");
                        shutdown_host(
                            &mut registry,
                            &mut loaded,
                            &mut pending_prepare,
                            &preview,
                            "ipc_shutdown",
                        );
                        return;
                    }
                    timed_section!("ipc_dispatch", {
                        dispatch(
                            cmd,
                            &mut registry,
                            &mut loaded,
                            &mut pending_prepare,
                            &mut delayed_redraws,
                            &mut pending_resizes,
                            &preview,
                            &mut preview_output_started,
                            &region_slots,
                            &mut out,
                        )
                    });
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    eprintln!("[plugin-host] stdin eof; parent likely exited; shutting down");
                    eprintln!("[PluginUIThread] loop exited reason=stdin_disconnect");
                    shutdown_host(
                        &mut registry,
                        &mut loaded,
                        &mut pending_prepare,
                        &preview,
                        "stdin_disconnect",
                    );
                    return;
                }
            }
        }

        // 1b. Apply coalesced editor resizes (latest size per instance). The
        //     processor clone is fetched with a bounded try-lock so a busy DSP
        //     block can never stall the pump during an interactive resize;
        //     entries that miss the lock are retried next tick (≤8ms away).
        timed_section!("editor_resize", {
            if !pending_resizes.is_empty() {
                // Clone processor handles under a short bounded lock, then
                // apply the (possibly slow) onSize work with the lock RELEASED
                // so the audio producer never waits on editor UI work.
                type ResizeJob = (String, u32, u32, u32, DAUx::vst3_processor::Vst3RuntimeProcessor);
                let jobs: Option<(Vec<ResizeJob>, Vec<String>)> = preview
                    .try_lock_for(Duration::from_millis(2))
                    .map(|engine| {
                        let mut jobs = Vec::new();
                        let mut gone = Vec::new();
                        for (instance_id, (width, height, dpi)) in pending_resizes.iter() {
                            match engine.clone_processor_for(instance_id) {
                                Some(processor) => jobs.push((
                                    instance_id.clone(),
                                    *width,
                                    *height,
                                    *dpi,
                                    processor,
                                )),
                                None => gone.push(instance_id.clone()),
                            }
                        }
                        (jobs, gone)
                    });
                if let Some((jobs, gone)) = jobs {
                    for instance_id in gone {
                        pending_resizes.remove(&instance_id); // unloaded — drop request
                    }
                    for (instance_id, width, height, dpi, processor) in jobs {
                        eprintln!(
                            "[plugin-bridge] ResizeEditor instance={instance_id} \
                             width={width} height={height} dpi={dpi}"
                        );
                        processor.embed_set_bounds(0, 0, width as i32, height as i32);
                        processor.embed_refresh();
                        pending_resizes.remove(&instance_id);
                    }
                }
            }
        });

        // 2. Keep attached editors painting / geometry in sync, and pump our own
        //    message queue so the foreign-parented IPlugView gets messages.
        //    The preview-engine mutex is shared with the DSP producer thread —
        //    this UI thread must NEVER block on it inside the pump path: use
        //    short bounded try-locks and skip the tick when the lock is busy.
        timed_section!("editor_refresh", {
            let refresh_targets: Option<Vec<(String, DAUx::vst3_processor::Vst3RuntimeProcessor)>> =
                preview.try_lock_for(Duration::from_millis(2)).map(|engine| {
                    registry
                        .keys()
                        .filter_map(|id| {
                            engine
                                .clone_processor_for(id)
                                .map(|processor| (id.clone(), processor))
                        })
                        .collect()
                });
            if let Some(refresh_targets) = refresh_targets {
                for (instance_id, processor) in refresh_targets {
                    processor.embed_refresh();
                    // Safe mode: no extra per-editor pump here — the main
                    // `pump_messages` below drains the whole thread queue.
                    if !platform::editor_safe_mode() {
                        if let Some(host_hwnd) = registry.get(&instance_id).copied() {
                            platform::pump_editor_messages(host_hwnd);
                        }
                    }
                }
            }
        });
        timed_section!("resize_poll", {
            let resizes = preview
                .try_lock_for(Duration::from_millis(2))
                .map(|engine| engine.poll_pending_editor_resizes())
                .unwrap_or_default();
            for (instance_id, width, height) in resizes {
                eprintln!(
                    "[PluginEditor] top window resize notify instance={instance_id} content={width}x{height}"
                );
                let _ = ipc::write_frame(
                    &mut out,
                    &HostEvent::EditorContentResize {
                        plugin_instance_id: instance_id,
                        width,
                        height,
                        dpi: platform::system_dpi(),
                    },
                );
            }
        });
        let now = Instant::now();
        timed_section!("delayed_redraw", {
            delayed_redraws.retain(|entry| {
                if now >= entry.deadline {
                    let processor = preview
                        .try_lock_for(Duration::from_millis(2))
                        .and_then(|engine| engine.clone_processor_for(&entry.instance_id));
                    let Some(processor) = processor else {
                        // Lock busy — keep the entry and retry next tick.
                        return true;
                    };
                    processor.embed_refresh();
                    if !platform::editor_safe_mode() {
                        if let Some(host_hwnd) = registry.get(&entry.instance_id).copied() {
                            platform::pump_editor_messages(host_hwnd);
                        }
                    }
                    false
                } else {
                    true
                }
            });
        });
        platform::set_editor_roots(registry.values().copied().collect());
        let dispatched = platform::pump_messages();
        // Freeze watchdog tiers (spec item 10):
        //  >50ms   name the slow section,
        //  >1000ms dump the window/thread snapshot,
        //  >3000ms notify the main app so it can surface "not responding"
        //          (the wrapper + close path live in the main process, so the
        //          user can always close a wedged editor).
        let gap_ms = last_pump_done.elapsed().as_millis() as u64;
        if !registry.is_empty() {
            if gap_ms > 50 {
                eprintln!(
                    "[PluginUIThread] pump gap ms={gap_ms} suspected_block={slowest_section} \
                     section_ms={slowest_section_ms} dispatched={dispatched}"
                );
            }
            if gap_ms > 1000 {
                platform::plugin_editor_snapshot("pump_gap");
            }
            if gap_ms > 3000 {
                eprintln!(
                    "[PluginUIThread] editor not responding gap_ms={gap_ms} notifying_main_app=true"
                );
                for instance_id in registry.keys() {
                    let _ = ipc::write_frame(
                        &mut out,
                        &HostEvent::EditorUnresponsive {
                            plugin_instance_id: instance_id.clone(),
                            gap_ms,
                        },
                    );
                }
            }
        }
        last_pump_done = Instant::now();

        // Stage 3 block production runs on the dedicated `plugin-host-audio`
        // thread (see `run_audio_producer`) — not here — so the engine's block
        // rate is met instead of throttled to this ~120 Hz idle loop.

        let tick = IDLE_TICK.fetch_add(1, Ordering::Relaxed);
        if platform::plugin_debug()
            && !platform::editor_safe_mode()
            && !registry.is_empty()
            && tick.is_multiple_of(120)
        {
            // Track plugin-created child/popup/dialog windows. Throttled to
            // ~1/sec (spec item 2); fully disabled in safe mode.
            let roots: Vec<u64> = registry.values().copied().collect();
            platform::log_window_tree_changes(&roots, &mut window_tree);
        }
        if tick.is_multiple_of(60) {
            eprintln!("[PluginUIThread] loop alive editor_count={}", registry.len());
            eprintln!("[plugin-host-ui-thread] message_loop_running=true");
            eprintln!("[plugin-host-ui-thread] editor_count={}", registry.len());
            eprintln!("[plugin-host-ui-thread] idle_tick={tick}");
            if let Ok(slots) = region_slots.lock() {
                eprintln!(
                    "[plugin-host-bridge] shared_audio mapped_regions={}",
                    slots.len()
                );
                for (instance_id, region) in slots.iter() {
                    let bridge = region.bridge();
                    let (peak_l, peak_r) = bridge.meters();
                    eprintln!(
                        "[plugin-host-bridge] instance={instance_id} request_seq={} done_seq={} dsp_output={} peak_l={peak_l:.3} peak_r={peak_r:.3}",
                        bridge.request_seq.load(Ordering::Relaxed),
                        bridge.done_seq.load(Ordering::Relaxed),
                        if bridge.dsp_output_ready() {
                            "ready"
                        } else {
                            "pending"
                        }
                    );
                }
            }
        }

        // 3. Wait for input instead of busy-polling (spec item 3): the loop
        //    idles in MsgWaitForMultipleObjectsEx and wakes immediately on any
        //    queued message or a wake_ui_thread kick from the stdin reader.
        //    With editors open the timeout keeps the old ~120 Hz refresh
        //    cadence; idle it stretches to 50ms. CPU is ~0% when nothing
        //    happens either way.
        let (wait_ms, wait_mode): (u32, &'static str) = if registry.is_empty() {
            (50, "idle_msgwait_50ms")
        } else {
            (8, "editor_msgwait_8ms")
        };
        if wait_mode != last_wait_mode {
            eprintln!("[PluginUIThread] idle wait mode={wait_mode}");
            last_wait_mode = wait_mode;
        }
        let woke_on_input = platform::wait_for_input(wait_ms);
        // Spin watchdog (spec item 3): repeated "input available" wakes that
        // then dispatch nothing means the queue is being signalled without
        // producing messages — the loop would spin at 100% CPU. Name it.
        if woke_on_input && dispatched == 0 {
            spin_iterations += 1;
            if spin_iterations >= 200 {
                eprintln!(
                    "[PluginUIThread] spin warning iterations={spin_iterations} messages=0 \
                     duration={:?}",
                    spin_window_start.elapsed()
                );
                spin_iterations = 0;
                spin_window_start = Instant::now();
            }
        } else {
            spin_iterations = 0;
            spin_window_start = Instant::now();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch(
    cmd: HostCommand,
    registry: &mut Registry,
    loaded: &mut LoadedRegistry,
    _pending_prepare: &mut PendingPrepareRegistry,
    delayed_redraws: &mut Vec<DelayedGpuRedraw>,
    pending_resizes: &mut HashMap<String, (u32, u32, u32)>,
    preview: &SharedPluginHostPreview,
    preview_output_started: &mut bool,
    region_slots: &SharedAudioRegions,
    out: &mut io::Stdout,
) {
    match cmd {
        HostCommand::Hello { protocol_version } => {
            // The startup `Ready` is the handshake response; `Hello` only carries
            // the client's version for a compatibility check.
            if protocol_version != PROTOCOL_VERSION {
                hlog!(
                    "[PluginHostEditor] protocol mismatch client={protocol_version} host={PROTOCOL_VERSION}"
                );
            }
        }
        HostCommand::Ping => {
            hlog!("[PluginHostEditor] Ping → Pong");
            let _ = ipc::write_frame(
                out,
                &HostEvent::Pong {
                    pid: std::process::id(),
                },
            );
        }
        HostCommand::LoadPlugin {
            plugin_instance_id,
            plugin_path,
            class_id,
            sample_rate,
            max_block_size,
        } => {
            hlog!(
                "[plugin-host] LoadPlugin instance={plugin_instance_id} path={plugin_path} class_id={class_id} sr={sample_rate} block={max_block_size}"
            );
            if !std::path::Path::new(&plugin_path).exists() {
                let error = format!("plugin path not found: {plugin_path}");
                let _ = ipc::write_frame(
                    out,
                    &HostEvent::PluginLoadFailed {
                        plugin_instance_id,
                        error,
                    },
                );
                return;
            }
            let name = std::path::Path::new(&plugin_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("VST3 Plugin")
                .to_string();
            if loaded.contains_key(&plugin_instance_id) {
                eprintln!(
                    "[plugin-host] LoadPlugin instance={plugin_instance_id} already_loaded=true reuse=true"
                );
                let _ = ipc::write_frame(
                    out,
                    &HostEvent::PluginAlreadyLoaded {
                        plugin_instance_id,
                        name,
                    },
                );
                return;
            }
            let _ = ipc::write_frame(
                out,
                &HostEvent::PluginLoading {
                    plugin_instance_id: plugin_instance_id.clone(),
                },
            );
            let plugin_path_loaded = plugin_path.clone();
            let class_id_loaded = class_id.clone();
            loaded.insert(
                plugin_instance_id.clone(),
                LoadedPlugin {
                    plugin_path,
                    class_id,
                    name: name.clone(),
                    sample_rate,
                    max_block_size,
                },
            );
            let load_ok = {
                let mut preview_engine = preview.lock();
                preview_engine.load_instance(
                    &plugin_instance_id,
                    &plugin_path_loaded,
                    &class_id_loaded,
                    sample_rate,
                    max_block_size,
                )
            };
            if !load_ok {
                loaded.remove(&plugin_instance_id);
                let error = format!(
                    "Plugin failed to load. It may require a newer CPU instruction set \
                     or a missing runtime dependency. path={plugin_path_loaded}"
                );
                eprintln!("[PluginHost] instance load failed id={plugin_instance_id} error={error}");
                let _ = ipc::write_frame(
                    out,
                    &HostEvent::PluginLoadFailed {
                        plugin_instance_id,
                        error,
                    },
                );
                return;
            }
            eprintln!("[PluginHost] instance created id={plugin_instance_id}");
            preview.lock().set_continuous_mode(true);
            let _ = ipc::write_frame(
                out,
                &HostEvent::PluginLoaded {
                    plugin_instance_id,
                    name,
                },
            );
        }
        HostCommand::OpenEditorWithParentHwnd {
            plugin_instance_id,
            parent_hwnd,
            width,
            height,
            dpi,
            ..
        } => {
            attach_unified_editor(
                &plugin_instance_id,
                parent_hwnd,
                width,
                height,
                dpi,
                registry,
                delayed_redraws,
                preview,
                out,
            );
        }
        HostCommand::PrepareEditorView {
            plugin_instance_id, ..
        } => {
            eprintln!("[PluginHost] editor open requested id={plugin_instance_id}");
            if !preview.lock().has_instance(&plugin_instance_id) {
                emit_attach_failed(
                    out,
                    &plugin_instance_id,
                    "plugin not loaded — call LoadPlugin first",
                );
                return;
            }
            eprintln!("[plugin-host] OpenEditor uses existing instance={plugin_instance_id}");
            sphere_plugin_host::plugin_host_preview::PluginHostPreviewEngine::verify_unified_runtime(
                &plugin_instance_id,
                &plugin_instance_id,
                &plugin_instance_id,
                &plugin_instance_id,
                &plugin_instance_id,
                &plugin_instance_id,
                &plugin_instance_id,
                &plugin_instance_id,
            );
            let (preferred_width, preferred_height) = preview
                .lock()
                .editor_content_size_for_instance(&plugin_instance_id);
            let _ = ipc::write_frame(
                out,
                &HostEvent::EditorPreferredSize {
                    plugin_instance_id,
                    width: preferred_width,
                    height: preferred_height,
                },
            );
        }
        HostCommand::ConfirmEditorContentReady {
            plugin_instance_id,
            parent_hwnd,
            width,
            height,
            dpi,
            ..
        } => {
            attach_unified_editor(
                &plugin_instance_id,
                parent_hwnd,
                width,
                height,
                dpi,
                registry,
                delayed_redraws,
                preview,
                out,
            );
        }
        HostCommand::ResizeEditor {
            plugin_instance_id,
            width,
            height,
            dpi,
        } => {
            // Coalesce into the pending map; applied by the UI loop with a
            // bounded try-lock (spec item 9: resizing must never block the
            // pump thread on the DSP/preview mutex). Interactive drags stream
            // many ResizeEditor commands — only the latest size matters.
            pending_resizes.insert(plugin_instance_id.clone(), (width, height, dpi));
            hlog!(
                "[PluginHostEditor] resize queued plugin_instance_id={plugin_instance_id} \
                 width={width} height={height} dpi={dpi}"
            );
        }
        HostCommand::CloseEditor { plugin_instance_id } => {
            eprintln!("[PluginHost] editor close requested id={plugin_instance_id}");
            preview.lock().preview_all_notes_off(&plugin_instance_id);
            registry.remove(&plugin_instance_id);
            pending_resizes.remove(&plugin_instance_id);
            if let Some(processor) = preview.lock().clone_processor_for(&plugin_instance_id) {
                processor.embed_detach();
            }
            delayed_redraws.retain(|entry| entry.instance_id != plugin_instance_id);
            let still_active = preview.lock().has_instance(&plugin_instance_id);
            eprintln!(
                "[PluginHost] editor closed id={plugin_instance_id} instance_still_active={still_active}"
            );
            let _ = ipc::write_frame(out, &HostEvent::EditorClosed { plugin_instance_id });
        }
        HostCommand::PreviewNoteOn {
            plugin_instance_id,
            channel,
            pitch,
            velocity,
        } => {
            // Stage 1: the legacy separate-CPAL audition stream is OFF by
            // default. The MIDI is still queued to the VSTi so the eventual
            // shared-memory mix path (Stage 3) can pull its output, but we never
            // open a second device stream — log the honest pending state instead.
            if debug_audio_out_enabled() {
                if !*preview_output_started {
                    *preview_output_started = try_start_preview_output(preview);
                }
            } else if !*preview_output_started {
                *preview_output_started = true; // log once
                eprintln!(
                    "[plugin-host-midi] dsp_output=pending reason=main_mix_integration_pending \
                     (separate CPAL preview disabled; set FUTUREBOARD_PLUGIN_HOST_CPAL_PREVIEW=1 to audition)"
                );
            }
            preview
                .lock()
                .preview_note_on(&plugin_instance_id, channel, pitch, velocity);
        }
        HostCommand::PreviewNoteOff {
            plugin_instance_id,
            channel,
            pitch,
        } => {
            preview
                .lock()
                .preview_note_off(&plugin_instance_id, channel, pitch);
        }
        HostCommand::PreviewAllNotesOff { plugin_instance_id } => {
            preview.lock().preview_all_notes_off(&plugin_instance_id);
        }
        HostCommand::MidiPanic { plugin_instance_id } => {
            preview.lock().midi_panic(&plugin_instance_id);
        }
        HostCommand::UnloadPlugin { plugin_instance_id } => {
            eprintln!(
                "[PluginHost] unload requested id={plugin_instance_id} reason=user_removed_insert"
            );
            registry.remove(&plugin_instance_id);
            preview.lock().unload_instance(&plugin_instance_id);
            loaded.remove(&plugin_instance_id);
            if let Ok(mut slots) = region_slots.lock() {
                slots.remove(&plugin_instance_id);
            }
            let instance_count = preview.lock().loaded_instance_ids().len();
            eprintln!(
                "[PluginHost] host shutdown deferred instance_count={instance_count} editor_count={}",
                registry.len()
            );
            hlog!(
                "[PluginHostEditor] unload plugin_instance_id={plugin_instance_id} released=true"
            );
            let _ = ipc::write_frame(out, &HostEvent::PluginUnloaded { plugin_instance_id });
        }
        HostCommand::ConfigureAudioBridge {
            sample_rate,
            max_block_size,
        } => {
            // Stage 1: the main engine owns sample rate / block size; follow it.
            let (sr, block) = preview.lock().configure(sample_rate, max_block_size);
            eprintln!(
                "[plugin-host-bridge] ConfigureAudioBridge engine_sr={sample_rate} engine_block={max_block_size} \
                 host_sr={sr} host_block={block} follows_engine=true"
            );
            let _ = ipc::write_frame(
                out,
                &HostEvent::AudioBridgeConfigured {
                    sample_rate: sr,
                    max_block_size: block,
                    follows_engine: true,
                },
            );
        }
        HostCommand::ProcessBlockShared { block_id, frames } => {
            // Stage 1 skeleton: the lock-free shared-memory audio/MIDI transport
            // is Stage 2/3. Acknowledge honestly — plugin DSP output is NOT yet
            // mixed into the main engine.
            let dsp_ready = preview.lock().dsp_ready();
            let dsp_output = if dsp_ready { "ready" } else { "pending" };
            eprintln!(
                "[plugin-host-bridge] ProcessBlockShared block_id={block_id} frames={frames} dsp_output={dsp_output}"
            );
            let _ = ipc::write_frame(
                out,
                &HostEvent::AudioBridgeStatus {
                    block_id,
                    dsp_output: dsp_output.to_string(),
                    latency_samples: 0,
                },
            );
        }
        HostCommand::AttachSharedAudio {
            name,
            bytes,
            plugin_instance_id,
        } => {
            #[cfg(windows)]
            {
                match SharedAudioRegion::open_named(&name) {
                    Ok(region) => {
                        let sr = region.bridge().sample_rate.load(Ordering::Relaxed);
                        let block = region.bridge().max_block_size.load(Ordering::Relaxed);
                        eprintln!(
                            "[plugin-host-bridge] AttachSharedAudio instance={plugin_instance_id} name={name} bytes={bytes} attached=true header_sr={sr} header_block={block}"
                        );
                        region.bridge().set_dsp_output_ready(true);
                        if let Ok(mut slots) = region_slots.lock() {
                            let key = if plugin_instance_id.is_empty() {
                                name.clone()
                            } else {
                                plugin_instance_id.clone()
                            };
                            slots.insert(key, Arc::new(region));
                        }
                        preview.lock().set_dsp_ready(true);
                        log_host_audio_mode();
                        let _ = ipc::write_frame(
                            out,
                            &HostEvent::SharedAudioAttached {
                                attached: true,
                                name,
                                bytes,
                            },
                        );
                    }
                    Err(error) => {
                        eprintln!(
                            "[plugin-host-bridge] AttachSharedAudio name={name} attached=false error={error}"
                        );
                        let _ = ipc::write_frame(
                            out,
                            &HostEvent::SharedAudioAttached {
                                attached: false,
                                name,
                                bytes,
                            },
                        );
                    }
                }
            }
            #[cfg(not(windows))]
            {
                let _ = region_slots;
                eprintln!("[plugin-host-bridge] AttachSharedAudio unsupported on this platform name={name}");
                let _ = ipc::write_frame(
                    out,
                    &HostEvent::SharedAudioAttached {
                        attached: false,
                        name,
                        bytes,
                    },
                );
            }
        }
        HostCommand::PrepareProcessing {
            plugin_instance_id,
            sample_rate,
            max_block_size,
            input_channels,
            output_channels,
        } => {
            eprintln!(
                "[plugin-bridge] PrepareProcessing instance={plugin_instance_id} sr={sample_rate} block={max_block_size}"
            );
            if !preview.lock().has_instance(&plugin_instance_id) {
                emit_attach_failed(
                    out,
                    &plugin_instance_id,
                    "PrepareProcessing: instance not loaded",
                );
                return;
            }
            eprintln!(
                "[plugin-host] PrepareProcessing uses existing instance={plugin_instance_id}"
            );
            let (sr, block) = preview.lock().configure(sample_rate, max_block_size);
            preview.lock().set_dsp_ready(true);
            eprintln!(
                "[plugin-host-dsp] prepared instance={plugin_instance_id} sr={sr} block={block} outputs={output_channels} same_instance=true"
            );
            let _ = ipc::write_frame(
                out,
                &HostEvent::ProcessingPrepared {
                    plugin_instance_id,
                    sample_rate: sr,
                    max_block_size: block,
                    output_channels,
                },
            );
            let _ = input_channels;
        }
        HostCommand::Shutdown => {
            // Handled in run_ipc_loop before dispatch; unreachable here.
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn attach_unified_editor(
    plugin_instance_id: &str,
    parent_hwnd: u64,
    width: u32,
    height: u32,
    dpi: u32,
    registry: &mut Registry,
    delayed_redraws: &mut Vec<DelayedGpuRedraw>,
    preview: &SharedPluginHostPreview,
    out: &mut io::Stdout,
) {
    eprintln!("[plugin-host] editor_ownership=main_owned forced=true");
    eprintln!("[plugin-host] using provided parent_hwnd=0x{parent_hwnd:x}");
    eprintln!("[plugin-host] not creating top-level editor window");
    eprintln!("[plugin-host] OpenEditor uses existing instance={plugin_instance_id}");
    if !preview.lock().has_instance(plugin_instance_id) {
        emit_attach_failed(
            out,
            plugin_instance_id,
            "plugin not loaded — call LoadPlugin first",
        );
        return;
    }
    if !platform::is_window(parent_hwnd) {
        emit_attach_failed(out, plugin_instance_id, "parent_hwnd is not a valid window");
        return;
    }
    let w = width.max(1) as i32;
    let h = height.max(1) as i32;
    eprintln!(
        "[PluginEditor] open requested while engine_state=Running transport_playing=unknown instance={plugin_instance_id}"
    );
    let processor = preview.lock().clone_processor_for(plugin_instance_id);
    let Some(processor) = processor else {
        emit_attach_failed(
            out,
            plugin_instance_id,
            "plugin not loaded — call LoadPlugin first",
        );
        return;
    };
    processor.embed_set_instance_label(plugin_instance_id);
    eprintln!("[plugin-editor] createView from existing controller (reuse loaded runtime)");
    let handle = processor.embed_editor(parent_hwnd, 0, 0, w, h);
    let Some(handle) = handle else {
        emit_attach_failed(
            out,
            plugin_instance_id,
            "embed_editor failed on existing runtime instance",
        );
        return;
    };
    // `embed_editor` returns an opaque monotonic handle; the registry must
    // hold the REAL attach HWND so the focus/pump helpers operate on a valid
    // window (passing the counter made both silently no-op behind IsWindow).
    let attach_hwnd = processor.embed_attach_hwnd();
    if attach_hwnd == 0 {
        eprintln!(
            "[PluginEditorHWND] WARNING attach_hwnd unavailable instance={plugin_instance_id} handle={handle}"
        );
    }
    registry.insert(
        plugin_instance_id.to_string(),
        if attach_hwnd != 0 { attach_hwnd } else { handle },
    );
    preview.lock().set_continuous_mode(true);
    if platform::editor_safe_mode() {
        // Safe mode: no focus walk and no re-entrant pumping inside the attach
        // handler — the main loop pump delivers messages a few ms later.
        eprintln!("[PluginEditorSafe] attach: skipped focus walk and attach-time pump");
    } else {
        platform::focus_plugin_editor_child(attach_hwnd);
        platform::pump_editor_messages(attach_hwnd);
    }
    // Capture safety + one-shot window snapshot on editor open (spec 5/8).
    platform::log_capture_on_open(attach_hwnd);
    platform::set_editor_roots(registry.values().copied().collect());
    platform::plugin_editor_snapshot("editor_open");
    let (preferred_width, preferred_height) = preview
        .lock()
        .editor_content_size_for_instance(plugin_instance_id);
    // Editor resize contract (spec item 1): IPlugView::canResize decides
    // whether the main app may let the user drag-resize the wrapper. Unknown
    // (no view) defaults to resizable so we never wrongly lock a window.
    let resizable = processor.editor_resizable().unwrap_or(true);
    eprintln!(
        "[PluginEditorResize] instance={plugin_instance_id} canResize={resizable} \
         preferred={preferred_width}x{preferred_height}"
    );
    eprintln!(
        "[PluginEditor] open complete engine_state=Running transport_playing=unknown instance={plugin_instance_id}"
    );
    sphere_plugin_host::plugin_host_preview::PluginHostPreviewEngine::verify_unified_runtime(
        plugin_instance_id,
        plugin_instance_id,
        plugin_instance_id,
        plugin_instance_id,
        plugin_instance_id,
        plugin_instance_id,
        plugin_instance_id,
        plugin_instance_id,
    );
    eprintln!(
        "[plugin-host] attached result=ok instance={plugin_instance_id} handle=0x{handle:x} unified=true"
    );
    hlog!(
        "[PluginHostEditor] attached_result=ok handle=0x{handle:x} onSize=({width}x{height}) dpi={dpi}"
    );
    delayed_redraws.push(DelayedGpuRedraw {
        instance_id: plugin_instance_id.to_string(),
        deadline: Instant::now() + Duration::from_millis(100),
    });
    let _ = ipc::write_frame(
        out,
        &HostEvent::EditorAttached {
            plugin_instance_id: plugin_instance_id.to_string(),
            result: 0,
            preferred_width,
            preferred_height,
            resizable,
            host_hwnd: attach_hwnd,
        },
    );
}

fn emit_attach_failed(out: &mut io::Stdout, plugin_instance_id: &str, error: &str) {
    let _ = ipc::write_frame(
        out,
        &HostEvent::EditorAttachFailed {
            plugin_instance_id: plugin_instance_id.to_string(),
            error: error.to_string(),
        },
    );
}

/// Self-test path (`--selftest`): prove that the host can create a real
/// content **child** HWND distinct from a top HWND, with the required Win32
/// styles, and (optionally) attach a plugin to it. Drives the acceptance logs
/// without needing the main app or a real plugin.
///
/// Set `FUTUREBOARD_SELFTEST_PLUGIN_PATH` + `FUTUREBOARD_SELFTEST_CLASS_ID` to
/// also exercise a real VST3 attach. Exit code 0 on success.
fn run_selftest() -> i32 {
    match platform::create_selftest_windows() {
        Some((top_hwnd, content_hwnd)) => {
            let content_is_child = content_hwnd != top_hwnd && content_hwnd != 0;
            eprintln!("[plugin-view] selected_host_mode=main_owned_window");
            eprintln!("[plugin-view] top_hwnd=0x{top_hwnd:x}");
            eprintln!("[plugin-view] content_hwnd=0x{content_hwnd:x}");
            eprintln!("[plugin-view] content_is_child={content_is_child}");
            eprintln!("[plugin-view] content_parent=0x{top_hwnd:x}");
            if content_hwnd == top_hwnd {
                eprintln!("[plugin-view] ERROR content_hwnd == top_hwnd — not attaching");
                platform::destroy_selftest_windows(top_hwnd, content_hwnd);
                return 1;
            }
            eprintln!("[plugin-view] content_hwnd != top_hwnd");

            let mut code = 0;
            if let (Ok(path), Ok(class_id)) = (
                std::env::var("FUTUREBOARD_SELFTEST_PLUGIN_PATH"),
                std::env::var("FUTUREBOARD_SELFTEST_CLASS_ID"),
            ) {
                let region = EmbedRegion {
                    x: 0,
                    y: 0,
                    width: 800,
                    height: 600,
                };
                match native_editor::attach_editor_into_parent(
                    content_hwnd,
                    &path,
                    &class_id,
                    region,
                ) {
                    Ok(handle) => {
                        eprintln!("[vst3-editor] attached begin parent=0x{content_hwnd:x}");
                        eprintln!("[vst3-editor] attached result=ok handle=0x{handle:x}");
                        native_editor::detach_editor(handle);
                    }
                    Err(err) => {
                        eprintln!("[vst3-editor] attached result=err {err}");
                        code = 1;
                    }
                }
            } else {
                eprintln!(
                    "[plugin-view] selftest: no FUTUREBOARD_SELFTEST_PLUGIN_PATH/CLASS_ID — \
                     HWND hierarchy only"
                );
            }

            platform::destroy_selftest_windows(top_hwnd, content_hwnd);
            code
        }
        None => {
            eprintln!("[plugin-view] selftest: window creation unavailable on this platform");
            // Not a failure on non-Windows — there is nothing to host there yet.
            0
        }
    }
}

// ---------------------------------------------------------------------------
// Platform shims. Windows is the real implementation; other targets get no-op
// stubs so the binary still compiles and the IPC loop still runs.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::HiDpi::{
        GetDpiForSystem, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::System::Threading::{
        GetCurrentThreadId, GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetCapture, GetFocus, IsWindowEnabled, ReleaseCapture, SetFocus,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        ChildWindowFromPointEx, CreateWindowExW, DestroyWindow, DispatchMessageW,
        EnumChildWindows, EnumThreadWindows, GetAncestor, GetClassNameW, GetParent, GetWindow,
        GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsChild, IsDialogMessageW,
        IsWindow, IsWindowVisible, MsgWaitForMultipleObjectsEx, PeekMessageW, PostThreadMessageW,
        TranslateMessage, WindowFromPoint, CWP_ALL, CW_USEDEFAULT, GA_PARENT, GA_ROOT, GWL_EXSTYLE,
        GWL_STYLE, GWLP_HWNDPARENT, GW_CHILD, GW_OWNER, MSG, MWMO_INPUTAVAILABLE, PM_REMOVE,
        QS_ALLINPUT, WINDOW_EX_STYLE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN,
        WM_MOUSEMOVE, WM_NULL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_TIMER, WS_CHILD, WS_CLIPCHILDREN,
        WS_CLIPSIBLINGS, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };
    use windows::core::BOOL;
    use windows::Win32::Foundation::{LPARAM, RECT, WAIT_OBJECT_0, WPARAM};

    /// End-to-end plugin debug switch (`FUTUREBOARD_PLUGIN_DEBUG=1`), shared
    /// with the narrower view-debug flag.
    pub fn plugin_debug() -> bool {
        static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FLAG.get_or_init(|| {
            std::env::var_os("FUTUREBOARD_PLUGIN_DEBUG").is_some()
                || std::env::var_os("FUTUREBOARD_PLUGIN_VIEW_DEBUG").is_some()
        })
    }

    /// Plugin editor safe mode (`FUTUREBOARD_PLUGIN_EDITOR_SAFE=1`): disables
    /// window-tree polling, per-message verbose logs, re-entrant pumping inside
    /// attach/load handlers, and experimental focus hacks. Keeps only minimal
    /// diagnostics (loop alive, pump gap, spin warning, focus/capture summary
    /// on click, snapshot on editor open).
    pub fn editor_safe_mode() -> bool {
        static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_PLUGIN_EDITOR_SAFE").is_some())
    }

    /// Coarse log rate limiter: allows at most `max` events per second.
    pub struct LogRate {
        window_start_ms: std::sync::atomic::AtomicU64,
        count: std::sync::atomic::AtomicU32,
        max_per_sec: u32,
    }

    impl LogRate {
        pub const fn new(max_per_sec: u32) -> Self {
            Self {
                window_start_ms: std::sync::atomic::AtomicU64::new(0),
                count: std::sync::atomic::AtomicU32::new(0),
                max_per_sec,
            }
        }

        pub fn allow(&self) -> bool {
            use std::sync::atomic::Ordering;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let start = self.window_start_ms.load(Ordering::Relaxed);
            if now.saturating_sub(start) >= 1000 {
                self.window_start_ms.store(now, Ordering::Relaxed);
                self.count.store(1, Ordering::Relaxed);
                return true;
            }
            self.count.fetch_add(1, Ordering::Relaxed) < self.max_per_sec
        }
    }

    /// Editor root HWNDs currently registered (registry mirror) — feeds the
    /// click-path diagnostic and the on-demand window snapshot without
    /// threading the registry through every pump call.
    static EDITOR_ROOTS: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

    pub fn set_editor_roots(roots: Vec<u64>) {
        if let Ok(mut guard) = EDITOR_ROOTS.lock() {
            if *guard != roots {
                *guard = roots;
            }
        }
    }

    fn editor_roots() -> Vec<u64> {
        EDITOR_ROOTS.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn class_name(hwnd: HWND) -> String {
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 64];
        let len = unsafe { GetClassNameW(hwnd, &mut buf) };
        if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        }
    }

    fn hwnd_from(handle: u64) -> HWND {
        HWND(handle as *mut core::ffi::c_void)
    }

    /// True if `hwnd` is a real Win32 dialog (class `#32770`).
    fn is_dialog_class(hwnd: HWND) -> bool {
        if hwnd.0.is_null() {
            return false;
        }
        let mut buf = [0u16; 16];
        let len = unsafe { GetClassNameW(hwnd, &mut buf) };
        len > 0 && String::from_utf16_lossy(&buf[..len as usize]) == "#32770"
    }

    /// Nearest dialog (`#32770`) in the parent chain of `hwnd`, if any.
    /// `IsDialogMessageW` must only run against real dialog windows — calling
    /// it with an arbitrary window as the "dialog" swallows Tab/arrow/Enter/
    /// Escape keystrokes destined for plugin editor controls.
    fn dialog_ancestor(hwnd: HWND) -> Option<HWND> {
        let mut cur = hwnd;
        let mut depth = 0;
        while !cur.0.is_null() && depth < 32 {
            if is_dialog_class(cur) {
                return Some(cur);
            }
            cur = unsafe { GetAncestor(cur, GA_PARENT) };
            depth += 1;
        }
        None
    }

    pub fn com_init() {
        // STA: VST3 editors require apartment-threaded COM (spec Part 9).
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
    }

    pub fn ensure_dpi_awareness() {
        unsafe {
            let ctx = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            eprintln!(
                "[PluginEditor] dpi_awareness_context=0x{:x} tid={}",
                ctx.0 as usize,
                GetCurrentThreadId()
            );
        }
    }

    pub fn system_dpi() -> u32 {
        unsafe {
            let dpi = GetDpiForSystem();
            if dpi == 0 { 96 } else { dpi }
        }
    }

    pub fn com_uninit() {
        unsafe { CoUninitialize() };
    }

    pub fn current_thread_id() -> u64 {
        unsafe { GetCurrentThreadId() as u64 }
    }

    pub fn is_process_alive(pid: u32) -> bool {
        const STILL_ACTIVE: u32 = 259;
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut code = 0u32;
            let alive = if GetExitCodeProcess(handle, &mut code).is_ok() {
                code == STILL_ACTIVE
            } else {
                false
            };
            let _ = CloseHandle(handle);
            alive
        }
    }

    pub fn is_window(handle: u64) -> bool {
        if handle == 0 {
            return false;
        }
        unsafe { IsWindow(Some(hwnd_from(handle))).as_bool() }
    }

    fn log_window_brief(label: &str, hwnd: HWND) {
        if hwnd.0.is_null() {
            return;
        }
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let owner = HWND(GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) as *mut core::ffi::c_void);
            eprintln!(
                "[PluginEditor] window styles {label} hwnd=0x{:x} owner=0x{:x} style=0x{style:08x}",
                hwnd.0 as u64,
                owner.0 as u64
            );
        }
    }

    /// Focus the deepest plugin-owned child under the embed host HWND.
    pub fn focus_plugin_editor_child(host_hwnd: u64) {
        if host_hwnd == 0 {
            return;
        }
        unsafe {
            let host = hwnd_from(host_hwnd);
            if !IsWindow(Some(host)).as_bool() {
                return;
            }
            log_window_brief("top", host);
            let mut target = host;
            let mut child = GetWindow(host, GW_CHILD).unwrap_or_default();
            while !child.0.is_null() && IsWindow(Some(child)).as_bool() {
                target = child;
                child = GetWindow(child, GW_CHILD).unwrap_or_default();
            }
            if target != host {
                let _ = SetFocus(Some(target));
                eprintln!(
                    "[PluginEditor] focus set child=0x{:x}",
                    target.0 as u64
                );
            } else {
                let _ = SetFocus(Some(host));
                eprintln!(
                    "[PluginEditor] focus set child=0x{:x}",
                    host.0 as u64
                );
            }
            {
                use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, GetFocus};
                eprintln!(
                    "[PluginEditorInput] focus=0x{:x} capture=0x{:x}",
                    GetFocus().0 as u64,
                    GetCapture().0 as u64
                );
            }
        }
    }

    /// Pump messages for the plugin editor subtree (host + descendants).
    /// Bounded: drains at most `MAX_PUMP_PER_CALL` messages per call so a
    /// message-storming plugin window can never wedge the loop here.
    pub fn pump_editor_messages(host_hwnd: u64) {
        if host_hwnd == 0 {
            return;
        }
        unsafe {
            let host = hwnd_from(host_hwnd);
            if !IsWindow(Some(host)).as_bool() {
                return;
            }
            static PUMP_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let mut pumped = 0u32;
            let mut msg = MSG::default();
            while pumped < MAX_PUMP_PER_CALL
                && PeekMessageW(&mut msg, Some(host), 0, 0, PM_REMOVE).as_bool()
            {
                let _ = TranslateMessage(&msg);
                // Generic dialog routing: only treat real `#32770` dialogs as
                // dialogs; never run IsDialogMessage against plugin windows.
                if let Some(dialog) = dialog_ancestor(msg.hwnd) {
                    if IsDialogMessageW(dialog, &mut msg).as_bool() {
                        pumped += 1;
                        continue;
                    }
                }
                DispatchMessageW(&msg);
                pumped += 1;
            }
            if pumped > 0 {
                let n = PUMP_LOG.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n % 120 == 0 {
                    eprintln!("[PluginEditor] modal/dialog message pump active drained={pumped}");
                }
            }
        }
    }

    /// Upper bound on messages drained by any single pump call. The loop comes
    /// back within milliseconds, so capping a single drain only bounds latency
    /// for pathological message storms — it never drops messages.
    const MAX_PUMP_PER_CALL: u32 = 512;

    /// Block until this thread's message queue has input or `timeout_ms`
    /// elapses. Returns `true` when woken by input. This replaces the old
    /// unconditional `sleep(8ms)` poll: the loop now idles in the kernel and
    /// wakes immediately on messages (or a `wake_ui_thread` kick), so it never
    /// spins and never adds fixed latency to plugin window input.
    pub fn wait_for_input(timeout_ms: u32) -> bool {
        unsafe {
            // MWMO_INPUTAVAILABLE: also wake for input that was already in the
            // queue when we started waiting (avoids the classic stale-QS-bits
            // missed-wakeup, which would otherwise show up as input lag).
            let result =
                MsgWaitForMultipleObjectsEx(None, timeout_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            result == WAIT_OBJECT_0
        }
    }

    /// Wake the UI thread out of `wait_for_input` (used by the stdin reader
    /// thread when a new IPC command arrives). WM_NULL is a no-op message.
    pub fn wake_ui_thread(thread_id: u64) {
        unsafe {
            let _ = PostThreadMessageW(thread_id as u32, WM_NULL, WPARAM(0), LPARAM(0));
        }
    }

    /// Capture safety on editor open (spec: capture should be null or
    /// plugin-owned while interacting). Logs the current focus/capture once;
    /// if an HWND *unrelated* to the new editor holds mouse capture on this
    /// thread, release it. Never sets capture.
    pub fn log_capture_on_open(host_hwnd: u64) {
        unsafe {
            let capture = GetCapture();
            let focus = GetFocus();
            eprintln!(
                "[PluginEditorInput] editor_open focus=0x{:x} capture=0x{:x}",
                focus.0 as u64,
                capture.0 as u64
            );
            if capture.0.is_null() {
                return;
            }
            let host = hwnd_from(host_hwnd);
            let related = host_hwnd != 0
                && (capture == host || IsChild(host, capture).as_bool());
            if !related {
                let _ = ReleaseCapture();
                eprintln!(
                    "[PluginEditorInput] released_unrelated_capture=0x{:x}",
                    capture.0 as u64
                );
            }
        }
    }

    /// Debug classification of a message target: wrapper chrome, our embed
    /// host windows, a dialog, or a plugin-owned window.
    fn classify_target(hwnd: HWND) -> &'static str {
        let class = class_name(hwnd);
        match class.as_str() {
            "FutureboardDauxVst3EditorContent" | "FutureboardDauxVst3EditorChild" => "embed_host",
            "FutureboardDauxVst3EditorDetached" => "embed_top",
            "SpherePluginEditorShell" | "SpherePluginEditorContent" => "wrapper",
            "#32770" => "dialog",
            _ => "plugin_owned",
        }
    }

    fn log_input_dispatch(msg: &MSG) {
        // Default dispatch logging covers only mouse button / key messages —
        // never per-move/per-timer/per-paint floods (those are throttled
        // separately in `log_throttled_noise`).
        let interesting = matches!(
            msg.message,
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
                | WM_KEYDOWN
        );
        if !interesting {
            return;
        }
        let x = (msg.lParam.0 & 0xFFFF) as i16 as i32;
        let y = ((msg.lParam.0 >> 16) & 0xFFFF) as i16 as i32;
        let root = unsafe { GetAncestor(msg.hwnd, GA_ROOT) };
        eprintln!(
            "[PluginUIThread] dispatch hwnd=0x{:x} msg=0x{:04x} target={} class='{}' \
             client=({x},{y}) screen=({},{}) root=0x{:x}",
            msg.hwnd.0 as u64,
            msg.message,
            classify_target(msg.hwnd),
            class_name(msg.hwnd),
            msg.pt.x,
            msg.pt.y,
            root.0 as u64,
        );
    }

    /// Throttled high-frequency message tracing (debug mode, not safe mode):
    /// WM_MOUSEMOVE and WM_TIMER at most 2/sec each.
    fn log_throttled_noise(msg: &MSG) {
        static MOUSE_MOVE_RATE: LogRate = LogRate::new(2);
        static TIMER_RATE: LogRate = LogRate::new(2);
        let rate = match msg.message {
            WM_MOUSEMOVE => &MOUSE_MOVE_RATE,
            WM_TIMER => &TIMER_RATE,
            _ => return,
        };
        if rate.allow() {
            eprintln!(
                "[PluginUIThread] trace hwnd=0x{:x} msg=0x{:04x} class='{}' (throttled 2/sec)",
                msg.hwnd.0 as u64,
                msg.message,
                class_name(msg.hwnd),
            );
        }
    }

    /// Click-path diagnostic (spec item 9): for a left click, log everything
    /// needed to tell wrong-hit-test / disabled-window / focus-capture /
    /// wrong-thread / consumed-by-dialog-routing apart. Throttled to 4/sec.
    fn log_click_path(msg: &MSG) {
        unsafe {
            let pt = msg.pt; // screen coordinates of the click
            let wfp = WindowFromPoint(pt);
            let focus = GetFocus();
            let capture = GetCapture();
            let mut target_pid = 0u32;
            let target_tid = GetWindowThreadProcessId(msg.hwnd, Some(&mut target_pid));
            let our_tid = windows::Win32::System::Threading::GetCurrentThreadId();
            eprintln!(
                "[PluginClickPath] screen=({},{}) msg_hwnd=0x{:x} class='{}' target={} \
                 enabled={} visible={} target_tid={target_tid} target_pid={target_pid} \
                 our_tid={our_tid} same_thread={}",
                pt.x,
                pt.y,
                msg.hwnd.0 as u64,
                class_name(msg.hwnd),
                classify_target(msg.hwnd),
                IsWindowEnabled(msg.hwnd).as_bool(),
                IsWindowVisible(msg.hwnd).as_bool(),
                target_tid == our_tid,
            );
            eprintln!(
                "[PluginClickPath] window_from_point=0x{:x} wfp_class='{}' wfp_enabled={} \
                 focus=0x{:x} capture=0x{:x}",
                wfp.0 as u64,
                class_name(wfp),
                IsWindowEnabled(wfp).as_bool(),
                focus.0 as u64,
                capture.0 as u64,
            );
            // Hit-test the wrapper (cross-process top-level) and each editor
            // root so a wrong/covered hit target is visible in one log line.
            let wrapper = GetAncestor(msg.hwnd, GA_ROOT);
            let mut probes: Vec<(&'static str, HWND)> = vec![("wrapper", wrapper)];
            let roots = editor_roots();
            for root in &roots {
                probes.push(("editor_root", hwnd_from(*root)));
            }
            for (label, probe) in probes {
                if probe.0.is_null() || !IsWindow(Some(probe)).as_bool() {
                    continue;
                }
                let mut client = pt;
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(probe, &mut client);
                let child = ChildWindowFromPointEx(probe, client, CWP_ALL);
                eprintln!(
                    "[PluginClickPath] {label}=0x{:x} child_from_point=0x{:x} child_class='{}'",
                    probe.0 as u64,
                    child.0 as u64,
                    class_name(child),
                );
            }
        }
    }

    /// Non-blocking drain of this thread's message queue. Returns the number
    /// of messages dispatched (pump-gap watchdog input). Bounded per call.
    pub fn pump_messages() -> u32 {
        let debug = plugin_debug();
        let safe = editor_safe_mode();
        static CLICK_PATH_RATE: LogRate = LogRate::new(4);
        let mut dispatched = 0u32;
        unsafe {
            let mut msg = MSG::default();
            while dispatched < MAX_PUMP_PER_CALL
                && PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool()
            {
                if debug && !safe {
                    log_throttled_noise(&msg);
                }
                if debug {
                    log_input_dispatch(&msg);
                }
                // Focus/capture + hit-test summary on click (kept in safe mode;
                // throttled so a click storm cannot flood stderr).
                let click_diag = msg.message == WM_LBUTTONDOWN && CLICK_PATH_RATE.allow();
                if click_diag {
                    log_click_path(&msg);
                }
                let _ = TranslateMessage(&msg);
                // `IsDialogMessageW(msg.hwnd, …)` treated EVERY window as a
                // dialog, swallowing Tab/arrow/Enter/Escape keystrokes that
                // belong to plugin editor controls. Only route through real
                // `#32770` dialogs in the target's parent chain, and never
                // consume a message that IsDialogMessage did not handle.
                let mut dialog_candidate = 0u64;
                let mut dialog_handled = false;
                if let Some(dialog) = dialog_ancestor(msg.hwnd) {
                    dialog_candidate = dialog.0 as u64;
                    static DIALOG_RATE: LogRate = LogRate::new(1);
                    if DIALOG_RATE.allow() {
                        eprintln!(
                            "[PluginUIThread] dialog candidate hwnd=0x{dialog_candidate:x}"
                        );
                    }
                    dialog_handled = IsDialogMessageW(dialog, &mut msg).as_bool();
                    if dialog_handled && debug && !safe {
                        eprintln!(
                            "[PluginUIThread] IsDialogMessage handled msg=0x{:04x} hwnd=0x{:x}",
                            msg.message, msg.hwnd.0 as u64
                        );
                    }
                }
                if click_diag {
                    eprintln!(
                        "[PluginClickPath] dialog_candidate=0x{dialog_candidate:x} \
                         is_dialog_message_handled={dialog_handled} dispatched={}",
                        !dialog_handled
                    );
                }
                if dialog_handled {
                    dispatched += 1;
                    continue;
                }
                DispatchMessageW(&msg);
                dispatched += 1;
            }
        }
        dispatched
    }

    /// One-shot window/thread state snapshot (spec item 8): wrapper, embed
    /// child, dialogs, and descendants with class/style/parent/owner/enabled/
    /// visible/rect/thread/process. Triggered once per editor open and from
    /// the pump-gap watchdog — throttled, never per-frame.
    pub fn plugin_editor_snapshot(reason: &str) {
        static SNAPSHOT_RATE: LogRate = LogRate::new(1);
        if !SNAPSHOT_RATE.allow() {
            return;
        }
        const MAX_WINDOWS: usize = 64;
        fn snapshot_one(label: &str, hwnd: HWND, count: &mut usize) {
            if hwnd.0.is_null() || *count >= MAX_WINDOWS {
                return;
            }
            unsafe {
                if !IsWindow(Some(hwnd)).as_bool() {
                    return;
                }
                *count += 1;
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let parent = GetParent(hwnd).unwrap_or_default();
                let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &mut rect);
                let mut pid = 0u32;
                let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
                eprintln!(
                    "[PluginEditorSnapshot] {label} hwnd=0x{:x} class='{}' style=0x{style:08x} \
                     exstyle=0x{exstyle:08x} parent=0x{:x} owner=0x{:x} enabled={} visible={} \
                     rect=({},{},{},{}) tid={tid} pid={pid}",
                    hwnd.0 as u64,
                    class_name(hwnd),
                    parent.0 as u64,
                    owner.0 as u64,
                    IsWindowEnabled(hwnd).as_bool(),
                    IsWindowVisible(hwnd).as_bool(),
                    rect.left,
                    rect.top,
                    rect.right,
                    rect.bottom,
                );
            }
        }
        struct SnapCtx {
            count: usize,
        }
        unsafe extern "system" fn snap_child(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let ctx = unsafe { &mut *(lparam.0 as *mut SnapCtx) };
            if ctx.count >= MAX_WINDOWS {
                return BOOL(0);
            }
            snapshot_one("descendant", hwnd, &mut ctx.count);
            BOOL(1)
        }
        unsafe extern "system" fn snap_thread_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let ctx = unsafe { &mut *(lparam.0 as *mut SnapCtx) };
            if ctx.count >= MAX_WINDOWS {
                return BOOL(0);
            }
            snapshot_one("thread_window", hwnd, &mut ctx.count);
            unsafe {
                let _ = EnumChildWindows(Some(hwnd), Some(snap_child), lparam);
            }
            BOOL(1)
        }
        let roots = editor_roots();
        eprintln!(
            "[PluginEditorSnapshot] begin reason={reason} editor_roots={}",
            roots.len()
        );
        let mut ctx = SnapCtx { count: 0 };
        unsafe {
            for root in &roots {
                let root_hwnd = hwnd_from(*root);
                if !IsWindow(Some(root_hwnd)).as_bool() {
                    continue;
                }
                // GA_ROOT crosses the process boundary to the main-app wrapper.
                let wrapper = GetAncestor(root_hwnd, GA_ROOT);
                snapshot_one("wrapper", wrapper, &mut ctx.count);
                if wrapper != root_hwnd {
                    snapshot_one("embed_root", root_hwnd, &mut ctx.count);
                }
                let _ = EnumChildWindows(
                    Some(wrapper),
                    Some(snap_child),
                    LPARAM(&mut ctx as *mut SnapCtx as isize),
                );
            }
            // Popups/dialogs the plugin created on this UI thread (not under
            // the wrapper tree — e.g. #32770 file dialogs, license prompts).
            let tid = windows::Win32::System::Threading::GetCurrentThreadId();
            let _ = EnumThreadWindows(
                tid,
                Some(snap_thread_window),
                LPARAM(&mut ctx as *mut SnapCtx as isize),
            );
            let focus = GetFocus();
            let capture = GetCapture();
            eprintln!(
                "[PluginEditorSnapshot] end windows={} focus=0x{:x} capture=0x{:x}",
                ctx.count,
                focus.0 as u64,
                capture.0 as u64
            );
        }
    }

    /// Debug helper: diff the set of windows on this UI thread plus every
    /// descendant of the given editor roots against `known`, logging windows
    /// that appeared or disappeared. Confirms plugin-created popups/dialogs
    /// exist, are enabled, and live in the expected tree — no vendor logic.
    pub fn log_window_tree_changes(
        roots: &[u64],
        known: &mut std::collections::HashMap<u64, String>,
    ) {
        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let set = unsafe { &mut *(lparam.0 as *mut Vec<u64>) };
            set.push(hwnd.0 as u64);
            unsafe {
                let _ = EnumChildWindows(Some(hwnd), Some(collect_children), lparam);
            }
            BOOL(1)
        }
        unsafe extern "system" fn collect_children(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let set = unsafe { &mut *(lparam.0 as *mut Vec<u64>) };
            set.push(hwnd.0 as u64);
            BOOL(1)
        }
        let mut current: Vec<u64> = Vec::with_capacity(64);
        unsafe {
            let tid = windows::Win32::System::Threading::GetCurrentThreadId();
            let _ = EnumThreadWindows(
                tid,
                Some(collect),
                LPARAM(&mut current as *mut Vec<u64> as isize),
            );
            for root in roots {
                if *root != 0 && IsWindow(Some(hwnd_from(*root))).as_bool() {
                    current.push(*root);
                    let _ = EnumChildWindows(
                        Some(hwnd_from(*root)),
                        Some(collect_children),
                        LPARAM(&mut current as *mut Vec<u64> as isize),
                    );
                }
            }
        }
        let current: std::collections::HashSet<u64> = current.into_iter().collect();
        for hwnd_v in &current {
            if known.contains_key(hwnd_v) {
                continue;
            }
            let hwnd = hwnd_from(*hwnd_v);
            let class = class_name(hwnd);
            unsafe {
                let parent = GetParent(hwnd).unwrap_or_default();
                let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
                eprintln!(
                    "[PluginEditorWindowTree] child hwnd=0x{hwnd_v:x} class='{class}' \
                     parent=0x{:x} owner=0x{:x} enabled={} visible={}",
                    parent.0 as u64,
                    owner.0 as u64,
                    IsWindowEnabled(hwnd).as_bool(),
                    IsWindowVisible(hwnd).as_bool(),
                );
            }
            known.insert(*hwnd_v, class);
        }
        known.retain(|hwnd_v, class| {
            if current.contains(hwnd_v) {
                return true;
            }
            eprintln!("[PluginEditorWindowTree] gone hwnd=0x{hwnd_v:x} class='{class}'");
            false
        });
    }

    /// Create a top window + a real WS_CHILD content window using the
    /// predefined `STATIC` class (no RegisterClass/WndProc needed). Returns
    /// `(top_hwnd, content_hwnd)` as `u64`s.
    pub fn create_selftest_windows() -> Option<(u64, u64)> {
        unsafe {
            let top = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("Futureboard Plugin Host Selftest"),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                820,
                640,
                None,
                None,
                None,
                None,
            )
            .ok()?;

            let content = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                800,
                600,
                Some(top),
                None,
                None,
                None,
            )
            .ok()?;

            Some((top.0 as u64, content.0 as u64))
        }
    }

    pub fn destroy_selftest_windows(top: u64, content: u64) {
        unsafe {
            if content != 0 {
                let _ = DestroyWindow(hwnd_from(content));
            }
            if top != 0 {
                let _ = DestroyWindow(hwnd_from(top));
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn com_init() {}
    pub fn ensure_dpi_awareness() {}
    pub fn system_dpi() -> u32 {
        96
    }
    pub fn com_uninit() {}
    pub fn current_thread_id() -> u64 {
        0
    }
    pub fn is_process_alive(_pid: u32) -> bool {
        true
    }
    pub fn is_window(handle: u64) -> bool {
        handle != 0
    }
    pub fn pump_messages() -> u32 {
        0
    }
    pub fn plugin_debug() -> bool {
        false
    }
    pub fn editor_safe_mode() -> bool {
        false
    }
    pub fn set_editor_roots(_roots: Vec<u64>) {}
    pub fn plugin_editor_snapshot(_reason: &str) {}
    pub fn log_capture_on_open(_host_hwnd: u64) {}
    pub fn wake_ui_thread(_thread_id: u64) {}
    pub fn wait_for_input(timeout_ms: u32) -> bool {
        std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
        false
    }
    pub fn log_window_tree_changes(
        _roots: &[u64],
        _known: &mut std::collections::HashMap<u64, String>,
    ) {
    }
    pub fn focus_plugin_editor_child(_host_hwnd: u64) {}
    pub fn pump_editor_messages(_host_hwnd: u64) {}
    pub fn create_selftest_windows() -> Option<(u64, u64)> {
        None
    }
    pub fn destroy_selftest_windows(_top: u64, _content: u64) {}
}
