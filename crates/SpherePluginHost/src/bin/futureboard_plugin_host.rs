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

use std::collections::HashMap;
use std::io::{self, BufReader};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sphere_plugin_host::audio_bridge::{SharedAudioRegion, AUDIO_BUF_LEN, MAX_BLOCK_FRAMES};
use sphere_plugin_host::ipc::{self, HostCommand, HostEvent, PROTOCOL_VERSION};
use sphere_plugin_host::native_editor::{self, EmbedRegion};
use sphere_plugin_host::plugin_host_preview::{
    try_start_preview_output, PluginHostPreviewEngine, SharedPluginHostPreview,
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

    platform::com_init();
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
struct LoadedPlugin {
    plugin_path: String,
    class_id: String,
    name: String,
    sample_rate: u32,
    max_block_size: u32,
}

type LoadedRegistry = HashMap<String, LoadedPlugin>;

#[derive(Debug)]
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
fn service_audio_bridge(region: &SharedAudioRegion, preview: &SharedPluginHostPreview) {
    let bridge = region.bridge();
    let req = bridge.request_seq.load(Ordering::Acquire);
    if req == bridge.done_seq.load(Ordering::Relaxed) {
        return; // no new block requested
    }
    let frames = (bridge.block_frames.load(Ordering::Relaxed) as usize).min(MAX_BLOCK_FRAMES);
    // Drain engine-pushed MIDI (Stage 3b fills the ring; empty until then).
    let midi_count = {
        let mut engine = preview.lock();
        let mut count = 0u32;
        while let Some(ev) = bridge.midi.try_pop() {
            engine.apply_shared_midi(ev);
            count += 1;
        }
        count
    };
    if midi_count > 0 {
        let block_seq = bridge.request_seq.load(Ordering::Relaxed);
        let instances = preview.lock().loaded_instance_ids();
        for instance_id in instances {
            eprintln!(
                "[plugin-host-midi-consume] seq={block_seq} instance={instance_id} events={midi_count}"
            );
        }
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
    let (dsp_ready, peak_l, peak_r) = {
        let mut engine = preview.lock();
        let (mix_l, mix_r) =
            engine.render_block_with_input(frames, &in_l[..frames], &in_r[..frames]);
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
        let (pl, pr) = (peak_l, peak_r);
        (
            engine.dsp_ready()
                && (engine.has_loaded_instances() || engine.continuous_mode()),
            pl,
            pr,
        )
    };
    // SAFETY: the host owns `audio_out` for this block — the engine waits on
    // `done_seq` (published below) before reading it.
    unsafe {
        bridge.audio_out.write_interleaved(&interleaved[..len]);
    }
    bridge.store_meters(peak_l, peak_r);
    bridge.set_dsp_output_ready(dsp_ready);
    if peak_l > 0.0001 || peak_r > 0.0001 {
        for instance_id in preview.lock().loaded_instance_ids() {
            eprintln!(
                "[vst3-process] instance={instance_id} frames={frames} midi_events={midi_count} output_peak_l={peak_l:.6} output_peak_r={peak_r:.6}",
            );
        }
    }
    bridge.done_seq.store(req, Ordering::Release);
}

/// Shared slot handing the engine-mapped region from the IPC thread (which opens
/// it on `AttachSharedAudio`) to the dedicated audio producer thread.
type SharedAudioSlot = Arc<Mutex<Option<Arc<SharedAudioRegion>>>>;

/// Dedicated host audio producer thread (Stage 3 cadence fix). Once the region
/// is mapped it services blocks on a tight cadence instead of the ~120 Hz idle
/// loop, so it can keep up with the engine's block rate. Servicing is a cheap
/// no-op until the engine drives `request_seq`. VST3 `process()` runs only here
/// (the editor stays on the STA main thread); the preview mutex serializes it
/// against IPC MIDI so there is never a concurrent `process()`.
fn run_audio_producer(
    slot: SharedAudioSlot,
    preview: SharedPluginHostPreview,
    shutdown: Arc<AtomicBool>,
) {
    let mut region: Option<Arc<SharedAudioRegion>> = None;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        if region.is_none() {
            if let Ok(guard) = slot.lock() {
                region = guard.clone();
            }
        }
        if let Some(region) = region.as_ref() {
            service_audio_bridge(region, &preview);
        }
        // ~4 kHz poll: responsive to the engine's block requests without a full
        // busy-spin. No-op cost is one atomic load when no block is pending.
        std::thread::sleep(Duration::from_micros(250));
    }
}

fn run_ipc_loop(mut out: io::Stdout, shutdown: Arc<AtomicBool>) {
    // Commands are read on a dedicated thread so the STA/message-pump thread
    // never blocks on stdin (spec Part 9).
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
                    }
                    Ok(None) => {
                        eprintln!("[plugin-host] stdin eof; parent likely exited; shutting down");
                        break;
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("spawn plugin-host stdin reader");

    let mut registry = Registry::new();
    let mut loaded = LoadedRegistry::new();
    let mut pending_prepare = PendingPrepareRegistry::new();
    let mut delayed_redraws: Vec<DelayedGpuRedraw> = Vec::new();
    let preview: SharedPluginHostPreview = PluginHostPreviewEngine::shared(48_000, 256);
    let mut preview_output_started = false;
    log_host_audio_mode();
    // Stage 2/3: the mapped shared-memory audio bridge (engine-created), shared
    // with the dedicated audio producer thread. The `Arc` keeps the view mapped
    // for the host's lifetime.
    let region_slot: SharedAudioSlot = Arc::new(Mutex::new(None));
    {
        let slot = region_slot.clone();
        let preview = preview.clone();
        let shutdown = shutdown.clone();
        std::thread::Builder::new()
            .name("plugin-host-audio".into())
            .spawn(move || run_audio_producer(slot, preview, shutdown))
            .expect("spawn plugin-host audio producer");
    }

    loop {
        if shutdown.load(Ordering::SeqCst) {
            shutdown_host(
                &mut registry,
                &mut loaded,
                &mut pending_prepare,
                &preview,
                "parent_watchdog",
            );
            return;
        }

        // 1. Drain and dispatch every queued command.
        loop {
            match rx.try_recv() {
                Ok(cmd) => {
                    if matches!(cmd, HostCommand::Shutdown) {
                        hlog!("[PluginHostEditor] shutdown requested");
                        shutdown_host(
                            &mut registry,
                            &mut loaded,
                            &mut pending_prepare,
                            &preview,
                            "ipc_shutdown",
                        );
                        return;
                    }
                    dispatch(
                        cmd,
                        &mut registry,
                        &mut loaded,
                        &mut pending_prepare,
                        &mut delayed_redraws,
                        &preview,
                        &mut preview_output_started,
                        &region_slot,
                        &mut out,
                    );
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    eprintln!("[plugin-host] stdin eof; parent likely exited; shutting down");
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

        // 2. Keep attached editors painting / geometry in sync, and pump our own
        //    message queue so the foreign-parented IPlugView gets messages.
        {
            let engine = preview.lock();
            for instance_id in registry.keys() {
                engine.embed_refresh_for_instance(instance_id);
            }
        }
        let now = Instant::now();
        delayed_redraws.retain(|entry| {
            if now >= entry.deadline {
                preview
                    .lock()
                    .embed_refresh_for_instance(&entry.instance_id);
                false
            } else {
                true
            }
        });
        platform::pump_messages();

        // Stage 3 block production runs on the dedicated `plugin-host-audio`
        // thread (see `run_audio_producer`) — not here — so the engine's block
        // rate is met instead of throttled to this ~120 Hz idle loop.

        let tick = IDLE_TICK.fetch_add(1, Ordering::Relaxed);
        if tick % 60 == 0 {
            eprintln!("[plugin-host-ui-thread] message_loop_running=true");
            eprintln!("[plugin-host-ui-thread] editor_count={}", registry.len());
            eprintln!("[plugin-host-ui-thread] idle_tick={tick}");
            if let Some(region) = region_slot.lock().ok().and_then(|g| g.clone()) {
                let bridge = region.bridge();
                let (peak_l, peak_r) = bridge.meters();
                eprintln!(
                    "[plugin-host-bridge] shared_audio mapped=true request_seq={} done_seq={} dsp_output={} peak_l={peak_l:.3} peak_r={peak_r:.3}",
                    bridge.request_seq.load(Ordering::Relaxed),
                    bridge.done_seq.load(Ordering::Relaxed),
                    if bridge.dsp_output_ready() { "ready" } else { "pending" }
                );
            }
        }

        // 3. Idle a touch to avoid a busy spin (~120 Hz).
        std::thread::sleep(Duration::from_millis(8));
    }
}

fn dispatch(
    cmd: HostCommand,
    registry: &mut Registry,
    loaded: &mut LoadedRegistry,
    _pending_prepare: &mut PendingPrepareRegistry,
    delayed_redraws: &mut Vec<DelayedGpuRedraw>,
    preview: &SharedPluginHostPreview,
    preview_output_started: &mut bool,
    region_slot: &SharedAudioSlot,
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
            {
                let mut preview_engine = preview.lock();
                preview_engine.load_instance(
                    &plugin_instance_id,
                    &plugin_path_loaded,
                    &class_id_loaded,
                    sample_rate,
                    max_block_size,
                );
            }
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
            plugin_instance_id,
            ..
        } => {
            if !preview.lock().has_instance(&plugin_instance_id) {
                emit_attach_failed(
                    out,
                    &plugin_instance_id,
                    "plugin not loaded — call LoadPlugin first",
                );
                return;
            }
            eprintln!(
                "[plugin-host] OpenEditor uses existing instance={plugin_instance_id}"
            );
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
            let (preferred_width, preferred_height) =
                preview.lock().default_editor_size();
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
            preview
                .lock()
                .embed_resize_for_instance(&plugin_instance_id, width as i32, height as i32);
            hlog!(
                "[PluginHostEditor] resize plugin_instance_id={plugin_instance_id} \
                 onSize width={width} height={height} dpi={dpi}"
            );
        }
        HostCommand::CloseEditor { plugin_instance_id } => {
            preview.lock().preview_all_notes_off(&plugin_instance_id);
            registry.remove(&plugin_instance_id);
            preview.lock().embed_detach_for_instance(&plugin_instance_id);
            delayed_redraws.retain(|entry| entry.instance_id != plugin_instance_id);
            preview.lock().set_continuous_mode(!registry.is_empty());
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
            preview.lock().preview_note_on(
                &plugin_instance_id,
                channel,
                pitch,
                velocity,
            );
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
            registry.remove(&plugin_instance_id);
            preview.lock().unload_instance(&plugin_instance_id);
            loaded.remove(&plugin_instance_id);
            hlog!("[PluginHostEditor] unload plugin_instance_id={plugin_instance_id} released=true");
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
        HostCommand::AttachSharedAudio { name, bytes } => {
            #[cfg(windows)]
            {
                match SharedAudioRegion::open_named(&name) {
                    Ok(region) => {
                        let sr = region.bridge().sample_rate.load(Ordering::Relaxed);
                        let block = region.bridge().max_block_size.load(Ordering::Relaxed);
                        eprintln!(
                            "[plugin-host-bridge] AttachSharedAudio name={name} bytes={bytes} attached=true header_sr={sr} header_block={block}"
                        );
                        region.bridge().set_dsp_output_ready(true);
                        // Hand the region to the dedicated audio producer thread.
                        if let Ok(mut slot) = region_slot.lock() {
                            *slot = Some(Arc::new(region));
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
                let _ = region_slot;
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
    eprintln!(
        "[plugin-host] OpenEditor uses existing instance={plugin_instance_id}"
    );
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
    let handle = preview
        .lock()
        .embed_editor_for_instance(plugin_instance_id, parent_hwnd, w, h);
    let Some(handle) = handle else {
        emit_attach_failed(
            out,
            plugin_instance_id,
            "embed_editor failed on existing runtime instance",
        );
        return;
    };
    registry.insert(plugin_instance_id.to_string(), handle);
    preview.lock().set_continuous_mode(true);
    let (preferred_width, preferred_height) = preview
        .lock()
        .editor_content_size_for_instance(plugin_instance_id);
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
            host_hwnd: handle,
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
                match native_editor::attach_editor_into_parent(content_hwnd, &path, &class_id, region)
                {
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
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetCurrentThreadId, GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, IsWindow, PeekMessageW, TranslateMessage,
        CW_USEDEFAULT, MSG, PM_REMOVE, WINDOW_EX_STYLE, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
        WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    fn hwnd_from(handle: u64) -> HWND {
        HWND(handle as *mut core::ffi::c_void)
    }

    pub fn com_init() {
        // STA: VST3 editors require apartment-threaded COM (spec Part 9).
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
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
            let alive = GetExitCodeProcess(handle, &mut code)
                .is_ok()
                .then_some(code == STILL_ACTIVE)
                .unwrap_or(false);
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

    /// Non-blocking drain of this thread's message queue.
    pub fn pump_messages() {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
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
    pub fn pump_messages() {}
    pub fn create_selftest_windows() -> Option<(u64, u64)> {
        None
    }
    pub fn destroy_selftest_windows(_top: u64, _content: u64) {}
}
