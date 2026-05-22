//! WASM bindings — struct-based, numeric-only interface.
//!
//! Uses a `WasmAudioEngine` struct instead of global JSON functions.
//! All exported methods use numeric primitive types and typed arrays only.
//! This means **TextEncoder and TextDecoder are not required** at the WASM
//! boundary, avoiding `ReferenceError: TextEncoder is not defined` in
//! AudioWorklet contexts where those globals may be absent.
//!
//! # WASM export name convention (wasm-bindgen)
//! Struct `WasmAudioEngine` → prefix `wasmaudioengine`
//! Method `play` → export `wasmaudioengine_play`
//! Constructor `new` → export `wasmaudioengine_new`
//! Drop → export `__wbg_wasmaudioengine_free`

use wasm_bindgen::prelude::*;

use crate::commands::EngineCommand;
use crate::dsp::{
    granular::time_stretch_granular,
    pitch::{pitch_shift_draft, pitch_shift_draft_quality, GRAIN_SIZE_DRAFT, GRAIN_SIZE_BALANCED, GRAIN_SIZE_HIGH},
    resample::resample_linear,
};
use crate::engine::{DspEngine, EngineConfig};

// ── Standalone DSP exports ────────────────────────────────────────────────────

/// Resample a mono f32 buffer by speed_ratio.
/// speed_ratio 2.0 → half length (plays twice as fast).
/// speed_ratio 0.5 → double length (plays at half speed).
#[wasm_bindgen]
pub fn process_speed_mono(input: &[f32], speed_ratio: f32) -> Vec<f32> {
    resample_linear(input, speed_ratio)
}

/// Pitch-shift a mono f32 buffer by semitones (±24), preserving duration.
#[wasm_bindgen]
pub fn process_pitch_mono(input: &[f32], semitones: f32) -> Vec<f32> {
    pitch_shift_draft(input, semitones)
}

/// Time-stretch a mono f32 buffer by stretch_ratio without changing pitch.
/// stretch_ratio 2.0 → twice as long (slower). 0.5 → half length (faster).
#[wasm_bindgen]
pub fn process_time_stretch_mono(input: &[f32], stretch_ratio: f32) -> Vec<f32> {
    time_stretch_granular(input, stretch_ratio, GRAIN_SIZE_BALANCED)
}

/// Pitch-shift with explicit grain size for quality control.
/// grain_size: use GRAIN_SIZE_DRAFT(1024), GRAIN_SIZE_BALANCED(2048), or GRAIN_SIZE_HIGH(4096).
#[wasm_bindgen]
pub fn process_pitch_mono_quality(input: &[f32], semitones: f32, grain_size: u32) -> Vec<f32> {
    pitch_shift_draft_quality(input, semitones, grain_size as usize)
}

/// Time-stretch with explicit grain size for quality control.
#[wasm_bindgen]
pub fn process_time_stretch_mono_quality(input: &[f32], stretch_ratio: f32, grain_size: u32) -> Vec<f32> {
    time_stretch_granular(input, stretch_ratio, grain_size as usize)
}

/// Returns the grain size for the given quality string index:
/// 0=draft(1024), 1=balanced(2048), 2=high(4096).
#[wasm_bindgen]
pub fn grain_size_for_quality(quality_index: u32) -> u32 {
    match quality_index {
        0 => GRAIN_SIZE_DRAFT    as u32,
        2 => GRAIN_SIZE_HIGH     as u32,
        _ => GRAIN_SIZE_BALANCED as u32,
    }
}

// ── Panic hook ────────────────────────────────────────────────────────────────

/// Called by wasm-bindgen on module init.
/// Sets up the console error panic hook when the feature is enabled so that
/// Rust panics produce readable stack traces in the browser DevTools.
#[wasm_bindgen(start)]
pub fn wasm_start() {
    #[cfg(feature = "console_panic")]
    console_error_panic_hook::set_once();
}

// ── Engine struct ─────────────────────────────────────────────────────────────

/// Rust WASM DSP engine — numeric-only WASM interface.
///
/// Construct with `new WasmAudioEngine(sampleRate, blockSize, channels, bpm)`.
/// All methods take/return numbers or Float32Arrays — no strings, no JSON.
#[wasm_bindgen]
pub struct WasmAudioEngine {
    inner: DspEngine,
    /// Smoothed peak meters (stereo).
    last_peak_l: f32,
    last_peak_r: f32,
    /// Throttle counter for transport-position events.
    pos_tick: u32,
    /// Emit a position event every N process() calls (≈15 Hz at 128 frames / 44.1 kHz).
    pos_tick_max: u32,
}

