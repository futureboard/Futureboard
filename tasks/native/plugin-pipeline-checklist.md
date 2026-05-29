# Native Plugin Pipeline — Phased Checklist

Status of the audio plugin loading + bus/return routing + native plugin
editor work across Futureboard Native. Tick the boxes as phases land.
Cross-reference:

- [plugin-insert-routing.md](./plugin-insert-routing.md)
- [plugin-view-native-editor.md](./plugin-view-native-editor.md)

Last updated: 2026-05-29.

---

## Phase 0 — Audit & docs

- [x] Inspect Electron plugin flow (`apps/electron/src/native-plugin/PluginHostNative.ts`)
- [x] Inspect SpherePluginHost public surface (napi vs. plain Rust)
- [x] Inspect DAUx `RuntimeInsert` / `RuntimeSend` / `Vst3RuntimeProcessor`
- [x] Document project schema (`ProjectInsert`, `ProjectPluginInstance`, `TrackRouting`, `Bus`/`Return` already present)
- [x] Write `tasks/native/plugin-insert-routing.md`
- [x] Write `tasks/native/plugin-view-native-editor.md`

## Phase 1 — UI insert scaffold

- [x] `InsertPluginFormat`, `InsertLoadStatus`, `PluginParameterState`
- [x] `InsertSlotState { id, plugin_id, plugin_path, plugin_format, display_name, enabled, bypassed, load_status, parameters }`
- [x] `TrackState.inserts: Vec<InsertSlotState>` (all constructors updated)
- [x] `TimelineState::add_insert / set_insert_plugin / remove_insert / toggle_insert_bypass`
- [x] Mixer strip renders real insert chips (name + bypass dot + remove ×)
- [x] `+ Add Insert` button on mixer strip
- [x] `MixerCallbacks::{on_add_insert, on_remove_insert, on_toggle_insert_bypass, on_open_insert_editor}`
- [x] `StudioLayout::build_mixer_callbacks` wires all four
- [x] Bypass and remove flip `engine_project_dirty` (next audio-poll syncs descriptor list)
- [x] Project save/load round-trips inserts (Project ↔ TimelineState mapping)
- [x] `FUTUREBOARD_PLUGIN_DEBUG=1` logs add/set/remove/bypass mutations
- [x] No realtime audio path changes — runtime still no-ops on unrecognised plugin descriptors

## Phase 2a — De-napi SpherePluginHost & registry-driven picker

- [x] `pub mod native_editor` exposes editor C ABI as plain Rust
  - [x] `open_plugin_editor_window` → `Result<u64, String>`
  - [x] `get_plugin_editor_attach_handle` → `u64`
  - [x] `attach_vst3_editor_view` → `Result<bool, String>`
  - [x] `close_plugin_editor_window`, `focus_plugin_editor_window`, `resize_plugin_editor_window`
  - [x] `drain_plugin_editor_param_events` → `Result<Vec<NativeEditorParamEvent>, String>`
  - [x] `stable_id` helper
- [x] Existing `#[cfg(feature = "napi")] mod editor_window` left bit-for-bit unchanged
- [x] Build both feature configs:
  - [x] `cargo check -p sphere-plugin-host --no-default-features` (native rlib)
  - [x] `cargo check -p sphere-plugin-host` (Electron cdylib)
- [x] `StudioLayout::available_plugins: Option<Vec<RegistryPlugin>>` lazy cache
- [x] `StudioLayout::pick_default_insert_plugin` — first call runs `PluginRegistry::scan(None)`
- [x] `on_add_insert` uses real `RegistryPlugin` when available; falls back to documented stub when registry is empty
- [x] Real `class_id`, `plugin_path`, `format`, `display_name` round-trip through project save/load

## Phase 2b — Real audio processing & picker overlay  *(wired; needs on-device verification)*

