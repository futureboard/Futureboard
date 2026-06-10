use crate::runtime::RuntimeProject;

/// Commands sent from the control thread to the audio callback via a
/// lock-free bounded channel.  The audio callback drains these with
/// `try_recv()` at the top of each block — no blocking, no allocation.
#[derive(Debug)]
pub enum EngineCommand {
    /// Replace the callback's render graph with a fully prepared project.
    LoadProject(Box<RuntimeProject>),
    /// Enable or disable the sine test tone.
    SetTestTone { enabled: bool, frequency: f32 },
    /// Set master output gain (linear, 0..2).
    SetMasterVolume { value: f32 },
    /// Set a track's gain (linear, 0..2).
    SetTrackVolume { track_id: String, value: f32 },
    /// Set a track's pan (-1..1).
    SetTrackPan { track_id: String, value: f32 },
    /// Mute or unmute a track.
    SetTrackMute { track_id: String, muted: bool },
    /// Solo or unsolo a track.
    SetTrackSolo { track_id: String, solo: bool },
    /// Set non-destructive stereo/mono/mid/side monitoring preview.
    SetTrackPreviewMode { track_id: String, value: f32 },
    /// Set a plugin/insert parameter.
    SetInsertParam {
        track_id: String,
        insert_id: String,
        param_id: String,
        value: f32,
    },
    /// Immediate MIDI preview note-on from the UI piano roll. Bypasses timeline
    /// scheduling and can render while transport is stopped.
    MidiPreviewNoteOn {
        track_id: String,
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    /// Immediate MIDI preview note-off from the UI piano roll.
    MidiPreviewNoteOff {
        track_id: String,
        channel: u8,
        pitch: u8,
    },
    /// Panic/cleanup for preview notes on one track.
    MidiPreviewAllNotesOff { track_id: String },
    /// Sample-synchronous bridged-plugin preview note-on (audio callback writes
    /// into the shared MIDI ring before the next process block).
    PluginPreviewNoteOn {
        track_id: String,
        plugin_instance_id: String,
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    PluginPreviewNoteOff {
        track_id: String,
        plugin_instance_id: String,
        channel: u8,
        pitch: u8,
    },
    PluginPreviewAllNotesOff {
        track_id: String,
        plugin_instance_id: String,
    },
    /// Start transport (playback) from current position.
    StartTransport,
    /// Stop transport (but keep position).
    StopTransport,
    /// Seek transport to an absolute position in seconds.
    Seek { position_seconds: f64 },
    /// Enable or disable generated metronome clicks.
    SetMetronomeEnabled(bool),
    /// Set project tempo for metronome scheduling (static tempo shortcut).
    SetBpm(f64),
    /// Replace the authoritative tempo map used for beat/time/sample conversion.
    SetTempoMap(crate::tempo_map::RuntimeTempoMapSnapshot),
    /// Set project time signature for metronome accent scheduling.
    SetTimeSignature(u32, u32),
    /// Replace the authoritative time-signature map for metronome accents.
    SetTimeSignatureMap(crate::time_signature_map::RuntimeTimeSignatureMapSnapshot),
    /// Enable/disable loop region and set its bounds in seconds.
    SetLoop {
        enabled: bool,
        start_seconds: f64,
        end_seconds: f64,
    },
    /// Stage 3b: install (or clear, with `sink = None`) the realtime
    /// plugin-bridge sink for `insert_id` (one shared-memory region + handshake
    /// per insert so serial FX chains do not share `request_seq`/`done_seq`).
    SetPluginBridgeSink {
        insert_id: String,
        sink: Option<std::sync::Arc<dyn crate::plugin_bridge::PluginBridgeSink>>,
    },
    /// Keep rendering a bridged track while its plugin editor is open (VSTi
    /// internal keyboard / groove preview needs a live DSP loop).
    SetBridgeEditorActive { track_id: String, active: bool },
}
