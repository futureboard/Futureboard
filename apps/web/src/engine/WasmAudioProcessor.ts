/**
 * WasmAudioProcessor — TypeScript type reference for the AudioWorklet processor.
 *
 * The active implementation is WasmAudioProcessor.worklet.js (plain JS, no imports).
 * That file is loaded by WasmAudioEngineAdapter via:
 *   new URL('./WasmAudioProcessor.worklet.js', import.meta.url)
 *
 * It is not bundled with ?worker&url because doing so inlines the wasm-bindgen
 * JS glue which calls new TextDecoder() at the module top level — a construct
 * that fails in some AudioWorklet bundle environments.
 */

export type WasmInitPayload = {
  wasmBytes: ArrayBuffer;
  config: {
    sample_rate: number;
    max_block_size: number;
    channel_count: number;
    bpm: number;
  };
};

export type WorkletMessage =
  | { type: "init"; payload: WasmInitPayload }
  | { type: "command"; payload: Record<string, unknown> };

export type WorkletResponse =
  | { type: "initialized" }
  | { type: "error"; error: string }
  | { type: "events"; payload: unknown[] }
  | { type: "commandResult"; result: unknown };
