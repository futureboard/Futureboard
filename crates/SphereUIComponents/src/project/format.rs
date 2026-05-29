use super::{
    AutomationLane, AutomationPoint, ClipSource, FutureboardProject, InputMonitorMode, MidiNote,
    PluginFormat, PluginStateBlob, ProjectAsset, ProjectClip, ProjectInsert, ProjectMixer,
    ProjectPluginInstance, ProjectSend, ProjectSettings, ProjectTrack, ProjectTrackType,
    TrackRouting,
};
use std::io::{self, Cursor, Read};
use std::path::PathBuf;

pub const PROJECT_MAGIC: &[u8; 8] = b"FBSTUD1\0";
pub const PROJECT_VERSION: u32 = 1;

#[derive(Debug)]
pub enum ProjectError {
    Io(io::Error),
    InvalidMagic,
    UnsupportedVersion(u32),
    Corrupted(String),
    ChecksumMismatch { expected: u32, got: u32 },
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "I/O error: {e}"),
            ProjectError::InvalidMagic => write!(f, "Not a Futureboard project file"),
            ProjectError::UnsupportedVersion(v) => write!(f, "Unsupported project version: {v}"),
            ProjectError::Corrupted(msg) => write!(f, "Corrupted project: {msg}"),
            ProjectError::ChecksumMismatch { expected, got } => {
                write!(
                    f,
                    "Checksum mismatch: expected {expected:#010x}, got {got:#010x}"
                )
            }
        }
    }
}

impl From<io::Error> for ProjectError {
    fn from(e: io::Error) -> Self {
        ProjectError::Io(e)
    }
}

// ── Low-level writer ──────────────────────────────────────────────────────────

pub struct FbWriter {
    buf: Vec<u8>,
}

impl FbWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(4096),
        }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    fn write_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn write_bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }

    fn write_str(&mut self, s: &str) {
        let bytes = s.as_bytes();
        self.write_u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }

    fn write_opt_str(&mut self, s: &Option<String>) {
        match s {
            None => self.write_u8(0),
            Some(v) => {
                self.write_u8(1);
                self.write_str(v);
            }
        }
    }

    fn write_opt_path(&mut self, p: &Option<PathBuf>) {
        match p {
            None => self.write_u8(0),
            Some(v) => {
                self.write_u8(1);
                self.write_str(&v.to_string_lossy());
            }
        }
    }

    fn write_opt_u32(&mut self, v: &Option<u32>) {
        match v {
            None => self.write_u8(0),
            Some(x) => {
                self.write_u8(1);
                self.write_u32(*x);
            }
        }
    }

    fn write_opt_u8(&mut self, v: &Option<u8>) {
        match v {
            None => self.write_u8(0),
            Some(x) => {
                self.write_u8(1);
                self.write_u8(*x);
            }
        }
    }

    fn write_opt_f64(&mut self, v: &Option<f64>) {
        match v {
            None => self.write_u8(0),
            Some(x) => {
                self.write_u8(1);
                self.write_f64(*x);
            }
        }
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }
}

// ── Low-level reader ──────────────────────────────────────────────────────────

pub struct FbReader<'a> {
    cur: Cursor<&'a [u8]>,
}

