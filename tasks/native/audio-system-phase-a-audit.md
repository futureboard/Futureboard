# Phase A — Audio Architecture Audit

Status: **audit / gap document**. No engine code is changed by this task.
This is the Phase A deliverable required by `audio-system-plan.md` §20–21
("gap document exists before engine changes").

Audited crate: `crates/SphereDirectAudioEngine` (lib name `DAUx`).
Audited at commit on branch `main` (post `midi-editor-tools-and-gpu-warmup`).

---

## A.1 What already exists (inventory)

The native engine is **substantially further along than the spec's greenfield
framing**. Current modules:

| Area | File(s) | State |
| --- | --- | --- |
| N-API surface | `lib.rs` | `SphereDirectAudioEngine` JS class: device/transport/project/meters/recording/editor. Guarded by `feature = "napi"`. |
| Native (non-NAPI) facade | `native.rs` | `AudioEngine` for GPUI shell; shares one `EngineInner`. No NAPI symbols. |
| Engine core | `engine.rs` (2631 ln) | `EngineInner`, shared atomics, cpal callback, block + per-sample render kernels. |
| Runtime snapshot | `runtime.rs` (1603 ln) | `RuntimeProject` / track / clip / insert / send / automation / MIDI runtime. Built off-callback. |
| Commands | `command.rs` | `EngineCommand` enum (transport, mixer, insert param, tempo/metronome). |
| Snapshot types | `types.rs` | `EngineProjectSnapshot` + JSON-serde DTOs from JS. |
| Device backends | `backend/` | cpal (shared), WASAPI exclusive event-driven, MMCSS, offline render. |
| Device enum | `device/mod.rs` | input/output device listing. |
| DSP | `dsp/` | gain/soft-limit, peak meter smoothing, sine oscillator (test tone/metronome). |
| Media | `audio_file.rs`, `audio_source.rs` | symphonia decode, mmap WAV streaming, peak/LOD generation. |
| Recording | `recording.rs` | input cpal stream → lock-free channel → disk writer thread. |
| Plugins | `vst3_processor.rs` + `SphereAudioPlugins` crate | VST3 runtime processor, MIDI events, editor attach handles. |

**Checklist items already covered (representative, not exhaustive):**
Realtime snapshot built off-callback; atomic master volume; per-track meters via
atomics; equal-power-ish pan + soft limit; WAV/AIFF/FLAC/MP3 decode (symphonia);
mmap large-file streaming; peak/LOD waveform generation; WASAPI shared +
exclusive; MMCSS; transport play/stop/seek; metronome + time signature; post/pre-
fader sends; bus/return routing; read-mode automation (vol/pan/mute/plugin/send);
MIDI clip note + CC scheduling with all-notes-off panic; VST3 insert
instantiate/bypass/param/editor; audio + MIDI recording; offline render path.

This means most of Phases C–E, I, J–L, M–O, P–R, U partially exist. The work
ahead is **hardening + closing the realtime-safety and validation gaps**, not
building from zero.

---

## A.2 Realtime-safety gaps (the important findings)

The spec's non-negotiable contract (`audio-system-spec.md` §1) forbids
allocation, locks, file I/O, logging, and graph rebuild in the callback. The
current callback **violates this in specific, fixable places**:

### A.2.1 Old graph is dropped *inside* the audio callback — **HIGH**
`engine.rs:2136` and `backend/render.rs:183`:
```rust
EngineCommand::LoadProject(next_runtime) => {
    runtime = next_runtime;   // old RuntimeProject Drop runs HERE, on the audio thread
}
```
Assigning the new runtime drops the previous `RuntimeProject` on the audio
thread. That `Drop` frees every track's `block_*`/`recv_*`/scratch `Vec`, decoded
`Arc<ClipAudioSource>` (potentially the last ref → munmap/free), and VST3
processor handles. This is **heap deallocation (and possibly munmap/C++ destroy)
in the realtime callback** — and directly contradicts spec §3 "Old graph is
dropped only when the callback can no longer observe it."
*Fix direction:* hand the old value back to the control thread for dropping
(e.g. push into a "graveyard" channel the control thread drains), or use a
triple-buffer / `arc-swap`-style publish.

### A.2.2 Unconditional `eprintln!` in the callback — **HIGH**
- `engine.rs:2130` `LoadProject` log — not behind any debug flag.
- `engine.rs:2223` / `render.rs:247` `SetTrackMute` log — fires on every mute
  toggle, on the audio thread.
- `render.rs:177` `LoadProject` log in the shared drain path.
- `render.rs:305` / `engine.rs` `renderPath=` first-block log (one-shot, lower risk).

String formatting + stdio write in the callback violates spec §1 "format strings
or log per block." `LoadProject`/mute happen on user action, not per block, but
they still execute on the realtime thread. *Fix direction:* gate all callback
`eprintln!` behind the cached `FUTUREBOARD_AUDIO_CALLBACK_DEBUG` flag routed
through a ring buffer (plan §18 already specifies this), or move logging to the
control thread.

### A.2.3 Lazy buffer `resize` in the render kernel — **MEDIUM**
`engine.rs:1561-1574`: `track.block_l.resize(frames, ...)` / `recv_*.resize`.
Buffers are preallocated to `DEFAULT_AUDIO_BLOCK_CAPACITY = 8192`, so this only
allocates if a block exceeds 8192 frames — but when it does, it allocates in the
callback. *Fix direction:* clamp/validate max block size at device-open time and
treat an oversized block as a diagnostic underrun rather than allocating.