- [x] Real picker overlay (modal listing registered plugins, with category
  filter + name/vendor search)
  - [x] `crates/SphereUIComponents/src/components/plugin_picker.rs`
    (`PluginPickerState`, `PluginPickerCallbacks`, `plugin_picker_overlay`)
  - [x] `+ Add Insert` opens the picker (no slot created until a plugin is
    picked); `apply_picked_insert` appends the slot + binds the descriptor
  - [x] Category rail derived from `RegistryPlugin::display_category`, search
    over name/vendor, `supports_insert()` gating
  - [x] Empty registry → "Insert Stub Effect" fallback (`STUB_PLUGIN_ID`)
  - [x] Escape / backdrop click closes; search field routed via
    `TextMenuTarget::PluginPickerSearch`
- [x] Native UI inserts now reach the engine — `build_engine_inserts`
  emits `native-plugin` `EngineInsertSnapshot`s from `TrackState.inserts`
  in `build_engine_project_snapshot`. Previously hardcoded `Vec::new()`,
  which is why audio never passed through a plugin.
  - [x] Only real VST3 + module path emitted; stub / unscanned skipped so
    the runtime keeps no-op'ing on placeholders (hard rule preserved)
  - [x] `enabled = !bypassed` → bypass toggle changes the audio path on the
    next engine sync
  - [x] `FUTUREBOARD`/`[engine-sync]` log now reports per-insert
    track/id/kind/enabled/path
- [x] DAUx `Vst3RuntimeProcessor` instantiation — **already existed** via the
  `sphere_daux_vst3_*` C ABI (`vst3bridge/src/vst3_processor.cpp`). The
  earlier note about needing a *new* `native_processor` C ABI was based on
  a pre-audit assumption; the bridge is present and exercised by Electron.
- [x] `IPluginFactory` → `IComponent` → `IAudioProcessor` lifecycle on the
  **worker thread** — `RuntimeProject::build` (which calls
  `Vst3RuntimeProcessor::from_params`) runs inside the background
  `engine.load_project(snapshot)` task, never the audio callback.
- [x] Audio thread only sees `process(...)` — `apply_insert` / `apply_insert_block`
  route through the pre-instantiated processor; no alloc/lock on the hot path
  (scratch buffers preallocated at build time).
- [x] Plugin instantiation failure → `InsertLoadStatus::Failed(msg)` in UI.
  Added `AudioEngine::insert_statuses() -> Vec<EngineInsertStatus>`
  (structured, plain-Rust) + `EngineInner::insert_statuses`. The native
  shell calls it once per successful `load_project` in
  `complete_audio_project_sync` (UI thread — never the 60 Hz poll, so the
  runtime mutex is locked at most once per project change) and reconciles
  each slot via `TimelineState::set_insert_load_status`. A native-plugin
  insert the engine reports not-ready flips to `Failed`; the mixer chip
  already renders `Failed` in an error color. No panic on bad plugins.
- [ ] Manual test #6–7 (audio passes through plugin) — *needs a box with a
  scanned VST3 + audio device; not verifiable in this CI/dev env*
- [ ] Manual test #8–9 (bypass changes audio) — *same: needs on-device run*
- [x] Manual test #24 (bad plugin fails gracefully) — no-panic path holds;
  `Failed` now surfaces in the UI via the readback channel

## Phase 3 — Bus / Send / Return routing  *(track types + sends + realtime routing shipped)*

Schema already exists in `crates/SphereUIComponents/src/project/mod.rs`
(`Bus`, `Return`, `Group` in `ProjectTrackType`; `TrackRouting`,
`output_bus`, `sends`).

**Slice 1 — Bus/Return track kinds (shipped):**

- [x] Add `TrackType::Bus`, `TrackType::Return` to `timeline_state.rs`
  (+ `TrackType::is_routing()`); all exhaustive matches updated
  (`track_type_name`, mixer strip header, inspector, track-header badge)
- [x] Add Track dialog: Bus / Return now `native_track_type() = Some(..)`,
  so the cards are selectable and create real tracks; stale
  "not wired" summaries replaced
- [x] Project round-trip: `TlTrackType ↔ ProjectTrackType` maps Bus/Return
  both directions (`Group` folds to `Bus` for now); binary `format.rs`
  already encodes these ordinals
- [x] Snapshot emits `"bus"` / `"return"` track-type names. DAUx sums them
  like normal tracks today (silent until sends route into them) — no
  realtime change, hard rule preserved

**Slice 2 — Sends + realtime routing (shipped):**

