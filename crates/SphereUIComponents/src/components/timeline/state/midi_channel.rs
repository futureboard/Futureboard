//! MIDI channel model.
//!
//! [`MidiChannel`] stores channels 0-based (0..=15) internally so arithmetic
//! never needs an off-by-one guard; UI code always goes through
//! [`MidiChannel::from_ui`] / [`MidiChannel::ui`] (1..=16) so a raw channel
//! index never leaks into a label or a dropdown by accident. [`MidiChannelMask`]
//! is the reusable "set of channels" building block for input filters and
//! editor view/edit filtering; [`MidiInputChannelFilter`] and
//! [`MidiOutputChannelMode`] are the track-level policies built on top of it.
//! Pure data — no project/UI coupling — so it can be reused by the Drum
//! Editor / Tracker modes and by playback scheduling.

/// A single MIDI channel, stored 0-based (0..=15) internally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MidiChannel(u8);

impl MidiChannel {
    pub const COUNT: u8 = 16;

    /// From a UI-facing channel number (1..=16), clamped into range.
    pub fn from_ui(ui_channel: u8) -> Self {
        Self(ui_channel.clamp(1, 16) - 1)
    }

    /// From an already 0-based internal value, clamped into range.
    pub fn from_raw(raw: u8) -> Self {
        Self(raw.min(15))
    }

    /// 0-based internal value (0..=15), the form the audio engine expects.
    pub fn raw(self) -> u8 {
        self.0
    }

    /// 1-based UI-facing channel number (1..=16).
    pub fn ui(self) -> u8 {
        self.0 + 1
    }

    pub fn label(self) -> String {
        format!("Ch {}", self.ui())
    }

    /// All 16 channels in ascending order, for selector/dropdown population.
    pub fn all() -> impl Iterator<Item = MidiChannel> {
        (0..Self::COUNT).map(MidiChannel)
    }
}

impl Default for MidiChannel {
    /// Channel 1 — matches the pre-existing single-channel track behavior.
    fn default() -> Self {
        Self(0)
    }
}

/// A bitmask over the 16 MIDI channels. The reusable "set of channels"
/// primitive behind input filters and editor view/edit filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiChannelMask(u16);

impl MidiChannelMask {
    pub const ALL: MidiChannelMask = MidiChannelMask(0xFFFF);
    pub const NONE: MidiChannelMask = MidiChannelMask(0);

    pub fn single(channel: MidiChannel) -> Self {
        Self(1u16 << channel.raw())
    }

    pub fn contains(self, channel: MidiChannel) -> bool {
        self.0 & (1u16 << channel.raw()) != 0
    }

    #[must_use]
    pub fn with(self, channel: MidiChannel) -> Self {
        Self(self.0 | (1u16 << channel.raw()))
    }

    #[must_use]
    pub fn without(self, channel: MidiChannel) -> Self {
        Self(self.0 & !(1u16 << channel.raw()))
    }

    #[must_use]
    pub fn toggled(self, channel: MidiChannel) -> Self {
        if self.contains(channel) {
            self.without(channel)
        } else {
            self.with(channel)
        }
    }

    pub fn is_all(self) -> bool {
        self == Self::ALL
    }

    pub fn is_none(self) -> bool {
        self == Self::NONE
    }
}

impl Default for MidiChannelMask {
    /// All channels visible/accepted — unchanged behavior until a filter is
    /// explicitly narrowed.
    fn default() -> Self {
        Self::ALL
    }
}

/// Track-level input channel filter (which incoming MIDI channels a track
/// listens to). Model only in this pass — not yet enforced on the recording
/// input path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiInputChannelFilter {
    All,
    Only(MidiChannelMask),
}

impl Default for MidiInputChannelFilter {
    fn default() -> Self {
        Self::All
    }
}

impl MidiInputChannelFilter {
    pub fn accepts(self, channel: MidiChannel) -> bool {
        match self {
            Self::All => true,
            Self::Only(mask) => mask.contains(channel),
        }
    }
}

/// Track-level output channel policy: either every note plays back on its own
/// channel, or every note is forced onto one fixed channel (the pre-existing
/// single-channel-per-track behavior).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiOutputChannelMode {
    PerNote,
    Fixed(MidiChannel),
}

impl Default for MidiOutputChannelMode {
    /// Matches the pre-existing behavior: one fixed channel (1) per track.
    fn default() -> Self {
        Self::Fixed(MidiChannel::default())
    }
}

impl MidiOutputChannelMode {
    /// The channel to actually emit for a note carrying `note_channel`.
    pub fn resolve(self, note_channel: MidiChannel) -> MidiChannel {
        match self {
            Self::PerNote => note_channel,
            Self::Fixed(channel) => channel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_channel_round_trips_and_clamps() {
        assert_eq!(MidiChannel::from_ui(1).raw(), 0);
        assert_eq!(MidiChannel::from_ui(16).raw(), 15);
        assert_eq!(MidiChannel::from_ui(0).raw(), 0); // clamped up
        assert_eq!(MidiChannel::from_ui(200).raw(), 15); // clamped down
        assert_eq!(MidiChannel::from_ui(5).ui(), 5);
    }

    #[test]
    fn raw_channel_clamps_into_range() {
        assert_eq!(MidiChannel::from_raw(0).raw(), 0);
        assert_eq!(MidiChannel::from_raw(15).raw(), 15);
        assert_eq!(MidiChannel::from_raw(255).raw(), 15);
    }

    #[test]
    fn default_channel_is_one() {
        assert_eq!(MidiChannel::default().ui(), 1);
    }

    #[test]
    fn mask_contains_and_toggle() {
        let mask = MidiChannelMask::NONE;
        let ch5 = MidiChannel::from_ui(5);
        assert!(!mask.contains(ch5));
        let mask = mask.with(ch5);
        assert!(mask.contains(ch5));
        assert!(!mask.contains(MidiChannel::from_ui(6)));
        let mask = mask.toggled(ch5);
        assert!(!mask.contains(ch5));
    }

    #[test]
    fn mask_all_contains_every_channel() {
        for ch in MidiChannel::all() {
            assert!(MidiChannelMask::ALL.contains(ch));
        }
        assert!(MidiChannelMask::ALL.is_all());
        assert!(MidiChannelMask::NONE.is_none());
    }

    #[test]
    fn input_filter_all_accepts_everything() {
        let filter = MidiInputChannelFilter::All;
        for ch in MidiChannel::all() {
            assert!(filter.accepts(ch));
        }
    }

    #[test]
    fn input_filter_only_restricts_to_mask() {
        let mask = MidiChannelMask::single(MidiChannel::from_ui(3));
        let filter = MidiInputChannelFilter::Only(mask);
        assert!(filter.accepts(MidiChannel::from_ui(3)));
        assert!(!filter.accepts(MidiChannel::from_ui(4)));
    }

    #[test]
    fn output_mode_per_note_vs_fixed() {
        let note_ch = MidiChannel::from_ui(7);
        assert_eq!(MidiOutputChannelMode::PerNote.resolve(note_ch), note_ch);
        let fixed = MidiOutputChannelMode::Fixed(MidiChannel::from_ui(2));
        assert_eq!(fixed.resolve(note_ch).ui(), 2);
    }

    #[test]
    fn output_mode_default_matches_legacy_single_channel_behavior() {
        assert_eq!(
            MidiOutputChannelMode::default(),
            MidiOutputChannelMode::Fixed(MidiChannel::default())
        );
    }
}
