//! Cross-process IPC protocol between `futureboard_native.exe` (the GPUI main
//! app, IPC *client*) and `FutureboardPluginHost-x64.exe` (the plugin host
//! process, IPC *server*).
//!
//! Transport is **newline-delimited JSON** over the child process's
//! stdin/stdout — one JSON object per line. This mirrors the existing
//! `futureboard_plugin_scanner` precedent (`scan::isolation`), needs no extra
//! dependency, and is trivially loggable/diffable. Commands flow
//! client → host on the host's **stdin**; events flow host → client on the
//! host's **stdout**. The host keeps **stderr** free for human-readable debug
//! logs (gated behind `FUTUREBOARD_PLUGIN_VIEW_DEBUG`).
//!
//! Slice 1 scope: the host owns the VST3 *editor* lifecycle for an HWND created
//! and owned by the main app (`mode = main_owned_window`). The plugin instance
//! is loaded by `plugin_path` + `class_id` (the self-contained path-based
//! loader in `native_editor`); sharing one instance with the audio engine is a
//! later slice. See the plan / `native_editor` module docs.

use std::io::{self, BufRead, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Wire-format version. Bump on any breaking change to [`HostCommand`] /
/// [`HostEvent`]. The client sends it in [`HostCommand::Hello`] and the host
/// echoes its own in [`HostEvent::Ready`]; a mismatch should be surfaced, not
/// silently tolerated.
pub const PROTOCOL_VERSION: u32 = 3;

/// Commands sent **client → host** (written to the host's stdin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum HostCommand {
    /// Handshake; carries the client's protocol version.
    Hello {
        protocol_version: u32,
    },
    /// Liveness handshake — the host replies with [`HostEvent::Pong`]. Sent by
    /// the bridge client right after spawn to confirm the process is alive and
    /// speaking the protocol before any editor command.
    Ping,
    /// Load a plugin instance into the external host runtime. The main app
    /// sends this as soon as a VST3 insert is created; editor attachment is a
    /// later command against the same `plugin_instance_id`.
    LoadPlugin {
        plugin_instance_id: String,
        plugin_path: String,
        class_id: String,
        sample_rate: u32,
        max_block_size: u32,
    },
    /// Attach a VST3 editor view into an HWND owned by the main app.
    ///
    /// `parent_hwnd` is the main-app-created **content child HWND**
    /// (`content_hwnd != top_hwnd`). HWNDs are process-global on Windows and
    /// travel as a `u64`. The main app must keep the HWND alive until the host
    /// reports [`HostEvent::EditorClosed`].
    OpenEditorWithParentHwnd {
        plugin_instance_id: String,
        plugin_path: String,
        class_id: String,
        parent_hwnd: u64,
        width: u32,
        height: u32,
        dpi: u32,
    },
    /// Phase 1: `createView` + `getSize` only; host emits [`HostEvent::EditorPreferredSize`].
    PrepareEditorView {
        plugin_instance_id: String,
        plugin_path: String,
        class_id: String,
    },
    /// Phase 2: attach after main app resized content HWND to preferred size.
    ConfirmEditorContentReady {
        plugin_instance_id: String,
        parent_hwnd: u64,
        width: u32,
        height: u32,
        dpi: u32,
    },
    /// Main app resized the content HWND; host re-issues `IPlugView::onSize`.
    ResizeEditor {
        plugin_instance_id: String,
        width: u32,
        height: u32,
        dpi: u32,
    },
    /// Detach the editor view (`IPlugView::removed`) but keep the plugin loaded.
    CloseEditor {
        plugin_instance_id: String,
    },
    /// Detach (if attached) and release the plugin instance entirely.
    UnloadPlugin {
        plugin_instance_id: String,
    },
    /// Preview a single MIDI note on a loaded VSTi instance (transport may be stopped).
    PreviewNoteOn {
        plugin_instance_id: String,
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    PreviewNoteOff {
        plugin_instance_id: String,
        channel: u8,
        pitch: u8,
    },
    PreviewAllNotesOff {
        plugin_instance_id: String,
    },
    MidiPanic {
        plugin_instance_id: String,
    },
    /// Stage 1 (shared audio bridge): the **main engine owns** the sample rate
    /// and block size; the host must *follow* these for all plugin DSP. Sent
    /// before the first `LoadPlugin` and whenever the engine's audio config
    /// changes. Diagnostics-only at this stage — the host applies the config and
    /// replies [`HostEvent::AudioBridgeConfigured`]; no shared-memory audio
    /// transport exists yet (that is Stage 2).
    ConfigureAudioBridge {
        sample_rate: u32,
        max_block_size: u32,
    },
    /// Prepare plugin DSP at the engine-owned sample rate / block size.
    PrepareProcessing {
        plugin_instance_id: String,
        sample_rate: u32,
        max_block_size: u32,
        input_channels: u32,
        output_channels: u32,
    },
    /// Stage 1 skeleton: request the host to process one DSP block of `frames`
    /// samples. The lock-free shared-memory audio/MIDI transport is Stage 2/3;
    /// for now the host acknowledges with [`HostEvent::AudioBridgeStatus`]
    /// reporting `dsp_output=pending` (plugin output is NOT yet mixed into the
    /// main engine — never faked through a second device stream).
    ProcessBlockShared {
        block_id: u64,
        frames: u32,
    },
    /// Stage 2: the engine created a named shared-memory region
    /// ([`crate::audio_bridge::SharedAudioBridge`]) and asks the host to map it.
    /// `bytes` is the region size for validation. The host replies
    /// [`HostEvent::SharedAudioAttached`]. The lock-free buffers carry audio
    /// in/out, the MIDI ring, the parameter-automation ring, and the
    /// status/latency/meter block — no heap alloc or blocking on the audio thread.
    AttachSharedAudio {
        name: String,
        bytes: u64,
    },
    /// Graceful host shutdown: detach everything and exit 0.
    Shutdown,
}

/// Events sent **host → client** (written to the host's stdout).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum HostEvent {
    /// Emitted once at startup. Pairs with [`HostCommand::Hello`].
    Ready { protocol_version: u32, pid: u32 },
    /// Reply to [`HostCommand::Ping`] — confirms the bridge is live.
    Pong { pid: u32 },
    /// Host accepted a load request and is resolving the plugin.
    PluginLoading { plugin_instance_id: String },
    /// Plugin runtime is available in the host process.
    PluginLoaded {
        plugin_instance_id: String,
        name: String,
    },
    /// [`HostCommand::LoadPlugin`] for an instance that is already loaded —
    /// the host reuses the existing component/controller (no second create).
    PluginAlreadyLoaded {
        plugin_instance_id: String,
        name: String,
    },
    /// Plugin load failed; the main app should surface this and must not
    /// silently fall back to in-process hosting while the bridge is enabled.
    PluginLoadFailed {
        plugin_instance_id: String,
        error: String,
    },
    /// Editor view attached to the supplied HWND. `result` is the raw VST3
    /// `tresult` from `attached` (0 == `kResultOk`).
    EditorAttached {
        plugin_instance_id: String,
        result: i32,
        preferred_width: u32,
        preferred_height: u32,
        /// Plugin-host child HWND (`IPlugView::attached` target); 0 if unknown.
        #[serde(default)]
        host_hwnd: u64,
    },
    /// Attach failed (bad HWND, plugin load failure, no view, …).
    EditorAttachFailed {
        plugin_instance_id: String,
        error: String,
    },
    /// Plugin-requested preferred content size (host → client hint; the main
    /// app decides the final shell size).
    EditorPreferredSize {
        plugin_instance_id: String,
        width: u32,
        height: u32,
    },
    /// Plug-in called `IPlugFrame::resizeView` — main app should resize shell.
    EditorContentResize {
        plugin_instance_id: String,
        width: u32,
        height: u32,
        #[serde(default)]
        dpi: u32,
    },
    /// Editor view detached (`IPlugView::removed` called).
    EditorClosed { plugin_instance_id: String },
    /// Freeze watchdog: the host UI thread's message pump stalled for
    /// `gap_ms` while this editor was open. The main app may surface a
    /// "plugin editor not responding" hint; the editor close path stays
    /// available because the wrapper window lives in the main process.
    EditorUnresponsive {
        plugin_instance_id: String,
        gap_ms: u64,
    },
    /// Plugin instance released.
    PluginUnloaded { plugin_instance_id: String },
    /// Out-of-band log line (host-side diagnostics surfaced to the client).
    Log { level: String, message: String },
    /// Stage 1 reply to [`HostCommand::ConfigureAudioBridge`]: the host accepted
    /// the engine-owned sample rate / block size and is following them.
    AudioBridgeConfigured {
        sample_rate: u32,
        max_block_size: u32,
        /// True once the host's plugin DSP runs at the engine's rate/block.
        follows_engine: bool,
    },
    /// Stage 1 status for the shared audio bridge. `dsp_output` is `"pending"`
    /// until plugin DSP output is actually mixed into the main engine
    /// (Stage 3) — it is never `"ready"` while audio only plays through a
    /// separate device stream. `latency_samples` is the reported plugin latency
    /// (0 until Stage 4).
    AudioBridgeStatus {
        block_id: u64,
        dsp_output: String,
        latency_samples: u32,
    },
    /// Stage 2 reply to [`HostCommand::AttachSharedAudio`]: whether the host
    /// mapped the shared-memory region and validated its header.
    SharedAudioAttached {
        attached: bool,
        name: String,
        bytes: u64,
    },
    /// Reply to [`HostCommand::PrepareProcessing`]: plugin DSP is active at the
    /// engine-owned rate/block.
    ProcessingPrepared {
        plugin_instance_id: String,
        sample_rate: u32,
        max_block_size: u32,
        output_channels: u32,
    },
}

