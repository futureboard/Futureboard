/**
 * WasmAudioProcessor.worklet.js — self-contained AudioWorklet processor.
 *
 * NO import statements — this file must parse cleanly before any browser API
 * is confirmed available.
 *
 * Why this file exists as plain JS instead of using ?worker&url:
 *   Vite's worker bundler inlines wasm-bindgen glue which calls
 *   `new TextDecoder()` / `new TextEncoder()` at the module top level.
 *   Some AudioWorklet contexts on Vercel/Chrome do not expose those globals
 *   at parse time, causing:
 *     ReferenceError: TextEncoder is not defined
 *   By loading this raw file via `new URL('./...worklet.js', import.meta.url)`
 *   we get a true ES-module AudioWorklet with no bundled side-effects.
 *
 * WASM API used (WasmAudioEngine struct, wasm-bindgen naming convention):
 *   Struct `WasmAudioEngine` → export prefix `wasmaudioengine`
 *   Constructor → `wasmaudioengine_new(sr, blockSize, channels, bpm) → ptr`
 *   play/pause/stop  → `wasmaudioengine_{method}(ptr)`
 *   seek_beat/set_bpm → `wasmaudioengine_{method}(ptr, value)`
 *   Getters          → `wasmaudioengine_{getter}(ptr) → number`
 *   process_interleaved → `wasmaudioengine_process_interleaved(ptr, outPtr, outLen, outRef, frames) → i32`
 *
 * All boundary types are numeric or typed-array — TextEncoder/TextDecoder are
 * NOT needed by the WASM interface. They are polyfilled below only as a guard
 * for any future use or for environments that omit them unexpectedly.
 */

// ── TextEncoder / TextDecoder polyfills ───────────────────────────────────────
// Placed at the very top so they are available before any code runs.
// In modern Chrome/Firefox AudioWorkletGlobalScope these already exist;
// the guard just prevents crashes on edge-case environments.

(function _ensureTextEncoding() {
  const g = globalThis;

  if (typeof g.TextEncoder === 'undefined') {
    g.TextEncoder = class MinimalTextEncoder {
      encode(input) {
        const s = String(input == null ? '' : input);
        const out = [];
        for (let i = 0; i < s.length; i++) {
          let c = s.charCodeAt(i);
          if (c < 0x80) {
            out.push(c);
          } else if (c < 0x800) {
            out.push(0xc0 | (c >> 6), 0x80 | (c & 0x3f));
          } else if (c >= 0xd800 && c <= 0xdbff && i + 1 < s.length) {
            const lo = s.charCodeAt(++i);
            const cp = 0x10000 + (((c & 0x3ff) << 10) | (lo & 0x3ff));
            out.push(
              0xf0 | (cp >> 18),
              0x80 | ((cp >> 12) & 0x3f),
              0x80 | ((cp >> 6) & 0x3f),
              0x80 | (cp & 0x3f),
            );
          } else {
            out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 0x3f), 0x80 | (c & 0x3f));
          }
        }
        return new Uint8Array(out);
      }
      encodeInto(src, dest) {
        const buf = this.encode(src);
        const written = Math.min(buf.length, dest.length);
        dest.set(buf.subarray(0, written));
        return { read: src.length, written };
      }
    };
  }

  if (typeof g.TextDecoder === 'undefined') {
    g.TextDecoder = class MinimalTextDecoder {
      decode(bytes) {
        const arr = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes.buffer ?? bytes);
        let out = '';
        let i = 0;
        while (i < arr.length) {
          const c = arr[i++];
          if (c < 0x80) {
            out += String.fromCharCode(c);
          } else if (c < 0xe0) {
            out += String.fromCharCode(((c & 0x1f) << 6) | (arr[i++] & 0x3f));
          } else if (c < 0xf0) {
            const c2 = arr[i++], c3 = arr[i++];
            out += String.fromCharCode(((c & 0x0f) << 12) | ((c2 & 0x3f) << 6) | (c3 & 0x3f));
          } else {
            const c2 = arr[i++], c3 = arr[i++], c4 = arr[i++];
            let cp = ((c & 0x07) << 18) | ((c2 & 0x3f) << 12) | ((c3 & 0x3f) << 6) | (c4 & 0x3f);
            cp -= 0x10000;
            out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
          }
        }
        return out;
      }
    };
  }
})();

// ── WASM runtime state ────────────────────────────────────────────────────────

/** WASM exports object, set after WebAssembly.instantiate. */
let _wasm = null;

/** Pointer to the WasmAudioEngine heap instance (u32). */
let _enginePtr = 0;

/** Scratch: element count from last _writeF32 call. */
let _vecLen = 0;

// ── Float32Array → WASM memory helper ────────────────────────────────────────
// Used only for process_interleaved. No string encoding involved.

function _writeF32(arr) {
  const ptr = _wasm.__wbindgen_malloc(arr.length * 4, 4) >>> 0;
  new Float32Array(_wasm.memory.buffer).set(arr, ptr >>> 2);
  _vecLen = arr.length;
  return ptr;
}

// ── wasm-bindgen import object builder ───────────────────────────────────────
// We inspect the compiled WASM module to find the exact import names before
// instantiation. This handles hash suffixes that may change on rebuild.

