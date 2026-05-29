# Plugin Insert + Routing Audit (Native)

Audit date: 2026-05-29. Maps what already exists vs. what's missing for
native plugin insert loading and bus/return routing.

## Electron reference flow

`apps/electron/src/native-plugin/PluginHostNative.ts` loads
`PluginHost.node` via `createRequire`. The N-API surface exposed by
`crates/SpherePluginHost` is:

- `initPluginHost()` / `shutdownPluginHost()`
- `scanVst3(paths)` / `scanClap(paths)` / `scanAudioPlugins(paths)`
- `openPluginEditorWindow(options)` / `closePluginEditorWindow(handle)`
- `openPluginEditorForPath(pluginPath)` (legacy path-only entry)
- `getPluginEditorAttachHandle(handle)` returns the native HWND/NSView
- `attachVst3EditorView(handle, plugin_path, class_id)`
- `resizePluginEditorWindow(handle, w, h)` / `focusPluginEditorWindow`
- `drainPluginEditorParamEvents()` → `[{ windowId, paramId, value }]`
- `getBackendVersion()`

In Electron the plugin editor opens in an external native window driven
by the C++ `plugin_editor_window.cpp` backend; the renderer never owns
the editor HWND.

For audio processing Electron uses `SphereAudioNative` (the DAUx N-API
wrapper) which already accepts inserts via `EngineProjectSnapshot.tracks[].inserts`.

## Existing native-side surfaces

**`crates/SpherePluginHost`** — registry, scanner, preset, editor window.
Public API is `napi::Result<…>` everywhere; cannot be linked from the
`futureboard_native` binary as-is (see `apps/native/Cargo.toml` comment
on N-API). The underlying C ABI is platform-neutral:

```c
sphere_plugin_editor_open_window(window_id, title, subtitle, w, h) -> u64
sphere_plugin_editor_get_attach_handle(handle) -> u64
sphere_plugin_editor_attach_vst3_view(handle, plugin_path, class_id) -> i32
sphere_plugin_editor_close_window(handle)
sphere_plugin_editor_resize_window(handle, w, h)
sphere_plugin_editor_focus_window(handle)
sphere_plugin_editor_drain_param_events_json() -> SpherePluginHostString
```

The N-API wrappers in `editor_window.rs` are 5-10 LOC adapters over
these. Factoring into a `sphere-plugin-host-core` crate (Phase 2) means
duplicating the function signatures sans `#[napi]` — small, mechanical.

**`crates/SphereDirectAudioEngine`** (DAUx) — already has:

