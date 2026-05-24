//! DspEngine — the top-level audio engine.
//!
//! This is the central struct that owns the transport, graph, tracks,
//! and event queue. It exposes the public API per spec section 4.

use serde::{Deserialize, Serialize};

use crate::commands::{CommandResult, EngineCommand};
use crate::devices::{self, ProcessContext};
use crate::events::{EngineEvent, EventQueue};
use crate::graph::AudioGraph;
use crate::ids::TrackId;
use crate::mixer::MixerTrack;
use crate::transport::{PlayState, Transport};

/// Engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub sample_rate: f64,
    pub max_block_size: usize,
    pub channel_count: usize,
    pub bpm: f64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sample_rate: 44100.0,
            max_block_size: 512,
            channel_count: 2,
            bpm: 120.0,
        }
    }
}

/// Engine status snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStatus {
    pub initialized: bool,
    pub playing: bool,
    pub paused: bool,
    pub sample_rate: f64,
    pub bpm: f64,
    pub beat_position: f64,
    pub sample_position: u64,
    pub time_seconds: f64,
    pub track_count: usize,
    pub master_volume: f32,
    pub loop_enabled: bool,
    pub loop_start_beat: f64,
    pub loop_end_beat: f64,
}

/// The core DSP engine.
pub struct DspEngine {
    config: EngineConfig,
    transport: Transport,
    graph: AudioGraph,
    tracks: Vec<MixerTrack>,
    events: EventQueue,
    initialized: bool,
    /// Meter event throttle counter (emit every N process calls).
    meter_throttle: u32,
    meter_throttle_interval: u32,
}

impl DspEngine {
    /// Create a new engine with the given configuration.
    pub fn new(config: EngineConfig) -> Self {
        let transport = Transport::new(config.sample_rate, config.bpm);
        let graph = AudioGraph::new(config.max_block_size, config.channel_count);

        let mut engine = Self {
            config: config.clone(),
            transport,
            graph,
            tracks: Vec::new(),
            events: EventQueue::new(256),
            initialized: true,
            meter_throttle: 0,
            // Emit meters every ~6 blocks (~15 FPS at 44.1kHz/512)
            meter_throttle_interval: 6,
        };

        engine.events.push(EngineEvent::Ready);
        engine
    }

    /// Reset the engine to initial state.
    pub fn reset(&mut self) {
        self.transport = Transport::new(self.config.sample_rate, self.config.bpm);
        self.tracks.clear();
        self.graph = AudioGraph::new(self.config.max_block_size, self.config.channel_count);
        self.events.push(EngineEvent::Ready);
    }

    /// Process audio into the output buffer.
    ///
    /// `output` should be a flat interleaved f32 buffer of size
    /// `frames * channel_count`. This method fills it from the engine's
    /// non-interleaved master buffer.
    ///
    /// This is the realtime-safe entry point.
    pub fn process(&mut self, output: &mut [f32], frames: usize) {
        let context = ProcessContext {
            sample_rate: self.config.sample_rate as f32,
            bpm: self.transport.bpm,
            transport: &self.transport,
        };

        // Process the graph
        self.graph.process(&mut self.tracks, frames, &context);

        // Copy master buffer to interleaved output
        let ch_count = self.config.channel_count;
        let master = &self.graph.master_buffer;
        for frame in 0..frames {
            for ch in 0..ch_count {
                let idx = frame * ch_count + ch;
                if idx < output.len() && ch < master.channel_count() {
                    output[idx] = master.channel(ch)[frame];
                }
            }
        }

        // Advance transport
        let _looped = self.transport.advance(frames);

        // Emit transport position (every block when playing)
        if self.transport.state == PlayState::Playing {
            self.events.push(EngineEvent::TransportPosition {
                beat: self.transport.beat_position(),
                sample: self.transport.sample_position,
                time_seconds: self.transport.time_seconds(),
            });
        }

        // Emit meter updates (throttled)
        self.meter_throttle += 1;
        if self.meter_throttle >= self.meter_throttle_interval {
            self.meter_throttle = 0;
            let meters: Vec<_> = self
                .tracks
                .iter()
                .map(|t| t.meter.snapshot(&t.id))
                .collect();
            if !meters.is_empty() {
                self.events.push(EngineEvent::MeterUpdate { meters });
            }
        }
    }