impl<'a> FbReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            cur: Cursor::new(data),
        }
    }

    fn read_u8(&mut self) -> Result<u8, ProjectError> {
        let mut b = [0u8; 1];
        self.cur
            .read_exact(&mut b)
            .map_err(|_| ProjectError::Corrupted("truncated u8".into()))?;
        Ok(b[0])
    }

    fn read_u32(&mut self) -> Result<u32, ProjectError> {
        let mut b = [0u8; 4];
        self.cur
            .read_exact(&mut b)
            .map_err(|_| ProjectError::Corrupted("truncated u32".into()))?;
        Ok(u32::from_le_bytes(b))
    }

    fn read_u64(&mut self) -> Result<u64, ProjectError> {
        let mut b = [0u8; 8];
        self.cur
            .read_exact(&mut b)
            .map_err(|_| ProjectError::Corrupted("truncated u64".into()))?;
        Ok(u64::from_le_bytes(b))
    }

    fn read_f32(&mut self) -> Result<f32, ProjectError> {
        let mut b = [0u8; 4];
        self.cur
            .read_exact(&mut b)
            .map_err(|_| ProjectError::Corrupted("truncated f32".into()))?;
        Ok(f32::from_le_bytes(b))
    }

    fn read_f64(&mut self) -> Result<f64, ProjectError> {
        let mut b = [0u8; 8];
        self.cur
            .read_exact(&mut b)
            .map_err(|_| ProjectError::Corrupted("truncated f64".into()))?;
        Ok(f64::from_le_bytes(b))
    }

    fn read_bool(&mut self) -> Result<bool, ProjectError> {
        Ok(self.read_u8()? != 0)
    }

    fn read_str(&mut self) -> Result<String, ProjectError> {
        let len = self.read_u32()? as usize;
        let mut buf = vec![0u8; len];
        self.cur
            .read_exact(&mut buf)
            .map_err(|_| ProjectError::Corrupted("truncated string".into()))?;
        String::from_utf8(buf).map_err(|_| ProjectError::Corrupted("invalid UTF-8 string".into()))
    }

    fn read_opt_str(&mut self) -> Result<Option<String>, ProjectError> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_str()?)),
            t => Err(ProjectError::Corrupted(format!("bad option tag {t}"))),
        }
    }

    fn read_opt_path(&mut self) -> Result<Option<PathBuf>, ProjectError> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(PathBuf::from(self.read_str()?))),
            t => Err(ProjectError::Corrupted(format!("bad option tag {t}"))),
        }
    }

    fn read_opt_u32(&mut self) -> Result<Option<u32>, ProjectError> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_u32()?)),
            t => Err(ProjectError::Corrupted(format!("bad option tag {t}"))),
        }
    }

    fn read_opt_u8(&mut self) -> Result<Option<u8>, ProjectError> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_u8()?)),
            t => Err(ProjectError::Corrupted(format!("bad option tag {t}"))),
        }
    }

    fn read_opt_f64(&mut self) -> Result<Option<f64>, ProjectError> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_f64()?)),
            t => Err(ProjectError::Corrupted(format!("bad option tag {t}"))),
        }
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>, ProjectError> {
        let len = self.read_u32()? as usize;
        let mut buf = vec![0u8; len];
        self.cur
            .read_exact(&mut buf)
            .map_err(|_| ProjectError::Corrupted("truncated bytes".into()))?;
        Ok(buf)
    }
}

// ── Encoding ──────────────────────────────────────────────────────────────────

fn encode_plugin_format(w: &mut FbWriter, f: PluginFormat) {
    w.write_u8(match f {
        PluginFormat::Vst3 => 0,
        PluginFormat::Clap => 1,
        PluginFormat::Au => 2,
        PluginFormat::Lv2 => 3,
        PluginFormat::Unknown => 0xFF,
    });
}

fn encode_opt_plugin_format(w: &mut FbWriter, f: &Option<PluginFormat>) {
    match f {
        None => w.write_u8(0),
        Some(fmt) => {
            w.write_u8(1);
            encode_plugin_format(w, *fmt);
        }
    }
}

fn encode_plugin_state_blob(w: &mut FbWriter, blob: &PluginStateBlob) {
    w.write_str(&blob.plugin_id);
    encode_opt_plugin_format(w, &blob.format);
    w.write_bytes(&blob.state_bytes);
    w.write_opt_str(&blob.vendor);
    w.write_opt_str(&blob.name);
    w.write_opt_str(&blob.version);
}

