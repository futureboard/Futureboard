//! Futureboard MIDI service layer.
//!
//! This crate owns MIDI data types, MIDI device enumeration, preference merge,
//! and UI/control-path MIDI event primitives. UI crates should depend on this
//! crate instead of carrying MIDI service state internally.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::thread::JoinHandle;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MidiDeviceDirection {
    Input,
    Output,
    InputOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MidiDeviceSetting {
    pub id: String,
    pub name: String,
    pub direction: MidiDeviceDirection,
    pub enabled: bool,
    pub connected: bool,
    #[serde(default)]
    pub clock_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct MidiHardwareSettings {
    #[serde(default)]
    pub devices: Vec<MidiDeviceSetting>,
    pub clock_sync: bool,
    /// Legacy — migrated into [`devices`] on load.
    #[serde(default, skip_serializing)]
    pub enabled_inputs: Vec<String>,
    #[serde(default, skip_serializing)]
    pub enabled_outputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedMidiDevice {
    pub id: String,
    pub name: String,
    pub direction: MidiDeviceDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiInputSource {
    Hardware,
    PianoRollPreview,
    VirtualKeyboard,
    DawRemote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualKeyboardEvent {
    NoteOn { note: u8, velocity: u8, channel: u8 },
    NoteOff { note: u8, channel: u8 },
    Sustain { down: bool, channel: u8 },
    Panic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiInputEvent {
    NoteOn {
        note: u8,
        velocity: u8,
        channel: u8,
    },
    NoteOff {
        note: u8,
        channel: u8,
    },
    ControlChange {
        controller: u8,
        value: u8,
        channel: u8,
    },
    AllNotesOff,
    Panic,
}

impl From<VirtualKeyboardEvent> for MidiInputEvent {
    fn from(event: VirtualKeyboardEvent) -> Self {
        match event {
            VirtualKeyboardEvent::NoteOn {
                note,
                velocity,
                channel,
            } => Self::NoteOn {
                note,
                velocity,
                channel,
            },
            VirtualKeyboardEvent::NoteOff { note, channel } => Self::NoteOff { note, channel },
            VirtualKeyboardEvent::Sustain { down, channel } => Self::ControlChange {
                controller: 64,
                value: if down { 127 } else { 0 },
                channel,
            },
            VirtualKeyboardEvent::Panic => Self::Panic,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiInputTarget {
    pub track_id: String,
    pub plugin_instance_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiInputRouteStatus {
    Routed,
    NoTarget,
    EngineUnavailable,
    DispatchFailed(String),
}

pub struct MidiInputRouter;

impl MidiInputRouter {
    pub fn sanitize_channel(channel: u8) -> u8 {
        channel.min(15)
    }

    pub fn sanitize_note(note: u8) -> u8 {
        note.min(127)
    }

    pub fn sanitize_velocity(velocity: u8) -> u8 {
        velocity.clamp(1, 127)
    }
}

pub fn midi_settings_debug_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_MIDI_SETTINGS_DEBUG").is_some())
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

#[derive(Debug, Clone, PartialEq)]
pub struct HardwareMidiEvent {
    /// MIDI output device id or display name. The GPUI routing UI currently
    /// stores the display name; matching accepts either for migration safety.
    pub device_id: String,
    /// Seconds after transport start at which this event should be sent.
    pub delay_seconds: f64,
    /// Raw MIDI bytes, e.g. [0x90 | channel, pitch, velocity].
    pub message: Vec<u8>,
}

pub struct HardwareMidiPlayback {
    cancel_tx: Option<std::sync::mpsc::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl Default for HardwareMidiPlayback {
    fn default() -> Self {
        Self::new()
    }
}

impl HardwareMidiPlayback {
    pub fn new() -> Self {
        Self {
            cancel_tx: None,
            handle: None,
        }
    }

    pub fn start(&mut self, mut events: Vec<HardwareMidiEvent>) {
        self.stop();
        if events.is_empty() {
            return;
        }
        events.sort_by(|a, b| a.delay_seconds.total_cmp(&b.delay_seconds));
        let (cancel_tx, cancel_rx) = std::sync::mpsc::channel();
        self.cancel_tx = Some(cancel_tx);
        self.handle = Some(spawn_hardware_midi_thread(events, cancel_rx));
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for HardwareMidiPlayback {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(not(target_os = "macos"))]
fn spawn_hardware_midi_thread(
    events: Vec<HardwareMidiEvent>,
    cancel_rx: std::sync::mpsc::Receiver<()>,
) -> JoinHandle<()> {
    std::thread::spawn(move || run_hardware_midi_thread(events, cancel_rx))
}

#[cfg(target_os = "macos")]
fn spawn_hardware_midi_thread(
    _events: Vec<HardwareMidiEvent>,
    _cancel_rx: std::sync::mpsc::Receiver<()>,
) -> JoinHandle<()> {
    std::thread::spawn(|| {})
}

#[cfg(not(target_os = "macos"))]
fn run_hardware_midi_thread(
    events: Vec<HardwareMidiEvent>,
    cancel_rx: std::sync::mpsc::Receiver<()>,
) {
    use midir::MidiOutputConnection;
    use std::time::{Duration, Instant};

    let mut connections: HashMap<String, MidiOutputConnection> = HashMap::new();
    let start = Instant::now();

    for event in events {
        let deadline = start + Duration::from_secs_f64(event.delay_seconds.max(0.0));
        let now = Instant::now();
        if deadline > now {
            if cancel_rx.recv_timeout(deadline - now).is_ok() {
                send_all_notes_off(&mut connections);
                return;
            }
        } else if cancel_rx.try_recv().is_ok() {
            send_all_notes_off(&mut connections);
            return;
        }

        if !connections.contains_key(&event.device_id) {
            if let Some(conn) = open_midi_output(&event.device_id) {
                connections.insert(event.device_id.clone(), conn);
            }
        }
        if let Some(conn) = connections.get_mut(&event.device_id) {
            let _ = conn.send(&event.message);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn open_midi_output(device_id_or_name: &str) -> Option<midir::MidiOutputConnection> {
    let midi_out = midir::MidiOutput::new("Futureboard MIDI playback").ok()?;
    for port in midi_out.ports() {
        let Ok(name) = midi_out.port_name(&port) else {
            continue;
        };
        let stable = stable_id(MidiDeviceDirection::Output, &name);
        if name == device_id_or_name || stable == device_id_or_name {
            return midi_out.connect(&port, "Futureboard MIDI Out").ok();
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn send_all_notes_off(connections: &mut HashMap<String, midir::MidiOutputConnection>) {
    for conn in connections.values_mut() {
        for channel in 0..16u8 {
            let _ = conn.send(&[0x80 | channel, 0, 0]);
            let _ = conn.send(&[0xb0 | channel, 64, 0]);
            let _ = conn.send(&[0xb0 | channel, 123, 0]);
            let _ = conn.send(&[0xb0 | channel, 120, 0]);
        }
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
        assert!(
            resolved
                .iter()
                .any(|d| d.id == "midi-in-old" && !d.connected)
        );
        assert!(
            resolved
                .iter()
                .any(|d| d.id == "midi-in-new" && d.connected)
        );
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