/// Serialize `msg` as a single JSON line (object + `\n`) and flush.
pub fn write_frame<W: Write>(writer: &mut W, msg: &impl Serialize) -> io::Result<()> {
    let line = serde_json::to_string(msg).map_err(io::Error::other)?;
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Read one newline-delimited JSON frame, skipping blank lines.
///
/// Returns `Ok(None)` on EOF (the peer closed the pipe — for the client this
/// means the host exited/crashed; for the host it means the main app is gone).
pub fn read_frame<T: DeserializeOwned, R: BufRead>(reader: &mut R) -> io::Result<Option<T>> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None); // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg = serde_json::from_str::<T>(trimmed).map_err(io::Error::other)?;
        return Ok(Some(msg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn command_round_trips_through_frame() {
        let cmd = HostCommand::LoadPlugin {
            plugin_instance_id: "track1:insert2".into(),
            plugin_path: "C:/VST3/Example.vst3".into(),
            class_id: "ABCDEF0123456789".into(),
            sample_rate: 48_000,
            max_block_size: 256,
        };
        let mut buf = Vec::new();
        write_frame(&mut buf, &cmd).unwrap();
        assert!(buf.ends_with(b"\n"));

        let mut reader = Cursor::new(buf);
        let decoded: HostCommand = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(decoded, cmd);
    }

    #[test]
    fn event_round_trips_and_skips_blank_lines() {
        let ev = HostEvent::EditorAttached {
            plugin_instance_id: "track1:insert2".into(),
            result: 0,
            preferred_width: 1236,
            preferred_height: 736,
            host_hwnd: 0,
        };
        let mut buf = Vec::new();
        // Leading blank lines must be tolerated.
        buf.extend_from_slice(b"\n\n");
        write_frame(&mut buf, &ev).unwrap();

        let mut reader = Cursor::new(buf);
        let decoded: HostEvent = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(decoded, ev);
    }

    #[test]
    fn read_frame_returns_none_on_eof() {
        let mut reader = Cursor::new(Vec::new());
        let decoded: Option<HostCommand> = read_frame(&mut reader).unwrap();
        assert!(decoded.is_none());
    }

    #[test]
    fn tagged_representation_is_stable() {
        let json = serde_json::to_string(&HostCommand::Shutdown).unwrap();
        assert_eq!(json, r#"{"cmd":"Shutdown"}"#);
        let json = serde_json::to_string(&HostEvent::Ready {
            protocol_version: PROTOCOL_VERSION,
            pid: 42,
        })
        .unwrap();
        assert_eq!(json, r#"{"event":"Ready","protocol_version":3,"pid":42}"#);
    }
}
