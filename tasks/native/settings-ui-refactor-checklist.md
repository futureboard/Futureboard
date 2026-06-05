# Settings UI Refactor — Checklist

## Part A — Audit

- [x] Create `tasks/native/settings-ui-refactor-audit.md`
- [x] Document settings files, dropdown impls, duplicate sources, window 0,0 cause

## Part B — Central Settings components

- [x] `settings_components.rs` — SettingsSection, SettingsRow, SettingsComboBox, SettingsToggle, restart UI
- [x] Wire into `components/mod.rs`
- [x] Migrate Plugins / Files / Advanced / About

## Part C — ComboBox fixes

- [x] Dedupe options before render (`dedupe_preserve_order`)
- [x] `FUTUREBOARD_COMBOBOX_DEBUG=1` logging
- [x] Close combo on settings content scroll
- [x] Refresh anchor on each overlay render

## Part D — Page migration (top 4)

- [x] MIDI — BoxListView device lists
- [x] Performance — shared section + restart footer
- [x] Audio — shared SettingsRow / SettingsComboBox
- [x] Appearance — shared rows (theme segmented kept; sliders via SettingsRow)

## Part E — BoxListView

- [x] `box_list_view.rs` exists
- [x] Used by MIDI devices section

## Part F — Window centering

- [x] `window_position.rs` — `centered_window_bounds`, `resolve_owner_bounds`
- [x] Preferences window uses helper
- [x] Add Track, Message Box, Mixer, Plugin Manager, MIDI Editor, Plugin Editor
- [x] `dispatch_command_id` resolves active window bounds
- [x] macOS menu + panel chrome pass bounds
- [x] `FUTUREBOARD_WINDOW_POSITION_DEBUG=1`

## Part G — Restart required UI

- [x] `settings_restart_label`, `settings_restart_footer`
- [x] Performance section uses centralized restart note

## Part H — Manual test checklist

### ComboBox

1. [ ] Preferences → Performance → Renderer dropdown anchored to trigger
2. [ ] No duplicate items in Renderer / GPU Device menus
3. [ ] Select CPU Render, reopen — still 2 items
4. [ ] Resize Preferences — dropdown reclamps
5. [ ] Scroll settings content while combo open — combo closes

### MIDI

1. [ ] Devices shown as BoxListView rows
2. [ ] Toggles aligned; Clock Source ComboBox works

### Audio

1. [ ] Backend / Input / Output ComboBoxes use shared styling
2. [ ] Dropdown anchored correctly

### Window

1. [ ] Preferences opens centered over Studio window
2. [ ] macOS menu Preferences opens centered
3. [ ] Panel chrome / shortcut paths centered
4. [ ] Multi-monitor: move Studio, reopen Preferences

### Build

```bash
cargo check -p sphere_ui_components
cargo clippy -p sphere_ui_components -- -D warnings
```
