/// `FUTUREBOARD_PLUGIN_DEBUG=1` enables eprintln traces for insert
/// mutations. Cached on first read.
pub fn plugin_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_PLUGIN_DEBUG").is_some())
}

/// `FUTUREBOARD_ROUTING_DEBUG=1` enables eprintln traces for send/routing
/// mutations (mirrors the DirectAudio-side flag). Cached on first read.
pub fn routing_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_ROUTING_DEBUG").is_some())
}

/// `FUTUREBOARD_MIDI_DEBUG=1` enables eprintln traces for MIDI clip/note
/// mutations (mirrors the plugin/routing debug flags). Cached on first read.
pub fn midi_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var_os("FUTUREBOARD_FORENSIC_TRACE").is_some()
            || std::env::var_os("FUTUREBOARD_MIDI_DEBUG").is_some()
    })
}

/// `FUTUREBOARD_AUTOMATION_DEBUG=1` enables eprintln traces for automation
/// mode/target/point mutations and evaluation. Cached on first read.
pub fn automation_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_AUTOMATION_DEBUG").is_some())
}

/// `FUTUREBOARD_AUTOMATION_SYNC_DEBUG=1` enables `[automation-sync]` traces that
/// follow Track Volume automation through the base/effective model: which beat
/// was evaluated, the resolved value, and the before/after effective volume with
/// the edit reason (playback_tick / seek / point_edit / fader_drag). Cached on
/// first read. Separate from `FUTUREBOARD_AUTOMATION_DEBUG` so the high-volume
/// sync trace can be enabled on its own.
pub fn automation_sync_debug_enabled() -> bool {
    static FLAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_AUTOMATION_SYNC_DEBUG").is_some())
}
