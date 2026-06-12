use super::*;

/// Monotonic source of transient automation-point identities. Like MIDI note
/// ids these are NOT persisted — they only let the lane editor track selection
/// and in-flight drag targets across edits. Fresh ids are minted on create and
/// on project load.
pub fn next_automation_point_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Monotonic source of transient note identities. Note ids are NOT persisted —
/// they exist only so the piano-roll editor can track selection / drag targets
/// across edits. Fresh ids are minted on create and on project load.
pub fn next_midi_note_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Source of transient identities for controller points (not serialized;
/// minted fresh on create and on project load, like [`next_midi_note_id`]).
pub fn next_controller_point_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Monotonic source of stable tempo-point identities. Persisted in project
/// files so edits target a point by id even after the user drags it to a new
/// beat position.
pub fn next_tempo_point_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("tempo-{ts:x}-{seq:x}")
}

pub fn next_time_signature_point_id() -> TimeSignaturePointId {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("ts-{ts:x}-{seq:x}")
}

pub fn next_timeline_marker_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("marker-{ts:x}-{seq:x}")
}

pub fn next_timeline_region_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("region-{ts:x}-{seq:x}")
}
