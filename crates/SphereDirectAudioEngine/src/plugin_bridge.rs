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
    /// frames actually read (0 when nothing is available). Wait-free.
    fn read_output(&self, out_l: &mut [f32], out_r: &mut [f32], frames: usize) -> usize;

    /// Push one MIDI event for the host to apply to the next block (Stage 4 clip
    /// playback / automation). Wait-free ring push; dropped if the ring is full.
    fn push_midi(&self, status: u8, data1: u8, data2: u8, sample_offset: u32);

    /// Write the track's pre-plugin stereo input for effect inserts (engine →
    /// host `audio_in`). Wait-free raw buffer copy.
    fn write_input(&self, in_l: &[f32], in_r: &[f32], frames: usize);

    /// Publish the request for the host to process `frames` next (sets the block
    /// size and bumps the request sequence). Wait-free.
    fn request_block(&self, frames: u32);
}

/// Shared handle to a realtime plugin-bridge sink.
pub type SharedPluginBridgeSink = Arc<dyn PluginBridgeSink>;
