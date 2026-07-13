/// One project-owned chord/lyric cue on the musical timeline.
///
/// `beat` uses the same quarter-note coordinate space as clips, markers, and
/// transport. Empty chord or lyric strings are valid, but a cue with both empty
/// should normally be removed by the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct SongTextCue {
    pub id: String,
    pub beat: f64,
    pub chord: String,
    pub lyric: String,
}

impl SongTextCue {
    pub fn new(id: impl Into<String>, beat: f64) -> Self {
        Self {
            id: id.into(),
            beat: beat.max(0.0),
            chord: String::new(),
            lyric: String::new(),
        }
    }
}

impl super::TimelineState {
    pub fn upsert_song_text_cue(&mut self, cue: SongTextCue) -> bool {
        if let Some(existing) = self
            .song_text_cues
            .iter_mut()
            .find(|item| item.id == cue.id)
        {
            if *existing == cue {
                return false;
            }
            *existing = cue;
        } else {
            self.song_text_cues.push(cue);
        }
        self.song_text_cues
            .sort_by(|a, b| a.beat.total_cmp(&b.beat));
        true
    }

    pub fn remove_song_text_cue(&mut self, id: &str) -> bool {
        let before = self.song_text_cues.len();
        self.song_text_cues.retain(|cue| cue.id != id);
        self.song_text_cues.len() != before
    }

    pub fn active_song_text_cue(&self) -> Option<&SongTextCue> {
        let playhead = self.transport.playhead_beats as f64;
        self.song_text_cues
            .iter()
            .rev()
            .find(|cue| cue.beat <= playhead + f64::EPSILON)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_sort_and_follow_playhead() {
        let mut state = crate::components::timeline::timeline_state::TimelineState::default();
        state.upsert_song_text_cue(SongTextCue::new("b", 8.0));
        state.upsert_song_text_cue(SongTextCue::new("a", 0.0));
        assert_eq!(state.song_text_cues[0].id, "a");
        state.transport.playhead_beats = 9.0;
        assert_eq!(
            state.active_song_text_cue().map(|cue| cue.id.as_str()),
            Some("b")
        );
    }
}
