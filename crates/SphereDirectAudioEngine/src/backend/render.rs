//! DAUx shared audio render kernel.
//!
//! `fill_output_f32` is the realtime hot path shared by all backends.
//! It is realtime-safe: no allocation, no locks, no I/O.
//!
//! Each backend creates a `LocalAudioState` per-stream and passes it along
//! with the shared `SharedState` and the mutable `RuntimeProject`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use crate::command::EngineCommand;
use crate::dsp::{meter::smooth_peak, oscillator::SineOscillator};
use crate::engine::{SharedState, PEAK_DECAY, TEST_TONE_AMPLITUDE};
use crate::runtime::{RuntimePreviewMode, RuntimeProject};
use crate::transport;

// Re-export helpers so wasapi_exclusive.rs can use them through render.
pub use crate::engine::{render_project_block_interleaved, render_project_sample};

fn command_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_AUDIO_COMMAND_DEBUG").is_some())
}

/// `FUTUREBOARD_AUDIO_CALLBACK_DEBUG=1` enables the realtime callback's
/// occasional eprintln traces (graph swap, mute, render-path). Off by default
/// so the audio thread never formats strings or writes to stdio — see
/// `tasks/native/audio-system-spec.md` §1 and Phase A finding A.2.2. Cached on
/// first read so the callback never touches the environment.
fn callback_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_AUDIO_CALLBACK_DEBUG").is_some())
}

fn transport_freeze_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("FUTUREBOARD_TRANSPORT_FREEZE_DEBUG").is_some())
}

/// Logs the first N audio blocks after `StartTransport` when freeze debug is on.
static POST_PLAY_CALLBACK_LOGS: AtomicU32 = AtomicU32::new(0);

#[inline]
fn log_post_play_callback(step: &str) {
    let remaining = POST_PLAY_CALLBACK_LOGS.load(Ordering::Relaxed);
    if remaining == 0 || !transport_freeze_debug_enabled() {
        return;
    }
    let left = POST_PLAY_CALLBACK_LOGS
        .fetch_sub(1, Ordering::Relaxed)
        .saturating_sub(1);
    eprintln!("[play-debug callback] {step} (remaining={left})");
}

// ── Per-stream oscillator + local playback state ──────────────────────────────

/// Local (non-shared) state for one audio stream.
/// Lives on the audio thread — no locks needed.
pub struct LocalAudioState {
    pub osc_l: SineOscillator,
    pub osc_r: SineOscillator,
    pub osc_freq: f32,
    pub osc_on: bool,
    pub playing_local: bool,
    pub prev_peak_l: f32,
    pub prev_peak_r: f32,
    /// Read cursor into the shared input ring (Layer 4 consumer state).
    pub input_read_frames: u64,
    /// Smoothed input-bus peaks for diagnostics (Layer 4 verification).
    pub prev_input_bus_l: f32,
    pub prev_input_bus_r: f32,
    pub render_path_logged: bool,
    /// Samples of instrument processing still owed after the last preview note
    /// went off, so the plugin's release tail renders out instead of being cut
    /// dead when transport is stopped. Counts down per block; refreshed while a
    /// preview note is held.
    pub preview_tail_samples: u64,
    /// Last logged preview-note count (gates PreviewRenderWake spam).
    pub prev_logged_preview_notes: u32,
    /// Blocks until next PreviewRenderWake log while preview is active.
    pub preview_wake_log_cooldown: u32,
    pub metronome_enabled: bool,
    pub metronome_ts_num: u32,
    pub metronome_ts_den: u32,
    pub time_signature_map: crate::time_signature_map::RuntimeTimeSignatureMapSnapshot,
    /// Next click position in quarter-note beats.
    pub metronome_next_beat: f64,
    pub tempo_map: crate::tempo_map::RuntimeTempoMapSnapshot,
    pub metronome_click_remaining: u32,
    pub metronome_click_len: u32,
    pub metronome_click_phase: f64,
    pub metronome_click_phase_inc: f64,
    pub metronome_click_gain: f32,
}

