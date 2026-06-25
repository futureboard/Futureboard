//! Process-wide Audio + MIDI device registry.
//!
//! Replaces the old mock device lists. Real scans run once at Splash/Welcome
//! startup (and on manual Refresh), and Preferences renders from the cached
//! snapshot — it never re-scans on every paint. The registry is a plain
//! `static` rather than a `gpui::Global` because the Welcome window opens before
//! the studio globals exist and both windows need to reach the same state (see
//! the `welcome-settings-no-global` note).
//!
//! Scans are cheap and non-destructive: MIDI port enumeration (`midir`) and
//! cpal device enumeration do not open the hardware. Both are wrapped so a
//! backend failure yields an empty list + warning instead of a panic.

use std::sync::{OnceLock, RwLock};
use std::time::Instant;

use sphere_midi_service::{scan_midi_ports, DetectedMidiDevice, MidiDeviceDirection};

/// One enumerated audio endpoint. Mirrors the engine's `JsAudioDeviceInfo`
/// fields we actually surface in Preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceEntry {
    pub id: String,
    pub name: String,
    pub channels: u32,
    pub default_sample_rate: u32,
    pub is_default: bool,
}

/// Immutable snapshot of the audio side of the registry.
#[derive(Debug, Clone, Default)]
pub struct AudioDeviceSnapshot {
    pub inputs: Vec<AudioDeviceEntry>,
    pub outputs: Vec<AudioDeviceEntry>,
    pub default_input: Option<String>,
    pub default_output: Option<String>,
    pub backend: String,
    pub revision: u64,
}

#[derive(Default)]
struct RegistryState {
    midi: Vec<DetectedMidiDevice>,
    midi_revision: u64,
    midi_scanned: bool,
    audio: AudioDeviceSnapshot,
    audio_scanned: bool,
}

fn state() -> &'static RwLock<RegistryState> {
    static STATE: OnceLock<RwLock<RegistryState>> = OnceLock::new();
    STATE.get_or_init(|| RwLock::new(RegistryState::default()))
}

// ── MIDI ────────────────────────────────────────────────────────────────────

/// Run a real MIDI port scan, replace the cache, bump the revision, and return
/// the new revision. Called at startup and on manual Refresh — never per paint.
pub fn scan_midi() -> u64 {
    let start = Instant::now();
    let devices = scan_midi_ports();
    let inputs = devices
        .iter()
        .filter(|d| d.direction == MidiDeviceDirection::Input)
        .count();
    let outputs = devices
        .iter()
        .filter(|d| d.direction == MidiDeviceDirection::Output)
        .count();
    let revision = {
        let mut s = state().write().unwrap();
        s.midi = devices;
        s.midi_revision = s.midi_revision.wrapping_add(1);
        s.midi_scanned = true;
        s.midi_revision
    };
    eprintln!(
        "[MidiDeviceScan] found inputs={inputs} outputs={outputs} duration={:.1}ms revision={revision}",
        start.elapsed().as_secs_f32() * 1000.0
    );
    revision
}

/// Cached MIDI devices for rendering. Lazily runs one scan the first time it is
/// read if startup never scanned (e.g. the `--skip-splash` path), so the cache
/// is correct without ever scanning on a hot render path.
pub fn cached_midi_devices() -> Vec<DetectedMidiDevice> {
    if !state().read().unwrap().midi_scanned {
        scan_midi();
    }
    state().read().unwrap().midi.clone()
}

pub fn midi_revision() -> u64 {
    state().read().unwrap().midi_revision
}

// ── Audio ───────────────────────────────────────────────────────────────────

fn to_entry(info: DirectAudio::types::JsAudioDeviceInfo) -> AudioDeviceEntry {
    AudioDeviceEntry {
        id: info.id,
        name: info.name,
        channels: info.channels,
        default_sample_rate: info.default_sample_rate,
        is_default: info.is_default,
    }
}

/// Run a real audio device scan via the engine's cpal enumeration, replace the
/// cache, bump the revision, and return the new revision.
pub fn scan_audio() -> u64 {
    use DirectAudio::device::{list_input_devices, list_output_devices};
    let start = Instant::now();
    let raw_in = list_input_devices();
    let raw_out = list_output_devices();
    let backend = raw_out
        .first()
        .or_else(|| raw_in.first())
        .map(|d| d.backend.clone())
        .unwrap_or_default();
    let inputs: Vec<AudioDeviceEntry> = raw_in.into_iter().map(to_entry).collect();
    let outputs: Vec<AudioDeviceEntry> = raw_out.into_iter().map(to_entry).collect();
    let default_input = inputs.iter().find(|d| d.is_default).map(|d| d.name.clone());
    let default_output = outputs
        .iter()
        .find(|d| d.is_default)
        .map(|d| d.name.clone());
    let (in_n, out_n) = (inputs.len(), outputs.len());
    let revision = {
        let mut s = state().write().unwrap();
        s.audio.revision = s.audio.revision.wrapping_add(1);
        s.audio = AudioDeviceSnapshot {
            inputs,
            outputs,
            default_input,
            default_output,
            backend: backend.clone(),
            revision: s.audio.revision,
        };
        s.audio_scanned = true;
        s.audio.revision
    };
    eprintln!(
        "[AudioDeviceScan] started backend={backend}; found inputs={in_n} outputs={out_n} duration={:.1}ms revision={revision}",
        start.elapsed().as_secs_f32() * 1000.0
    );
    revision
}

/// Cached audio snapshot. Lazily scans once if startup never did.
pub fn audio_snapshot() -> AudioDeviceSnapshot {
    if !state().read().unwrap().audio_scanned {
        scan_audio();
    }
    state().read().unwrap().audio.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_midi_bumps_revision_and_marks_scanned() {
        // Real enumeration returns whatever the test host has (often empty in
        // CI) — the contract under test is "each scan bumps the revision and
        // marks the cache scanned". The registry is a process-wide static and
        // tests run in parallel, so assert monotonic change, not exact deltas.
        let r1 = scan_midi();
        let r2 = scan_midi();
        assert_ne!(r1, r2, "a second scan must change the revision");
        assert!(midi_revision() >= r2, "revision is monotonic");
        // Cache is populated/marked scanned, so a read never triggers a re-scan.
        let _ = cached_midi_devices();
    }

    #[test]
    fn cached_midi_never_panics() {
        // Lazy scan path must be panic-free even with no MIDI backend.
        let _ = cached_midi_devices();
    }
}
