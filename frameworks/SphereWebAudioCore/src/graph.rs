//! Audio graph — flat ordered processing graph.
//!
//! First pass: simple flat graph where all tracks route to master.
//! Tracks are processed in insertion order, mixed into the master buffer.

use crate::buffer::AudioBuffer;
use crate::devices::ProcessContext;
use crate::meters::StereoMeter;
use crate::mixer::{update_solo_state, MixerTrack};

/// The audio graph manages track processing order and master output.
pub struct AudioGraph {
    /// Master output volume (linear gain).
    pub master_volume: f32,
    /// Master meter.
    pub master_meter: StereoMeter,
    /// Master output buffer.
    pub master_buffer: AudioBuffer,
}

impl AudioGraph {
    pub fn new(max_block_size: usize, channel_count: usize) -> Self {
        Self {
            master_volume: 1.0,
            master_meter: StereoMeter::default(),
            master_buffer: AudioBuffer::new(channel_count, max_block_size),
        }
    }

    /// Process all tracks, mix into master, apply master volume, update meters.
    ///
    /// This is the main audio processing entry point per block.
    pub fn process(&mut self, tracks: &mut [MixerTrack], frames: usize, context: &ProcessContext) {
        // Set frame count for this block
        self.master_buffer.set_frames(frames);
        self.master_buffer.clear();

        // Update solo muting state
        update_solo_state(tracks);

        // Process each track and mix into master
        for track in tracks.iter_mut() {
            track.buffer.set_frames(frames);

            // Process track (insert chain + volume + pan + meters)
            track.process(context);

            // Mix into master buffer
            if track.is_audible() {
                self.master_buffer.mix_from(&track.buffer, 1.0);
            }
        }

        // Apply master volume
        self.master_buffer.apply_gain(self.master_volume);

        // Update master meter
        self.master_meter.update_from_buffer(&self.master_buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::TrackId;
    use crate::transport::Transport;

    #[test]
    fn empty_graph_produces_silence() {
        let mut graph = AudioGraph::new(128, 2);
        let mut tracks: Vec<MixerTrack> = vec![];
        let transport = Transport::new(44100.0, 120.0);
        let ctx = ProcessContext {
            sample_rate: 44100.0,
            bpm: 120.0,
            transport: &transport,
        };
        graph.process(&mut tracks, 128, &ctx);
        assert_eq!(graph.master_buffer.peak(), 0.0);
    }

    #[test]
    fn tracks_mix_into_master() {
        let mut graph = AudioGraph::new(4, 2);
        let mut tracks = vec![
            MixerTrack::new(TrackId::new("t1"), 4, 2),
            MixerTrack::new(TrackId::new("t2"), 4, 2),
        ];
        // Put audio into track buffers
        tracks[0].buffer.channel_mut(0).copy_from_slice(&[0.3; 4]);
        tracks[0].buffer.channel_mut(1).copy_from_slice(&[0.3; 4]);
        tracks[1].buffer.channel_mut(0).copy_from_slice(&[0.2; 4]);
        tracks[1].buffer.channel_mut(1).copy_from_slice(&[0.2; 4]);

        let transport = Transport::new(44100.0, 120.0);
        let ctx = ProcessContext {
            sample_rate: 44100.0,
            bpm: 120.0,
            transport: &transport,
        };
        graph.process(&mut tracks, 4, &ctx);
        // Master should have sum (with equal-power pan adjustments at center)
        assert!(graph.master_buffer.peak() > 0.0, "Master should have audio");
    }

    #[test]
    fn master_volume_affects_output() {
        let mut graph = AudioGraph::new(4, 2);
        graph.master_volume = 0.5;

        let mut tracks = vec![MixerTrack::new(TrackId::new("t1"), 4, 2)];
        tracks[0].buffer.channel_mut(0).copy_from_slice(&[1.0; 4]);
        tracks[0].buffer.channel_mut(1).copy_from_slice(&[1.0; 4]);

        let transport = Transport::new(44100.0, 120.0);
        let ctx = ProcessContext {
            sample_rate: 44100.0,
            bpm: 120.0,
            transport: &transport,
        };
        graph.process(&mut tracks, 4, &ctx);
        // Should be attenuated by master volume AND equal-power pan at center (~0.707)
        let peak = graph.master_buffer.peak();
        assert!(peak < 1.0, "Master volume should attenuate: {peak}");
        assert!(peak > 0.0, "Should still have audio: {peak}");
    }
}