    /// Handle an engine command. Returns a result.
    pub fn handle_command(&mut self, command: EngineCommand) -> CommandResult {
        match command {
            EngineCommand::Init {
                sample_rate,
                max_block_size,
                channel_count,
                bpm,
            } => {
                self.config = EngineConfig {
                    sample_rate,
                    max_block_size,
                    channel_count,
                    bpm,
                };
                self.reset();
                CommandResult::ok()
            }

            // ── Transport ────────────────────────────────────
            EngineCommand::Play { position_beat } => {
                if let Some(beat) = position_beat {
                    self.transport.seek_beat(beat);
                }
                self.transport.play();
                self.events.push(EngineEvent::PlaybackStarted);
                CommandResult::ok()
            }
            EngineCommand::Pause => {
                self.transport.pause();
                self.events.push(EngineEvent::PlaybackPaused);
                CommandResult::ok()
            }
            EngineCommand::Stop => {
                self.transport.stop();
                self.events.push(EngineEvent::PlaybackStopped);
                CommandResult::ok()
            }
            EngineCommand::SeekBeat { beat } => {
                self.transport.seek_beat(beat);
                CommandResult::ok()
            }
            EngineCommand::SetBpm { bpm } => {
                self.transport.set_bpm(bpm);
                CommandResult::ok()
            }
            EngineCommand::SetLoop {
                enabled,
                start_beat,
                end_beat,
            } => {
                self.transport.set_loop(enabled, start_beat, end_beat);
                CommandResult::ok()
            }
            EngineCommand::SetTimeSignature {
                numerator,
                denominator,
            } => {
                self.transport.time_sig_num = numerator;
                self.transport.time_sig_den = denominator;
                CommandResult::ok()
            }

            // ── Tracks ───────────────────────────────────────
            EngineCommand::CreateTrack {
                track_id,
                volume,
                pan,
                muted,
                solo,
            } => {
                if self.find_track(&track_id).is_some() {
                    return CommandResult::error(
                        "DUPLICATE_TRACK",
                        format!("Track {} already exists", track_id),
                    );
                }
                let mut track = MixerTrack::new(
                    track_id.clone(),
                    self.config.max_block_size,
                    self.config.channel_count,
                );
                track.volume = volume;
                track.pan = pan;
                track.muted = muted;
                track.solo = solo;
                self.tracks.push(track);
                self.events.push(EngineEvent::TrackCreated { track_id });
                CommandResult::ok()
            }
            EngineCommand::RemoveTrack { track_id } => {
                let len_before = self.tracks.len();
                self.tracks.retain(|t| t.id != track_id);
                if self.tracks.len() < len_before {
                    self.events.push(EngineEvent::TrackRemoved { track_id });
                    CommandResult::ok()
                } else {
                    CommandResult::error(
                        "INVALID_TRACK_ID",
                        format!("Track {} not found", track_id),
                    )
                }
            }
            EngineCommand::SetTrackVolume { track_id, volume } => {
                self.with_track_mut(&track_id, |t| {
                    t.volume = volume.clamp(0.0, 4.0);
                })
            }
            EngineCommand::SetTrackPan { track_id, pan } => self.with_track_mut(&track_id, |t| {
                t.pan = pan.clamp(-1.0, 1.0);
            }),
            EngineCommand::SetTrackMute { track_id, muted } => {
                self.with_track_mut(&track_id, |t| {
                    t.muted = muted;
                })
            }
            EngineCommand::SetTrackSolo { track_id, solo } => self.with_track_mut(&track_id, |t| {
                t.solo = solo;
            }),

            // ── Devices ──────────────────────────────────────
            EngineCommand::AddInsertDevice {
                track_id,
                device_id,
                device_type,
                index,
            } => {
                if let Some(device) = devices::create_device(&device_type) {
                    match self.find_track_mut(&track_id) {
                        Some(track) => {
                            track.add_device(device_id, device, index);
                            CommandResult::ok()
                        }
                        None => CommandResult::error(
                            "INVALID_TRACK_ID",
                            format!("Track {} not found", track_id),
                        ),
                    }
                } else {
                    CommandResult::error(
                        "INVALID_DEVICE_TYPE",
                        format!("Unknown device type: {device_type}"),
                    )
                }
            }
            EngineCommand::RemoveInsertDevice {
                track_id,
                device_id,
            } => match self.find_track_mut(&track_id) {
                Some(track) => {
                    if track.remove_device(&device_id) {
                        CommandResult::ok()
                    } else {
                        CommandResult::error(
                            "INVALID_DEVICE_ID",
                            format!("Device {} not found", device_id),
                        )
                    }
                }
                None => CommandResult::error(
                    "INVALID_TRACK_ID",
                    format!("Track {} not found", track_id),
                ),
            },
            EngineCommand::SetInsertEnabled {
                track_id,
                device_id,
                enabled,
            } => match self.find_track_mut(&track_id) {
                Some(track) => match track.find_device_mut(&device_id) {
                    Some(dev) => {
                        dev.set_enabled(enabled);
                        CommandResult::ok()
                    }
                    None => CommandResult::error(
                        "INVALID_DEVICE_ID",
                        format!("Device {} not found", device_id),
                    ),
                },
                None => CommandResult::error(
                    "INVALID_TRACK_ID",
                    format!("Track {} not found", track_id),
                ),
            },
            EngineCommand::SetInsertParam {
                track_id,
                device_id,
                param,
                value,
            } => match self.find_track_mut(&track_id) {
                Some(track) => match track.find_device_mut(&device_id) {
                    Some(dev) => match dev.set_param(&param, value) {
                        Ok(()) => CommandResult::ok(),
                        Err(e) => CommandResult::error("INVALID_PARAM", e.to_string()),
                    },
                    None => CommandResult::error(
                        "INVALID_DEVICE_ID",
                        format!("Device {} not found", device_id),
                    ),
                },
                None => CommandResult::error(
                    "INVALID_TRACK_ID",
                    format!("Track {} not found", track_id),
                ),
            },

            // ── Master ───────────────────────────────────────
            EngineCommand::SetMasterVolume { volume } => {
                self.graph.master_volume = volume.clamp(0.0, 4.0);
                CommandResult::ok()
            }

            // ── Status ───────────────────────────────────────
            EngineCommand::GetStatus => {
                let status = self.get_status();
                match serde_json::to_value(&status) {
                    Ok(v) => CommandResult::ok_with(v),
                    Err(e) => CommandResult::error("SERIALIZE_ERROR", e.to_string()),
                }
            }
            EngineCommand::Ping => {
                self.events.push(EngineEvent::Pong);
                CommandResult::ok()
            }
        }
    }

