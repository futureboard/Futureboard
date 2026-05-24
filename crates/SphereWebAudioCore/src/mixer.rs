//! Track mixer — per-track gain, pan, mute, solo with device insert chain.

use crate::buffer::AudioBuffer;
use crate::devices::{AudioDevice, ProcessContext};
use crate::ids::{DeviceId, TrackId};
use crate::meters::StereoMeter;

/// A single mixer track with volume, pan, mute, solo, insert chain, and meter.
pub struct MixerTrack {
    pub id: TrackId,
    /// Linear gain (0.0 – ~2.0+).
    pub volume: f32,
    /// Pan: -1.0 (L) to 1.0 (R).
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    /// Whether this track is effectively muted (due to other tracks being soloed).
    pub solo_muted: bool,
    /// Insert device chain, processed in order.
    pub inserts: Vec<(DeviceId, Box<dyn AudioDevice>)>,
    /// Per-track meter.
    pub meter: StereoMeter,
    /// Track's own audio buffer (pre-allocated).
    pub buffer: AudioBuffer,
}

impl MixerTrack {
    pub fn new(id: TrackId, max_block_size: usize, channel_count: usize) -> Self {
        Self {
            id,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            solo_muted: false,
            inserts: Vec::new(),
            meter: StereoMeter::default(),
            buffer: AudioBuffer::new(channel_count, max_block_size),
        }
    }

    /// Is this track effectively producing audio?
    pub fn is_audible(&self) -> bool {
        !self.muted && !self.solo_muted && self.volume > 1e-7
    }

    /// Process this track's buffer through the insert chain, then apply
    /// volume/pan and update meters.
    pub fn process(&mut self, context: &ProcessContext) {
        if !self.is_audible() {
            self.buffer.clear();
            self.meter.update_from_buffer(&self.buffer);
            return;
        }

        // Run insert chain
        for (_id, device) in &mut self.inserts {
            if device.enabled() {
                device.process(&mut self.buffer, context);
            }
        }

        // Apply track volume
        self.buffer.apply_gain(self.volume);

        // Apply track pan
        self.buffer.apply_pan(self.pan);

        // Update meter
        self.meter.update_from_buffer(&self.buffer);
    }

    /// Find a device in the insert chain by ID.
    pub fn find_device_mut(&mut self, device_id: &DeviceId) -> Option<&mut Box<dyn AudioDevice>> {
        self.inserts
            .iter_mut()
            .find(|(id, _)| id == device_id)
            .map(|(_, dev)| dev)
    }

    /// Add a device to the insert chain.
    pub fn add_device(
        &mut self,
        device_id: DeviceId,
        device: Box<dyn AudioDevice>,
        index: Option<usize>,
    ) {
        match index {
            Some(i) if i < self.inserts.len() => {
                self.inserts.insert(i, (device_id, device));
            }
            _ => {
                self.inserts.push((device_id, device));
            }
        }
    }

    /// Remove a device from the insert chain.
    pub fn remove_device(&mut self, device_id: &DeviceId) -> bool {
        let len_before = self.inserts.len();
        self.inserts.retain(|(id, _)| id != device_id);
        self.inserts.len() < len_before
    }
}

/// Recalculate solo_muted flags across all tracks.
/// If any track is soloed, non-soloed tracks become solo_muted.
pub fn update_solo_state(tracks: &mut [MixerTrack]) {
    let any_solo = tracks.iter().any(|t| t.solo);
    for track in tracks.iter_mut() {
        track.solo_muted = any_solo && !track.solo;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_muted_produces_silence() {
        let mut track = MixerTrack::new(TrackId::new("t1"), 4, 2);
        track.muted = true;
        track.buffer.channel_mut(0).copy_from_slice(&[1.0; 4]);
        track.buffer.channel_mut(1).copy_from_slice(&[1.0; 4]);

        let transport = crate::transport::Transport::new(44100.0, 120.0);
        let ctx = ProcessContext {
            sample_rate: 44100.0,
            bpm: 120.0,
            transport: &transport,
        };
        track.process(&ctx);
        assert_eq!(track.buffer.peak(), 0.0);
    }

    #[test]
    fn solo_logic() {
        let mut tracks = vec![
            MixerTrack::new(TrackId::new("t1"), 4, 2),
            MixerTrack::new(TrackId::new("t2"), 4, 2),
            MixerTrack::new(TrackId::new("t3"), 4, 2),
        ];
        tracks[1].solo = true;
        update_solo_state(&mut tracks);
        assert!(tracks[0].solo_muted);
        assert!(!tracks[1].solo_muted);
        assert!(tracks[2].solo_muted);
    }

    #[test]
    fn no_solo_means_no_mute() {
        let mut tracks = vec![
            MixerTrack::new(TrackId::new("t1"), 4, 2),
            MixerTrack::new(TrackId::new("t2"), 4, 2),
        ];
        update_solo_state(&mut tracks);
        assert!(!tracks[0].solo_muted);
        assert!(!tracks[1].solo_muted);
    }
}
