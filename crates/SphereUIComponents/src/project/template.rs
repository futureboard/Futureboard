//! Non-UI project template descriptors.
//!
//! Extracted from the old `project_wizard` modal so the welcome screen and the
//! `project:new-*` commands can create template-backed workspace states without
//! depending on any wizard UI. This is pure data — track counts, tempo, and
//! time signature defaults — consumed by `StudioLayout::new_project_from_template`.

use std::path::PathBuf;

/// A starting layout for a brand-new (unsaved) workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTemplate {
    /// Blank arrangement, no tracks.
    Empty,
    /// Audio tracks, monitoring, record-ready routing.
    Recording,
    /// MIDI lanes for drums, bass, keys, texture.
    BeatMaking,
    /// Audio channels organized for edit/mix work.
    Mixing,
    /// MIDI-first layout for cues / arrangement sketches.
    Scoring,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectCreateOptions {
    pub name: String,
    pub base_dir: PathBuf,
    pub template: ProjectTemplate,
    pub sample_rate: u32,
    pub bpm: f32,
    pub time_signature_num: u32,
    pub time_signature_den: u32,
}

impl ProjectTemplate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Recording => "Recording",
            Self::BeatMaking => "Beat Making",
            Self::Mixing => "Mixing",
            Self::Scoring => "Scoring",
        }
    }

    pub fn audio_tracks(self) -> u32 {
        match self {
            Self::Empty => 0,
            Self::Recording => 4,
            Self::BeatMaking => 0,
            Self::Mixing => 8,
            Self::Scoring => 0,
        }
    }

    pub fn midi_tracks(self) -> u32 {
        match self {
            Self::Empty | Self::Recording | Self::Mixing => 0,
            Self::BeatMaking => 4,
            Self::Scoring => 8,
        }
    }

    pub fn default_bpm(self) -> f32 {
        if matches!(self, Self::BeatMaking) {
            140.0
        } else {
            120.0
        }
    }

    pub fn time_signature(self) -> (u32, u32) {
        (4, 4)
    }
}