fn encode_plugin_instance(w: &mut FbWriter, inst: &ProjectPluginInstance) {
    w.write_str(&inst.instance_id);
    encode_plugin_format(w, inst.format);
    w.write_opt_path(&inst.plugin_path);
    w.write_str(&inst.plugin_uid);
    w.write_str(&inst.display_name);
    encode_plugin_state_blob(w, &inst.state);
}

fn encode_insert(w: &mut FbWriter, ins: &ProjectInsert) {
    w.write_str(&ins.id);
    w.write_u32(ins.slot_index);
    w.write_bool(ins.bypassed);
    match &ins.plugin {
        None => w.write_u8(0),
        Some(inst) => {
            w.write_u8(1);
            encode_plugin_instance(w, inst);
        }
    }
}

fn encode_automation_lane(w: &mut FbWriter, lane: &AutomationLane) {
    w.write_str(&lane.id);
    w.write_str(&lane.parameter_name);
    w.write_bool(lane.visible);
    w.write_u32(lane.points.len() as u32);
    for p in &lane.points {
        w.write_f32(p.beat);
        w.write_f32(p.value);
    }
}

fn encode_midi_note(w: &mut FbWriter, n: &MidiNote) {
    w.write_u8(n.pitch);
    w.write_f32(n.start_beats);
    w.write_f32(n.duration_beats);
    w.write_u8(n.velocity);
}

fn encode_clip(w: &mut FbWriter, c: &ProjectClip) {
    w.write_str(&c.id);
    w.write_str(&c.name);
    w.write_f64(c.start_beat);
    w.write_f64(c.duration_beats);
    w.write_f32(c.offset_beats);
    w.write_f32(c.gain);
    w.write_bool(c.muted);
    match &c.source {
        ClipSource::Empty => w.write_u8(0),
        ClipSource::Audio {
            asset_id,
            source_path,
        } => {
            w.write_u8(1);
            w.write_str(asset_id);
            w.write_opt_path(source_path);
        }
        ClipSource::Midi { notes } => {
            w.write_u8(2);
            w.write_u32(notes.len() as u32);
            for n in notes {
                encode_midi_note(w, n);
            }
        }
    }
}

fn encode_input_monitor(w: &mut FbWriter, m: InputMonitorMode) {
    w.write_u8(match m {
        InputMonitorMode::Off => 0,
        InputMonitorMode::Always => 1,
        InputMonitorMode::WhenRecordArmed => 2,
    });
}

fn encode_track_type(w: &mut FbWriter, t: ProjectTrackType) {
    w.write_u8(match t {
        ProjectTrackType::Audio => 0,
        ProjectTrackType::Midi => 1,
        ProjectTrackType::Instrument => 2,
        ProjectTrackType::Bus => 3,
        ProjectTrackType::Return => 4,
        ProjectTrackType::Group => 5,
        ProjectTrackType::Master => 6,
    });
}

fn encode_track(w: &mut FbWriter, t: &ProjectTrack) {
    w.write_str(&t.id);
    w.write_str(&t.name);
    encode_track_type(w, t.track_type);
    w.write_str(&t.color_hex);
    w.write_f32(t.volume_norm);
    w.write_f32(t.pan);
    w.write_bool(t.muted);
    w.write_bool(t.solo);
    w.write_bool(t.record_arm);
    encode_input_monitor(w, t.input_monitor);
    // routing
    w.write_opt_str(&t.routing.output_bus);
    w.write_u32(t.routing.sends.len() as u32);
    for s in &t.routing.sends {
        w.write_str(&s.id);
        w.write_str(&s.target_track_id);
        w.write_bool(s.enabled);
        w.write_bool(s.pre_fader);
        w.write_f32(s.gain_db);
    }
    // inserts
    w.write_u32(t.inserts.len() as u32);
    for ins in &t.inserts {
        encode_insert(w, ins);
    }
    // automation lanes
    w.write_u32(t.automation_lanes.len() as u32);
    for lane in &t.automation_lanes {
        encode_automation_lane(w, lane);
    }
    // clips
    w.write_u32(t.clips.len() as u32);
    for c in &t.clips {
        encode_clip(w, c);
    }
}

