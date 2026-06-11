//! Engine-facing realtime sink (Stage 3b): implements DAUx's
//! [`PluginBridgeSink`] over the shared-memory [`SharedAudioRegion`].
//!
//! The main app's audio callback (in `DAUx`) holds an `Arc<dyn PluginBridgeSink>`
//! and calls these methods per block to read the host's produced output and
//! request the next one. All methods are wait-free — they only touch the
//! lock-free shared region (atomics + raw buffer copies), never allocate or lock.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use DAUx::plugin_bridge::{PluginBridgeSink, SharedPluginBridgeSink};

use crate::audio_bridge::{BridgeKickEvent, SharedAudioRegion, SharedMidiEvent, MAX_BLOCK_FRAMES};

/// Wraps the engine-side shared audio region as a realtime [`PluginBridgeSink`].
pub struct SharedRegionSink {
    region: Arc<SharedAudioRegion>,
    /// Signaled after every `request_seq` bump so the host's audio producer
    /// wakes immediately instead of polling on a timer tick. `None` falls back
    /// to the host's timeout-paced poll (tests / event creation failure).
    kick: Option<Arc<BridgeKickEvent>>,
    /// `done_seq` of the last block this sink actually returned from
    /// [`PluginBridgeSink::read_output`]. Freshness guard: when the host has
    /// not produced a new block since the last read (its service thread is
    /// stalled behind an editor open/close or a plugin load), `read_output`
    /// returns 0 so the engine bypasses/silences the block instead of
    /// replaying the stale `audio_out` contents every callback.
    last_read_seq: AtomicU64,
}

impl std::fmt::Debug for SharedRegionSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedRegionSink")
            .field("bytes", &self.region.bytes())
            .finish()
    }
}

impl SharedRegionSink {
    pub fn new(region: Arc<SharedAudioRegion>) -> Self {
        Self::with_kick(region, None)
    }

    pub fn with_kick(region: Arc<SharedAudioRegion>, kick: Option<Arc<BridgeKickEvent>>) -> Self {
        Self {
            region,
            kick,
            last_read_seq: AtomicU64::new(0),
        }
    }

    /// Wrap as the trait object the engine holds.
    pub fn into_shared(
        region: Arc<SharedAudioRegion>,
        kick: Option<Arc<BridgeKickEvent>>,
    ) -> SharedPluginBridgeSink {
        Arc::new(Self::with_kick(region, kick))
    }
}

impl PluginBridgeSink for SharedRegionSink {
    fn dsp_ready(&self) -> bool {
        self.region.bridge().dsp_output_ready()
    }

    fn read_output(&self, out_l: &mut [f32], out_r: &mut [f32], frames: usize) -> usize {
        let bridge = self.region.bridge();
        // Freshness guard: only hand a produced block to the engine once. When
        // the host misses its deadline (editor open/close or plugin load holds
        // its engine lock), `done_seq` stops advancing and we return 0 — the
        // engine bypasses/silences the block. Never replay stale output.
        let done = bridge.done_seq.load(Ordering::Acquire);
        if done == self.last_read_seq.load(Ordering::Relaxed) {
            return 0;
        }
        self.last_read_seq.store(done, Ordering::Relaxed);
        // SAFETY: the engine consumes `audio_out` while it holds the block (the
        // host waits on `done_seq` before producing the next one).
        unsafe { bridge.audio_out.read_deinterleaved(out_l, out_r, frames) }
    }

    fn push_midi(&self, status: u8, data1: u8, data2: u8, sample_offset: u32) {
        let ok = self.region.bridge().midi.try_push(SharedMidiEvent {
            sample_offset,
            status,
            data1,
            data2,
            _pad: 0,
        });
        if !ok {
            eprintln!(
                "[plugin-dsp-midi] ring_full dropped status=0x{status:02X} pitch={data1} velocity={data2}"
            );
        }
    }

    fn write_input(&self, in_l: &[f32], in_r: &[f32], frames: usize) {
        // SAFETY: the engine owns `audio_in` for this block (before `request_seq`).
        unsafe {
            self.region
                .bridge()
                .audio_in
                .write_deinterleaved(in_l, in_r, frames);
        }
    }

    fn request_block(&self, frames: u32) {
        let bridge = self.region.bridge();
        bridge
            .block_frames
            .store(frames.min(MAX_BLOCK_FRAMES as u32), Ordering::Relaxed);
        bridge.request_seq.fetch_add(1, Ordering::Release);
        // Wake the host producer for this request. `SetEvent` never blocks —
        // it is the same class of kernel signal the OS itself uses to drive
        // the WASAPI period callback — so the audio thread stays wait-free.
        // Ordering: signaled strictly after the `request_seq` Release bump so
        // a woken producer always observes the new request.
        if let Some(kick) = &self.kick {
            kick.set();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_bridge::SharedAudioRegion;

    /// The engine-side sink reads exactly what the host produced into `audio_out`
    /// (deinterleaved), and `request_block` drives the handshake — validates the
    /// realtime path without a real plugin or second process.
    #[test]
    fn sink_reads_host_output_and_requests_block() {
        let region = Arc::new(SharedAudioRegion::new_in_process());
        region.bridge().init_header(48_000, 256, 2);
        let sink = SharedRegionSink::new(region.clone());

        // Host produces a known interleaved block: L = i, R = -i.
        let frames = 4usize;
        let mut interleaved = vec![0.0f32; frames * 2];
        for i in 0..frames {
            interleaved[i * 2] = i as f32;
            interleaved[i * 2 + 1] = -(i as f32);
        }
        // SAFETY: single-threaded test; no concurrent reader.
        unsafe { region.bridge().audio_out.write_interleaved(&interleaved) };
        // The host publishes the produced block by bumping `done_seq`.
        region.bridge().done_seq.store(1, Ordering::Release);

        let mut out_l = [0.0f32; 8];
        let mut out_r = [0.0f32; 8];
        let got = sink.read_output(&mut out_l[..frames], &mut out_r[..frames], frames);
        assert_eq!(got, frames);
        for i in 0..frames {
            assert_eq!(out_l[i], i as f32);
            assert_eq!(out_r[i], -(i as f32));
        }

        // Freshness guard: the same produced block is never handed out twice —
        // a stalled host yields 0 (engine bypasses) instead of stale audio.
        let again = sink.read_output(&mut out_l[..frames], &mut out_r[..frames], frames);
        assert_eq!(again, 0);
        // Once the host produces the next block, reads resume.
        region.bridge().done_seq.store(2, Ordering::Release);
        let fresh = sink.read_output(&mut out_l[..frames], &mut out_r[..frames], frames);
        assert_eq!(fresh, frames);

        // request_block sets block_frames and advances request_seq (the host's
        // service loop fires when request_seq != done_seq).
        let before = region.bridge().request_seq.load(Ordering::Acquire);
        sink.request_block(frames as u32);
        assert_eq!(
            region.bridge().block_frames.load(Ordering::Relaxed),
            frames as u32
        );
        assert_eq!(
            region.bridge().request_seq.load(Ordering::Acquire),
            before + 1
        );
    }

    #[test]
    fn sink_push_midi_lands_in_ring() {
        let region = Arc::new(SharedAudioRegion::new_in_process());
        region.bridge().init_header(48_000, 256, 2);
        let sink = SharedRegionSink::new(region.clone());

        sink.push_midi(0x90, 60, 100, 7);
        let ev = region.bridge().midi.try_pop().expect("event queued");
        assert_eq!(ev.status, 0x90);
        assert_eq!(ev.data1, 60);
        assert_eq!(ev.data2, 100);
        assert_eq!(ev.sample_offset, 7);
    }
}