impl LocalAudioState {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            osc_l: SineOscillator::new(440.0, sample_rate),
            osc_r: SineOscillator::new(440.0, sample_rate),
            osc_freq: 440.0,
            osc_on: false,
            playing_local: false,
            prev_peak_l: 0.0,
            prev_peak_r: 0.0,
            input_read_frames: 0,
            prev_input_bus_l: 0.0,
            prev_input_bus_r: 0.0,
            render_path_logged: false,
            preview_tail_samples: 0,
            prev_logged_preview_notes: u32::MAX,
            preview_wake_log_cooldown: 0,
            metronome_enabled: false,
            metronome_ts_num: 4,
            metronome_ts_den: 4,
            time_signature_map: crate::time_signature_map::RuntimeTimeSignatureMapSnapshot::static_sig(
                4, 4,
            ),
            metronome_next_beat: 0.0,
            tempo_map: crate::tempo_map::RuntimeTempoMapSnapshot::static_tempo(120.0),
            metronome_click_remaining: 0,
            metronome_click_len: (sample_rate * 0.024).round().max(1.0) as u32,
            metronome_click_phase: 0.0,
            metronome_click_phase_inc: 0.0,
            metronome_click_gain: 0.0,
        }
    }

    pub fn set_metronome_enabled(&mut self, enabled: bool, position_sample: u64, sample_rate: u32) {
        self.metronome_enabled = enabled;
        self.metronome_click_remaining = 0;
        self.reset_metronome_schedule(position_sample, sample_rate);
    }

    pub fn set_bpm(&mut self, bpm: f64, position_sample: u64, sample_rate: u32) {
        self.tempo_map = crate::tempo_map::RuntimeTempoMapSnapshot::static_tempo(bpm);
        self.reset_metronome_schedule(position_sample, sample_rate);
    }

    pub fn set_tempo_map(
        &mut self,
        tempo_map: crate::tempo_map::RuntimeTempoMapSnapshot,
        position_sample: u64,
        sample_rate: u32,
    ) {
        self.tempo_map = tempo_map;
        self.reset_metronome_schedule(position_sample, sample_rate);
    }

    pub fn set_time_signature(
        &mut self,
        numerator: u32,
        denominator: u32,
        position_sample: u64,
        sample_rate: u32,
    ) {
        self.metronome_ts_num = numerator.clamp(1, 64);
        self.metronome_ts_den = denominator.clamp(1, 64);
        self.time_signature_map =
            crate::time_signature_map::RuntimeTimeSignatureMapSnapshot::static_sig(
                self.metronome_ts_num as u16,
                self.metronome_ts_den as u16,
            );
        self.reset_metronome_schedule(position_sample, sample_rate);
    }

    pub fn set_time_signature_map(
        &mut self,
        map: crate::time_signature_map::RuntimeTimeSignatureMapSnapshot,
        position_sample: u64,
        sample_rate: u32,
    ) {
        self.time_signature_map = map;
        if let Some(pt) = self.time_signature_map.points().first() {
            self.metronome_ts_num = pt.numerator as u32;
            self.metronome_ts_den = pt.denominator as u32;
        }
        self.reset_metronome_schedule(position_sample, sample_rate);
    }

    pub fn reset_metronome_schedule(&mut self, position_sample: u64, sample_rate: u32) {
        let sr = sample_rate.max(1) as f64;
        let current_beat = self.tempo_map.beat_at_samples(position_sample, sr);
        self.metronome_next_beat = self
            .time_signature_map
            .next_metronome_click_at_or_after(current_beat);
    }

    #[inline]
    pub fn metronome_sample(&mut self, project_sample: u64, sample_rate: u32) -> f32 {
        if !self.metronome_enabled {
            return 0.0;
        }

        let sr = sample_rate.max(1) as f64;
        while project_sample >= self.tempo_map.samples_at_beat(self.metronome_next_beat, sr) {
            let accent = self
                .time_signature_map
                .metronome_accent_at_beat(self.metronome_next_beat);
            let (freq, gain) = match accent {
                crate::time_signature_map::MetronomeAccent::Downbeat => (1760.0, 0.34),
                crate::time_signature_map::MetronomeAccent::Group => (1320.0, 0.28),
                crate::time_signature_map::MetronomeAccent::Normal => (980.0, 0.22),
            };
            self.metronome_click_phase = 0.0;
            self.metronome_click_phase_inc = freq / sr;
            self.metronome_click_gain = gain;
            self.metronome_click_remaining = self.metronome_click_len;
            self.metronome_next_beat = self
                .time_signature_map
                .next_metronome_click_after(self.metronome_next_beat);
        }

        if self.metronome_click_remaining == 0 {
            return 0.0;
        }

        let age = self
            .metronome_click_len
            .saturating_sub(self.metronome_click_remaining) as f32;
        let t = age / self.metronome_click_len.max(1) as f32;
        let env = (1.0 - t).max(0.0);
        let sample = (self.metronome_click_phase * std::f64::consts::TAU).sin() as f32
            * env
            * env
            * self.metronome_click_gain;
        self.metronome_click_phase += self.metronome_click_phase_inc;
        self.metronome_click_phase -= self.metronome_click_phase.floor();
        self.metronome_click_remaining = self.metronome_click_remaining.saturating_sub(1);
        sample
    }
}

// ── f32 helper store/load ─────────────────────────────────────────────────────

#[inline]
pub fn f32_store(v: f32) -> u32 {
    v.to_bits()
}
#[inline]
pub fn f32_load(v: u32) -> f32 {
    f32::from_bits(v)
}

// ── Command drain ─────────────────────────────────────────────────────────────