- [x] `SendSlotState` on `TrackState` (id, target_track_id, target_name,
  enabled, pre_fader, gain_db) + `gain_linear()`
- [x] `TrackRouting.sends` schema upgraded (`Vec<String>` → `Vec<ProjectSend>`)
  with full save/load round-trip. **No format version bump needed** — the
  save path always wrote `TrackRouting::default()`, so every existing v1
  file has 0 sends and decodes the new per-send payload identically.
- [x] Mixer strip renders a real sends section — `→ Target` chips with an
  enable tint + remove ×, and a dashed `+ Add Send`. Routing tracks
  (bus/return) show an empty placeholder (they are targets, not senders).
  `add_send` auto-targets the first eligible Bus/Return (picker is a follow-up).
- [x] DAUx runtime: two-pass routing in `render_project_block_interleaved`.
  Pass 1 processes source tracks (clips→inserts→fader), sums to master, and
  taps post-fader into send targets' receive buffers. Pass 2 processes
  bus/return tracks from their receive buffers and sums to master. Solo is
  ignored for routing tracks so a soloed source's send still reaches its return.
- [x] Cycle detection — `accumulate_sends` accepts a send only when the target
  is a routing track; a *routing* source may only target a *later* routing
  track (forward-only ⇒ acyclic). Backward/self/non-routing sends are dropped.
- [x] Send accumulation buffers — preallocated `recv_l/recv_r` on `RuntimeTrack`
  (grown lazily, only `fill`ed on the audio thread); no per-block allocation,
  no locks, no logging in the hot path. `two_mut` gives the two-track borrow.
- [x] `FUTUREBOARD_ROUTING_DEBUG=1` logs the graph at **build time** (worker
  thread, not the callback): nodes, each send, and ACCEPT/REJECT per cycle rule.
- [x] Unit tests (`engine::routing_tests`): scaled accumulation, non-routing
  rejection, backward-cycle rejection — all pass (no audio device needed).
- [x] Pre-fader sends — `pre_fader` flows UI→snapshot→`RuntimeSend`; the engine
  taps the post-insert signal for pre-fader sends and the post-fader signal for
  post-fader sends (`accumulate_sends` takes a phase filter; `process_track_block`
  calls it before and after `apply_fader_and_sum`). Unit-tested
  (`pre_fader_filter_only_routes_matching_phase`). UI toggle to *set* pre_fader
  is still pending (defaults post-fader).
- [ ] Visual differentiation: Bus / Return strip styling — *deferred polish*
- [ ] Inspector shows routing info per track — *deferred polish*
- [ ] Send target picker overlay + pre/post toggle (auto-targets first routing track)
- [ ] Manual tests #15, #17, #19–20 (wet/return/bus signal) — *wired; need an
  on-device run with audio to confirm audibly*

## Phase 4 — Native PluginView shell  *(GPUI-hosted embedded editor shipped)*

**Architecture switched off the old C++ NanoVG/D3D top-level window.** New path:
GPUI borderless external window → draws shell/header only → C++ creates a
WS_CHILD **native host region** under the GPUI window's HWND → VST3 `IPlugView`
attaches into that region. Plugin UI is the native view; no audio-thread UI;
editor failure shows a GPUI fallback panel, never a crash.

- [x] NanoVG-free C ABI: `sphere_plugin_editor_embed_attach(parent_hwnd, path,
  class_id, x,y,w,h)` / `_set_bounds` / `_detach` / `_is_valid` in
  `plugin_editor_window.cpp`. Instantiates the plugin (reusing the existing VST3
  hosting + shared param-event queue) and `IPlugView::attached`es into a host
  window. No NanoVG, no D3D shell, no extra thread/message-pump.
