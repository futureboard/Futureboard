//! DAUx shared audio render kernel.
//!
//! `fill_output_f32` is the realtime hot path shared by all backends.
//! It is realtime-safe: no allocation, no locks, no I/O.
//!
//! Each backend creates a `LocalAudioState` per-stream and passes it along
//! with the shared `SharedState` and the mutable `RuntimeProject`.

use std::sync::atomic::Ordering;
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
    pub render_path_logged: bool,
    pub metronome_enabled: bool,
    pub metronome_bpm: f64,
    pub metronome_ts_num: u32,
    pub metronome_ts_den: u32,
    pub metronome_next_sample: u64,
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
            render_path_logged: false,
            metronome_enabled: false,
            metronome_bpm: 120.0,
            metronome_ts_num: 4,
            metronome_ts_den: 4,
            metronome_next_sample: 0,
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
        self.metronome_bpm = bpm.clamp(1.0, 999.0);
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
        self.reset_metronome_schedule(position_sample, sample_rate);
    }

    pub fn reset_metronome_schedule(&mut self, position_sample: u64, sample_rate: u32) {
        let interval = self.metronome_interval_samples(sample_rate);
        self.metronome_next_sample = position_sample
            .saturating_add(interval - 1)
            .saturating_div(interval)
            .saturating_mul(interval);
    }

    #[inline]
    fn metronome_interval_samples(&self, sample_rate: u32) -> u64 {
        let beat_unit_quarters = 4.0 / self.metronome_ts_den.max(1) as f64;
        ((sample_rate.max(1) as f64 * 60.0 / self.metronome_bpm.max(1.0)) * beat_unit_quarters)
            .round()
            .max(1.0) as u64
    }

    #[inline]
    pub fn metronome_sample(&mut self, project_sample: u64, sample_rate: u32) -> f32 {
        if !self.metronome_enabled {
            return 0.0;
        }

        let interval = self.metronome_interval_samples(sample_rate);
        while project_sample >= self.metronome_next_sample {
            let beat_index = self.metronome_next_sample / interval.max(1);
            let accent = beat_index.is_multiple_of(self.metronome_ts_num.max(1) as u64);
            let freq = if accent { 1760.0 } else { 980.0 };
            self.metronome_click_phase = 0.0;
            self.metronome_click_phase_inc = freq / sample_rate.max(1) as f64;
            self.metronome_click_gain = if accent { 0.34 } else { 0.22 };
            self.metronome_click_remaining = self.metronome_click_len;
            self.metronome_next_sample = self.metronome_next_sample.saturating_add(interval);
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
                let old = std::mem::replace(runtime, *next_runtime);
                runtime.sample_rate = output_sample_rate;
                crate::graveyard::retire(old);
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
                local.playing_local = true;
                shared.playing.store(true, Ordering::Relaxed);
            }
            EngineCommand::StopTransport => {
                if command_debug_enabled() {
                    eprintln!("[DAUx] StopTransport");
                }
                local.playing_local = false;
                shared.playing.store(false, Ordering::Relaxed);
            }
            EngineCommand::Seek { position_seconds } => {
                let sr = shared.sample_rate.load(Ordering::Relaxed) as f64;
                let pos = (position_seconds * sr) as u64;
                if command_debug_enabled() {
                    eprintln!("[DAUx] Seek -> {:.3}s ({}sa)", position_seconds, pos);
                }
                shared.position_samples.store(pos, Ordering::Relaxed);
                local.reset_metronome_schedule(pos, output_sample_rate);
            }
            EngineCommand::SetMetronomeEnabled(enabled) => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                shared.metronome_enabled.store(enabled, Ordering::Relaxed);
                local.set_metronome_enabled(enabled, pos, output_sample_rate);
            }
            EngineCommand::SetBpm(bpm) => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                transport::store_f64_bits(&shared.bpm_bits, bpm);
                local.set_bpm(bpm, pos, output_sample_rate);
            }
            EngineCommand::SetTimeSignature(num, den) => {
                let pos = shared.position_samples.load(Ordering::Relaxed);
                shared.time_sig_num.store(num.max(1), Ordering::Relaxed);
                shared.time_sig_den.store(den.max(1), Ordering::Relaxed);
                local.set_time_signature(num, den, pos, output_sample_rate);
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
                runtime.update_track_mute(&track_id, muted);
            }
            EngineCommand::SetTrackSolo { track_id, solo } => {
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
        }
    }
    false
}

// ── Core f32 stereo render ────────────────────────────────────────────────────

/// Fill interleaved f32 output data (stereo, `channels` wide).
///
/// Returns the number of frames written.
/// Realtime-safe — no allocation, no locking.
pub fn fill_output_f32(
    data: &mut [f32],
    channels: usize,
    runtime: &mut RuntimeProject,
    shared: &Arc<SharedState>,
    local: &mut LocalAudioState,
) -> u64 {
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

    if channels >= 2 && local.playing_local {
        frames = render_project_block_interleaved(runtime, base_sample, master_vol, data, channels);
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
        crate::recording::apply_recording_monitor_mix(data, channels, shared, master_vol);
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
            let (proj_l, proj_r) = if local.playing_local {
                render_project_sample(runtime, base_sample + frames, master_vol)
            } else {
                (0.0, 0.0)
            };
            let click = if local.playing_local {
                local.metronome_sample(base_sample + frames, runtime.sample_rate) * master_vol
            } else {
                0.0
            };
            let l = (tone_l + proj_l + click).clamp(-1.0, 1.0);
            let r = (tone_r + proj_r + click).clamp(-1.0, 1.0);
            if shared.recording_monitor_mix.load(Ordering::Relaxed) {
                let mon_l = f32::from_bits(shared.recording_monitor_l.load(Ordering::Relaxed))
                    * master_vol
                    * 0.85;
                let mon_r = f32::from_bits(shared.recording_monitor_r.load(Ordering::Relaxed))
                    * master_vol
                    * 0.85;
                frame[0] = (l + mon_l).clamp(-1.0, 1.0);
                frame[1] = (r + mon_r).clamp(-1.0, 1.0);
            } else {
                frame[0] = l;
                frame[1] = r;
            }
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
            let (proj_l, proj_r) = if local.playing_local {
                render_project_sample(runtime, base_sample + frames, master_vol)
            } else {
                (0.0, 0.0)
            };
            let click = if local.playing_local {
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
    if local.playing_local && channels > 0 {
        shared.position_samples.fetch_add(frames, Ordering::Relaxed);
        let sample_rate = runtime.sample_rate;
        transport::apply_loop_wrap(shared, runtime, sample_rate, |start| {
            local.reset_metronome_schedule(start, sample_rate);
        });
    }

    frames
}