- `RuntimeInsert`, `RuntimeSend`, `Vst3RuntimeProcessor`
- `apply_insert` / `apply_insert_block` — per-block insert chain
- `apply_track_chain_block` — full track signal path (currently
  per-track only; cross-track summing for sends/bus isn't reviewed yet)
- C++ bridge `vst3bridge/src/vst3_processor.cpp`,
  `editor_mac.mm`, `editor_linux.cpp`
- `EngineProjectSnapshot.tracks[].inserts` accepts plugin descriptors
- Without a real instantiation path the runtime currently bypasses
  unknown processors

**Project format** (`crates/SphereUIComponents/src/project/mod.rs`):

- `ProjectTrackType { Audio, Midi, Instrument, Bus, Return, Group, Master }`
- `ProjectInsert { id, slot_index, bypassed, plugin: Option<ProjectPluginInstance> }`
- `ProjectPluginInstance { instance_id, format, plugin_path, plugin_uid, display_name, state }`
- `TrackRouting { output_bus, sends }`
- `ProjectTrack.inserts: Vec<ProjectInsert>`

Disk schema is complete. Backward-compatible: new fields default to
empty/None.

## Gap list

1. `TimelineState::TrackState` has no `inserts` field — UI can't reflect
   loaded plugins on the mixer strip.
2. No `MixerCommand::LoadPluginInsert` / `RemoveInsert` /
   `SetInsertBypass` / `OpenInsertEditor` dispatched from the UI.
3. `SpherePluginHost` only exposes N-API; native binary can't call it.
   Phase 2 splits it.
4. `Vst3RuntimeProcessor` constructor wiring vs. an actual plugin
   instantiation call — needs Phase 2's de-napi'd host core.
5. Bus/Return track kinds don't exist in `timeline_state::TrackType`
   (Audio/Midi/Instrument/Master only). Add Track dialog doesn't offer
   them.
6. Routing graph in `engine.rs::apply_track_chain_block` doesn't yet
   walk cross-track sends → bus/return topology. Cycle detection TBD.

## Phase 1 — shipped (UI insert scaffold)

UI scaffold only. No runtime changes.

- `InsertSlotState`, `InsertLoadStatus`, `PluginParameterState`,
  `InsertPluginFormat` added to `timeline_state.rs`.
- `TrackState.inserts: Vec<InsertSlotState>` (all 4 constructors updated).
- `TimelineState::{add_insert, set_insert_plugin, remove_insert,
  toggle_insert_bypass}`.
- 4 new `MixerCallbacks` (`on_add_insert`, `on_remove_insert`,
  `on_toggle_insert_bypass`, `on_open_insert_editor`).
- Mixer strip renders insert chips + dashed `+` button.
- Project save/load round-trips inserts via the existing
  `ProjectInsert` + `ProjectPluginInstance` disk schema.
- `FUTUREBOARD_PLUGIN_DEBUG=1` traces mutations.
- Bypass / remove / add flip `engine_project_dirty` so DAUx receives
  the new descriptor list on next audio-poll sync (no `LoadProject`
  emitted from the UI directly).

## Phase 2a — shipped (de-napi editor + registry picker)

- `pub mod native_editor` exposes the editor C ABI as plain Rust
  (`Result<_, String>`); the `#[cfg(feature = "napi")]` wrapper is
  untouched.
- Native binary now links the rlib build of `sphere-plugin-host` and
  can call the editor lifecycle directly — Phase 4 unblocked.
- `StudioLayout::pick_default_insert_plugin` lazily populates a
  `Vec<RegistryPlugin>` cache via `PluginRegistry::scan(None)` on
  first `+ Add Insert`. Real plugin name + class_id + path now flow
  through `set_insert_plugin` and round-trip with the project.
- When the registry is empty (no plugins scanned, clean dev box), the
  documented `"futureboard.stub.gain"` fallback continues to be
  inserted so the round-trip is exercisable everywhere.

## Later phase TODOs

See [plugin-pipeline-checklist.md](./plugin-pipeline-checklist.md)
for the live tick-list.

- **Phase 2b**: **shipped** — picker overlay (`plugin_picker.rs`) +
  real audio wiring. The DAUx `sphere_daux_vst3_*` bridge and
  `Vst3RuntimeProcessor` instantiation already existed; the actual native
  gap was `build_engine_project_snapshot` hardcoding `inserts: Vec::new()`.
  Now `build_engine_inserts` emits `native-plugin` descriptors so DAUx
  instantiates the processor on its `load_project` worker and routes audio
  through it (`enabled = !bypassed`). Status readback **shipped** too:
  `AudioEngine::insert_statuses()` → `complete_audio_project_sync`
  reconciles `InsertLoadStatus::Failed`. Remaining: on-device verification
  of tests #6–9.
- **Phase 3**: **shipped** — `TrackType::Bus`/`Return`, `SendSlotState` +
  `TrackRouting`→`ProjectSend` round-trip, mixer sends UI (add/remove), and
  the DAUx two-pass realtime routing in `render_project_block_interleaved`
  (post-fader send taps → `recv_*` buffers → bus/return processing → master)
  with forward-only cycle-safe send rules and `FUTUREBOARD_ROUTING_DEBUG`
  build-time graph logging. Unit-tested in `engine::routing_tests`.
  Remaining polish: pre-fader sends, send target picker, bus/return strip
  styling, inspector routing readout, on-device audio verification.
- **Phase 4**: GPUI shell consumes `native_editor::*` for external
  plugin windows; HWND / NSView path; fallback panel on attach
  failure.
- **Phase 5**: param event drain pump (~30 Hz, UI thread).

## Hard rules carried forward

- Plugin instantiation runs on a worker thread, never the audio thread.
- No `LoadProject` for UI-only actions (bypass toggle, slot select).
- Bad plugin → `InsertLoadStatus::Failed(msg)`; never panic.
- Audio callback never allocates, never JSON-parses, never logs.
- Cross-process plugin isolation deferred per `SKILL.md` §13.
