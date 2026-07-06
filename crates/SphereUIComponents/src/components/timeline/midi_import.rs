use std::collections::HashMap;

use super::timeline_state::{
    MidiChannel, MidiControllerKind, MidiControllerLane, MidiControllerPoint, MidiNoteState,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMidiClip {
    pub notes: Vec<MidiNoteState>,
    pub controller_lanes: Vec<MidiControllerLane>,
    pub sysex_events: Vec<ImportedSysExEvent>,
    pub markers: Vec<ImportedMidiMarker>,
    pub duration_beats: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMidiTrack {
    pub name: Option<String>,
    pub channel_hint: Option<MidiChannel>,
    pub clip: ImportedMidiClip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedSysExKind {
    Normal,
    Escaped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSysExEvent {
    pub kind: ImportedSysExKind,
    pub absolute_tick: u64,
    pub beat: f32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedMidiMarker {
    pub text: String,
    pub absolute_tick: u64,
    pub beat: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiImportError {
    Truncated(&'static str),
    InvalidHeader,
    UnsupportedDivision,
    UnsupportedFormat(u16),
}

impl std::fmt::Display for MidiImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated(section) => write!(f, "truncated MIDI {section}"),
            Self::InvalidHeader => write!(f, "invalid MIDI header"),
            Self::UnsupportedDivision => write!(f, "SMPTE MIDI timing is not supported yet"),
            Self::UnsupportedFormat(format) => write!(f, "unsupported MIDI format {format}"),
        }
    }
}

impl std::error::Error for MidiImportError {}

pub fn parse_smf_notes(data: &[u8]) -> Result<ImportedMidiClip, MidiImportError> {
    let tracks = parse_smf_tracks(data)?;
    let mut merged = empty_imported_clip();
    for track in tracks {
        merged.notes.extend(track.clip.notes);
        merge_controller_lanes(&mut merged.controller_lanes, track.clip.controller_lanes);
        merged.sysex_events.extend(track.clip.sysex_events);
        merged.markers.extend(track.clip.markers);
        merged.duration_beats = merged.duration_beats.max(track.clip.duration_beats);
    }
    finalize_imported_clip(&mut merged, 1);
    Ok(merged)
}

pub fn parse_smf_tracks(data: &[u8]) -> Result<Vec<ImportedMidiTrack>, MidiImportError> {
    let mut r = Reader::new(data);
    if r.read_exact(4)? != b"MThd" {
        return Err(MidiImportError::InvalidHeader);
    }
    let header_len = r.read_u32()? as usize;
    if header_len < 6 {
        return Err(MidiImportError::InvalidHeader);
    }
    let format = r.read_u16()?;
    let track_count = r.read_u16()?;
    let division = r.read_u16()?;
    if format > 1 {
        return Err(MidiImportError::UnsupportedFormat(format));
    }
    if division & 0x8000 != 0 {
        return Err(MidiImportError::UnsupportedDivision);
    }
    let ticks_per_beat = (division as u32).max(1);
    r.skip(header_len - 6)?;

    let mut imported_tracks = Vec::new();
    for _ in 0..track_count {
        if r.remaining() < 8 {
            break;
        }
        if r.read_exact(4)? != b"MTrk" {
            return Err(MidiImportError::InvalidHeader);
        }
        let len = r.read_u32()? as usize;
        let track_data = r.read_exact(len)?;
        let mut clip = empty_imported_clip();
        let mut track_name = None;
        let mut max_tick = 0u64;
        parse_track(
            track_data,
            ticks_per_beat,
            &mut clip.notes,
            &mut clip.controller_lanes,
            &mut clip.sysex_events,
            &mut clip.markers,
            &mut max_tick,
            &mut track_name,
        )?;
        clip.duration_beats = max_tick as f32 / ticks_per_beat as f32;
        finalize_imported_clip(&mut clip, ticks_per_beat);
        let track_name = track_name.filter(|name| !name.is_empty());
        imported_tracks.extend(split_imported_track_by_channel(track_name, clip));
    }

    Ok(imported_tracks)
}

fn empty_imported_clip() -> ImportedMidiClip {
    ImportedMidiClip {
        notes: Vec::new(),
        controller_lanes: Vec::new(),
        sysex_events: Vec::new(),
        markers: Vec::new(),
        duration_beats: 0.0,
    }
}

fn finalize_imported_clip(clip: &mut ImportedMidiClip, _ticks_per_beat: u32) {
    clip.notes.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then(a.pitch.cmp(&b.pitch))
            .then(a.id.cmp(&b.id))
    });
    clip.controller_lanes.retain(|lane| !lane.points.is_empty());
    clip.controller_lanes
        .sort_by(|a, b| controller_kind_sort_key(a.kind).cmp(&controller_kind_sort_key(b.kind)));
    let note_end = clip
        .notes
        .iter()
        .map(|note| note.start + note.duration)
        .fold(0.0_f32, f32::max);
    let controller_end = clip
        .controller_lanes
        .iter()
        .flat_map(|lane| lane.points.iter().map(|point| point.beat))
        .fold(0.0_f32, f32::max);
    clip.duration_beats = clip.duration_beats.max(note_end).max(controller_end);
}

fn split_imported_track_by_channel(
    track_name: Option<String>,
    clip: ImportedMidiClip,
) -> Vec<ImportedMidiTrack> {
    let mut channels: Vec<MidiChannel> = Vec::new();
    for note in &clip.notes {
        if !channels.contains(&note.channel) {
            channels.push(note.channel);
        }
    }
    channels.sort_by_key(|channel| channel.raw());

    if channels.len() <= 1 {
        return vec![ImportedMidiTrack {
            name: track_name,
            channel_hint: channels.first().copied(),
            clip,
        }];
    }

    channels
        .into_iter()
        .map(|channel| {
            let mut channel_clip = ImportedMidiClip {
                notes: clip
                    .notes
                    .iter()
                    .filter(|note| note.channel == channel)
                    .cloned()
                    .collect(),
                // The current controller-lane model is clip-global, so keep CC /
                // pitch-bend data available on each split channel clip instead of
                // silently dropping it during a type-0/channel-split import.
                controller_lanes: clip.controller_lanes.clone(),
                sysex_events: clip.sysex_events.clone(),
                markers: clip.markers.clone(),
                duration_beats: clip.duration_beats,
            };
            finalize_imported_clip(&mut channel_clip, 1);
            ImportedMidiTrack {
                name: track_name
                    .as_ref()
                    .map(|name| format!("{} Ch {}", name, channel.ui())),
                channel_hint: Some(channel),
                clip: channel_clip,
            }
        })
        .collect()
}

fn merge_controller_lanes(target: &mut Vec<MidiControllerLane>, lanes: Vec<MidiControllerLane>) {
    for lane in lanes {
        for point in lane.points {
            push_controller_point(target, lane.kind, point.beat, point.value);
        }
    }
}

fn parse_track(
    data: &[u8],
    ticks_per_beat: u32,
    notes: &mut Vec<MidiNoteState>,
    controller_lanes: &mut Vec<MidiControllerLane>,
    sysex_events: &mut Vec<ImportedSysExEvent>,
    markers: &mut Vec<ImportedMidiMarker>,
    max_tick: &mut u64,
    track_name: &mut Option<String>,
) -> Result<(), MidiImportError> {
    let mut r = Reader::new(data);
    let mut tick = 0u64;
    let mut running_status: Option<u8> = None;
    let mut active: HashMap<(u8, u8), Vec<(u64, u8)>> = HashMap::new();

    while r.remaining() > 0 {
        tick = tick.saturating_add(r.read_vlq()? as u64);
        *max_tick = (*max_tick).max(tick);
        let first = r.read_u8()?;
        let status = if first & 0x80 != 0 {
            first
        } else if let Some(status) = running_status {
            r.unread_one();
            status
        } else {
            return Err(MidiImportError::InvalidHeader);
        };

        match status {
            0x80..=0x9f => {
                running_status = Some(status);
                let pitch = r.read_u8()?.min(127);
                let velocity = r.read_u8()?.min(127);
                let channel = status & 0x0f;
                if status & 0xf0 == 0x90 && velocity > 0 {
                    active
                        .entry((channel, pitch))
                        .or_default()
                        .push((tick, velocity));
                } else if let Some(starts) = active.get_mut(&(channel, pitch)) {
                    if let Some((start_tick, start_velocity)) = starts.pop() {
                        push_note(
                            notes,
                            ticks_per_beat,
                            pitch,
                            start_tick,
                            tick,
                            start_velocity,
                            channel,
                        );
                    }
                }
            }
            0xa0..=0xaf | 0xb0..=0xbf | 0xe0..=0xef => {
                running_status = Some(status);
                let data1 = r.read_u8()?;
                let data2 = r.read_u8()?;
                match status & 0xf0 {
                    0xb0 => push_controller_point(
                        controller_lanes,
                        MidiControllerKind::CC(data1.min(127)),
                        ticks_to_beats(tick, ticks_per_beat),
                        data2.min(127) as f32 / 127.0,
                    ),
                    0xe0 => {
                        let value14 = ((data2 as u16) << 7) | data1 as u16;
                        push_controller_point(
                            controller_lanes,
                            MidiControllerKind::PitchBend,
                            ticks_to_beats(tick, ticks_per_beat),
                            value14 as f32 / 16383.0,
                        );
                    }
                    // Poly pressure needs per-note association. The current lane
                    // model has only one normalized stream, so preserve the data
                    // model contract by not importing it as a misleading global
                    // lane yet.
                    _ => {}
                }
            }
            0xc0..=0xdf => {
                running_status = Some(status);
                let data = r.read_u8()?;
                if status & 0xf0 == 0xd0 {
                    push_controller_point(
                        controller_lanes,
                        MidiControllerKind::ChannelPressure,
                        ticks_to_beats(tick, ticks_per_beat),
                        data.min(127) as f32 / 127.0,
                    );
                }
            }
            0xff => {
                running_status = None;
                let meta_type = r.read_u8()?;
                let len = r.read_vlq()? as usize;
                let payload = r.read_exact(len)?;
                if meta_type == 0x03 {
                    let name = decode_midi_text(payload);
                    if !name.is_empty() {
                        *track_name = Some(name);
                    }
                }
                if meta_type == 0x06 {
                    markers.push(ImportedMidiMarker {
                        text: decode_midi_text(payload),
                        absolute_tick: tick,
                        beat: ticks_to_beats(tick, ticks_per_beat),
                    });
                }
                if meta_type == 0x2f {
                    break;
                }
            }
            0xf0 | 0xf7 => {
                running_status = None;
                let len = r.read_vlq()? as usize;
                let payload = r.read_exact(len)?;
                sysex_events.push(ImportedSysExEvent {
                    kind: if status == 0xf0 {
                        ImportedSysExKind::Normal
                    } else {
                        ImportedSysExKind::Escaped
                    },
                    absolute_tick: tick,
                    beat: ticks_to_beats(tick, ticks_per_beat),
                    data: payload.to_vec(),
                });
            }
            _ => return Err(MidiImportError::InvalidHeader),
        }
    }

    Ok(())
}

fn decode_midi_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

fn ticks_to_beats(tick: u64, ticks_per_beat: u32) -> f32 {
    tick as f32 / ticks_per_beat.max(1) as f32
}

fn push_controller_point(
    lanes: &mut Vec<MidiControllerLane>,
    kind: MidiControllerKind,
    beat: f32,
    value: f32,
) {
    let Some(lane) = lanes.iter_mut().find(|lane| lane.kind == kind) else {
        lanes.push(MidiControllerLane {
            kind,
            points: vec![MidiControllerPoint::new(beat, value)],
            visible: true,
            height: 80.0,
            collapsed: false,
        });
        return;
    };
    if let Some(point) = lane
        .points
        .iter_mut()
        .find(|point| (point.beat - beat).abs() < 1.0e-3)
    {
        point.value = value.clamp(0.0, 1.0);
    } else {
        lane.points.push(MidiControllerPoint::new(beat, value));
        lane.points.sort_by(|a, b| {
            a.beat
                .partial_cmp(&b.beat)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

fn controller_kind_sort_key(kind: MidiControllerKind) -> (u8, u8) {
    match kind {
        MidiControllerKind::CC(n) => (0, n),
        MidiControllerKind::PitchBend => (1, 0),
        MidiControllerKind::ChannelPressure => (2, 0),
        MidiControllerKind::PolyPressure => (3, 0),
    }
}

fn push_note(
    notes: &mut Vec<MidiNoteState>,
    ticks_per_beat: u32,
    pitch: u8,
    start_tick: u64,
    end_tick: u64,
    velocity: u8,
    channel: u8,
) {
    if end_tick <= start_tick {
        return;
    }
    let start = start_tick as f32 / ticks_per_beat as f32;
    let duration = (end_tick - start_tick) as f32 / ticks_per_beat as f32;
    let mut note = MidiNoteState::new(pitch, start, duration, velocity.max(1));
    note.channel = MidiChannel::from_raw(channel);
    notes.push(note);
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], MidiImportError> {
        if self.remaining() < len {
            return Err(MidiImportError::Truncated("chunk"));
        }
        let start = self.pos;
        self.pos += len;
        Ok(&self.data[start..self.pos])
    }

    fn read_u8(&mut self) -> Result<u8, MidiImportError> {
        Ok(*self
            .read_exact(1)?
            .first()
            .ok_or(MidiImportError::Truncated("byte"))?)
    }

    fn read_u16(&mut self) -> Result<u16, MidiImportError> {
        let b = self.read_exact(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, MidiImportError> {
        let b = self.read_exact(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_vlq(&mut self) -> Result<u32, MidiImportError> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self.read_u8()?;
            value = (value << 7) | (byte & 0x7f) as u32;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(MidiImportError::InvalidHeader)
    }

    fn skip(&mut self, len: usize) -> Result<(), MidiImportError> {
        self.read_exact(len).map(|_| ())
    }

    fn unread_one(&mut self) {
        self.pos = self.pos.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smf(track: &[u8]) -> Vec<u8> {
        smf_format(0, &[track])
    }

    fn smf_format(format: u16, tracks: &[&[u8]]) -> Vec<u8> {
        let mut data = vec![b'M', b'T', b'h', b'd', 0, 0, 0, 6];
        data.extend_from_slice(&format.to_be_bytes());
        data.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
        data.extend_from_slice(&480u16.to_be_bytes());
        for track in tracks {
            data.extend_from_slice(b"MTrk");
            data.extend_from_slice(&(track.len() as u32).to_be_bytes());
            data.extend_from_slice(track);
        }
        data
    }

    #[test]
    fn parses_note_on_off_track() {
        let data = smf(&[0, 0x90, 60, 100, 0x83, 0x60, 0x80, 60, 0, 0, 0xff, 0x2f, 0]);
        let imported = parse_smf_notes(&data).unwrap();
        assert_eq!(imported.notes.len(), 1);
        assert_eq!(imported.notes[0].pitch, 60);
        assert_eq!(imported.notes[0].velocity, 100);
        assert!((imported.notes[0].duration - 1.0).abs() < 1.0e-4);
    }

    #[test]
    fn parses_format_one_tracks_separately() {
        let track_a = [
            0, 0xff, 0x03, 5, b'P', b'i', b'a', b'n', b'o', 0, 0x90, 60, 100, 0x83, 0x60, 0x80, 60,
            0, 0, 0xff, 0x2f, 0,
        ];
        let track_b = [
            0, 0xff, 0x03, 4, b'B', b'a', b's', b's', 0, 0x91, 48, 96, 0x83, 0x60, 0x81, 48, 0, 0,
            0xff, 0x2f, 0,
        ];
        let data = smf_format(1, &[&track_a, &track_b]);
        let tracks = parse_smf_tracks(&data).unwrap();

        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].name.as_deref(), Some("Piano"));
        assert_eq!(tracks[1].name.as_deref(), Some("Bass"));
        assert_eq!(tracks[0].clip.notes.len(), 1);
        assert_eq!(tracks[1].clip.notes.len(), 1);
        assert_eq!(tracks[0].clip.notes[0].pitch, 60);
        assert_eq!(tracks[1].clip.notes[0].pitch, 48);

        let merged = parse_smf_notes(&data).unwrap();
        assert_eq!(merged.notes.len(), 2);
    }

    #[test]
    fn splits_single_track_multichannel_midi_by_channel() {
        let track = [
            0, 0xff, 0x03, 5, b'S', b'o', b'n', b'g', b'1', 0, 0x90, 60, 100, 0, 0x91, 48, 96,
            0x83, 0x60, 0x80, 60, 0, 0, 0x81, 48, 0, 0, 0xff, 0x2f, 0,
        ];
        let data = smf(&track);
        let tracks = parse_smf_tracks(&data).unwrap();

        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].name.as_deref(), Some("Song1 Ch 1"));
        assert_eq!(tracks[1].name.as_deref(), Some("Song1 Ch 2"));
        assert_eq!(tracks[0].channel_hint.unwrap().ui(), 1);
        assert_eq!(tracks[1].channel_hint.unwrap().ui(), 2);
        assert_eq!(tracks[0].clip.notes.len(), 1);
        assert_eq!(tracks[1].clip.notes.len(), 1);
        assert_eq!(tracks[0].clip.notes[0].channel.ui(), 1);
        assert_eq!(tracks[1].clip.notes[0].channel.ui(), 2);
    }

    #[test]
    fn parses_controller_lanes() {
        let data = smf(&[
            0, 0xb0, 1, 64, 0x83, 0x60, 0xe0, 0, 0x40, 0x83, 0x60, 0xd0, 100, 0, 0xff, 0x2f, 0,
        ]);
        let imported = parse_smf_notes(&data).unwrap();
        assert_eq!(imported.controller_lanes.len(), 3);

        let cc1 = imported
            .controller_lanes
            .iter()
            .find(|lane| lane.kind == MidiControllerKind::CC(1))
            .unwrap();
        assert_eq!(cc1.points.len(), 1);
        assert!((cc1.points[0].value - 64.0 / 127.0).abs() < 1.0e-4);

        let bend = imported
            .controller_lanes
            .iter()
            .find(|lane| lane.kind == MidiControllerKind::PitchBend)
            .unwrap();
        assert!((bend.points[0].beat - 1.0).abs() < 1.0e-4);
        assert!((bend.points[0].value - 8192.0 / 16383.0).abs() < 1.0e-4);

        let pressure = imported
            .controller_lanes
            .iter()
            .find(|lane| lane.kind == MidiControllerKind::ChannelPressure)
            .unwrap();
        assert!((pressure.points[0].beat - 2.0).abs() < 1.0e-4);
        assert!((pressure.points[0].value - 100.0 / 127.0).abs() < 1.0e-4);
    }

    #[test]
    fn preserves_normal_and_escaped_sysex_events() {
        let data = smf(&[
            0, 0xf0, 3, 0x43, 0x12, 0x00, 0x83, 0x60, 0xf7, 2, 0x7d, 0x01, 0, 0xff, 0x2f, 0,
        ]);
        let imported = parse_smf_notes(&data).unwrap();
        assert_eq!(imported.sysex_events.len(), 2);
        assert_eq!(imported.sysex_events[0].kind, ImportedSysExKind::Normal);
        assert_eq!(imported.sysex_events[0].absolute_tick, 0);
        assert_eq!(imported.sysex_events[0].data, vec![0x43, 0x12, 0x00]);
        assert_eq!(imported.sysex_events[1].kind, ImportedSysExKind::Escaped);
        assert!((imported.sysex_events[1].beat - 1.0).abs() < 1.0e-4);
        assert_eq!(imported.sysex_events[1].data, vec![0x7d, 0x01]);
    }

    #[test]
    fn parses_marker_meta_events() {
        let data = smf(&[
            0x83, 0x60, 0xff, 0x06, 5, b'V', b'e', b'r', b's', b'e', 0, 0xff, 0x2f, 0,
        ]);
        let imported = parse_smf_notes(&data).unwrap();
        assert_eq!(imported.markers.len(), 1);
        assert_eq!(imported.markers[0].text, "Verse");
        assert_eq!(imported.markers[0].absolute_tick, 480);
        assert!((imported.markers[0].beat - 1.0).abs() < 1.0e-4);
    }
}