/// Drain all pending engine commands.  Returns true if the engine should stop.
///
/// Realtime-safe: only modifies local state or atomics.
pub fn drain_commands(
    cmd_rx: &crossbeam_channel::Receiver<EngineCommand>,
    runtime: &mut RuntimeProject,
    shared: &Arc<SharedState>,
    local: &mut LocalAudioState,
    output_sample_rate: u32,
) -> bool {
    while let Ok(cmd) = cmd_rx.try_recv() {
        match cmd {
            EngineCommand::LoadProject(next_runtime) => {
                if callback_debug_enabled() {
                    eprintln!(
                        "[DAUx] LoadProject: {} tracks, {} clips (sr={})",
                        next_runtime.tracks.len(),
                        next_runtime.clips.len(),
                        output_sample_rate,
                    );
                }
                // Swap in the new graph and retire the old one to the
                // background dropper — never run its destructor on this
                // realtime thread (frees buffers / munmaps sources / destroys
                // VST3 handles). See `crate::graveyard`.
                runtime.all_notes_off("project_load");
                let old = std::mem::replace(runtime, *next_runtime);
                runtime.sample_rate = output_sample_rate;
                // Preserve the plugin-bridge sinks across reloads (Stage 3b) — a
                // freshly built project never carries them.
                runtime.plugin_bridge_sinks = old.plugin_bridge_sinks.clone();
                runtime.bridge_editor_active = old.bridge_editor_active.clone();
                // The panic the all_notes_off above pushed into the (preserved)
                // sinks still needs flushing through the new graph.
                runtime.bridge_panic_flush_samples = old.bridge_panic_flush_samples;
                crate::graveyard::retire(old);
                // Transport/audio-graph separation: a graph swap must never
                // change the user's transport state.  If the transport was
                // Running when the swap arrived (e.g. an insert was added
                // during playback), keep rendering the new graph immediately —
                // the user must not have to press Play again.  If the
                // transport was stopped (project open/close paths call
                // StopTransport first, which clears `shared.playing`), the
                // swap lands in Paused exactly as before.
                let was_playing = shared.playing.load(Ordering::Relaxed);
                local.playing_local = was_playing;
                let pos = shared.position_samples.load(Ordering::Relaxed);
                runtime.reset_midi_playback(pos);
                local.set_tempo_map(runtime.tempo_map.clone(), pos, output_sample_rate);
                let old_state =
                    crate::engine::AudioEngineState::from_u8(shared.engine_state.load(Ordering::Relaxed));
                let new_state = if was_playing {
                    crate::engine::AudioEngineState::Running
                } else {
                    crate::engine::AudioEngineState::Paused
                };
                shared
                    .engine_state
                    .store(new_state as u8, Ordering::Relaxed);
                eprintln!(
                    "[AudioEngineState] old={old_state:?} new={new_state:?} source=graph_swap was_playing={was_playing}"
                );
            }
            EngineCommand::SetTestTone { enabled, frequency } => {
                local.osc_on = enabled;
                local.osc_freq = frequency;
                local.osc_l.set_frequency(frequency as f64);
                local.osc_r.set_frequency(frequency as f64);
            }
            EngineCommand::StartTransport => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                let active_clips = runtime.active_clip_count_at_sample(pos);
                if command_debug_enabled() {
                    eprintln!(
                        "[DAUx] StartTransport: pos={}sa ({:.3}s), active={}, scheduled={}",
                        pos,
                        pos as f64 / output_sample_rate as f64,
                        active_clips,
                        runtime.clips.len(),
                    );
                }
                if transport_freeze_debug_enabled() {
                    eprintln!("[play-debug callback] StartTransport command applied");
                    POST_PLAY_CALLBACK_LOGS.store(5, Ordering::Relaxed);
                }
                local.playing_local = true;
                shared.playing.store(true, Ordering::Relaxed);
                let old_state = crate::engine::AudioEngineState::from_u8(
                    shared.engine_state.swap(
                        crate::engine::AudioEngineState::Running as u8,
                        Ordering::Relaxed,
                    ),
                );
                eprintln!("[AudioEngineState] old={old_state:?} new=Running source=StartTransport");
                runtime.reset_midi_playback(pos);
            }
            EngineCommand::StopTransport => {
                if command_debug_enabled() {
                    eprintln!("[DAUx] StopTransport");
                }
                local.playing_local = false;
                shared.playing.store(false, Ordering::Relaxed);
                let old_state = crate::engine::AudioEngineState::from_u8(
                    shared.engine_state.swap(
                        crate::engine::AudioEngineState::Paused as u8,
                        Ordering::Relaxed,
                    ),
                );
                eprintln!("[AudioEngineState] old={old_state:?} new=Paused source=StopTransport");
                runtime.all_notes_off("stop");
            }
            EngineCommand::Seek { position_seconds } => {
                let sr = shared.sample_rate.load(Ordering::Relaxed) as f64;
                let pos = (position_seconds * sr) as u64;
                if command_debug_enabled() {
                    eprintln!("[DAUx] Seek -> {:.3}s ({}sa)", position_seconds, pos);
                }
                shared.position_samples.store(pos, Ordering::Relaxed);
                local.reset_metronome_schedule(pos, output_sample_rate);
                runtime.reset_midi_playback(pos);
            }
            EngineCommand::SetMetronomeEnabled(enabled) => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                shared.metronome_enabled.store(enabled, Ordering::Relaxed);
                local.set_metronome_enabled(enabled, pos, output_sample_rate);
            }
            EngineCommand::SetBpm(bpm) => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                transport::store_f64_bits(&shared.bpm_bits, bpm);
                let map = crate::tempo_map::RuntimeTempoMapSnapshot::static_tempo(bpm);
                local.set_tempo_map(map.clone(), pos, output_sample_rate);
                let next_pos = runtime.apply_tempo_map(map, pos);
                shared.position_samples.store(next_pos, Ordering::Relaxed);
            }
            EngineCommand::SetTempoMap(map) => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                local.set_tempo_map(map.clone(), pos, output_sample_rate);
                let next_pos = runtime.apply_tempo_map(map, pos);
                shared.position_samples.store(next_pos, Ordering::Relaxed);
            }
            EngineCommand::SetTimeSignature(num, den) => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                shared.time_sig_num.store(num.max(1), Ordering::Relaxed);
                shared.time_sig_den.store(den.max(1), Ordering::Relaxed);
                local.set_time_signature(num, den, pos, output_sample_rate);
            }
            EngineCommand::SetTimeSignatureMap(map) => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                if let Some(pt) = map.points().first() {
                    shared
                        .time_sig_num
                        .store(pt.numerator.max(1) as u32, Ordering::Relaxed);
                    shared
                        .time_sig_den
                        .store(pt.denominator.max(1) as u32, Ordering::Relaxed);
                }
                local.set_time_signature_map(map, pos, output_sample_rate);
            }
            EngineCommand::SetLoop {
                enabled,
                start_seconds,
                end_seconds,
            } => {
                let sr = shared.sample_rate.load(Ordering::Relaxed) as f64;
                let start = (start_seconds.max(0.0) * sr) as u64;
                let end = (end_seconds.max(0.0) * sr) as u64;
                shared.loop_enabled.store(enabled, Ordering::Relaxed);
                shared.loop_start_samples.store(start, Ordering::Relaxed);
                shared.loop_end_samples.store(end, Ordering::Relaxed);
            }
            EngineCommand::SetMasterVolume { value } => {
                shared
                    .master_volume
                    .store(f32_store(value), Ordering::Relaxed);
            }
            EngineCommand::SetPluginBridgeSink { insert_id, sink } => match sink {
                Some(sink) => {
                    runtime.plugin_bridge_sinks.insert(insert_id, sink);
                }
                None => {
                    runtime.plugin_bridge_sinks.remove(&insert_id);
                }
            },
            EngineCommand::SetBridgeEditorActive { track_id, active } => {
                runtime.set_bridge_editor_active(&track_id, active);
            }
            EngineCommand::SetTrackVolume { track_id, value } => {
                runtime.update_track_volume(&track_id, value);
            }
            EngineCommand::SetTrackPan { track_id, value } => {
                runtime.update_track_pan(&track_id, value);
            }
            EngineCommand::SetTrackMute { track_id, muted } => {
                if callback_debug_enabled() {
                    eprintln!("[DAUx] SetTrackMute track={track_id} muted={muted}");
                }
                runtime.all_notes_off("track_mute");
                runtime.update_track_mute(&track_id, muted);
            }
            EngineCommand::SetTrackSolo { track_id, solo } => {
                runtime.all_notes_off("track_solo");
                runtime.update_track_solo(&track_id, solo);
            }
            EngineCommand::SetTrackPreviewMode { track_id, value } => {
                runtime.update_track_preview_mode(&track_id, RuntimePreviewMode::from_code(value));
            }
            EngineCommand::SetInsertParam {
                track_id,
                insert_id,
                param_id,
                value,
            } => {
                runtime.update_insert_param(&track_id, &insert_id, &param_id, value);
            }
            EngineCommand::MidiPreviewNoteOn {
                track_id,
                channel,
                pitch,
                velocity,
            } => {
                runtime.midi_preview_note_on(&track_id, channel, pitch, velocity);
            }
            EngineCommand::MidiPreviewNoteOff {
                track_id,
                channel,
                pitch,
            } => {
                runtime.midi_preview_note_off(&track_id, channel, pitch);
            }
            EngineCommand::MidiPreviewAllNotesOff { track_id } => {
                runtime.midi_preview_all_notes_off(&track_id);
            }
            EngineCommand::PluginPreviewNoteOn {
                track_id,
                plugin_instance_id,
                channel,
                pitch,
                velocity,
            } => {
                if crate::forensic_trace::engine_midi_verbose_enabled() {
                    eprintln!(
                        "[midi-preview-audio] dequeue note_on instance={plugin_instance_id} pitch={pitch}"
                    );
                }
                runtime.bridge_preview_note_on(
                    &track_id,
                    &plugin_instance_id,
                    channel,
                    pitch,
                    velocity,
                );
            }
            EngineCommand::PluginPreviewNoteOff {
                track_id,
                plugin_instance_id,
                channel,
                pitch,
            } => {
                if crate::forensic_trace::engine_midi_verbose_enabled() {
                    eprintln!(
                        "[midi-preview-audio] dequeue note_off instance={plugin_instance_id} pitch={pitch}"
                    );
                }
                runtime.bridge_preview_note_off(&track_id, &plugin_instance_id, channel, pitch);
            }
            EngineCommand::PluginPreviewAllNotesOff {
                track_id,
                plugin_instance_id,
            } => {
                runtime.bridge_preview_all_notes_off(&track_id, &plugin_instance_id);
            }
        }
    }
    false
}

