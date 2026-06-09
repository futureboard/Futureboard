//! MIDI device enumeration and preference merge for Settings → MIDI.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::settings::{MidiDeviceDirection, MidiDeviceSetting, MidiHardwareSettings};

pub fn midi_settings_debug_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_MIDI_SETTINGS_DEBUG").is_some())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedMidiDevice {
    pub id: String,
    pub name: String,
    pub direction: MidiDeviceDirection,
}

fn stable_id(direction: MidiDeviceDirection, name: &str) -> String {
    let slug = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let prefix = match direction {
        MidiDeviceDirection::Input => "midi-in",
        MidiDeviceDirection::Output => "midi-out",
        MidiDeviceDirection::InputOutput => "midi-io",
    };
    format!("{prefix}-{slug}")
}

/// Real MIDI port scan via `midir` (WinMM on Windows, CoreMIDI on macOS, ALSA on
/// Linux). Enumeration only reads port names — it never opens the hardware.
/// Wrapped in `catch_unwind` so a misbehaving backend yields an empty list and a
/// warning rather than taking down the UI thread.
///
/// Prefer [`crate::device_registry::scan_midi`] / `cached_midi_devices` over
/// calling this directly: those cache the result so rendering never re-scans.
pub fn scan_midi_ports() -> Vec<DetectedMidiDevice> {
    match std::panic::catch_unwind(real_scan_midi_ports) {
        Ok(devices) => {
            if midi_settings_debug_enabled() {
                eprintln!("[MIDI settings] detected devices ({})", devices.len());
                for device in &devices {
                    eprintln!(
                        "  - {} ({:?}) id={}",
                        device.name, device.direction, device.id
                    );
                }
            }
            devices
        }
        Err(_) => {
            eprintln!("[MidiDeviceScan] enumeration panicked — returning empty list");
            Vec::new()
        }
    }
}

