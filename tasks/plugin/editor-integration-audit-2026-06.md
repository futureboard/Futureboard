# PluginEditor / VST3 Integration Audit — Root Causes & Patch Plan (2026-06-10)

Scope: generic VST3 + Win32 + GPUI integration. No vendor-specific logic anywhere in this plan.

Evidence sources: code audit of `SpherePluginHost`, `SphereDirectAudioEngine` (incl. `vst3bridge/vst3_processor.cpp`),
`SphereUIComponents` (layout/plugin_ops, native_editor_shell, plugin_editor_window), `apps/native`, plus one
instrumented run (`target/perf-abc-session.log`, FUTUREBOARD_UI_PERF/NOTIFY_DEBUG/PLUGIN_VIEW_DEBUG/INPUT_DEBUG):

- Main GPUI thread: 43 `[ui-stall]` gaps, worst **3475 ms**; avg frame 28–31 ms, max 440 ms; status bar dipped to **5 fps / 202.8 ms / peak 493 ms** with the editor open.
- Host pump watchdog: 7 `[PluginUIThread] pump gap` events, worst **661 ms** (`suspected_block=ipc_dispatch`), plus 408 ms (ipc_dispatch) and 276 ms (resize_poll).
- Audio producer: `[PluginHost] process timeout … lock_misses=10` during editor open/close (engine bypassed those blocks).
- Bridge meters published garbage: `[plugin-host-bridge] … peak_l=1650.000`.
- Per-frame instrumented render cost was only ~1.4 ms — the other ~29 ms/frame is outside GPUI scopes (present/compositor path).

---

## A. Root causes, ranked

### RC1 — Cross-process synchronous Win32 coupling between the main UI thread and the host UI thread (symptoms 1, 2, 3, 6)

The default bridge editor is a **cross-process WS_CHILD sandwich**:

```
SpherePluginEditorShell (main app, top-level borderless)        native_editor_shell.rs
  └─ SpherePluginEditorContent (main app, WS_CHILD)
       └─ FutureboardDauxVst3EditorDetached (HOST process, WS_CHILD!)   vst3_processor.cpp daux_embed_create_top (kind=0)
            └─ FutureboardDauxVst3EditorContent (HOST, attach HWND for IPlugView)
                 └─ plugin's own children
```

`plugin_host_client.rs:92` forces `FUTUREBOARD_PLUGIN_EDITOR_MODE=child` for the host, so
`daux_embed_resolve_host_kind()` (vst3_processor.cpp:1835) always picks kind=0 in bridge mode.

Cross-process parenting **attaches the input queues** of both UI threads (documented Win32 behavior; the host's own
comment at futureboard_plugin_host.rs:447 acknowledges it). On top of that, several main-thread calls send
**synchronous messages into host-owned windows** and vice versa:

- `NativeEditorShell::ensure_visible_zorder` (native_editor_shell.rs:1945): `RedrawWindow(content, RDW_INVALIDATE|RDW_UPDATENOW|RDW_ALLCHILDREN)` — `RDW_UPDATENOW` + `ALLCHILDREN` delivers WM_PAINT synchronously to the host-owned subtree → main thread blocks until the host pump services it. Called from `mark_attached` and from `drive_bridge_editors` **on every resize event** (plugin_ops.rs:684).
- `apply_shell_layout` (native_editor_shell.rs:449): `SetWindowPos(content, HWND_TOP, …)` runs **unconditionally** (the `changed` flag is computed but only used for logging) on every WM_SIZE / WM_MOVE / WM_WINDOWPOSCHANGED / WM_SHOWWINDOW.
- Host→main: the embed windows are created **without `WS_EX_NOPARENTNOTIFY`** (`daux_embed_create_top`, `daux_embed_create_content_child`), so child create/destroy/click generates `WM_PARENTNOTIFY` sent synchronously up into the main-owned shell — the host thread blocks until the main GPUI thread pumps.

Both threads stall independently (host: `IPlugView::attached`/plugin load inside `ipc_dispatch`, measured 661 ms;
main: 28–500 ms GPUI frames, worst 3.5 s). With queues attached and sync messages crossing both ways, each stall
freezes the *other* side's input → "editor renders but is unclickable", "frozen frame", "editor drags main app down".
NVIDIA machines mostly hide it because the main thread frames are fast; Intel Arc magnifies it (see RC2).

### RC2 — Main app permanently disables DirectComposition and runs the slow present path (symptom 3, amplifier for RC1)

`apps/native/src/main.rs:39-41` force `GPUI_DISABLE_DIRECT_COMPOSITION=1` at boot "(plugin editor HWND embedding)".
`gpui_windows/src/platform.rs:167` then disables DComp presentation for **every GPUI window, always** — but the
*default* editor path no longer needs it: the bridge editor lives in its own native top-level shell, not under the
GPUI window. Only the legacy in-process editor (`FUTUREBOARD_PLUGIN_LEGACY_IN_PROCESS=1`) needs the GPUI compositor
compromise. Measured: ~29 ms/frame outside instrumented scopes on a trivial scene (1 track / 1 clip), i.e. the
present path, with the timeline additionally on "CPU fallback · gpui-paint". On Intel Arc the non-DComp blt present
is exactly the path that hitches; combined with RC1's coupling this becomes a freeze.