// ── Core f32 stereo render ────────────────────────────────────────────────────

/// Output-callback block of the last slow-block log (throttle: one watchdog
/// log per ~200 callbacks so a sustained stall cannot flood stderr).
static SLOW_CALLBACK_LAST_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Fill interleaved f32 output data (stereo, `channels` wide).
///
/// Returns the number of frames written.
/// Realtime-safe — no allocation, no locking.
///
/// Wraps the render kernel with the callback-duration watchdog (audio-hang
/// spec §12): publishes last/max duration to [`SharedState`] and emits a
/// throttled warning when a block exceeds the realtime budget.
pub fn fill_output_f32(
    data: &mut [f32],
    channels: usize,
    runtime: &mut RuntimeProject,
    shared: &Arc<SharedState>,
    local: &mut LocalAudioState,
) -> u64 {
    let started = std::time::Instant::now();
    let frames = fill_output_f32_inner(data, channels, runtime, shared, local);
    let elapsed_us = started.elapsed().as_micros().min(u32::MAX as u128) as u32;
    shared.last_callback_us.store(elapsed_us, Ordering::Relaxed);
    if elapsed_us > shared.max_callback_us.load(Ordering::Relaxed) {
        shared.max_callback_us.store(elapsed_us, Ordering::Relaxed);
    }
    if elapsed_us >= 2_000 {
        shared.slow_callback_count.fetch_add(1, Ordering::Relaxed);
        if elapsed_us >= 5_000 {
            let cb = shared.output_cb_count.load(Ordering::Relaxed);
            let last = SLOW_CALLBACK_LAST_LOG.load(Ordering::Relaxed);
            if cb.wrapping_sub(last) > 200 {
                SLOW_CALLBACK_LAST_LOG.store(cb, Ordering::Relaxed);
                let state = crate::engine::AudioEngineState::from_u8(
                    shared.engine_state.load(Ordering::Relaxed),
                );
                let severity = if elapsed_us >= 10_000 { "error" } else { "warning" };
                eprintln!(
                    "[AudioCallback] slow block severity={severity} duration_us={elapsed_us} state={} frames={frames}",
                    state.as_str()
                );
            }
        }
    }
    frames
}

