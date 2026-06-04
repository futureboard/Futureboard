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
    /// Start transport (playback) from current position.
    StartTransport,
    /// Stop transport (but keep position).
    StopTransport,
    /// Seek transport to an absolute position in seconds.
    Seek { position_seconds: f64 },
    /// Enable or disable generated metronome clicks.
    SetMetronomeEnabled(bool),
    /// Set project tempo for metronome scheduling.
    SetBpm(f64),
    /// Set project time signature for metronome accent scheduling.
    SetTimeSignature(u32, u32),
    /// Enable/disable loop region and set its bounds in seconds.
    SetLoop {
        enabled: bool,
        start_seconds: f64,
        end_seconds: f64,
    },
}