fn encode_asset(w: &mut FbWriter, a: &ProjectAsset) {
    w.write_str(&a.id);
    w.write_str(&a.original_filename);
    w.write_opt_str(&a.relative_path);
    w.write_opt_path(&a.absolute_path);
    w.write_opt_f64(&a.duration_secs);
    w.write_opt_u32(&a.sample_rate);
    w.write_opt_u8(&a.channels);
}

fn encode_body(project: &FutureboardProject) -> Vec<u8> {
    let mut w = FbWriter::new();

    // Header fields
    w.write_str(&project.id);
    w.write_str(&project.name);
    w.write_u64(project.created_at);
    w.write_u64(project.modified_at);

    // Settings
    w.write_f64(project.settings.bpm);
    w.write_u32(project.settings.time_sig_num);
    w.write_u32(project.settings.time_sig_den);
    w.write_u32(project.settings.sample_rate);
    w.write_u32(project.settings.bit_depth);

    // Mixer
    w.write_f32(project.mixer.master_volume_norm);

    // Tracks
    w.write_u32(project.tracks.len() as u32);
    for t in &project.tracks {
        encode_track(&mut w, t);
    }

    // Assets
    w.write_u32(project.assets.len() as u32);
    for a in &project.assets {
        encode_asset(&mut w, a);
    }

    w.into_bytes()
}

/// Encodes a `FutureboardProject` into the full `.fbproj` binary format.
pub fn encode_project(project: &FutureboardProject) -> Vec<u8> {
    let body = encode_body(project);
    let checksum = crc32fast::hash(&body);

    let mut out = Vec::with_capacity(8 + 4 + 4 + 4 + body.len() + 4);
    out.extend_from_slice(PROJECT_MAGIC);
    out.extend_from_slice(&PROJECT_VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out.extend_from_slice(&checksum.to_le_bytes());
    out
}

// ── Decoding ──────────────────────────────────────────────────────────────────

fn decode_plugin_format(r: &mut FbReader) -> Result<PluginFormat, ProjectError> {
    Ok(match r.read_u8()? {
        0 => PluginFormat::Vst3,
        1 => PluginFormat::Clap,
        2 => PluginFormat::Au,
        3 => PluginFormat::Lv2,
        _ => PluginFormat::Unknown,
    })
}

fn decode_opt_plugin_format(r: &mut FbReader) -> Result<Option<PluginFormat>, ProjectError> {
    match r.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(decode_plugin_format(r)?)),
        t => Err(ProjectError::Corrupted(format!("bad option tag {t}"))),
    }
}

fn decode_plugin_state_blob(r: &mut FbReader) -> Result<PluginStateBlob, ProjectError> {
    Ok(PluginStateBlob {
        plugin_id: r.read_str()?,
        format: decode_opt_plugin_format(r)?,
        state_bytes: r.read_bytes()?,
        vendor: r.read_opt_str()?,
        name: r.read_opt_str()?,
        version: r.read_opt_str()?,
    })
}

fn decode_plugin_instance(r: &mut FbReader) -> Result<ProjectPluginInstance, ProjectError> {
    Ok(ProjectPluginInstance {
        instance_id: r.read_str()?,
        format: decode_plugin_format(r)?,
        plugin_path: r.read_opt_path()?,
        plugin_uid: r.read_str()?,
        display_name: r.read_str()?,
        state: decode_plugin_state_blob(r)?,
    })
}

fn decode_insert(r: &mut FbReader) -> Result<ProjectInsert, ProjectError> {
    let id = r.read_str()?;
    let slot_index = r.read_u32()?;
    let bypassed = r.read_bool()?;
    let plugin = match r.read_u8()? {
        0 => None,
        1 => Some(decode_plugin_instance(r)?),
        t => {
            return Err(ProjectError::Corrupted(format!(
                "bad plugin option tag {t}"
            )))
        }
    };
    Ok(ProjectInsert {
        id,
        slot_index,
        bypassed,
        plugin,
    })
}