fn fill_output_f32_inner(
    data: &mut [f32],
    channels: usize,
    runtime: &mut RuntimeProject,
    shared: &Arc<SharedState>,
    local: &mut LocalAudioState,
) -> u64 {
    shared.output_cb_count.fetch_add(1, Ordering::Relaxed);
    let engine_state =
        crate::engine::AudioEngineState::from_u8(shared.engine_state.load(Ordering::Relaxed));
    // The transport only advances in Running — a stale `playing_local` left
    // over from a graph swap must never drive rendering while Paused.
    let transport_playing =
        local.playing_local && matches!(engine_state, crate::engine::AudioEngineState::Running);
    if engine_state.outputs_silence() {
        // Paused still services MIDI preview, the post-panic bridge flush, and
        // open plugin editors: those need the block/handshake loop alive so
        // bridged VSTi hosts keep draining MIDI and producing audio while the
        // transport is stopped. Every other non-Running state (loading /
        // closing / device switch / suspended) is hard silence.
        let preview_wake = matches!(engine_state, crate::engine::AudioEngineState::Paused)
            && (runtime.has_active_midi_preview()
                || runtime.bridge_panic_flush_samples > 0
                || runtime.has_bridge_editor_active()
                || local.preview_tail_samples > 0
                || runtime
                    .tracks
                    .iter()
                    .any(|t| !t.midi_block_events.is_empty()));
        // When preview_wake holds, fall through to the normal body with the
        // transport treated as stopped — only preview/flush processing runs.
        if !preview_wake {
            for sample in data.iter_mut() {
                *sample = 0.0;
            }
            local.preview_tail_samples = 0;
            local.prev_peak_l = 0.0;
            local.prev_peak_r = 0.0;
            shared.peak_l.store(crate::engine::f32_store(0.0), Ordering::Relaxed);
            shared.peak_r.store(crate::engine::f32_store(0.0), Ordering::Relaxed);
            shared.rms_l.store(crate::engine::f32_store(0.0), Ordering::Relaxed);
            shared.rms_r.store(crate::engine::f32_store(0.0), Ordering::Relaxed);
            runtime.end_meter_block(0);
            let frames = data.len() / channels.max(1);
            if callback_debug_enabled()
                && shared.output_cb_count.load(Ordering::Relaxed) % 400 == 1
            {
                eprintln!(
                    "[AudioEngine] callback silence reason={} frames={frames}",
                    engine_state.as_str()
                );
                eprintln!("[AudioEngine] output cleared");
            }
            return frames as u64;
        }
    }
    if transport_playing {
        log_post_play_callback("block entered");
    }
    // Sync oscillator from atomics (set from control thread between blocks).
    let tone_on = shared.test_tone_enabled.load(Ordering::Relaxed);
    let tone_freq = f32_load(shared.test_tone_freq.load(Ordering::Relaxed));
    if tone_freq != local.osc_freq {
        local.osc_freq = tone_freq;
        local.osc_l.set_frequency(tone_freq as f64);
        local.osc_r.set_frequency(tone_freq as f64);
    }
    let gen_tone = tone_on || local.osc_on;
    let master_vol = f32_load(shared.master_volume.load(Ordering::Relaxed));
    let base_sample = shared.position_samples.load(Ordering::Relaxed);

    let mut peak_l = 0.0f32;
    let mut peak_r = 0.0f32;
    let mut sum_sq_l = 0.0f32;
    let mut sum_sq_r = 0.0f32;
    let mut frames = 0u64;
    runtime.begin_meter_block();

    if transport_playing {
        let frames_needed = data.len().checked_div(channels).unwrap_or(0) as u64;
        if frames_needed > 0 {
            runtime.schedule_midi_block(base_sample, frames_needed);
        }
    }

    let pending_midi = channels > 0
        && runtime
            .tracks
            .iter()
            .any(|t| !t.midi_block_events.is_empty());
    let frames_in_block = data.len().checked_div(channels).unwrap_or(0) as u64;
    let has_preview = runtime.has_active_midi_preview();
    if transport_playing {
        // Transport drives processing while playing; don't carry a stale tail.
        local.preview_tail_samples = 0;
        // Playing blocks request/drain the bridge anyway — flush is implicit.
        runtime.bridge_panic_flush_samples = 0;
    } else if has_preview || pending_midi {
        // A preview note is held (or its on/off just queued) — keep enough tail
        // queued to render the instrument's release after the eventual note-off.
        local.preview_tail_samples = (runtime.sample_rate as u64).saturating_mul(2);
    }
    // Post-panic flush: keep requesting bridge blocks until the host has had
    // time to drain the panic CCs (stop/seek/mute) — counted down per block.
    let panic_flush = runtime.bridge_panic_flush_samples > 0;
    if !transport_playing && panic_flush {
        runtime.bridge_panic_flush_samples = runtime
            .bridge_panic_flush_samples
            .saturating_sub(frames_in_block);
    }
    // An open external plugin editor keeps the graph rendering while stopped
    // so the plugin's own UI keyboard stays audible (parity with the legacy
    // callback path).
    let bridge_editor_wakeup = runtime.has_bridge_editor_active();
    let preview_render_active = has_preview
        || pending_midi
        || panic_flush
        || bridge_editor_wakeup
        || local.preview_tail_samples > 0;
    if preview_render_active
        && !transport_playing
        && (has_preview || pending_midi || local.preview_tail_samples > 0)
    {
        let active_notes: usize = runtime
            .midi_tracks
            .iter()
            .map(|mt| mt.preview_active.len())
            .sum();
        let active_u32 = active_notes as u32;
        let changed = active_u32 != local.prev_logged_preview_notes;
        if changed {
            eprintln!(
                "[PreviewRenderWake] active_preview_notes changed {} -> {} tail_samples={}",
                local.prev_logged_preview_notes, active_u32, local.preview_tail_samples
            );
            local.prev_logged_preview_notes = active_u32;
            local.preview_wake_log_cooldown = 0;
        } else if active_notes > 0 {
            local.preview_wake_log_cooldown = local.preview_wake_log_cooldown.saturating_add(1);
            let sr = runtime.sample_rate.max(1);
            let log_interval_blocks = (sr / frames_in_block.max(1) as u32).max(1);
            if local.preview_wake_log_cooldown >= log_interval_blocks {
                local.preview_wake_log_cooldown = 0;
                eprintln!(
                    "[PreviewRenderWake] active_preview_notes={} tail_samples={} rendering_while_stopped=true",
                    active_notes, local.preview_tail_samples
                );
            }
        }
        // Once no note is held and nothing is queued, the remaining tail is pure
        // decay — count it down so processing eventually stops.
        if !has_preview && !pending_midi {
            local.preview_tail_samples = local.preview_tail_samples.saturating_sub(frames_in_block);
            if local.preview_tail_samples == 0 {
                local.prev_logged_preview_notes = u32::MAX;
            }
        }
    }

    if channels >= 2 && (transport_playing || preview_render_active) {
        frames = render_project_block_interleaved(
            runtime,
            base_sample,
            master_vol,
            data,
            channels,
            transport_playing,
            shared.time_sig_num.load(Ordering::Relaxed),
            shared.time_sig_den.load(Ordering::Relaxed),
        );
        if !local.render_path_logged {
            local.render_path_logged = true;
            if callback_debug_enabled() {
                eprintln!(
                    "[SphereAudio callback] renderPath=daux-block frames={} channels={} tracks={}",
                    frames,
                    channels,
                    runtime.tracks.len()
                );
            }
        }
        if gen_tone {
            for frame in data.chunks_mut(channels) {
                let tone_l = local.osc_l.next_sample() * TEST_TONE_AMPLITUDE * master_vol;
                let tone_r = local.osc_r.next_sample() * TEST_TONE_AMPLITUDE * master_vol;
                frame[0] = (frame[0] + tone_l).clamp(-1.0, 1.0);
                frame[1] = (frame[1] + tone_r).clamp(-1.0, 1.0);
            }
        }
        for (i, frame) in data.chunks_mut(channels).enumerate() {
            let click = local.metronome_sample(base_sample + i as u64, runtime.sample_rate);
            if click != 0.0 {
                frame[0] = (frame[0] + click * master_vol).clamp(-1.0, 1.0);
                frame[1] = (frame[1] + click * master_vol).clamp(-1.0, 1.0);
            }
        }
        // Live monitoring is mixed below via the input ring (single, clean
        // path) — the old per-block sample-and-hold monitor was removed because
        // it held one input sample across the whole output block (warble).
        for frame in data.chunks(channels) {
            let l = frame[0];
            let r = frame[1];
            peak_l = peak_l.max(l.abs());
            peak_r = peak_r.max(r.abs());
            sum_sq_l += l * l;
            sum_sq_r += r * r;
        }
    } else if channels >= 2 {
        for frame in data.chunks_mut(channels) {
            let (tone_l, tone_r) = if gen_tone {
                (
                    local.osc_l.next_sample() * TEST_TONE_AMPLITUDE * master_vol,
                    local.osc_r.next_sample() * TEST_TONE_AMPLITUDE * master_vol,
                )
            } else {
                (0.0, 0.0)
            };
            let (proj_l, proj_r) = if transport_playing {
                render_project_sample(runtime, base_sample + frames, master_vol)
            } else {
                (0.0, 0.0)
            };
            let click = if transport_playing {
                local.metronome_sample(base_sample + frames, runtime.sample_rate) * master_vol
            } else {
                0.0
            };
            let l = (tone_l + proj_l + click).clamp(-1.0, 1.0);
            let r = (tone_r + proj_r + click).clamp(-1.0, 1.0);
            // Live monitor is added afterwards from the input ring (see below).
            frame[0] = l;
            frame[1] = r;
            for extra in frame.iter_mut().skip(2) {
                *extra = 0.0;
            }
            peak_l = peak_l.max(l.abs());
            peak_r = peak_r.max(r.abs());
            sum_sq_l += l * l;
            sum_sq_r += r * r;
            frames += 1;
        }
    } else if channels == 1 {
        for sample in data.iter_mut() {
            let tone = if gen_tone {
                local.osc_l.next_sample() * TEST_TONE_AMPLITUDE * master_vol
            } else {
                0.0
            };
            let (proj_l, proj_r) = if transport_playing {
                render_project_sample(runtime, base_sample + frames, master_vol)
            } else {
                (0.0, 0.0)
            };
            let click = if transport_playing {
                local.metronome_sample(base_sample + frames, runtime.sample_rate) * master_vol
            } else {
                0.0
            };
            let v = (tone + (proj_l + proj_r) * 0.5 + click).clamp(-1.0, 1.0);
            *sample = v;
            peak_l = peak_l.max(v.abs());
            sum_sq_l += v * v;
            frames += 1;
        }
    }

    if shared.live_input_active.load(Ordering::Relaxed) {
        // Per-track input meters from the latest captured sample (Layer 6).
        let input_l = f32_load(shared.live_input_l.load(Ordering::Relaxed));
        let input_r = f32_load(shared.live_input_r.load(Ordering::Relaxed));
        runtime.accumulate_live_input_meters(input_l, input_r);

        // Live monitoring: drain the input ring and mix it into the output
        // (Layers 4 + 7). Runs whether or not the transport is playing so the
        // user hears input as soon as Monitor is enabled.
        let (mon_peak_l, mon_peak_r) = mix_monitor_input(data, channels, shared, local, master_vol);
        // Fold the monitored signal into the master peak so the master meter
        // reflects what is actually leaving the device.
        peak_l = peak_l.max(mon_peak_l);
        peak_r = peak_r.max(mon_peak_r);
    } else {
        // No live input — clear the input-bus peak so diagnostics decay to 0.
        shared
            .input_bus_peak_l
            .store(f32_store(0.0), Ordering::Relaxed);
        shared
            .input_bus_peak_r
            .store(f32_store(0.0), Ordering::Relaxed);
        local.prev_input_bus_l = 0.0;
        local.prev_input_bus_r = 0.0;
    }

    // Legacy master-bus bridge fallback (disabled by default — per-track routing
    // through external-bridge-plugin inserts is the normal path).
    if plugin_bridge_master_fallback_enabled() {
        let (bridge_peak_l, bridge_peak_r) = mix_plugin_bridge(data, channels, runtime, master_vol);
        peak_l = peak_l.max(bridge_peak_l);
        peak_r = peak_r.max(bridge_peak_r);
    }

    // Update meters.
    let rms_l = if frames > 0 {
        (sum_sq_l / frames as f32).sqrt()
    } else {
        0.0
    };
    let (pk_r, rms_r) = if channels >= 2 {
        (
            peak_r,
            if frames > 0 {
                (sum_sq_r / frames as f32).sqrt()
            } else {
                0.0
            },
        )
    } else {
        (peak_l, rms_l)
    };
    runtime.end_meter_block(frames);

    local.prev_peak_l = smooth_peak(local.prev_peak_l, peak_l, PEAK_DECAY);
    local.prev_peak_r = smooth_peak(local.prev_peak_r, pk_r, PEAK_DECAY);

    shared
        .peak_l
        .store(f32_store(local.prev_peak_l), Ordering::Relaxed);
    shared
        .peak_r
        .store(f32_store(local.prev_peak_r), Ordering::Relaxed);
    shared.rms_l.store(f32_store(rms_l), Ordering::Relaxed);
    shared.rms_r.store(f32_store(rms_r), Ordering::Relaxed);

    // Advance transport position.
    if transport_playing && channels > 0 {
        shared.position_samples.fetch_add(frames, Ordering::Relaxed);
        let sample_rate = runtime.sample_rate;
        transport::apply_loop_wrap(shared, runtime, sample_rate, |start| {
            local.reset_metronome_schedule(start, sample_rate);
        });
    }

    // Consumed for this block — clear AFTER render so drain_commands preview
    // events queued earlier in the same callback survive until apply_insert.
    for track in &mut runtime.tracks {
        track.midi_block_events.clear();
    }

    frames
}