/// macOS placeholder: midir's CoreMIDI backend currently conflicts with gpui's
/// pinned `core-foundation` version, so the macOS port scan is unavailable. We
/// return an empty list (no mock data) until that dependency pin is reconciled.
#[cfg(target_os = "macos")]
fn real_scan_midi_ports() -> Vec<DetectedMidiDevice> {
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
fn real_scan_midi_ports() -> Vec<DetectedMidiDevice> {
    use midir::{MidiInput, MidiOutput};

    let mut devices = Vec::new();
    match MidiInput::new("Futureboard MIDI scan (in)") {
        Ok(input) => {
            for port in input.ports() {
                if let Ok(name) = input.port_name(&port) {
                    devices.push(DetectedMidiDevice {
                        id: stable_id(MidiDeviceDirection::Input, &name),
                        name,
                        direction: MidiDeviceDirection::Input,
                    });
                }
            }
        }
        Err(e) => eprintln!("[MidiDeviceScan] MIDI input backend unavailable: {e}"),
    }
    match MidiOutput::new("Futureboard MIDI scan (out)") {
        Ok(output) => {
            for port in output.ports() {
                if let Ok(name) = output.port_name(&port) {
                    devices.push(DetectedMidiDevice {
                        id: stable_id(MidiDeviceDirection::Output, &name),
                        name,
                        direction: MidiDeviceDirection::Output,
                    });
                }
            }
        }
        Err(e) => eprintln!("[MidiDeviceScan] MIDI output backend unavailable: {e}"),
    }
    devices
}

/// Detected MIDI devices for rendering. Cheap: returns the registry's cached
/// snapshot from the last scan (run at startup / Refresh) and never re-scans the
/// OS on a hot render path. See [`crate::device_registry`].
pub fn enumerate_midi_devices() -> Vec<DetectedMidiDevice> {
    crate::device_registry::cached_midi_devices()
}

/// Merge saved preferences with freshly detected devices. Saved-only entries stay visible as missing.
pub fn resolve_midi_devices(
    saved: &[MidiDeviceSetting],
    detected: &[DetectedMidiDevice],
) -> Vec<MidiDeviceSetting> {
    if midi_settings_debug_enabled() {
        eprintln!("[MIDI settings] saved preferences ({})", saved.len());
        for device in saved {
            eprintln!(
                "  - {} enabled={} connected={} clock={}",
                device.name, device.enabled, device.connected, device.clock_enabled
            );
        }
    }

    let saved_by_id: HashMap<&str, &MidiDeviceSetting> =
        saved.iter().map(|d| (d.id.as_str(), d)).collect();
    let mut resolved = Vec::new();

    for det in detected {
        let saved = saved_by_id.get(det.id.as_str());
        resolved.push(MidiDeviceSetting {
            id: det.id.clone(),
            name: det.name.clone(),
            direction: det.direction,
            enabled: saved.map(|s| s.enabled).unwrap_or(false),
            connected: true,
            clock_enabled: saved.map(|s| s.clock_enabled).unwrap_or(false),
        });
    }

    for saved_device in saved {
        if detected.iter().any(|d| d.id == saved_device.id) {
            continue;
        }
        if midi_settings_debug_enabled() {
            eprintln!(
                "[MIDI settings] missing saved device: {} ({})",
                saved_device.name, saved_device.id
            );
        }
        resolved.push(MidiDeviceSetting {
            id: saved_device.id.clone(),
            name: saved_device.name.clone(),
            direction: saved_device.direction,
            enabled: saved_device.enabled,
            connected: false,
            clock_enabled: saved_device.clock_enabled,
        });
    }

    resolved
}

pub fn upsert_midi_device(midi: &mut MidiHardwareSettings, device: MidiDeviceSetting) {
    if let Some(existing) = midi.devices.iter_mut().find(|d| d.id == device.id) {
        *existing = device;
    } else {
        midi.devices.push(device);
    }
}

pub fn migrate_legacy_midi_settings(midi: &mut MidiHardwareSettings) {
    if !midi.devices.is_empty() {
        midi.enabled_inputs.clear();
        midi.enabled_outputs.clear();
        return;
    }

    let mut devices = Vec::new();
    for name in &midi.enabled_inputs {
        devices.push(MidiDeviceSetting {
            id: stable_id(MidiDeviceDirection::Input, name),
            name: name.clone(),
            direction: MidiDeviceDirection::Input,
            enabled: true,
            connected: true,
            clock_enabled: false,
        });
    }
    for name in &midi.enabled_outputs {
        devices.push(MidiDeviceSetting {
            id: stable_id(MidiDeviceDirection::Output, name),
            name: name.clone(),
            direction: MidiDeviceDirection::Output,
            enabled: true,
            connected: true,
            clock_enabled: midi.clock_sync,
        });
    }
    if !devices.is_empty() {
        midi.devices = devices;
    }
    midi.enabled_inputs.clear();
    midi.enabled_outputs.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_keeps_missing_saved_devices() {
        let saved = vec![MidiDeviceSetting {
            id: "midi-in-old".to_string(),
            name: "Old Controller".to_string(),
            direction: MidiDeviceDirection::Input,
            enabled: true,
            connected: true,
            clock_enabled: false,
        }];
        let detected = vec![DetectedMidiDevice {
            id: "midi-in-new".to_string(),
            name: "New Controller".to_string(),
            direction: MidiDeviceDirection::Input,
        }];
        let resolved = resolve_midi_devices(&saved, &detected);
        assert_eq!(resolved.len(), 2);
        assert!(resolved
            .iter()
            .any(|d| d.id == "midi-in-old" && !d.connected));
        assert!(resolved
            .iter()
            .any(|d| d.id == "midi-in-new" && d.connected));
    }

    #[test]
    fn migrate_legacy_inputs_outputs() {
        let mut midi = MidiHardwareSettings {
            devices: Vec::new(),
            clock_sync: true,
            enabled_inputs: vec!["Keyboard Controller".to_string()],
            enabled_outputs: vec!["Interface".to_string()],
        };
        migrate_legacy_midi_settings(&mut midi);
        assert_eq!(midi.devices.len(), 2);
        assert!(midi.enabled_inputs.is_empty());
        assert!(midi.enabled_outputs.is_empty());
    }
}
