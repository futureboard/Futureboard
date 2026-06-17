//! UI/control-path MIDI input routing.
//!
//! This is the shared entry point for MIDI that originates outside timeline
//! playback: hardware devices, piano-roll audition, virtual keyboard, and later
//! step input/remote surfaces. It intentionally emits into the existing engine
//! command path instead of calling plugin instances or editor hosts directly.

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