/// Largest block the bridge mix reads in one callback (stack scratch bound).
const BRIDGE_MAX_FRAMES: usize = 2048;

/// Whether the legacy master-bus bridge mix fallback is enabled. Bridge DSP is
/// normally routed per-track through `external-bridge-plugin` inserts; set
/// `FUTUREBOARD_PLUGIN_BRIDGE_AUDIO=0` to disable the master fallback.
fn plugin_bridge_master_fallback_enabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("FUTUREBOARD_PLUGIN_BRIDGE_AUDIO")
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                !matches!(v.as_str(), "0" | "false" | "no" | "off")
            })
            .unwrap_or(false)
    })
}

/// Stage 3b: read the external plugin host's previously produced block from the
/// shared region and mix it into the master output `data`, returning the mixed
/// peak so the caller can fold it into the master meter. Then request the next
/// block (one-block latency — never blocks the audio thread).
///
/// Realtime-safe: fixed stack scratch, atomics + arithmetic only, no allocation
/// or locking. No-op unless the bridge audio path is enabled and a sink is set.
fn mix_plugin_bridge(
    data: &mut [f32],
    channels: usize,
    runtime: &RuntimeProject,
    master_vol: f32,
) -> (f32, f32) {
    if runtime.plugin_bridge_sinks.is_empty() {
        return (0.0, 0.0);
    }
    let ch = channels.max(1);
    let frames = data.len() / ch;
    if frames == 0 {
        return (0.0, 0.0);
    }
    let n = frames.min(BRIDGE_MAX_FRAMES);
    let mut scratch_l = [0.0f32; BRIDGE_MAX_FRAMES];
    let mut scratch_r = [0.0f32; BRIDGE_MAX_FRAMES];
    let mut peak_l = 0.0f32;
    let mut peak_r = 0.0f32;
    // Mix every registered track's bridged plugin output into the master.
    // (Per-track routing through each track's fader/mute/solo is a later,
    // runtime-validated step; this sums them onto the master bus for now.)
    for sink in runtime.plugin_bridge_sinks.values() {
        let got = sink.read_output(&mut scratch_l[..n], &mut scratch_r[..n], n);
        for i in 0..got {
            let l = scratch_l[i] * master_vol;
            let r = scratch_r[i] * master_vol;
            let base = i * ch;
            data[base] += l;
            if ch > 1 {
                data[base + 1] += r;
            }
            peak_l = peak_l.max(l.abs());
            peak_r = peak_r.max(r.abs());
        }
        // Request the next block (the host fills it asynchronously for next time).
        sink.request_block(frames as u32);
    }
    (peak_l, peak_r)
}

