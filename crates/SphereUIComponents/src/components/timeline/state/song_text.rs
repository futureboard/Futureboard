use super::{next_song_text_event_id, TimelineState};

pub type SongTextEventId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SongTextEventType {
    Section,
    Chord,
    Lyric,
}

impl SongTextEventType {
    pub const fn sort_key(self) -> u8 {
        match self {
            Self::Section => 0,
            Self::Chord => 1,
            Self::Lyric => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Section => "Section",
            Self::Chord => "Chord",
            Self::Lyric => "Lyric",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LyricSyllableMode {
    #[default]
    Phrase,
    Syllables,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LyricSyllable {
    pub text: String,
    pub offset_beats: f64,
    pub duration_beats: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChordEvent {
    /// Free-form by design. Custom spelling is never normalized or rewritten.
    pub symbol: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LyricEvent {
    pub text: String,
    pub syllable_mode: LyricSyllableMode,
    pub continuation: bool,
    /// Optional phrase duration. `None` chases until the next lyric event.
    pub duration_beats: Option<f64>,
    /// Reserved for explicit future syllable timing; no fake word timing is generated.
    pub syllables: Vec<LyricSyllable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SongSectionType {
    Intro,
    Verse,
    PreChorus,
    Chorus,
    Bridge,
    Solo,
    Outro,
    #[default]
    Custom,
}

impl SongSectionType {
    pub const fn tag(self) -> u8 {
        match self {
            Self::Custom => 0,
            Self::Intro => 1,
            Self::Verse => 2,
            Self::PreChorus => 3,
            Self::Chorus => 4,
            Self::Bridge => 5,
            Self::Solo => 6,
            Self::Outro => 7,
        }
    }

    pub const fn from_tag(tag: u8) -> Self {
        match tag {
            1 => Self::Intro,
            2 => Self::Verse,
            3 => Self::PreChorus,
            4 => Self::Chorus,
            5 => Self::Bridge,
            6 => Self::Solo,
            7 => Self::Outro,
            _ => Self::Custom,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionEvent {
    pub name: String,
    pub section_type: SongSectionType,
    pub color_hex: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SongTextEventKind {
    Chord(ChordEvent),
    Lyric(LyricEvent),
    Section(SectionEvent),
}

impl SongTextEventKind {
    pub const fn event_type(&self) -> SongTextEventType {
        match self {
            Self::Section(_) => SongTextEventType::Section,
            Self::Chord(_) => SongTextEventType::Chord,
            Self::Lyric(_) => SongTextEventType::Lyric,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::Chord(event) => &event.symbol,
            Self::Lyric(event) => &event.text,
            Self::Section(event) => &event.name,
        }
    }

    fn normalize(&mut self) {
        match self {
            Self::Chord(event) => event.symbol = event.symbol.trim().to_string(),
            Self::Lyric(event) => {
                event.text = event.text.trim().to_string();
                event.duration_beats = event
                    .duration_beats
                    .filter(|duration| duration.is_finite() && *duration > 0.0);
                event.syllables.retain(|syllable| {
                    !syllable.text.is_empty()
                        && syllable.offset_beats.is_finite()
                        && syllable.offset_beats >= 0.0
                });
            }
            Self::Section(event) => {
                event.name = event.name.trim().to_string();
                if event.color_hex.trim().is_empty() {
                    event.color_hex = "#72d7d7".to_string();
                }
            }
        }
    }
}

/// One project-owned text event at a canonical quarter-note beat position.
///
/// Beat space is shared with clips, MIDI, snapping, tempo-map conversion, and
/// transport. Seconds and pixels are always derived and never persisted here.
#[derive(Debug, Clone, PartialEq)]
pub struct SongTextEvent {
    pub id: SongTextEventId,
    pub beat: f64,
    pub kind: SongTextEventKind,
}

impl SongTextEvent {
    pub fn chord(beat: f64, symbol: impl Into<String>) -> Option<Self> {
        Self::new(
            beat,
            SongTextEventKind::Chord(ChordEvent {
                symbol: symbol.into(),
            }),
        )
    }

    pub fn lyric(beat: f64, text: impl Into<String>) -> Option<Self> {
        Self::new(
            beat,
            SongTextEventKind::Lyric(LyricEvent {
                text: text.into(),
                syllable_mode: LyricSyllableMode::Phrase,
                continuation: false,
                duration_beats: None,
                syllables: Vec::new(),
            }),
        )
    }

    pub fn section(
        beat: f64,
        name: impl Into<String>,
        section_type: SongSectionType,
    ) -> Option<Self> {
        Self::new(
            beat,
            SongTextEventKind::Section(SectionEvent {
                name: name.into(),
                section_type,
                color_hex: "#72d7d7".to_string(),
            }),
        )
    }

    pub fn new(beat: f64, kind: SongTextEventKind) -> Option<Self> {
        Self::with_id(next_song_text_event_id(), beat, kind)
    }

    pub fn with_id(
        id: impl Into<SongTextEventId>,
        beat: f64,
        mut kind: SongTextEventKind,
    ) -> Option<Self> {
        if !beat.is_finite() {
            return None;
        }
        kind.normalize();
        let id = id.into();
        if id.trim().is_empty() || kind.text().is_empty() {
            return None;
        }
        Some(Self {
            id,
            beat: beat.max(0.0),
            kind,
        })
    }

    pub const fn event_type(&self) -> SongTextEventType {
        self.kind.event_type()
    }

    pub fn text(&self) -> &str {
        self.kind.text()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SongTextIndex {
    sections: Vec<usize>,
    chords: Vec<usize>,
    lyrics: Vec<usize>,
}

impl SongTextIndex {
    fn rebuild(events: &[SongTextEvent]) -> Self {
        let mut index = Self::default();
        for (event_index, event) in events.iter().enumerate() {
            match event.event_type() {
                SongTextEventType::Section => index.sections.push(event_index),
                SongTextEventType::Chord => index.chords.push(event_index),
                SongTextEventType::Lyric => index.lyrics.push(event_index),
            }
        }
        index
    }

    fn for_type(&self, event_type: SongTextEventType) -> &[usize] {
        match event_type {
            SongTextEventType::Section => &self.sections,
            SongTextEventType::Chord => &self.chords,
            SongTextEventType::Lyric => &self.lyrics,
        }
    }
}

fn compare_events(a: &SongTextEvent, b: &SongTextEvent) -> std::cmp::Ordering {
    a.beat
        .total_cmp(&b.beat)
        .then_with(|| a.event_type().sort_key().cmp(&b.event_type().sort_key()))
        .then_with(|| a.id.cmp(&b.id))
}

impl TimelineState {
    pub fn replace_song_text_events(&mut self, mut events: Vec<SongTextEvent>) {
        events.retain(|event| {
            event.beat.is_finite() && event.beat >= 0.0 && !event.text().trim().is_empty()
        });
        let mut seen_ids = std::collections::HashSet::with_capacity(events.len());
        for event in &mut events {
            if seen_ids.insert(event.id.clone()) {
                continue;
            }
            let base = event.id.clone();
            let mut suffix = 2_u64;
            loop {
                let candidate = format!("{base}:duplicate-{suffix}");
                if seen_ids.insert(candidate.clone()) {
                    event.id = candidate;
                    break;
                }
                suffix += 1;
            }
        }
        events.sort_by(compare_events);
        self.song_text_events = events;
        self.song_text_index = SongTextIndex::rebuild(&self.song_text_events);
        self.song_text_revision = self.song_text_revision.wrapping_add(1);
        self.selection
            .selected_song_text_event_ids
            .retain(|id| self.song_text_events.iter().any(|event| &event.id == id));
    }

    pub fn upsert_song_text_event(&mut self, mut event: SongTextEvent) -> bool {
        if !event.beat.is_finite() {
            return false;
        }
        event.beat = event.beat.max(0.0);
        event.kind.normalize();
        if event.id.trim().is_empty() || event.text().is_empty() {
            return false;
        }
        if let Some(existing) = self
            .song_text_events
            .iter_mut()
            .find(|item| item.id == event.id)
        {
            if *existing == event {
                return false;
            }
            *existing = event;
        } else {
            self.song_text_events.push(event);
        }
        self.song_text_events.sort_by(compare_events);
        self.song_text_index = SongTextIndex::rebuild(&self.song_text_events);
        self.song_text_revision = self.song_text_revision.wrapping_add(1);
        true
    }

    pub fn apply_song_text_patch(&mut self, remove: &[SongTextEvent], insert: &[SongTextEvent]) {
        let remove_ids: std::collections::HashSet<_> =
            remove.iter().map(|event| event.id.as_str()).collect();
        let insert_ids: std::collections::HashSet<_> =
            insert.iter().map(|event| event.id.as_str()).collect();
        let mut events = std::mem::take(&mut self.song_text_events);
        events.retain(|event| {
            !remove_ids.contains(event.id.as_str()) && !insert_ids.contains(event.id.as_str())
        });
        events.extend(insert.iter().cloned());
        self.replace_song_text_events(events);
    }

    pub fn remove_song_text_event(&mut self, id: &str) -> Option<SongTextEvent> {
        let index = self
            .song_text_events
            .iter()
            .position(|event| event.id == id)?;
        let removed = self.song_text_events.remove(index);
        self.selection
            .selected_song_text_event_ids
            .retain(|selected| selected != id);
        self.song_text_index = SongTextIndex::rebuild(&self.song_text_events);
        self.song_text_revision = self.song_text_revision.wrapping_add(1);
        Some(removed)
    }

    pub fn song_text_event(&self, id: &str) -> Option<&SongTextEvent> {
        self.song_text_events.iter().find(|event| event.id == id)
    }

    pub fn active_song_text_event(&self, event_type: SongTextEventType) -> Option<&SongTextEvent> {
        let playhead = self.transport.playhead_beats as f64;
        let event = self.song_text_event_at_or_before(event_type, playhead)?;
        if let SongTextEventKind::Lyric(lyric) = &event.kind {
            if lyric
                .duration_beats
                .is_some_and(|duration| playhead >= event.beat + duration)
            {
                return None;
            }
        }
        Some(event)
    }

    pub fn song_text_event_at_or_before(
        &self,
        event_type: SongTextEventType,
        beat: f64,
    ) -> Option<&SongTextEvent> {
        let indexes = self.song_text_index.for_type(event_type);
        let upper = indexes.partition_point(|index| self.song_text_events[*index].beat <= beat);
        upper
            .checked_sub(1)
            .map(|index| &self.song_text_events[indexes[index]])
    }

    pub fn song_text_events_in_range(&self, start_beat: f64, end_beat: f64) -> &[SongTextEvent] {
        let start = self
            .song_text_events
            .partition_point(|event| event.beat < start_beat);
        let end = self
            .song_text_events
            .partition_point(|event| event.beat <= end_beat);
        &self.song_text_events[start..end]
    }

    pub fn previous_song_text_event(&self, id: &str) -> Option<&SongTextEvent> {
        let index = self
            .song_text_events
            .iter()
            .position(|event| event.id == id)?;
        index
            .checked_sub(1)
            .map(|previous| &self.song_text_events[previous])
    }

    pub fn next_song_text_event(&self, id: &str) -> Option<&SongTextEvent> {
        let index = self
            .song_text_events
            .iter()
            .position(|event| event.id == id)?;
        self.song_text_events.get(index + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_chord_and_lyric_chase_uses_binary_indexes() {
        let mut state = TimelineState::default();
        state.upsert_song_text_event(SongTextEvent::chord(0.0, "C").unwrap());
        state.upsert_song_text_event(SongTextEvent::lyric(4.0, "Hello").unwrap());
        state.upsert_song_text_event(SongTextEvent::chord(8.0, "Am7").unwrap());
        state.transport.playhead_beats = 9.0;

        assert_eq!(
            state
                .active_song_text_event(SongTextEventType::Chord)
                .map(SongTextEvent::text),
            Some("Am7")
        );
        assert_eq!(
            state
                .active_song_text_event(SongTextEventType::Lyric)
                .map(SongTextEvent::text),
            Some("Hello")
        );
    }

    #[test]
    fn rejects_empty_and_invalid_events() {
        assert!(SongTextEvent::chord(0.0, "   ").is_none());
        assert!(SongTextEvent::lyric(f64::NAN, "line").is_none());
        assert_eq!(SongTextEvent::lyric(-2.0, "  line  ").unwrap().beat, 0.0);
        assert_eq!(
            SongTextEvent::lyric(0.0, "  line  ").unwrap().text(),
            "line"
        );
    }

    #[test]
    fn duplicate_time_order_is_deterministic_and_preserved() {
        let mut state = TimelineState::default();
        let lyric = SongTextEvent::with_id(
            "lyric-b",
            4.0,
            SongTextEventKind::Lyric(LyricEvent {
                text: "line".to_string(),
                syllable_mode: LyricSyllableMode::Phrase,
                continuation: false,
                duration_beats: None,
                syllables: Vec::new(),
            }),
        )
        .unwrap();
        let chord_b = SongTextEvent::with_id(
            "chord-b",
            4.0,
            SongTextEventKind::Chord(ChordEvent {
                symbol: "G".to_string(),
            }),
        )
        .unwrap();
        let chord_a = SongTextEvent::with_id(
            "a",
            4.0,
            SongTextEventKind::Chord(ChordEvent {
                symbol: "C".to_string(),
            }),
        )
        .unwrap();
        state.replace_song_text_events(vec![lyric, chord_b, chord_a]);

        let order: Vec<_> = state
            .song_text_events
            .iter()
            .map(|event| (event.event_type(), event.id.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                (SongTextEventType::Chord, "a"),
                (SongTextEventType::Chord, "chord-b"),
                (SongTextEventType::Lyric, "lyric-b"),
            ]
        );
    }

    #[test]
    fn duplicate_ids_are_sanitized_without_dropping_events() {
        let mut state = TimelineState::default();
        let first = SongTextEvent::with_id(
            "same",
            1.0,
            SongTextEventKind::Chord(ChordEvent {
                symbol: "C".to_string(),
            }),
        )
        .unwrap();
        let second = SongTextEvent::with_id(
            "same",
            2.0,
            SongTextEventKind::Lyric(LyricEvent {
                text: "line".to_string(),
                syllable_mode: LyricSyllableMode::Phrase,
                continuation: false,
                duration_beats: None,
                syllables: Vec::new(),
            }),
        )
        .unwrap();
        state.replace_song_text_events(vec![first, second]);
        let ids: std::collections::HashSet<_> = state
            .song_text_events
            .iter()
            .map(|event| event.id.as_str())
            .collect();
        assert_eq!(state.song_text_events.len(), 2);
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn lyric_duration_expires_without_fake_word_timing() {
        let mut state = TimelineState::default();
        let mut lyric = SongTextEvent::lyric(4.0, "held line").unwrap();
        if let SongTextEventKind::Lyric(event) = &mut lyric.kind {
            event.duration_beats = Some(2.0);
        }
        state.upsert_song_text_event(lyric);
        state.transport.playhead_beats = 5.999;
        assert!(state
            .active_song_text_event(SongTextEventType::Lyric)
            .is_some());
        state.transport.playhead_beats = 6.0;
        assert!(state
            .active_song_text_event(SongTextEventType::Lyric)
            .is_none());
    }

    #[test]
    fn tempo_and_signature_changes_do_not_rewrite_event_beats() {
        let mut state = TimelineState::default();
        let event = SongTextEvent::chord(7.5, "F#dim").unwrap();
        let id = event.id.clone();
        state.upsert_song_text_event(event);
        state.bpm = 73.0;
        state.time_signature_map.add_or_update_point(4.0, 7, 8);
        assert_eq!(state.song_text_event(&id).unwrap().beat, 7.5);
        let position = state.time_signature_map.bar_beat_at_beat(7.5);
        assert_eq!((position.bar, position.beat_in_bar), (3, 1));
    }

    #[test]
    fn visible_range_is_logarithmic_slice_over_sorted_events() {
        let mut state = TimelineState::default();
        state.replace_song_text_events(
            (0..10_000)
                .map(|beat| SongTextEvent::lyric(beat as f64, beat.to_string()).unwrap())
                .collect(),
        );
        let visible = state.song_text_events_in_range(5000.0, 5010.0);
        assert_eq!(visible.len(), 11);
        assert_eq!(visible.first().unwrap().beat, 5000.0);
        assert_eq!(visible.last().unwrap().beat, 5010.0);
    }
}