### RC3 — One global preview-engine mutex serializes plugin DSP production against editor lifecycle (symptom 4)

In the host process, `PluginHostPreviewEngine` is a single `Mutex`. The audio producer thread
(`run_audio_producer` → `service_audio_bridge`, futureboard_plugin_host.rs:260) takes it per block with a 2 ms
try-lock; the IPC/pump thread holds it across `LoadPlugin` and `embed_editor`/`IPlugView::attached` (hundreds of ms
to seconds). During every editor open/close/load the producer misses blocks (`lock_misses=10` measured), the engine's
freshness guard (plugin_bridge_sink.rs:55) correctly turns that into dry-bypass (effect) / silence (instrument) —
but the audible result of opening an effect editor during playback is a dropout window, and if main↔host stalls
(RC1) extend the lock hold, it can last until the user toggles playback (by which time the attach finished).
The engine-side state machine itself is clean: `LoadProject` preserves a running transport
(render.rs:284-303), the missed-block rules are per-insert and self-recovering, `drain_commands` runs before the
silence early-return. No transport hack is needed — the fix is lock granularity in the host.

### RC4 — Editor-open keeps the full render+meter loop alive at idle, and bridge meters can be garbage (symptom 6, repaint leak)

With any bridge editor open, `bridge_editor_active` keeps the callback rendering while the transport is stopped
(render.rs:686 `bridge_editor_wakeup`) — intended for VSTi on-screen keyboards, but it applies to **effect** tracks
too. The host additionally publishes unvalidated meter peaks (`store_meters` — measured `peak_l=1650.0`). Any
nonzero/garbage meter keeps `apply_engine_meters` → `poll_native_audio` returning `changed=true` → `cx.notify`
("transport") at the meter cap rate → full `StudioLayout` re-render ~30×/s **while idle**, each costing ~30 ms (RC2).
Also `dispatch_editor_event` (plugin_ops.rs:507) ends with an **unconditional** `cx.notify()` for every editor host
event.

### RC5 — No editor-mode isolation policy (symptom 9 / Intel Arc)

Detached mode (kind=2) is fully implemented in C++ (`daux_detached_wnd_proc` with a correct resize/DPI contract) but
only reachable via the `FUTUREBOARD_PLUGIN_EDITOR_MODE` env var, which `sanitize_child_env` **overwrites with
`child`** for the bridge host — detached mode is effectively unreachable in the default path. There is no
`PluginEditorMode` setting and no adapter-based auto policy.

### Verified non-issues (already correct — do not re-fix)

- WM_TIMER: only the two host wake-timer IDs (`0xDA01/0xDA02`) are killed; plugin timers pass through (vst3_processor.cpp:194, 2243).
- `IsDialogMessageW` runs only against real `#32770` ancestors, and unhandled messages are still dispatched (futureboard_plugin_host.rs:1896, native_editor_shell.rs:309).
- No `AttachThreadInput` anywhere.
- Host loop is `MsgWaitForMultipleObjectsEx`-based (8 ms with editors / 50 ms idle), bounded pumps, spin watchdog — not a busy loop.
- Resize/DPI contract: `getSize` is content-size source of truth; `canResize` queried post-attach and locks the wrapper (`WM_GETMINMAXINFO` pin, no resize hit-test edges); `WM_SIZING` constrains through `checkSizeConstraint`; `resizeView` has a recursion guard and falls back to `onSize` only when needed; `AdjustWindowRectExForDpi` + per-monitor-V2 DPI used on both sides. Titlebar/client split is single-sourced in `compute_plugin_shell_layout`.
- Mouse path: `WM_MOUSEACTIVATE → MA_ACTIVATE` everywhere; content `WM_LBUTTONDOWN` focuses the child under the point then falls through to `DefWindowProc`; shell consumes clicks only on its chrome buttons; the shell takes mouse capture only during a chrome-button press.
- Engine transport/graph-swap separation and the `done_seq` freshness guard.

---

## B. Patch plan (small, safe commits)

**Commit 1 — instrumentation only.**
Tag every cross-process-synchronous call site with a timed log (≥ 2 ms): `ensure_visible_zorder`,
`apply_shell_layout`'s SetWindowPos, `shell.focus()`, host `embed_refresh`, `pump_editor_messages`. Add a
`[notify-source]` counter for `dispatch_editor_event`. All behind `FUTUREBOARD_PLUGIN_VIEW_DEBUG`, rate-limited.