### A.2.4 Per-sample heap clones in `render_project_sample` — **MEDIUM**
`engine.rs:1269,1300`: inside the clip loop, `clip.track_id.clone()` (String) and
`runtime.tracks[track_index].sends.clone()` (Vec) allocate **per active clip**.
This is the per-sample fallback path (mono / non-stereo / `channels < 2`); the
primary stereo path is `render_project_block_interleaved`, which is allocation-
free. Still a latent violation for any device that lands on the per-sample path.
*Fix direction:* restructure to borrow/clone-free, or drop the per-sample path
once block rendering covers all channel layouts.

### A.2.5 `O(n)` String-keyed lookups in the hot loop — **MEDIUM (perf, not alloc)**
Throughout the render kernels: `runtime.tracks.iter().position(|t| t.id == ...)`
and send/output resolution by String compare, per clip, per block. Spec §3
requires "stable numeric handles." Not an allocation, but scales poorly (1000
clips × 100 tracks stress targets). *Fix direction:* resolve String IDs → dense
indices once at snapshot-build time; store `usize`/newtype handles in
`RuntimeClip`/`RuntimeSend`.

---

## A.3 Architecture / validation gaps

### A.3.1 No real graph publish/swap primitive
Graph swap is "assign in callback" (see A.2.1). There is no documented atomic
block-boundary swap, no "keep previous valid graph on build failure" path, and
no graveyard for deferred drops. Spec §3 / plan §7 require all three.

### A.3.2 Routing cycle handling is heuristic, not validation
Cycle safety is enforced two ways today: a build-time `FUTUREBOARD_ROUTING_DEBUG`
log (`runtime.rs:665`) and a render-time array-order guard (`accumulate_sends`,
`engine.rs:1526` "routing source may only target a later routing track"). There
is **no topological sort and no pre-swap rejection** of invalid graphs with UI
feedback (plan §8 / spec §5 / Phase O). Invalid routes are silently dropped at
render time rather than rejected before publish.

### A.3.3 Constant-tempo only
`samples_per_beat` is a single scalar at snapshot BPM (`runtime.rs:377`). Tempo
map (plan §12 / Phase T) is a documented TODO. MIDI + clip scheduling resolve to
absolute samples at build time, so tempo changes require a full rebuild.

### A.3.4 Disk streaming is partial
Large WAV uses mmap (`audio_source.rs`) and compressed formats decode fully into
memory (`MAX_IN_MEMORY_DECODE_BYTES`). There is **no ring-buffer prefetch worker
per active source** and **no underrun counter** as Phase F / spec §7 require —
long compressed files rely on full in-memory decode.

### A.3.5 Diagnostics counters mostly absent
`JsDauxStatus.glitch_count` exists, but the spec §14 counter set (callback CPU
load, graph node count, active plugin count, disk underruns, missing-media count,
meter-queue drops, command-queue overflows) is not yet surfaced. The debug env
flags in plan §18 are only partially wired (`FUTUREBOARD_AUDIO_COMMAND_DEBUG`,
`FUTUREBOARD_ROUTING_DEBUG`, `FUTUREBOARD_MIDI_ENGINE_DEBUG` exist; the audio /
callback / waveform / disk-stream / automation / transport flags do not all).

### A.3.6 Command queue overflow policy undefined
Commands cross via a crossbeam channel drained with `try_recv()` (good). But
there is no documented bounded capacity or overflow/drop policy (spec §1 forbids
"unbounded channels"; plan §Y wants overflow handling). Need to confirm the
channel is bounded and define behavior when full.

### A.3.7 Plugin host de-NAPI: in good shape
`native.rs` already gives the GPUI shell a NAPI-free `AudioEngine`, and the crate
splits `rlib`/`cdylib` with `#[cfg(feature = "napi")]` guards. Phase K is largely
satisfied; remaining work is verifying no NAPI symbols leak into the native
binary (build check) — not a redesign.

### A.3.8 Scanner process isolation: out of this crate
Out-of-process VST3 scanner + crash blacklist (plan §10, checklist "Plugin Host
Architecture") lives in `SpherePluginHost`, not here. Flagged as a cross-crate
dependency for Phase J/K hardening, not a `SphereDirectAudioEngine` gap.

---

## A.4 Recommended next slices (after this audit)

Per plan §21 the next phase is **B — Realtime Safety Foundation**. Based on the
findings above, B should concretely deliver:

1. **Deferred-drop graveyard** for old `RuntimeProject` on `LoadProject`
   (fixes A.2.1) — highest-value, smallest-surface realtime fix.
2. **Gate every callback `eprintln!`** behind a cached ring-buffered debug flag
   (fixes A.2.2).
3. **Document + assert max block size** at device open; remove the in-callback
   `resize` path (fixes A.2.3).
4. Write the **realtime-safety contract doc** + a debug-build assertion harness
   (an allocation guard around the callback) so future work has a checkable
   contract. This is the actual Phase B acceptance criterion.

Then Phase D (handle-based snapshot, fixes A.2.4/A.2.5) and Phase O (real cycle
validation, fixes A.3.2).

Do **not** bundle these into one patch — each is its own scoped task with a
`cargo check -p sphere-direct-audio-engine` gate.

---

## A.5 Validation performed for this audit

- Read: `lib.rs`, `command.rs`, `types.rs`, `runtime.rs`, `native.rs`,
  `backend/render.rs`, and the render + callback-drain sections of `engine.rs`.
- No code changed. No build run (audit only).
- Findings cite `file:line` against the audited tree; re-verify line numbers
  before acting, as the engine files are actively edited.