function _buildImports(wasmModule) {
  const bg = {
    // Externref slot table — always needed for WasmAudioEngine object lifetime.
    __wbindgen_init_externref_table() {
      const table = _wasm.__wbindgen_externrefs;
      const offset = table.grow(4);
      table.set(0, undefined);
      table.set(offset + 0, undefined);
      table.set(offset + 1, null);
      table.set(offset + 2, true);
      table.set(offset + 3, false);
    },
  };

  // Scan actual module imports so we supply the right name regardless of hash.
  for (const { module: mod, name } of WebAssembly.Module.imports(wasmModule)) {
    if (mod !== './futureboard_core_bg.js') continue;
    if (name in bg) continue; // already provided above

    if (name.includes('copy_to_typed_array')) {
      // Called by WASM to copy processed audio back to the JS Float32Array.
      // arg0 = src ptr in WASM memory, arg1 = byte length, arg2 = dest typed array
      bg[name] = function (srcPtr, srcByteLen, destArr) {
        const src = new Uint8Array(_wasm.memory.buffer, srcPtr >>> 0, srcByteLen);
        new Uint8Array(destArr.buffer, destArr.byteOffset, destArr.byteLength).set(src);
      };
    } else if (name === '__wbindgen_throw') {
      bg[name] = function (ptr, len) {
        // Rust panic reached JS — surface a meaningful error.
        throw new Error('[wasm panic] see browser console for details');
      };
    } else {
      // Provide a stub for any unrecognised import to prevent link errors.
      bg[name] = function () {};
    }
  }

  return { './futureboard_core_bg.js': bg };
}

// ── AudioWorklet Processor ────────────────────────────────────────────────────

class WasmAudioProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this._ready = false;
    this.port.onmessage = this._onMessage.bind(this);
  }

  // ── Message handler ─────────────────────────────────────────────────────────

  async _onMessage({ data: { type, payload } }) {
    if (type === 'init') {
      try {
        // Compile first so we can inspect imports before providing them.
        const wasmModule = await WebAssembly.compile(payload.wasmBytes);
        const imports = _buildImports(wasmModule);
        const instance = await WebAssembly.instantiate(wasmModule, imports);

        _wasm = instance.exports;

        // wasm-bindgen start function sets up the externref table.
        if (typeof _wasm.__wbindgen_start === 'function') {
          _wasm.__wbindgen_start();
        }

        // Construct the engine — all numeric, no strings.
        const { sample_rate, max_block_size, channel_count, bpm } = payload.config;
        _enginePtr = _wasm.wasmaudioengine_new(
          sample_rate,
          max_block_size,
          channel_count,
          bpm,
        );

        this._ready = true;
        this.port.postMessage({ type: 'initialized' });
      } catch (err) {
        this.port.postMessage({ type: 'error', error: String(err) });
      }
    } else if (type === 'command' && this._ready) {
      this._dispatch(payload);
    }
  }

  // ── Command dispatch (all numeric, no JSON parsing) ─────────────────────────

  _dispatch(cmd) {
    try {
      switch (cmd.type) {
        case 'Play':
          if (cmd.position_beat != null) {
            _wasm.wasmaudioengine_seek_beat(_enginePtr, cmd.position_beat);
          }
          _wasm.wasmaudioengine_play(_enginePtr);
          this.port.postMessage({ type: 'events', payload: [{ type: 'PlaybackStarted' }] });
          break;

        case 'Pause':
          _wasm.wasmaudioengine_pause(_enginePtr);
          this.port.postMessage({ type: 'events', payload: [{ type: 'PlaybackPaused' }] });
          break;

        case 'Stop':
          _wasm.wasmaudioengine_stop(_enginePtr);
          this.port.postMessage({ type: 'events', payload: [{ type: 'PlaybackStopped' }] });
          break;

        case 'SeekBeat':
          _wasm.wasmaudioengine_seek_beat(_enginePtr, cmd.beat ?? 0);
          break;

        case 'SetBpm':
          _wasm.wasmaudioengine_set_bpm(_enginePtr, cmd.bpm);
          break;

        case 'SetLoop':
          _wasm.wasmaudioengine_set_loop_enabled(_enginePtr, cmd.enabled ? 1 : 0);
          if (cmd.start_beat != null && cmd.end_beat != null) {
            _wasm.wasmaudioengine_set_loop_range(_enginePtr, cmd.start_beat, cmd.end_beat);
          }
          break;

        default:
          break;
      }
    } catch (_) {
      // Never crash the worklet from a bad command.
    }
  }

  // ── Audio processing ────────────────────────────────────────────────────────

  process(_inputs, outputs) {
    if (!this._ready || !_enginePtr) return true;

    const out = outputs[0];
    if (!out || !out.length) return true;

    const frames = out[0].length;
    const channels = out.length;
    const interleaved = new Float32Array(frames * channels);

    // Fill interleaved buffer via WASM — outputs silence for an empty project.
    try {
      const ptr = _writeF32(interleaved);
      const len = _vecLen;
      // Returns non-zero every ~8 calls while playing → emit position event.
      const emitPos = _wasm.wasmaudioengine_process_interleaved(
        _enginePtr, ptr, len, interleaved, frames,
      );

      // De-interleave into per-channel Web Audio output buffers.
      for (let i = 0; i < frames; i++) {
        for (let c = 0; c < channels; c++) {
          out[c][i] = interleaved[i * channels + c];
        }
      }

      // Throttled transport position event (numeric getters, no JSON in hot path).
      if (emitPos) {
        const beat = _wasm.wasmaudioengine_beat_position(_enginePtr);
        const bpm  = _wasm.wasmaudioengine_bpm(_enginePtr);
        this.port.postMessage({
          type: 'events',
          payload: [{
            type: 'TransportPosition',
            beat,
            time_seconds: (bpm > 0) ? (beat * 60 / bpm) : 0,
          }],
        });
      }
    } catch (_) {
      // On any DSP error: output silence and keep the worklet running.
    }

    return true;
  }
}

registerProcessor('wasm-audio-processor', WasmAudioProcessor);