**Commit 2 — cut the synchronous cross-process edges (WndProc/message pass-through).**
- vst3_processor.cpp: create the embed top + content child with `WS_EX_NOPARENTNOTIFY`.
- native_editor_shell.rs `apply_shell_layout`: only call `SetWindowPos` when the layout actually `changed`, and use `SWP_NOZORDER` (one explicit raise on attach instead of HWND_TOP every event).
- native_editor_shell.rs `ensure_visible_zorder`: drop `RDW_UPDATENOW | RDW_ALLCHILDREN` (keep async `RDW_INVALIDATE` on the main-owned content only — the host repaints its own subtree from its own pump).
- plugin_ops.rs `drive_bridge_editors`: stop calling `ensure_visible_zorder` per resize event; the shell `WM_SIZE` path already repositions the content child.

**Commit 3 — host lock granularity (pump must never wait on DSP, DSP must never wait on editor).**
Replace the single `PluginHostPreviewEngine` mutex contention on the block path with a lock-free published snapshot:
keep an `arc_swap::ArcSwap<Vec<(instance_id, Vst3RuntimeProcessor)>>` (or `Mutex`-guarded copy swapped on
load/unload only) that `service_audio_bridge` reads without touching the engine mutex. VST3's threading model
already allows `process()` concurrent with editor-thread calls. Result: editor attach/load no longer drops audio
blocks; `ipc_dispatch` pump gaps stop starving the producer. (Keep the freshness guard untouched.)

**Commit 4 — repaint-leak fixes (main app).**
- Sanitize bridge meters: host `store_meters` clamps to finite `[0, 8]`; engine treats non-finite as 0.
- `bridge_editor_wakeup`: scope to tracks whose bridge insert role is `instrument` (generic role param already exists in `engine_snapshot.rs:111`) — an open *effect* editor must not keep the graph rendering while stopped.
- `dispatch_editor_event`: `cx.notify()` only when session state/size actually changed.
- (Optional) meter-driven notify while transport stopped: skip when all smoothed values are 0.

**Commit 5 — audio state verification (no behavior hack).**
With Commit 3 in, re-test "add/open effect during playback": the dropout window should disappear. Add a regression
test in `sphere-direct-audio-engine`: simulate a sink that returns 0 for N blocks then resumes; assert effect output
is dry during the gap and wet after, and that transport state never changes across `LoadProject` while playing.

**Commit 6 — editor isolation mode (`PluginEditorMode`).**
- Add `PluginEditorMode { Auto, Embedded, Detached, Safe }` as a real setting (Settings → Plugins), plumbed to the host via the existing env var instead of hard-coding `child` in `sanitize_child_env`.
- `Auto`: Embedded by default; switch to Detached when (a) the DXGI adapter description matches a known-bad class queried at startup (adapter/driver-based, never plugin-based), or (b) a previous session recorded `EditorUnresponsive`/pump-gap watchdog events for embedded mode (persisted flag).
- Detached mode already exists in C++ (kind=2) — main-app work is accepting `EditorAttached` with a host-owned top-level (no shell content child) and skipping shell-geometry sync, which `plugin_editor_window.rs` already models (`PluginEditorPresentationMode::DetachedNativeWindow`).
- `Safe` maps to the existing `FUTUREBOARD_PLUGIN_EDITOR_SAFE` behavior.

**Commit 7 — stop globally disabling DirectComposition.**
`apps/native/src/main.rs`: set `GPUI_DISABLE_DIRECT_COMPOSITION=1` **only** when `FUTUREBOARD_PLUGIN_LEGACY_IN_PROCESS=1`.
Bridge-mode editors live in separate native windows and do not need the GPUI compositor disabled. Validate on Intel
Arc + NVIDIA + iGPU (this is the riskiest change; it is last and independently revertable).

## C. Validation

```
cargo check -p sphere_plugin_host && cargo build -p sphere_plugin_host --release
cargo check -p sphere-direct-audio-engine && cargo test -p sphere-direct-audio-engine
cargo check -p sphere_ui_components
cargo build --release   # workspace, rebuilds futureboard_native + FutureboardPluginHost-x64
target/release/FutureboardPluginHost-x64.exe --selftest
```

Runtime smoke (each on NVIDIA, Intel iGPU, Intel Arc; FUTUREBOARD_UI_PERF=1 + FUTUREBOARD_PLUGIN_VIEW_DEBUG=1):
1. Main app only — fps/frame_ms baseline, no notify loop at idle.
2. Plugin DSP loaded, editor closed — same as (1); `[AudioRealtime] output_xruns` stable.
3. Editor open (embedded) — fps within ~10% of (1); zero `[PluginUIThread] pump gap`>100 ms at idle; click knobs + open a plugin file dialog; Tab/arrows inside the dialog.
4. Resize editor (resizable + fixed-size plugin) — wrapper lock honored, no blank strips, no pump gaps.
5. Add effect during playback — no audible dropout > 1 block; no transport change; close/reopen editor during playback.
6. Save, reopen project, open editor (restore path).
7. `PluginEditorMode=Detached` on Arc — main app fps unaffected by editor; editor input live while main app busy.

## D. Safety constraints honored

No vendor/plugin-name branches; no transport auto-toggle; no global repaint hacks; all new logs rate-limited or
debug-gated; no busy loops added (all pumps stay bounded + MsgWait-based); no new UI↔audio lock coupling (Commit 3
removes one).
