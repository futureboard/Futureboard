# Play freeze debug — call path and fixes

## Symptom

After `[transport] Play requested` the native UI freezes. No `[playback] starting` line appears, so the hang occurs **before** `engine.debug_snapshot()` / `engine.play()`.

## UI entry points

| Source | File | Handler |
|--------|------|---------|
| Transport Play button | `transport_ops.rs` | `make_command_handler("transport:play-pause")` → `this.update` → `dispatch_command_id` |
| Spacebar | `studio_render.rs` | `capture_key_down` → `shortcut_target.update` → `dispatch_command_id_from_bounds("transport:play-pause")` |
| Command router | `layout.rs` | `dispatch_command_id` → `transport_command_from_id` → `dispatch_transport_command` |
| Play/Pause toggle | `transport_ops.rs` | `TransportCommand::PlayPause` → `start_native_playback` or `stop_native_playback` |

## Transport → audio path

```
dispatch_transport_command(PlayPause)
  └─ start_native_playback(cx)          [audio_transport.rs]
       ├─ (was) ensure_audio_stream_warm()  → AudioEngine::start() → EngineInner::start() → cpal Stream::play()
       ├─ audio_sync_in_flight / engine_project_dirty → schedule_audio_project_sync (async thread) + return
       ├─ sync_transport_controls()     → set_bpm / set_time_signature / set_metronome / set_loop
       ├─ timeline.read (playhead)
       ├─ engine.seek(seconds)
       ├─ engine.debug_snapshot()       → EngineInner::get_debug_info() [locks runtime + project]
       ├─ engine.play()                 → send_command(StartTransport)
       ├─ timeline.update (playing=true)
       └─ poll_native_audio(cx)
```

## Audio engine command path

```
AudioEngine::play() [native.rs]
  └─ EngineInner::play() [engine.rs]
       └─ send_command(StartTransport)
            └─ clone cmd_tx from active_stream (must not hold stream mutex across send)
            └─ crossbeam bounded try_send (512)

cpal callback [cpal_backend.rs]
  └─ drain_commands() [backend/render.rs]
       └─ StartTransport → local.playing_local=true, shared.playing=true, reset_midi_playback
  └─ fill_output_f32() → render when playing_local
```

## Stream lifecycle

| Stage | Where | Notes |
|-------|-------|-------|
| Open | `StudioLayout::new` → `engine.start()` → `open_daux` if needed | Logs `[DAUx] Stream committed` |
| Warm | `engine.start()` → `stream.play()` | `status.running=true`, transport still paused |
| Play | `StartTransport` command | Does **not** call `stream.play()` again on DAUx cpal path |

## Locks and blocking (audit)

| Location | Lock / wait | Risk |
|----------|-------------|------|
| `ensure_audio_stream_warm` before dirty check | `engine.start()` → `stream.play()` on UI | **Could block** if stream already active / driver stall |
| `send_command` (old) | `active_stream.lock()` held during `try_send` | Contention with `open_daux` / `get_daux_status` |
| `get_debug_info` on Play path | `runtime.lock()` + `project.lock()` | Blocks UI if load thread holds `runtime` |
| `schedule_audio_project_sync` | `thread::spawn` + `join` on **background** executor task | Not UI thread |
| `load_project` | `runtime.lock()` on worker thread | Must not run on UI during Play |
| Audio callback | No engine mutex; local `RuntimeProject` | Realtime-safe |

## Play vs LoadProject

- Studio init: `schedule_audio_project_sync(force=true, "studio_init")` → async `load_project`.
- Play must **not** call `load_project` unless `engine_project_dirty || engine_media_dirty`; then it sets `pending_play_after_sync` and schedules async sync only.
- `complete_audio_project_sync` calls `start_native_playback` after pending play.

## Root cause (fixed)

`start_native_playback` called `ensure_audio_stream_warm()` **before** checking `audio_sync_in_flight` / `engine_project_dirty`. With `stats.running == false` (or stale stats) while the project was still dirty, `engine.start()` → `cpal::Stream::play()` could block the UI thread on Windows WASAPI before the early-return sync path ran.

## Fix summary

1. Reorder Play handler: idempotent playing check → sync/dirty gates → then `ensure_audio_stream_warm`.
2. `EngineInner::start()` no-op when `stream_open && running`.
3. `send_command`: clone `Sender` and drop `active_stream` lock before `try_send`.
4. Remove `debug_snapshot()` from the hot Play path (optional env-gated log only).
5. Drop timeline lock before engine calls; avoid `poll_native_audio` at end of Play (poll loop handles it).
6. `FUTUREBOARD_TRANSPORT_FREEZE_DEBUG=1` sequence logs + 500ms watchdog warning.

## Manual test checklist

1. Empty project → Play → UI responsive, silent transport.
2. Stop / Spacebar Play-Pause.
3. Play while `studio_init` sync in flight → queues `pending_play_after_sync`, no freeze.
4. Project with clip + plugin → Play → audio, no freeze.
5. Repeated Play presses → idempotent, no freeze.