fn decode_automation_lane(r: &mut FbReader) -> Result<AutomationLane, ProjectError> {
    let id = r.read_str()?;
    let parameter_name = r.read_str()?;
    let visible = r.read_bool()?;
    let count = r.read_u32()? as usize;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        points.push(AutomationPoint {
            beat: r.read_f32()?,
            value: r.read_f32()?,
        });
    }
    Ok(AutomationLane {
        id,
        parameter_name,
        visible,
        points,
    })
}

fn decode_midi_note(r: &mut FbReader) -> Result<MidiNote, ProjectError> {
    Ok(MidiNote {
        pitch: r.read_u8()?,
        start_beats: r.read_f32()?,
        duration_beats: r.read_f32()?,
        velocity: r.read_u8()?,
    })
}

fn decode_clip(r: &mut FbReader) -> Result<ProjectClip, ProjectError> {
    let id = r.read_str()?;
    let name = r.read_str()?;
    let start_beat = r.read_f64()?;
    let duration_beats = r.read_f64()?;
    let offset_beats = r.read_f32()?;
    let gain = r.read_f32()?;
    let muted = r.read_bool()?;
    let source = match r.read_u8()? {
        0 => ClipSource::Empty,
        1 => ClipSource::Audio {
            asset_id: r.read_str()?,
            source_path: r.read_opt_path()?,
        },
        2 => {
            let count = r.read_u32()? as usize;
            let mut notes = Vec::with_capacity(count);
            for _ in 0..count {
                notes.push(decode_midi_note(r)?);
            }
            ClipSource::Midi { notes }
        }
        t => {
            return Err(ProjectError::Corrupted(format!(
                "unknown clip source tag {t}"
            )))
        }
    };
    Ok(ProjectClip {
        id,
        name,
        start_beat,
        duration_beats,
        offset_beats,
        gain,
        muted,
        source,
    })
}

fn decode_track_type(r: &mut FbReader) -> Result<ProjectTrackType, ProjectError> {
    Ok(match r.read_u8()? {
        0 => ProjectTrackType::Audio,
        1 => ProjectTrackType::Midi,
        2 => ProjectTrackType::Instrument,
        3 => ProjectTrackType::Bus,
        4 => ProjectTrackType::Return,
        5 => ProjectTrackType::Group,
        6 => ProjectTrackType::Master,
        t => return Err(ProjectError::Corrupted(format!("unknown track type {t}"))),
    })
}

fn decode_input_monitor(r: &mut FbReader) -> Result<InputMonitorMode, ProjectError> {
    Ok(match r.read_u8()? {
        0 => InputMonitorMode::Off,
        1 => InputMonitorMode::Always,
        2 => InputMonitorMode::WhenRecordArmed,
        t => {
            return Err(ProjectError::Corrupted(format!(
                "unknown input monitor mode {t}"
            )))
        }
    })
}

