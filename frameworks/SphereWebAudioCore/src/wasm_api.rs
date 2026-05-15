//! WASM bindings for the DSP engine.
//!
//! Exports functions via wasm-bindgen for use from JavaScript/TypeScript.
//! Commands and results use JSON serialization for the first pass.
//!
//! Uses thread_local storage since WASM runs single-threaded in an
//! AudioWorklet context.

use std::cell::RefCell;
use wasm_bindgen::prelude::*;

use crate::commands::{CommandResult, EngineCommand};
use crate::engine::{DspEngine, EngineConfig};

thread_local! {
    static ENGINE: RefCell<Option<DspEngine>> = RefCell::new(None);
}

/// Helper to run a closure with the engine, returning a JSON error if not initialized.
fn with_engine_mut<F>(f: F) -> String
where
    F: FnOnce(&mut DspEngine) -> String,
{
    ENGINE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        match borrow.as_mut() {
            Some(engine) => f(engine),
            None => serde_json::to_string(&CommandResult::error(
                "ENGINE_NOT_INITIALIZED",
                "Call create_engine first",
            ))
            .unwrap_or_default(),
        }
    })
}

fn with_engine_ref<F>(f: F) -> String
where
    F: FnOnce(&DspEngine) -> String,
{
    ENGINE.with(|cell| {
        let borrow = cell.borrow();
        match borrow.as_ref() {
            Some(engine) => f(engine),
            None => r#"{"initialized":false}"#.to_string(),
        }
    })
}

/// Create the engine with a JSON config string.
///
/// Config JSON: `{ "sample_rate": 44100, "max_block_size": 512, "channel_count": 2, "bpm": 120 }`
#[wasm_bindgen]
pub fn create_engine(config_json: &str) -> String {
    let config: EngineConfig = match serde_json::from_str(config_json) {
        Ok(c) => c,
        Err(e) => {
            return serde_json::to_string(&CommandResult::error(
                "INVALID_CONFIG",
                e.to_string(),
            ))
            .unwrap_or_default();
        }
    };

    ENGINE.with(|cell| {
        *cell.borrow_mut() = Some(DspEngine::new(config));
    });

    serde_json::to_string(&CommandResult::ok()).unwrap_or_default()
}

/// Destroy the engine instance.
#[wasm_bindgen]
pub fn destroy_engine() {
    ENGINE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// Handle a JSON command string. Returns a JSON result string.
#[wasm_bindgen]
pub fn handle_command(command_json: &str) -> String {
    with_engine_mut(|engine| {
        let command: EngineCommand = match serde_json::from_str(command_json) {
            Ok(c) => c,
            Err(e) => {
                return serde_json::to_string(&CommandResult::error(
                    "INVALID_COMMAND",
                    e.to_string(),
                ))
                .unwrap_or_default();
            }
        };
        let result = engine.handle_command(command);
        serde_json::to_string(&result).unwrap_or_default()
    })
}

/// Process audio. Fills the provided float buffer with interleaved audio.
///
/// Called from AudioWorkletProcessor.process().
#[wasm_bindgen]
pub fn process_audio(output: &mut [f32], frames: usize) {
    ENGINE.with(|cell| {
        if let Some(engine) = cell.borrow_mut().as_mut() {
            engine.process(output, frames);
        }
        // If no engine, output stays silent (all zeros from JS side)
    });
}

/// Drain pending events as a JSON array string.
#[wasm_bindgen]
pub fn get_events() -> String {
    with_engine_mut(|engine| {
        let events = engine.drain_events();
        serde_json::to_string(&events).unwrap_or_else(|_| "[]".to_string())
    })
}

/// Get engine status as JSON string.
#[wasm_bindgen]
pub fn get_status() -> String {
    with_engine_ref(|engine| {
        let status = engine.get_status();
        serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string())
    })
}