    /// Drain all pending events.
    pub fn drain_events(&mut self) -> Vec<EngineEvent> {
        self.events.drain()
    }

    /// Get engine status snapshot.
    pub fn get_status(&self) -> EngineStatus {
        EngineStatus {
            initialized: self.initialized,
            playing: self.transport.state == PlayState::Playing,
            paused: self.transport.state == PlayState::Paused,
            sample_rate: self.config.sample_rate,
            bpm: self.transport.bpm,
            beat_position: self.transport.beat_position(),
            sample_position: self.transport.sample_position,
            time_seconds: self.transport.time_seconds(),
            track_count: self.tracks.len(),
            master_volume: self.graph.master_volume,
            loop_enabled: self.transport.loop_enabled,
            loop_start_beat: self.transport.loop_start_beat,
            loop_end_beat: self.transport.loop_end_beat,
        }
    }

    // ── Private helpers ─────────────────────────────────────

    fn find_track(&self, id: &TrackId) -> Option<&MixerTrack> {
        self.tracks.iter().find(|t| &t.id == id)
    }

    fn find_track_mut(&mut self, id: &TrackId) -> Option<&mut MixerTrack> {
        self.tracks.iter_mut().find(|t| &t.id == id)
    }

    fn with_track_mut(&mut self, id: &TrackId, f: impl FnOnce(&mut MixerTrack)) -> CommandResult {
        match self.find_track_mut(id) {
            Some(track) => {
                f(track);
                CommandResult::ok()
            }
            None => CommandResult::error("INVALID_TRACK_ID", format!("Track {} not found", id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamValue;

    fn make_engine() -> DspEngine {
        DspEngine::new(EngineConfig::default())
    }

    #[test]
    fn engine_creates_and_emits_ready() {
        let mut engine = make_engine();
        let events = engine.drain_events();
        assert!(events.iter().any(|e| matches!(e, EngineEvent::Ready)));
    }

    #[test]
    fn process_silence_no_panic() {
        let mut engine = make_engine();
        let mut output = vec![0.0_f32; 512 * 2];
        engine.process(&mut output, 512);
        // All zeros
        assert!(output.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn play_stop_commands() {
        let mut engine = make_engine();
        engine.drain_events(); // Clear Ready

        let r = engine.handle_command(EngineCommand::Play {
            position_beat: None,
        });
        assert!(matches!(r, CommandResult::Ok { .. }));

        let r = engine.handle_command(EngineCommand::Stop);
        assert!(matches!(r, CommandResult::Ok { .. }));

        let events = engine.drain_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::PlaybackStarted))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, EngineEvent::PlaybackStopped))
        );
    }

    #[test]
    fn create_and_remove_track() {
        let mut engine = make_engine();

        let r = engine.handle_command(EngineCommand::CreateTrack {
            track_id: TrackId::new("t1"),
            volume: 0.8,
            pan: -0.2,
            muted: false,
            solo: false,
        });
        assert!(matches!(r, CommandResult::Ok { .. }));

        let status = engine.get_status();
        assert_eq!(status.track_count, 1);

        let r = engine.handle_command(EngineCommand::RemoveTrack {
            track_id: TrackId::new("t1"),
        });
        assert!(matches!(r, CommandResult::Ok { .. }));
        assert_eq!(engine.get_status().track_count, 0);
    }

    #[test]
    fn invalid_track_id_returns_error() {
        let mut engine = make_engine();
        let r = engine.handle_command(EngineCommand::SetTrackVolume {
            track_id: TrackId::new("nonexistent"),
            volume: 0.5,
        });
        assert!(matches!(r, CommandResult::Error { .. }));
    }

    #[test]
    fn ping_returns_pong() {
        let mut engine = make_engine();
        engine.drain_events();
        engine.handle_command(EngineCommand::Ping);
        let events = engine.drain_events();
        assert!(events.iter().any(|e| matches!(e, EngineEvent::Pong)));
    }

    #[test]
    fn add_device_to_track() {
        let mut engine = make_engine();
        engine.handle_command(EngineCommand::CreateTrack {
            track_id: TrackId::new("t1"),
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
        });

        let r = engine.handle_command(EngineCommand::AddInsertDevice {
            track_id: TrackId::new("t1"),
            device_id: crate::ids::DeviceId::new("g1"),
            device_type: "gain".into(),
            index: None,
        });
        assert!(matches!(r, CommandResult::Ok { .. }));

        // Set param on the device
        let r = engine.handle_command(EngineCommand::SetInsertParam {
            track_id: TrackId::new("t1"),
            device_id: crate::ids::DeviceId::new("g1"),
            param: "gain".into(),
            value: ParamValue::Float(0.5),
        });
        assert!(matches!(r, CommandResult::Ok { .. }));
    }

    #[test]
    fn get_status_returns_data() {
        let mut engine = make_engine();
        let r = engine.handle_command(EngineCommand::GetStatus);
        match r {
            CommandResult::Ok { data } => {
                assert!(data.is_some());
            }
            _ => panic!("Expected Ok with data"),
        }
    }
}
