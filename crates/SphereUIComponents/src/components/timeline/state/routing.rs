use super::*;

pub use crate::project::InputMonitorMode;

/// A single aux send from this track to a Bus/Return track (Phase 3). The
/// runtime sums `gain_db`-scaled signal into the target's input. UI stores the
/// descriptor; DAUx owns the realtime accumulation.
#[derive(Debug, Clone, PartialEq)]
pub struct SendSlotState {
    pub id: String,
    /// Id of the destination Bus/Return track.
    pub target_track_id: String,
    /// Display label for the destination (resolved at edit time; refreshed
    /// from the track list on render).
    pub target_name: String,
    pub enabled: bool,
    /// `true` = tap before the source track fader; `false` = post-fader.
    /// Realtime currently honours post-fader only (pre-fader is a refinement).
    pub pre_fader: bool,
    pub gain_db: f32,
}

impl SendSlotState {
    /// Linear send gain from `gain_db` (clamped to a sane range).
    pub fn gain_linear(&self) -> f32 {
        10f32.powf(self.gain_db.clamp(-60.0, 6.0) / 20.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackInputRouting {
    None,
    AllInputs,
    AudioDeviceChannel {
        device_id: String,
        channel: u32,
    },
    AudioDeviceChannels {
        device_id: String,
        channels: Vec<u32>,
    },
    MidiDevice {
        device_id: String,
    },
}

impl TrackInputRouting {
    pub fn label(&self) -> String {
        match self {
            Self::None => "None".to_string(),
            Self::AllInputs => "All Inputs".to_string(),
            Self::AudioDeviceChannel { device_id, channel } => {
                format!("{device_id} ch {}", channel + 1)
            }
            Self::AudioDeviceChannels {
                device_id,
                channels,
            } => {
                let labels = channels
                    .iter()
                    .map(|channel| (channel + 1).to_string())
                    .collect::<Vec<_>>()
                    .join("+");
                format!("{device_id} ch {labels}")
            }
            Self::MidiDevice { device_id } => device_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackOutputRouting {
    Main,
    Bus { bus_id: String },
    HardwareOutput { device_id: String, channel: u32 },
    None,
}

impl TrackOutputRouting {
    pub fn label(&self) -> String {
        match self {
            Self::Main => "Main".to_string(),
            Self::Bus { bus_id } => bus_id.clone(),
            Self::HardwareOutput { device_id, channel } => {
                format!("{device_id} ch {}", channel + 1)
            }
            Self::None => "None".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackAudioFormat {
    Mono,
    Stereo,
}

impl TrackAudioFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::Stereo => "Stereo",
        }
    }
}

fn input_route_matches_audio_format(
    input: &TrackInputRouting,
    audio_format: TrackAudioFormat,
) -> bool {
    match input {
        TrackInputRouting::None | TrackInputRouting::AllInputs => true,
        TrackInputRouting::AudioDeviceChannel { .. } => audio_format == TrackAudioFormat::Mono,
        TrackInputRouting::AudioDeviceChannels { channels, .. } => match audio_format {
            TrackAudioFormat::Mono => channels.len() == 1,
            TrackAudioFormat::Stereo => channels.len() == 2,
        },
        TrackInputRouting::MidiDevice { .. } => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackMidiInputRouting {
    None,
    AllInputs,
    MidiDevice { device_id: String },
}

impl TrackMidiInputRouting {
    pub fn label(&self) -> String {
        match self {
            Self::None => "None".to_string(),
            Self::AllInputs => "All MIDI Inputs".to_string(),
            Self::MidiDevice { device_id } => device_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRoutingState {
    pub input: TrackInputRouting,
    pub output: TrackOutputRouting,
    pub audio_format: TrackAudioFormat,
    pub midi_input: TrackMidiInputRouting,
    /// `None` means All channels. `Some` is clamped to 1..=16 by mutation
    /// helpers and project-load conversion.
    pub midi_channel: Option<u8>,
}

impl TrackRoutingState {
    pub fn for_track_type(track_type: TrackType) -> Self {
        match track_type {
            TrackType::Audio => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::Main,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::None,
                midi_channel: None,
            },
            TrackType::Instrument => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::Main,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::AllInputs,
                midi_channel: None,
            },
            TrackType::Midi => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::None,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::AllInputs,
                midi_channel: None,
            },
            TrackType::Bus | TrackType::Return => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::Main,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::None,
                midi_channel: None,
            },
            TrackType::Master => Self {
                input: TrackInputRouting::None,
                output: TrackOutputRouting::Main,
                audio_format: TrackAudioFormat::Stereo,
                midi_input: TrackMidiInputRouting::None,
                midi_channel: None,
            },
        }
    }
}

impl TimelineState {
    pub fn set_track_input_routing(&mut self, track_id: &str, input: TrackInputRouting) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if !input_route_matches_audio_format(&input, t.routing.audio_format) {
                return false;
            }
            if t.routing.input != input {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] input track={} old={:?} new={:?}",
                        track_id, t.routing.input, input
                    );
                }
                t.routing.input = input;
                return true;
            }
        }
        false
    }

    pub fn set_track_output_routing(&mut self, track_id: &str, output: TrackOutputRouting) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.routing.output != output {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] output track={} old={:?} new={:?}",
                        track_id, t.routing.output, output
                    );
                }
                t.routing.output = output;
                return true;
            }
        }
        false
    }

    pub fn set_track_audio_format(
        &mut self,
        track_id: &str,
        audio_format: TrackAudioFormat,
    ) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.routing.audio_format != audio_format {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] audio_format track={} old={:?} new={:?}",
                        track_id, t.routing.audio_format, audio_format
                    );
                }
                t.routing.audio_format = audio_format;
                if !input_route_matches_audio_format(&t.routing.input, audio_format) {
                    t.routing.input = TrackInputRouting::None;
                }
                return true;
            }
        }
        false
    }

    pub fn set_track_midi_input(
        &mut self,
        track_id: &str,
        midi_input: TrackMidiInputRouting,
    ) -> bool {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.routing.midi_input != midi_input {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] midi_input track={} old={:?} new={:?}",
                        track_id, t.routing.midi_input, midi_input
                    );
                }
                t.routing.midi_input = midi_input;
                return true;
            }
        }
        false
    }

    pub fn set_track_midi_channel(&mut self, track_id: &str, channel: Option<u8>) -> bool {
        let channel = channel.map(|ch| ch.clamp(1, 16));
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if t.routing.midi_channel != channel {
                if routing_debug_enabled() {
                    eprintln!(
                        "[routing] midi_channel track={} old={:?} new={:?}",
                        track_id, t.routing.midi_channel, channel
                    );
                }
                t.routing.midi_channel = channel;
                return true;
            }
        }
        false
    }

    /// Add an aux send from `track_id` to the first Bus/Return track that
    /// isn't already a target (Phase 3 — a richer target picker is a follow-up,
    /// mirroring how inserts auto-seeded before the picker overlay). Returns
    /// the new send id, or `None` if there is no eligible routing track or the
    /// track already sends to every routing track.
    pub fn add_send(&mut self, track_id: &str) -> Option<String> {
        let existing: Vec<String> = self
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .map(|t| t.sends.iter().map(|s| s.target_track_id.clone()).collect())
            .unwrap_or_default();
        let target = self
            .tracks
            .iter()
            .find(|t| t.id != track_id && t.track_type.is_routing() && !existing.contains(&t.id))?;
        let target_id = target.id.clone();
        self.add_send_to_target(track_id, &target_id)
    }

    pub fn add_send_to_target(&mut self, track_id: &str, target_track_id: &str) -> Option<String> {
        if track_id == target_track_id {
            return None;
        }
        let (target_id, target_name) = self
            .tracks
            .iter()
            .find(|t| t.id == target_track_id && t.track_type.is_routing())
            .map(|target| (target.id.clone(), target.name.clone()))?;

        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        if track.track_type.is_routing()
            || track.sends.iter().any(|s| s.target_track_id == target_id)
        {
            return None;
        }
        let send_id = format!("send-{}-{}", track.id, track.sends.len() + 1);
        track.sends.push(SendSlotState {
            id: send_id.clone(),
            target_track_id: target_id.clone(),
            target_name,
            enabled: true,
            pre_fader: false,
            gain_db: 0.0,
        });
        if routing_debug_enabled() {
            eprintln!(
                "[routing] add_send track={} send={} -> {}",
                track_id, send_id, target_id
            );
        }
        Some(send_id)
    }

    pub fn create_return_and_send(&mut self, track_id: &str) -> Option<(String, String)> {
        if self
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .is_none_or(|track| track.track_type.is_routing())
        {
            return None;
        }
        let next_return = self
            .tracks
            .iter()
            .filter(|track| track.track_type == TrackType::Return)
            .count()
            + 1;
        let return_id = self.create_track(CreateTrackOptions {
            track_type: TrackType::Return,
            name: format!("Return {next_return}"),
            color: self.track_color_for_index(self.tracks.len()),
            volume: volume::db_to_norm(0.0),
            pan: 0.0,
            armed: false,
            input_monitor: InputMonitorMode::Off,
        });
        let send_id = self.add_send_to_target(track_id, &return_id)?;
        Some((return_id, send_id))
    }

    pub fn remove_send(&mut self, track_id: &str, send_id: &str) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.sends.retain(|s| s.id != send_id);
            if routing_debug_enabled() {
                eprintln!("[routing] remove_send track={} send={}", track_id, send_id);
            }
        }
    }

    pub fn toggle_send_enabled(&mut self, track_id: &str, send_id: &str) -> Option<bool> {
        let track = self.tracks.iter_mut().find(|t| t.id == track_id)?;
        let send = track.sends.iter_mut().find(|s| s.id == send_id)?;
        send.enabled = !send.enabled;
        Some(send.enabled)
    }
}
