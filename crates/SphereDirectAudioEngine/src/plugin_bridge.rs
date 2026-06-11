//! Realtime-safe sink the audio callback uses to exchange one block with the
//! external plugin host (Stage 3b of the shared-memory audio bridge).
//!
//! DAUx defines the contract; `sphere-plugin-host` implements it over the
//! shared-memory region (it cannot live there — the engine must not depend on
//! the host crate). **Every method runs on the audio thread**, so each must be
//! wait-free: no allocation, no locks, no syscalls, no blocking.

use std::sync::Arc;

/// One-block exchange with the external plugin host, called from the audio
/// callback. The engine reads the host's previously produced block (one-block
/// latency) and requests the next one — it never spins waiting for the host.
pub trait PluginBridgeSink: Send + Sync + std::fmt::Debug {
    /// True when the host signals it is producing DSP output.
    fn dsp_ready(&self) -> bool;

    /// Read the host's most-recently produced block (deinterleaved) into
    /// `out_l` / `out_r` (each at least `frames` long). Returns the number of
    /// frames actually read. Each produced block is handed out **at most
    /// once**: 0 means the host has not produced a new block since the last
    /// read (stalled on an editor open/close, plugin load, or not started) —
    /// the caller must bypass or output silence for this block and must never
    /// reuse previous output. Wait-free.
    fn read_output(&self, out_l: &mut [f32], out_r: &mut [f32], frames: usize) -> usize;

    /// Push one MIDI event for the host to apply to the next block (Stage 4 clip
    /// playback / automation). Wait-free ring push; dropped if the ring is full.
    fn push_midi(&self, status: u8, data1: u8, data2: u8, sample_offset: u32);

    /// Push one parameter change (normalized `0.0..=1.0` VST3 ParamID `value`)
    /// for the host to apply to the next block. Wait-free ring push; dropped if
    /// the ring is full. Default no-op for sinks that do not carry a param ring
    /// (test stubs); the shared-region sink forwards it to the host plugin.
    fn push_param(&self, _param_id: u32, _value: f32, _sample_offset: u32) {}

    /// Write the track's pre-plugin stereo input for effect inserts (engine →
    /// host `audio_in`). Wait-free raw buffer copy.
    fn write_input(&self, in_l: &[f32], in_r: &[f32], frames: usize);

    /// Publish the request for the host to process `frames` next (sets the block
    /// size and bumps the request sequence). Wait-free.
    fn request_block(&self, frames: u32);

    /// Publish the transport ProcessContext (tempo, time signature, project
    /// position, playing/recording) for the next block, so the host fills the
    /// bridged plugin's VST3 `ProcessContext` with real transport instead of a
    /// hardcoded stub. Called once per block right before [`Self::request_block`].
    /// Wait-free (plain atomic stores). Default no-op for sinks that do not
    /// carry transport (e.g. test stubs).
    fn set_transport(&self, _ctx: &crate::vst3_processor::RuntimeTransportContext) {}
}

/// Shared handle to a realtime plugin-bridge sink.
pub type SharedPluginBridgeSink = Arc<dyn PluginBridgeSink>;