fn decode_track(r: &mut FbReader) -> Result<ProjectTrack, ProjectError> {
    let id = r.read_str()?;
    let name = r.read_str()?;
    let track_type = decode_track_type(r)?;
    let color_hex = r.read_str()?;
    let volume_norm = r.read_f32()?;
    let pan = r.read_f32()?;
    let muted = r.read_bool()?;
    let solo = r.read_bool()?;
    let record_arm = r.read_bool()?;
    let input_monitor = decode_input_monitor(r)?;

    let output_bus = r.read_opt_str()?;
    let send_count = r.read_u32()? as usize;
    let mut sends = Vec::with_capacity(send_count);
    for _ in 0..send_count {
        let id = r.read_str()?;
        let target_track_id = r.read_str()?;
        let enabled = r.read_bool()?;
        let pre_fader = r.read_bool()?;
        let gain_db = r.read_f32()?;
        sends.push(ProjectSend {
            id,
            target_track_id,
            enabled,
            pre_fader,
            gain_db,
        });
    }
    let routing = TrackRouting { output_bus, sends };

    let insert_count = r.read_u32()? as usize;
    let mut inserts = Vec::with_capacity(insert_count);
    for _ in 0..insert_count {
        inserts.push(decode_insert(r)?);
    }

    let lane_count = r.read_u32()? as usize;
    let mut automation_lanes = Vec::with_capacity(lane_count);
    for _ in 0..lane_count {
        automation_lanes.push(decode_automation_lane(r)?);
    }

    let clip_count = r.read_u32()? as usize;
    let mut clips = Vec::with_capacity(clip_count);
    for _ in 0..clip_count {
        clips.push(decode_clip(r)?);
    }

    Ok(ProjectTrack {
        id,
        name,
        track_type,
        color_hex,
        volume_norm,
        pan,
        muted,
        solo,
        record_arm,
        input_monitor,
        routing,
        inserts,
        automation_lanes,
        clips,
    })
}

fn decode_asset(r: &mut FbReader) -> Result<ProjectAsset, ProjectError> {
    Ok(ProjectAsset {
        id: r.read_str()?,
        original_filename: r.read_str()?,
        relative_path: r.read_opt_str()?,
        absolute_path: r.read_opt_path()?,
        duration_secs: r.read_opt_f64()?,
        sample_rate: r.read_opt_u32()?,
        channels: r.read_opt_u8()?,
    })
}

fn decode_body(body: &[u8]) -> Result<FutureboardProject, ProjectError> {
    let mut r = FbReader::new(body);

    let id = r.read_str()?;
    let name = r.read_str()?;
    let created_at = r.read_u64()?;
    let modified_at = r.read_u64()?;

    let bpm = r.read_f64()?;
    let time_sig_num = r.read_u32()?;
    let time_sig_den = r.read_u32()?;
    let sample_rate = r.read_u32()?;
    let bit_depth = r.read_u32()?;

    let master_volume_norm = r.read_f32()?;

    let track_count = r.read_u32()? as usize;
    let mut tracks = Vec::with_capacity(track_count);
    for _ in 0..track_count {
        tracks.push(decode_track(&mut r)?);
    }

    let asset_count = r.read_u32()? as usize;
    let mut assets = Vec::with_capacity(asset_count);
    for _ in 0..asset_count {
        assets.push(decode_asset(&mut r)?);
    }

    Ok(FutureboardProject {
        id,
        name,
        created_at,
        modified_at,
        settings: super::ProjectSettings {
            bpm,
            time_sig_num,
            time_sig_den,
            sample_rate,
            bit_depth,
        },
        tracks,
        mixer: ProjectMixer { master_volume_norm },
        assets,
    })
}

/// Decodes a `.fbproj` binary blob into a `FutureboardProject`.
pub fn decode_project(data: &[u8]) -> Result<FutureboardProject, ProjectError> {
    if data.len() < 20 {
        return Err(ProjectError::Corrupted("file too small".into()));
    }

    // Magic
    if &data[0..8] != PROJECT_MAGIC {
        return Err(ProjectError::InvalidMagic);
    }

    // Version
    let version = u32::from_le_bytes(data[8..12].try_into().unwrap());
    if version != PROJECT_VERSION {
        return Err(ProjectError::UnsupportedVersion(version));
    }

    // reserved (4 bytes) — skip
    let body_len = u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize;

    if data.len() < 20 + body_len + 4 {
        return Err(ProjectError::Corrupted("file truncated".into()));
    }

    let body = &data[20..20 + body_len];
    let stored_crc = u32::from_le_bytes(data[20 + body_len..20 + body_len + 4].try_into().unwrap());
    let computed_crc = crc32fast::hash(body);

    if computed_crc != stored_crc {
        return Err(ProjectError::ChecksumMismatch {
            expected: stored_crc,
            got: computed_crc,
        });
    }

    decode_body(body)
}