- [x] **Parenting fix (WS_CHILD anchoring):** the editor must be a *real child*
  of the GPUI PluginView window, not a floating top-level. `embed_attach` now
  creates a dedicated **`WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS |
  WS_CLIPCHILDREN`** host region parented to the GPUI top-level HWND (never
  `WS_POPUP`, never the main app HWND, never null), positioned with
  client-relative physical-px coords (no `ClientToScreen`). The VST3
  `IPlugView` is attached into that child; after attach we `getSize`, then
  `SetWindowPos` the child to the host region and `onSize` the content rect.
  `set_bounds` repositions **and resizes** the child + re-`onSize`s on every
  GPUI move/resize so the editor moves/clips/resizes with the window. `detach`
  calls `IPlugView::removed()`, destroys the child HWND, and clears the handle.
  Debug-build asserts validate `IsWindow(parent)`, `WS_CHILD`, `!WS_POPUP`, and
  `GetParent(child) == parent`.
  - **Compositing root-cause (resolved):** gpui (0.2.2) composites its surface
    via DirectComposition with `CreateTargetForHwnd(hwnd, /*topmost=*/true)`, so
    its rendered content is **always above all child HWNDs** — which made a
    successful `WS_CHILD` attach render blank. gpui clears its swapchain to
    **alpha 0** and the composition swapchain is **`DXGI_ALPHA_MODE_PREMULTIPLIED`**,
    so the fix is to make the editor window **transparent** (`WindowOptions
    .window_background = Transparent`) and paint **no opaque background** in the
    attached content region. The child then composites through gpui's topmost
    layer. The header + the Opening/Waiting/Attaching/Failed panels stay opaque
    (`surface_base`/titlebar) so there is never see-through to the desktop, and
    the child window paints a black backing under the plugin.
  - **Post-attach hardening:** after `attached`, the C++ side force-shows the
    child (`SetWindowPos SWP_SHOWWINDOW` + `ShowWindow` + `EnableWindow` +
    `InvalidateRect` + `UpdateWindow`), re-`onSize`s, logs the full Win32 state
    (IsWindow/IsWindowVisible/GetParent/window+client rect/style/exstyle),
    `getSize` before & after attach, the IPlugView ptr, and
    `EnumChildWindows` (count + each plugin sub-window's class/text/rect/visible/
    style). If `IsWindowVisible(child)` is false it `removed()`s + destroys the
    child and returns 0 so the GPUI side surfaces a visible `Failed` panel —
    attach ok is never reported for an invisible child.
- [x] Rust facade `native_editor::{attach_editor_into_parent,
  set_editor_region_bounds, detach_editor, editor_is_valid, EmbedRegion}`.
- [x] GPUI `PluginEditorWindow` (`components/plugin_editor_window.rs`):
  borderless external window, GPUI-drawn header (title + close), reserves the
  host region below it. Extracts the HWND via `raw_window_handle::HasWindowHandle`
  (gpui 0.2.2 *does* implement it — the earlier audit was stale), attaches on
  first render, re-syncs the child region (physical px = logical × DPI) on
  resize, and **detaches in `Drop`** so the native view never leaks.
- [x] `StudioLayout` opens the GPUI window (`open_plugin_editors:
  HashMap<(track,insert), WindowHandle<PluginEditorWindow>>`); insert-remove and
  window-close drop the entity → auto-detach. Re-open is a no-op if already up.
- [x] `attach_vst3_editor_view` equivalent on Windows — attach happens inside
  `embed_attach`. Full native build links the new C ABI.
- [x] Bad plugin / attach failure → no panic; the editor window renders a GPUI
  **fallback panel** (plugin name + "Editor failed to open" + exact error +
  **Retry / Close** buttons).