#[wasm_bindgen]
impl WasmAudioEngine {
    /// Create and initialize the engine.
    ///
    /// All parameters are clamped to safe ranges so invalid input cannot panic.
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f64, block_size: usize, channels: usize, bpm: f64) -> WasmAudioEngine {
        let config = EngineConfig {
            sample_rate: sample_rate.clamp(8_000.0, 384_000.0),
            max_block_size: block_size.clamp(16, 8192),
            channel_count: channels.clamp(1, 64),
            bpm: bpm.clamp(20.0, 999.0),
        };
        WasmAudioEngine {
            inner: DspEngine::new(config),
            last_peak_l: 0.0,
            last_peak_r: 0.0,
            pos_tick: 0,
            pos_tick_max: 8,
        }
    }

    // ── Transport controls ────────────────────────────────────────────────────

    /// Start playback from current position.
    pub fn play(&mut self) {
        self.inner.handle_command(EngineCommand::Play { position_beat: None });
    }

    /// Pause playback (position is preserved).
    pub fn pause(&mut self) {
        self.inner.handle_command(EngineCommand::Pause);
    }

    /// Stop playback and reset position to zero.
    pub fn stop(&mut self) {
        self.inner.handle_command(EngineCommand::Stop);
    }

    /// Seek to a beat position.
    pub fn seek_beat(&mut self, beat: f64) {
        self.inner.handle_command(EngineCommand::SeekBeat { beat: beat.max(0.0) });
    }

    /// Set BPM (clamped to 20–999).
    pub fn set_bpm(&mut self, bpm: f64) {
        self.inner.handle_command(EngineCommand::SetBpm { bpm });
    }

    /// Enable or disable the loop region without changing its bounds.
    pub fn set_loop_enabled(&mut self, enabled: bool) {
        let s = self.inner.get_status();
        self.inner.handle_command(EngineCommand::SetLoop {
            enabled,
            start_beat: s.loop_start_beat,
            end_beat: s.loop_end_beat,
        });
    }

    /// Set the loop region bounds (in beats) without changing the enabled flag.
    pub fn set_loop_range(&mut self, start_beat: f64, end_beat: f64) {
        let s = self.inner.get_status();
        self.inner.handle_command(EngineCommand::SetLoop {
            enabled: s.loop_enabled,
            start_beat,
            end_beat,
        });
    }

    // ── Transport status (numeric getters — no JSON) ──────────────────────────

    /// Returns `true` while the engine is playing.
    pub fn is_playing(&self) -> bool {
        self.inner.get_status().playing
    }

    /// Returns `true` while paused (stopped and position > 0).
    pub fn is_paused(&self) -> bool {
        self.inner.get_status().paused
    }

    /// Current beat position.
    pub fn beat_position(&self) -> f64 {
        self.inner.get_status().beat_position
    }

    /// Current BPM.
    pub fn bpm(&self) -> f64 {
        self.inner.get_status().bpm
    }

    /// Low 32 bits of the sample position.
    ///
    /// Combine with `sample_position_high()` to reconstruct the full u64 position:
    /// ```js
    /// const pos = BigInt(engine.sample_position_high()) * 2n**32n + BigInt(engine.sample_position_low());
    /// ```
    pub fn sample_position_low(&self) -> u32 {
        (self.inner.get_status().sample_position & 0xFFFF_FFFF) as u32
    }

    /// High 32 bits of the sample position.
    pub fn sample_position_high(&self) -> u32 {
        (self.inner.get_status().sample_position >> 32) as u32
    }

    /// Smoothed peak level for the left channel (0.0–1.0).
    pub fn last_peak_l(&self) -> f32 {
        self.last_peak_l
    }

    /// Smoothed peak level for the right channel (0.0–1.0).
    pub fn last_peak_r(&self) -> f32 {
        self.last_peak_r
    }

    // ── Audio processing ──────────────────────────────────────────────────────

    /// Fill `output` with interleaved f32 audio for `frames` frames.
    ///
    /// Returns `true` approximately every 8 calls when the engine is playing,
    /// signalling the caller to emit a transport-position event. This throttling
    /// keeps the per-block cost minimal.
    pub fn process_interleaved(&mut self, output: &mut [f32], frames: usize) -> bool {
        // Run the DSP graph (outputs silence for an empty project, no panic).
        self.inner.process(output, frames);

        // Update smoothed stereo peak meters from the just-written output.
        let ch = 2_usize;
        if output.len() >= ch {
            let n = frames.min(output.len() / ch);
            let mut pl = 0.0_f32;
            let mut pr = 0.0_f32;
            for i in 0..n {
                let l = output[i * ch].abs();
                let r = output[i * ch + 1].abs();
                if l > pl { pl = l; }
                if r > pr { pr = r; }
            }
            // Exponential decay so the meter falls smoothly between calls.
            self.last_peak_l = self.last_peak_l.mul_add(0.85, pl * 0.15);
            self.last_peak_r = self.last_peak_r.mul_add(0.85, pr * 0.15);
        }

        // Throttle: signal JS to emit a position event only every N blocks.
        self.pos_tick += 1;
        if self.pos_tick >= self.pos_tick_max {
            self.pos_tick = 0;
            return self.inner.get_status().playing;
        }
        false
    }
}
