//! Optional descriptive metadata attached to an exported audio file.
//!
//! Container support is best-effort: WAV/RAUF ignore it for now, FLAC writes
//! Vorbis comments where practical, and MP3 writes a minimal ID3 tag. Missing
//! metadata never blocks an export.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Free-form date string (e.g. an ISO `2026-06-13`). Not validated.
    pub date: Option<String>,
    pub comment: Option<String>,
}

impl AudioMetadata {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.date.is_none()
            && self.comment.is_none()
    }
}
