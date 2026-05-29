# PluginView Native Editor Audit

Audit date: 2026-05-29. Maps the existing native plugin editor surface
and identifies what's missing for `futureboard_native` to host VST3
editors via GPUI.

## Existing C ABI

`crates/SpherePluginHost/vst3backend/include/sphere_plugin_host_vst3.h`
exposes a platform-neutral C ABI:

```c
sphere_plugin_editor_open_window(window_id, title, subtitle, w, h) -> u64
sphere_plugin_editor_get_attach_handle(handle) -> u64
sphere_plugin_editor_attach_vst3_view(handle, plugin_path, class_id) -> i32
sphere_plugin_editor_resize_window(handle, w, h)
sphere_plugin_editor_focus_window(handle)
sphere_plugin_editor_close_window(handle)
sphere_plugin_editor_drain_param_events_json() -> SpherePluginHostString
```

The `attach_handle` returned by `get_attach_handle` is the platform
window handle (`HWND` on Windows, `NSView*` on macOS, X11 window id on
Linux) that the IPlugView lifecycle code consumes.

C++ side (`plugin_editor_window.cpp`): owns one `IPlugView`-backed
native window per `handle`. It:
1. Creates a platform-native top-level window.
2. Instantiates the plugin controller from `plugin_path`/`class_id`.
3. Calls `createView(kViewTypeEditor)` → checks `kPlatformTypeHWND`
   (or `kPlatformTypeNSView` on macOS).
4. `attached(parent, platform_type)`.
5. Translates resize/close from the OS event loop into IPlugView calls.

This pathway is already exercised by the Electron app.

## Rust N-API wrapper

`crates/SpherePluginHost/src/editor_window.rs` is ~210 LOC of
`#[napi]`-annotated `napi::Result<...>` adapters over the C ABI. The
unsafe blocks are minimal — each wrapper is a `CString`-conversion + a
single `extern "C"` call.

## Electron flow

Renderer requests an editor → main process calls
`openPluginEditorWindow({windowId, pluginPath, classId, ...})` → C
backend opens a native window owned by the main process, NOT embedded
in any Electron `BrowserWindow`. Param events are drained over IPC at
~30 Hz via `drainPluginEditorParamEvents`.

## Gaps for native GPUI

1. ~~**N-API dependency**~~ — **resolved in Phase 2a**.
   `crates/SpherePluginHost/src/native_editor.rs` exposes the editor
   C ABI as plain Rust (`Result<u64, String>`) and is always-built.
   Native binary can now call:
   - `open_plugin_editor_window(NativeEditorWindowOptions)`
   - `get_plugin_editor_attach_handle(handle)`
   - `attach_vst3_editor_view(handle, plugin_path, class_id)`
   - `close_plugin_editor_window(handle)`,
     `focus_plugin_editor_window`, `resize_plugin_editor_window`
   - `drain_plugin_editor_param_events()`
   The existing `#[cfg(feature = "napi")] mod editor_window` is
   untouched — Electron's `*.node` keeps the same binary surface.

2. **GPUI window → native handle**: Two options:
   a. **External native window** (recommended first): the C backend
      already creates its own top-level window. GPUI doesn't host the
      editor at all; it just owns the lifecycle command + tracks the
      `handle: u64` returned. Same model as Electron. No GPUI internals
      poked. Resize is the OS chrome's responsibility.
   b. **Embedded in GPUI window**: requires extracting `HWND` /
      `NSView*` from `gpui::Window`. GPUI 0.2.2 doesn't expose
      `raw_window_handle()` on its windows. Would need an unsafe
      platform-specific helper in `gpui_macos`/`gpui_windows` (see
      `references/crates/gpui*/`). Deferred — too invasive for Phase 4.

3. **Param event drain pump**: Phase 5. A `cx.spawn` loop at ~30 Hz
   calls `drain_param_events_json`, parses, dispatches into project
   state. No realtime touch — pump runs on UI thread.

## PluginView GPUI shell (Phase 4 design)

```rust
pub struct PluginEditorHost {
    pub editor_id: String,        // u64 from open_window, stringified
    pub track_id: String,
    pub insert_id: String,
    pub plugin_instance_id: String,
    pub native_window_handle: u64,
}

pub enum PluginViewCommand {
    Open { track_id, insert_id },
    Close { editor_id },
    Resize { editor_id, width, height },
    Focus { editor_id },
}
```

Phase 4 ships with the **external native window** approach. The GPUI
PluginView is the *requester*, not the host. Closing the GPUI requester
shell calls `close_window(handle)`; the native window also closes when
the user closes it directly (then `drain_param_events` reports the
detach event and PluginView updates its state).

If `attach_vst3_editor_view` returns a non-zero error code, PluginView
falls back to a GPUI panel showing:
- plugin name (from `ProjectPluginInstance.display_name`)
- attach error code
- bypass + remove buttons
- generic parameter list once param introspection lands

## Phase 1 / 2a scope re: editor

- Phase 1 persists `instance_id`/`plugin_path`/`plugin_uid`/`display_name`
  on `InsertSlotState`.
- Phase 2a removes the N-API dependency from the editor C ABI surface
  via `native_editor`.
- **Phase 4 (revised architecture) shipped** — the old C++ NanoVG/D3D
  top-level editor window is **retired from the path**. New design:
  - GPUI owns a borderless external window (`PluginEditorWindow`) and draws
    only the shell/header.
  - The HWND is extracted via `raw_window_handle::HasWindowHandle` (gpui 0.2.2
    implements it for `Window` — gap #2 above is resolved; option 2b chosen).
  - A new NanoVG-free C ABI (`sphere_plugin_editor_embed_attach/_set_bounds/
    _detach/_is_valid`) creates a `WS_CHILD` native host region under that HWND
    and attaches the VST3 `IPlugView` into it. Plugin UI is the native view.
  - `StudioLayout` tracks `WindowHandle<PluginEditorWindow>` per slot; the
    window entity's `Drop` detaches the native view (no leaks). Attach failure
    renders a GPUI fallback panel — no crash. `FUTUREBOARD_PLUGIN_VIEW_DEBUG`
    traces both sides.
  - The legacy `sphere_plugin_editor_open_window` + NanoVG/Yoga/D3D shell is
    now dead code (no caller), removable in a follow-up cleanup.
  - Pending: plugin-initiated resize negotiation, macOS/Linux embed, Phase 5
    param drain pump, and on-device verification of child compositing + DPI.

See [plugin-pipeline-checklist.md](./plugin-pipeline-checklist.md) for
the live tick-list.

## Hard rules carried forward

- IPlugView calls happen on the UI/main thread, never the audio thread.
- Editor close detaches before dropping the native handle.
- Resize forwards to the IPlugView before changing the OS window.
- Bad plugin / invalid editor size never crashes the app.
- No leaked handles — every `open_window` pairs with `close_window` in
  PluginView's drop path.
- macOS / Linux paths build but may not run until their respective
  IPlugView platform types are fully wired in the C++ backend.