/// Drain the shared input ring into the output buffer (Layers 4 + 7).
///
/// Always advances the read cursor — even when monitoring is off — so the
/// input-bus peak stays live for diagnostics and the monitor mix never replays
/// stale audio when it is toggled on. Returns the *post-gain* monitor peak so
/// the caller can fold it into the master meter.
///
/// Realtime-safe: atomics + arithmetic only, no allocation or locking.
fn mix_monitor_input(
    data: &mut [f32],
    channels: usize,
    shared: &Arc<SharedState>,
    local: &mut LocalAudioState,
    master_vol: f32,
) -> (f32, f32) {
    let ring = &shared.input_ring;
    if !ring.is_active() || channels == 0 {
        return (0.0, 0.0);
    }
    let frames = data.len() / channels;
    if frames == 0 {
        return (0.0, 0.0);
    }
    let head = ring.write_head();
    if head == 0 {
        return (0.0, 0.0);
    }
    let frames64 = frames as u64;

    // Hold a small, stable monitoring latency behind the producer. The input
    // and output callbacks are separate WASAPI clients with different block
    // sizes, so reading right up to the head underruns on every block tail
    // (the warble). Target ≈15 ms of buffered input; resync only when drift
    // leaves the safe window. Same physical device ⇒ shared word clock ⇒ no
    // sustained drift, so corrections are rare.
    let cap = ring.capacity_frames();
    let sr = shared.sample_rate.load(Ordering::Relaxed).max(1) as u64;
    let target = ((sr * 15) / 1000).max(frames64 * 2);

    // Resync on gross overrun (cursor lapped) or if the cursor is ahead of the
    // producer (should not happen): jump to `target` frames behind the head.
    if local.input_read_frames > head || head.saturating_sub(local.input_read_frames) > cap {
        local.input_read_frames = head.saturating_sub(target);
        shared.monitor_ring_overruns.fetch_add(1, Ordering::Relaxed);
    }
    // Latency crept too high (input outran output): skip forward to `target`.
    if head.saturating_sub(local.input_read_frames) > target + frames64 {
        local.input_read_frames = head.saturating_sub(target);
        shared.monitor_ring_overruns.fetch_add(1, Ordering::Relaxed);
    }

    let available = head.saturating_sub(local.input_read_frames);
    if available < frames64 {
        // Not enough buffered to fill the block — count an underrun. We still
        // read what's there and pad the remainder with silence (never replay
        // stale samples).
        shared
            .monitor_ring_underruns
            .fetch_add(1, Ordering::Relaxed);
        shared.output_xruns.fetch_add(1, Ordering::Relaxed);
    }

    let monitor_on = shared.monitor_enabled_any.load(Ordering::Relaxed);
    let mon_gain = f32_load(shared.monitor_gain.load(Ordering::Relaxed));

    let mut bus_peak_l = 0.0f32;
    let mut bus_peak_r = 0.0f32;
    let mut out_peak_l = 0.0f32;
    let mut out_peak_r = 0.0f32;
    let mut read = local.input_read_frames;
    let mut consumed = 0u64;

    for frame in data.chunks_mut(channels) {
        let (in_l, in_r) = if read < head {
            let s = ring.read_frame(read);
            read += 1;
            consumed += 1;
            s
        } else {
            // Underrun: emit silence rather than repeating the last block.
            (0.0, 0.0)
        };
        bus_peak_l = bus_peak_l.max(in_l.abs());
        bus_peak_r = bus_peak_r.max(in_r.abs());
        if monitor_on && channels >= 2 {
            let m_l = in_l * mon_gain * master_vol;
            let m_r = in_r * mon_gain * master_vol;
            frame[0] = (frame[0] + m_l).clamp(-1.0, 1.0);
            frame[1] = (frame[1] + m_r).clamp(-1.0, 1.0);
            out_peak_l = out_peak_l.max(m_l.abs());
            out_peak_r = out_peak_r.max(m_r.abs());
        }
    }
    local.input_read_frames = read;
    shared
        .monitor_frames_consumed
        .fetch_add(consumed, Ordering::Relaxed);

    // Smooth + publish the input-bus peak (pre-master) and monitor-output peak
    // for diagnostics.
    local.prev_input_bus_l = smooth_peak(local.prev_input_bus_l, bus_peak_l, PEAK_DECAY);
    local.prev_input_bus_r = smooth_peak(local.prev_input_bus_r, bus_peak_r, PEAK_DECAY);
    shared
        .input_bus_peak_l
        .store(f32_store(local.prev_input_bus_l), Ordering::Relaxed);
    shared
        .input_bus_peak_r
        .store(f32_store(local.prev_input_bus_r), Ordering::Relaxed);
    shared
        .monitor_output_peak
        .store(f32_store(out_peak_l.max(out_peak_r)), Ordering::Relaxed);

    (out_peak_l, out_peak_r)
}