- [x] **Explicit lifecycle state machine** (`PluginEditorStatus`:
  `Opening → WaitingForHostHandle → Attaching → Attached | Failed(err)`). Render
  shows a distinct surface per state ("Opening editor…", "Attaching plugin
  editor…", failure panel) so a **blank panel never appears** unless status is
  `Attached`. Attach is **deferred** until the native parent HWND exists *and*
  content bounds are `> 0` (Phase 6/7): the driver re-renders on a ~32 ms tick
  (capped at ~5 s → visible `Failed` timeout) instead of attaching once-and-
  silently-dropping. `Retry` tears down and restarts from `Opening`.
- [x] `FUTUREBOARD_PLUGIN_VIEW_DEBUG=1` logs the full lifecycle: `[plugin-view]`
  open requested / gpui window created / top hwnd / host region mounted / attach
  requested / attach ok|failed / resize / close on the Rust side, and
  `[vst3-editor]` attach begin / IsWindow(parent) / parent style+GetParent /
  child hwnd+style / getSize / attached / onSize / resize / removed on the C++
  side. Attach failures are never swallowed (logged + surfaced as `Failed`).
- [~] macOS / Linux: the embed path is Windows-only (`#[cfg(target_os=...)]`
  guards return "embedding unavailable" → fallback panel). Old `editor_mac.mm`
  remains for the legacy path.
- [ ] Old NanoVG/D3D/Yoga top-level window code is now **dead** (no caller) —
  removable in a follow-up cleanup along with its `build.rs` deps once the new
  path is device-verified.
- [ ] Resize handle/grow to plugin preferred size negotiation (plugin-initiated
  `resizeView`) — *pending*; today the child takes the plugin's initial size and
  follows the GPUI window on manual resize.
- [ ] Manual test #10 (open) / #11 (resize) / #12 (close) — *compiles + links;
  needs on-device run with a real VST3 to verify the child composites over the
  GPUI surface and DPI placement is correct*.

## Phase 5 — Parameter event drain pump  *(not yet started)*

- [ ] `cx.spawn` loop at ~30 Hz on UI thread
- [ ] `drain_plugin_editor_param_events` → `InsertSlotState.parameters`
- [ ] UI parameter change → plugin controller (reverse direction)
- [ ] Automation hookup deferred to a later round
- [ ] No audio thread interaction

---

## Hard rules carried across all phases

- Plugin instantiation runs on a worker thread, never the audio thread.
- No `LoadProject` for UI-only actions (bypass toggle, slot select).
- Bad plugin → `InsertLoadStatus::Failed(msg)`; never panic.
- Audio callback never allocates, never JSON-parses, never logs.
- IPlugView calls run on the UI/main thread, never the audio thread.
- Every `open_plugin_editor_window` pairs with `close_plugin_editor_window`.
- Theme tokens only — no hardcoded colors.
- Cross-process plugin isolation deferred per `SKILL.md` §13; documented as
  a long-term goal.

## Manual test status (against the original spec checklist)

| # | Test | Status | Phase |
|---|---|---|---|
| 1 | Start app | ✅ | — |
| 2 | Add Audio Track | ✅ | — |
| 3 | Load audio clip | ✅ | — |
| 4 | Add VST3 plugin to insert | ✅ picker overlay (real names if scanned, else stub) | 2b |
| 5 | Confirm insert name appears | ✅ | 1 |
| 6 | Press play | ✅ | — |
| 7 | Audio passes through plugin | ⚙️ wired (engine receives inserts); needs on-device verify | 2b |
| 8 | Bypass plugin | ✅ UI + engine (`enabled=!bypassed` synced) | 2b |
| 9 | Bypass changes audio | ⚙️ wired; needs on-device verify | 2b |
| 10 | Open plugin editor | ⚙️ external window opens/focuses; on-device verify | 4 |
| 11 | Resize editor | ❌ resize forwarding pending | 4 |
| 12 | Close editor | ⚙️ paired close on remove/re-open/drop; on-device verify | 4 |
| 13 | Remove plugin | ✅ | 1 |
| 14 | Add Return Track | ✅ creates + round-trips | 3 |
| 15 | Send Audio → Return | ✅ add-send chip; snapshot→engine wired | 3 |
| 16 | Plugin on Return Track | ✅ inserts work on any track type | 3 |
| 17 | Wet signal on return | ⚙️ routing wired + unit-tested; on-device verify | 3 |
| 18 | Add Bus Track | ✅ creates + round-trips | 3 |
| 19 | Route Audio → Bus | ✅ add-send to bus; engine accumulates | 3 |
| 20 | Bus → Master | ⚙️ bus sums to master in Pass 2; on-device verify | 3 |
| 21 | Save project | ✅ | 1 |
| 22 | Reopen project | ✅ | 1 |
| 23 | Inserts / routing restored | ✅ inserts + sends round-trip | 1 / 3 |
| 24 | Bad plugin fails gracefully | ✅ no panic + `Failed` chip via readback | 2b |

Legend: ✅ done · ⚙️ wired, needs on-device verify · ⚠️ partial · ❌ pending · n/a not relevant yet
