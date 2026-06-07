# VST3 editor hosting — alignment with the SDK `editorhost` sample

Reference: `external/vst3sdk/public.sdk/samples/vst-hosting/editorhost`
(`source/editorhost.cpp`, `source/platform/win32/window.cpp`).

Our host: `crates/SphereDirectAudioEngine/vst3bridge/src/vst3_processor.cpp`
(`sphere_daux_vst3_embed_editor` + helpers), driven by the GPUI shell in
`crates/SphereUIComponents/src/components/plugin_editor_window.rs`.

## What editorhost does (Win32)

1. `controller->createView(kEditor)` → `IPlugView`.
2. `view->getSize(&rect)` **before** creating the window.
3. Create a **plain top-level `HWND`** sized so the *client* area == getSize
   (`AdjustWindowRectEx`), style `WS_CAPTION|WS_SYSMENU|WS_CLIPCHILDREN|WS_CLIPSIBLINGS`
   (+`WS_SIZEBOX|WS_MAXIMIZEBOX` if `canResize`), exStyle `WS_EX_APPWINDOW`, **parent = nullptr**.
4. Window class has **`hbrBackground = nullptr`**; `WM_ERASEBKGND` returns TRUE
   (no fill); `WM_PAINT` is an empty `BeginPaint`/`EndPaint`. **The host never
   paints its client area** — the plug-in owns every pixel.
5. `show()` → `onShow`: `isPlatformTypeSupported(HWND)` → **`setFrame(frame)` →
   `attached(hwnd, kPlatformTypeHWND)`**, then `SetWindowPos(... SWP_SHOWWINDOW)`.
6. The plug-in attaches **directly to the top-level HWND** — there is **no
   intermediate child HWND**; the plug-in creates its own children inside.
7. Resize: `WM_SIZE` → `onResize` → `view->onSize(rect)`. Plug-in-driven resize
   comes back through `IPlugFrame::resizeView` → `window->resize` → `onSize`.
8. Close: `setFrame(nullptr)` → `removed()`.

## What Futureboard does (and the differences)

- We attach to a host HWND that is **owned by / overlaid on the GPUI window**:
  default = owned tool window (`WS_POPUP|WS_EX_TOOLWINDOW`, kind 1); optional
  `WS_CHILD` (kind 0). This exists because GPUI's DirectComposition/D3D swap
  chain composites **over** a `WS_CHILD` host (→ blank). The tool window tracks
  the GPUI content region every frame.
- Lifecycle order already matches the sample: `createView → setFrame → getSize →
  attached → onSize` (see logs `[vst3-editor] createView/setFrame/getSize/attached/onSize`).
- **Key difference:** our editor host window class fills a background
  (`hbrBackground = BLACK_BRUSH`, and `WM_ERASEBKGND` → `FillRect RGB(11,15,20)`),
  whereas the sample paints nothing. When a plug-in's view doesn't paint
  immediately (GPU/GL surfaces, async WebView), our fill is what shows.
- We never offered the sample's **plain standalone top-level window** — every
  mode was tied to the GPUI window/compositor.

## Likely causes of the blank gray window

1. The plug-in's editor attaches but its GPU/GL/WebView surface is composited
   under or clipped by the GPUI overlay/child host (the documented reason the
   default is already the tool window).
2. The host class' own background fill is visible because the plug-in hasn't
   painted yet.

Both are **generic** (no specific plug-in/framework); the fix must not hardcode
vendors.

## Changes made

- Added a generic **detached** host mode (`embed_host_kind == 2`) that
  reproduces the editorhost pattern exactly: a standalone top-level OS window,
  **no background paint** (`hbrBackground=nullptr`, `WM_ERASEBKGND`→1,
  empty `WM_PAINT`), client sized to `getSize`, `setFrame`→`attached(HWND)`,
  `WM_SIZE`→`onSize`, `WM_CLOSE`→shell teardown. Not owned by / composited under
  GPUI. Selected per-run via `FUTUREBOARD_PLUGIN_EDITOR_MODE=detached`
  (also `embedded`→child, `default`/`tool`→owned). **No plug-in hardcoding.**
- The GPUI shell detects detached mode (`embed_host_kind()==2`): it does not
  resize itself to the plug-in or push host bounds, shows an explanatory panel,
  and closes the editor when the detached window is closed (`embed_take_user_close`).
- Added the requested diagnostics (gated/at-open): `sdk_reference=editorhost`,
  `ui_thread_id`, `platform_type=HWND`, `host_mode`, `top_hwnd`, `content_hwnd`,
  `create_tid`, `attach_tid`, plus the existing `setFrame/getSize/attached/onSize`
  result lines.

The existing tool-window/child paths are unchanged (zero regression). If
detached resolves a given plug-in, a per-plug-in `preferred_editor_mode` user
preference (not a source hardcode) is the intended follow-up.
