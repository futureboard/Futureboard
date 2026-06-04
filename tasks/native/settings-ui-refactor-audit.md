# Settings UI Refactor — Audit

Date: 2026-06-04

## Settings / Preferences entry points

| File | Role |
|------|------|
| `crates/SphereUIComponents/src/components/settings_dialog.rs` | Main Preferences window (`SettingsWindow`), all tab content, hardware ComboBox overlay |
| `crates/SphereUIComponents/src/components/settings_layout.rs` | Legacy layout primitives (`settings_daw_row`, section cards, nav) |
| `crates/SphereUIComponents/src/components/box_list_view.rs` | Reusable device list rows (MIDI devices) |
| `crates/SphereUIComponents/src/settings.rs` | Settings schema + persistence (`SettingsModel`) |
| `crates/SphereUIComponents/src/midi_devices.rs` | MIDI enumeration + preference merge |
| `crates/SphereUIComponents/src/layout/window_ops.rs` | Opens Preferences, Add Track, Mixer, Plugin Manager |
| `crates/SphereUIComponents/src/layout.rs` | `dispatch_command_id` routes menu/shortcut commands |
| `crates/SphereUIComponents/src/layout/studio_render.rs` | Menu + keyboard dispatch with `window.bounds()` |
| `crates/SphereUIComponents/src/platform_chrome.rs` | macOS native menu dispatch (**no owner bounds**) |

There is no separate `preferences.rs` — all native Preferences UI lives in `settings_dialog.rs`.

## Settings tabs (pages)

All rendered in `build_settings_content()` inside `settings_dialog.rs`:

- General, Audio, MIDI, Recording, Playback, Editing, Appearance, Plugins, Files & Media, Shortcuts, Performance, Advanced, About

## Custom select / dropdown implementations

| Component | File | Used by |
|-----------|------|---------|
| `combo_box_trigger` + `combo_box_string_menu` | `combo_box.rs` | Settings hardware combos, Inspector MIDI routing |
| `hardware_select` + `hardware_combo_overlay` | `settings_dialog.rs` | All Preferences ComboBoxes |
| `menu_dropdown` | `menu_dropdown.rs` | Top menu bar (not settings) |
| `context_menu` | `context_menu.rs` | Timeline/browser (not settings) |
| `fb_segmented_button` | `controls.rs` | Appearance theme (segmented, not ComboBox) |

## Dropdown positioning (current)

1. Click on ComboBox trigger → `form_combo_trigger_bounds()` estimates bounds from:
   - `settings_form_column()` — sidebar + label width math from **window width**
   - Mouse `event.position.y` — window coordinates
2. Anchor stored in `SettingsWindow.hardware_combo_anchor`
3. Each render: `refresh_form_anchor()` updates horizontal geometry only (not scroll Y)
4. Menu rendered as `absolute()` sibling at root of Preferences window

### Known positioning bugs

- **Scroll drift**: Content scrolls in `#settings-content-scroll` but anchor Y is frozen at open time → menu floats away from trigger after scroll.
- **Estimated X column**: Trigger left edge is computed from layout constants, not measured element bounds — usually OK for Preferences but fragile if padding changes.
- **No recomputation on resize while open**: Horizontal refresh runs via `refresh_form_anchor`; vertical anchor can still be stale after scroll.

## Duplicate ComboBox items — root causes

| Source | Mechanism | Risk |
|--------|-----------|------|
| `window_ops.rs` | Pushes saved audio in/out device into `available_*` lists on each open | Low — guarded by `.contains()` |
| `HardwareCombo::GpuDevice` | Builds options from `list_available_gpu_devices()` each overlay open | Medium if enumeration returns duplicates |
| `combo_box_string_menu` | Renders options slice as-is — **no dedup** | High if upstream vec has dupes |
| Render-time mutation | No evidence of `push` during render in settings overlay | Low |

Suspicious pattern: passing `available_inputs`/`available_outputs` slices that may contain same name twice if engine list + saved merge both add without dedup (contains check should prevent).

**Fix applied**: `dedupe_preserve_order()` in `combo_box.rs` before rendering + `FUTUREBOARD_COMBOBOX_DEBUG=1` duplicate detection.

## Pages using ad-hoc controls (pre-refactor)

| Page | Issue |
|------|-------|
| MIDI | ~~Loose checkbox device rows~~ → **BoxListView** (done) |
| Performance | Manual restart footer string; ComboBox via `hardware_select` |
| Audio | ComboBox rows OK; latency section uses custom readouts |
| Appearance | Theme uses **segmented buttons** instead of ComboBox; sliders ad-hoc |
| General | Checkboxes for toggles (start screen, updates) |
| Plugins | Path list + buttons (not BoxListView yet) |

## Window creation — Preferences at 0,0

| Opener | Bounds source |
|--------|---------------|
| Menu command | `window.bounds()` (global) ✓ |
| Keyboard shortcut | `window.bounds()` ✓ |
| `dispatch_command_id()` (no bounds) | Falls back to `origin: 0,0`, size 1400×900 |
| macOS native menu | `dispatch_command_id()` → **no bounds** ✗ |
| Panel chrome handlers | `dispatch_command_id()` → **no bounds** ✗ |

`open_settings_window()` centers child over `owner_bounds`. When parent origin is `(0,0)` with synthetic 1400×900 size, child opens at ~(310,170) screen space — **not** centered on main Studio window or monitor.

**Fix**: `resolve_owner_bounds()` uses `cx.active_window()` when bounds missing; `centered_window_bounds()` clamps to monitor work area.

## External windows using manual centering (duplicated logic)

- `settings_dialog.rs` — `open_settings_window`
- `add_track_dialog.rs` — `open_add_track_window`
- `message_box_dialog.rs` — `open_message_box_window`
- `plugin_manager.rs`, `mixer_window.rs`, `midi_editor_window.rs`, `plugin_editor_window.rs`

All use copy-pasted `(parent_x + (parent_w - width) / 2)` without work-area clamp.

## ComboBox elsewhere

Inspector MIDI routing (`panel.rs`) uses `inspector_combo_trigger_bounds` — separate anchor path, must not break.
