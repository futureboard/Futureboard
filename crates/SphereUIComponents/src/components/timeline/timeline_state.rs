use gpui::rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackType {
    Audio,
    Midi,
    Instrument,
    Master,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MidiNoteState {
    pub pitch: u8,
    pub start: f32, // beats relative to clip start
    pub duration: f32, // beats
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipType {
    Audio {
        file_id: String,
        /// Absolute path to the decoded source file, if this clip was created
        /// by importing a real audio file. Used as the waveform cache key.
        source_path: Option<String>,
    },
    Midi { notes: Vec<MidiNoteState> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipState {
    pub id: String,
    pub name: String,
    pub start_beat: f32,
    pub duration_beats: f32,
    pub offset_beats: f32,
    pub gain: f32,
    pub clip_type: ClipType,
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationPoint {
    pub beat: f32,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutomationLaneState {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub points: Vec<AutomationPoint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackState {
    pub id: String,
    pub name: String,
    pub track_type: TrackType,
    pub color: gpui::Rgba,
    /// Normalized fader position in `0.0..=1.0`. `1.0` is the top of the fader
    /// (≈ +6 dB) and `0.0` is the bottom (≈ -60 dB). See `Volume::norm_to_db`.
    pub volume: f32,
    /// Pan position in `-1.0..=1.0`. `-1.0` is hard left, `+1.0` is hard right.
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    pub armed: bool,
    /// Whether the track is monitoring its input. UI-only for now (no audio).
    pub input_monitor: bool,
    /// Latest peak meter levels in `0.0..=1.0`. Currently a static placeholder
    /// per track; will be driven by the audio engine when that lands.
    pub meter_level_l: f32,
    pub meter_level_r: f32,
    pub clips: Vec<ClipState>,
    pub automation_lanes: Vec<AutomationLaneState>,
}

/// Volume / dB mapping helpers. Linear in dB between the soft floor and a
/// little headroom above unity.
pub mod volume {
    pub const MIN_DB: f32 = -60.0;
    pub const MAX_DB: f32 = 6.0;

    pub fn norm_to_db(norm: f32) -> f32 {
        let n = norm.clamp(0.0, 1.0);
        MIN_DB + n * (MAX_DB - MIN_DB)
    }

    pub fn db_to_norm(db: f32) -> f32 {
        ((db - MIN_DB) / (MAX_DB - MIN_DB)).clamp(0.0, 1.0)
    }

    pub fn format_db(norm: f32) -> String {
        let db = norm_to_db(norm);
        if norm <= 0.001 || db <= MIN_DB + 0.05 {
            "-∞".to_string()
        } else if db >= 0.0 {
            format!("+{:.1}", db)
        } else {
            format!("{:.1}", db)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineTool {
    Pointer,
    Pen,
    Cut,
    Glue,
    Mute,
    Time,
    Automation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineViewport {
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub pixels_per_second: f32,
    pub track_area_height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransportState {
    pub playhead_beats: f32,
    pub loop_enabled: bool,
    pub loop_start_beats: f32,
    pub loop_end_beats: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineSelection {
    pub selected_track_id: Option<String>,
    pub selected_clip_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapDivision {
    Auto,
    Off,
    Bar1,
    Div1_1,
    Div1_2,
    Div1_4,
    Div1_8,
    Div1_16,
    Div1_32,
    Div1_64,
}

impl SnapDivision {
    pub fn label(&self) -> &'static str {
        match self {
            SnapDivision::Auto => "Auto",
            SnapDivision::Off => "Off",
            SnapDivision::Bar1 => "1 bar",
            SnapDivision::Div1_1 => "1/1",
            SnapDivision::Div1_2 => "1/2",
            SnapDivision::Div1_4 => "1/4",
            SnapDivision::Div1_8 => "1/8",
            SnapDivision::Div1_16 => "1/16",
            SnapDivision::Div1_32 => "1/32",
            SnapDivision::Div1_64 => "1/64",
        }
    }

    pub fn step_beats(&self, bpb: f32) -> f32 {
        match self {
            SnapDivision::Auto => 0.0,
            SnapDivision::Off => 0.0,
            SnapDivision::Bar1 => bpb,
            SnapDivision::Div1_1 => 4.0,
            SnapDivision::Div1_2 => 2.0,
            SnapDivision::Div1_4 => 1.0,
            SnapDivision::Div1_8 => 0.5,
            SnapDivision::Div1_16 => 0.25,
            SnapDivision::Div1_32 => 0.125,
            SnapDivision::Div1_64 => 0.0625,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridLineLevel {
    Bar,
    Beat,
    Sub,
}

pub struct GridLine {
    pub x: f32,
    pub beat: f32,
    pub level: GridLineLevel,
    pub show_label: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MasterBusState {
    pub volume: f32,
    pub meter_level_l: f32,
    pub meter_level_r: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineState {
    pub bpm: f32,
    pub time_signature_num: u32,
    pub time_signature_den: u32,
    pub viewport: TimelineViewport,
    pub transport: TransportState,
    pub tracks: Vec<TrackState>,
    pub master: MasterBusState,
    pub selection: TimelineSelection,
    pub active_tool: TimelineTool,
    pub snap_to_grid: bool,
    pub grid_division: SnapDivision,
}

impl Default for TimelineState {
    /// Clean, empty project. No tracks, no clips, no MIDI — the real runtime
    /// startup state. Use [`TimelineState::demo_project`] when you explicitly
    /// want the seeded demo content (development / screenshots).
    fn default() -> Self {
        Self {
            bpm: 120.0,
            time_signature_num: 4,
            time_signature_den: 4,
            viewport: TimelineViewport {
                scroll_x: 0.0,
                scroll_y: 0.0,
                pixels_per_second: 150.0,
                track_area_height: 500.0,
            },
            transport: TransportState {
                playhead_beats: 0.0,
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 16.0,
            },
            tracks: Vec::new(),
            master: MasterBusState {
                volume: volume::db_to_norm(0.0),
                meter_level_l: 0.0,
                meter_level_r: 0.0,
            },
            selection: TimelineSelection {
                selected_track_id: None,
                selected_clip_ids: Vec::new(),
            },
            active_tool: TimelineTool::Pointer,
            snap_to_grid: true,
            grid_division: SnapDivision::Div1_16,
        }
    }
}

impl TimelineState {
    /// Seeded demo project — three tracks with synthetic audio/MIDI clips.
    /// Intended for development, screenshots, and the demo seed flag in the
    /// app entry point; never used by the real runtime default.
    pub fn demo_project() -> Self {
        let track1 = TrackState {
            id: "track-1".to_string(),
            name: "Audio 1".to_string(),
            track_type: TrackType::Audio,
            color: rgb(0x56C7C9), // Teal
            volume: volume::db_to_norm(-3.0),
            pan: 0.0,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: false,
            meter_level_l: 0.62,
            meter_level_r: 0.68,
            clips: vec![
                ClipState {
                    id: "clip-1".to_string(),
                    name: "vocals_dry.wav".to_string(),
                    start_beat: 1.0,
                    duration_beats: 8.0,
                    offset_beats: 0.0,
                    gain: 1.0,
                    clip_type: ClipType::Audio {
                        file_id: "file-vocals-dry".to_string(),
                        source_path: None,
                    },
                    muted: false,
                },
                ClipState {
                    id: "clip-2".to_string(),
                    name: "vocals_harmony.wav".to_string(),
                    start_beat: 10.0,
                    duration_beats: 6.0,
                    offset_beats: 0.0,
                    gain: 1.0,
                    clip_type: ClipType::Audio {
                        file_id: "file-vocals-harmony".to_string(),
                        source_path: None,
                    },
                    muted: false,
                },
            ],
            automation_lanes: vec![AutomationLaneState {
                id: "lane-1".to_string(),
                name: "Volume".to_string(),
                visible: false,
                points: vec![
                    AutomationPoint { beat: 0.0, value: 0.8 },
                    AutomationPoint { beat: 4.0, value: 0.5 },
                    AutomationPoint { beat: 8.0, value: 0.8 },
                ],
            }],
        };

        let track2 = TrackState {
            id: "track-2".to_string(),
            name: "Audio 2".to_string(),
            track_type: TrackType::Audio,
            color: rgb(0x7EDB9A), // Green
            volume: volume::db_to_norm(-6.0),
            pan: -0.2,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: false,
            meter_level_l: 0.42,
            meter_level_r: 0.48,
            clips: vec![ClipState {
                id: "clip-3".to_string(),
                name: "drums_loop_120.wav".to_string(),
                start_beat: 0.0,
                duration_beats: 16.0,
                offset_beats: 0.0,
                gain: 1.0,
                clip_type: ClipType::Audio {
                    file_id: "file-drums-loop".to_string(),
                    source_path: None,
                },
                muted: false,
            }],
            automation_lanes: vec![],
        };

        let track3 = TrackState {
            id: "track-3".to_string(),
            name: "Synth 3".to_string(),
            track_type: TrackType::Midi,
            color: rgb(0xF2C96D), // Yellow
            volume: volume::db_to_norm(-1.5),
            pan: 0.3,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: false,
            meter_level_l: 0.15,
            meter_level_r: 0.12,
            clips: vec![ClipState {
                id: "clip-4".to_string(),
                name: "synth_lead.mid".to_string(),
                start_beat: 4.0,
                duration_beats: 8.0,
                offset_beats: 0.0,
                gain: 1.0,
                clip_type: ClipType::Midi {
                    notes: vec![
                        MidiNoteState { pitch: 60, start: 0.0, duration: 1.0 },
                        MidiNoteState { pitch: 64, start: 1.0, duration: 1.0 },
                        MidiNoteState { pitch: 67, start: 2.0, duration: 1.0 },
                        MidiNoteState { pitch: 72, start: 3.0, duration: 2.0 },
                        MidiNoteState { pitch: 67, start: 5.0, duration: 1.0 },
                        MidiNoteState { pitch: 64, start: 6.0, duration: 1.0 },
                        MidiNoteState { pitch: 60, start: 7.0, duration: 1.0 },
                    ],
                },
                muted: false,
            }],
            automation_lanes: vec![],
        };

        Self {
            bpm: 120.0,
            time_signature_num: 4,
            time_signature_den: 4,
            viewport: TimelineViewport {
                scroll_x: 0.0,
                scroll_y: 0.0,
                pixels_per_second: 150.0, // Default zoom level in Web UI
                track_area_height: 500.0,
            },
            transport: TransportState {
                playhead_beats: 2.0,
                loop_enabled: true,
                loop_start_beats: 0.0,
                loop_end_beats: 16.0,
            },
            tracks: vec![track1, track2, track3],
            master: MasterBusState {
                volume: volume::db_to_norm(0.0),
                meter_level_l: 0.0,
                meter_level_r: 0.0,
            },
            selection: TimelineSelection {
                selected_track_id: Some("track-1".to_string()),
                selected_clip_ids: vec![],
            },
            active_tool: TimelineTool::Pointer,
            snap_to_grid: true,
            grid_division: SnapDivision::Div1_16,
        }
    }

    /// Mutating variant of [`TimelineState::demo_project`]. Seeds the given
    /// state with demo content in-place.
    pub fn seed_demo_content(&mut self) {
        *self = Self::demo_project();
    }
}

// ── Time conversions and coordinate helpers ───────────────────────────────────────

pub const HEADER_WIDTH: f32 = 320.0; // Keep it slightly wider for native controls
pub const TRACK_HEIGHT: f32 = 76.0;
pub const RULER_HEIGHT: f32 = 30.0;

impl TimelineState {
    pub fn seconds_per_beat(&self) -> f32 {
        60.0 / self.bpm.max(1.0)
    }

    pub fn seconds_to_beats(&self, seconds: f32) -> f32 {
        seconds / self.seconds_per_beat()
    }

    pub fn beats_to_seconds(&self, beats: f32) -> f32 {
        beats * self.seconds_per_beat()
    }

    pub fn beats_per_bar(&self) -> f32 {
        self.time_signature_num as f32 * (4.0 / self.time_signature_den as f32)
    }

    pub fn time_to_content_x(&self, time_sec: f32) -> f32 {
        (time_sec * self.viewport.pixels_per_second - self.viewport.scroll_x).round()
    }

    pub fn content_x_to_time(&self, x: f32) -> f32 {
        ((x + self.viewport.scroll_x) / self.viewport.pixels_per_second).max(0.0)
    }

    pub fn beats_to_x(&self, beats: f32) -> f32 {
        self.time_to_content_x(beats * self.seconds_per_beat())
    }

    pub fn x_to_beats(&self, x: f32) -> f32 {
        self.seconds_to_beats(self.content_x_to_time(x))
    }

    pub fn get_visible_beat_range(&self, width: f32) -> (f32, f32) {
        let start = self.x_to_beats(0.0);
        let end = self.x_to_beats(width);
        (start, end)
    }

    pub fn build_interval_list(&self) -> Vec<f32> {
        let bpb = self.beats_per_bar();
        let mut result = Vec::new();
        for &sub in &[1.0 / 32.0, 1.0 / 16.0, 1.0 / 8.0, 1.0 / 4.0, 1.0 / 2.0, 1.0, 2.0] {
            if sub < bpb {
                result.push(sub);
            }
        }
        for &mult in &[1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0] {
            result.push(bpb * mult);
        }
        result
    }

    pub fn get_grid_interval_beats(&self, ppb: f32) -> f32 {
        let min_beats = 100.0 / ppb.max(1.0);
        let intervals = self.build_interval_list();
        for &n in &intervals {
            if n >= min_beats {
                return n;
            }
        }
        *intervals.last().unwrap_or(&4.0)
    }

    pub fn get_grid_sub_beats(&self, ppb: f32) -> f32 {
        let _bpb = self.beats_per_bar();
        let interval = self.get_grid_interval_beats(ppb);
        let intervals = self.build_interval_list();
        if let Some(idx) = intervals.iter().position(|&x| x == interval) {
            if idx > 0 {
                return intervals[idx - 1];
            }
        }
        interval
    }

    pub fn snap_time(&self, seconds: f32) -> f32 {
        if !self.snap_to_grid || self.grid_division == SnapDivision::Off {
            return seconds;
        }
        let ppb = self.viewport.pixels_per_second * self.seconds_per_beat();
        let bpb = self.beats_per_bar();
        let sub_div = match self.grid_division {
            SnapDivision::Auto => self.get_grid_sub_beats(ppb),
            SnapDivision::Bar1 => bpb,
            _ => self.grid_division.step_beats(bpb),
        };
        if sub_div <= 0.0 {
            return seconds;
        }
        let spb = self.seconds_per_beat();
        let total_beats = seconds / spb;
        let snapped = (total_beats / sub_div).round() * sub_div;
        (snapped * spb).max(0.0)
    }

    pub fn get_arrangement_grid_lines(
        &self,
        viewport_width: f32,
    ) -> Vec<GridLine> {
        let ppb = self.viewport.pixels_per_second * self.seconds_per_beat();
        let bpb = self.beats_per_bar();
        let base_sub = match self.grid_division {
            SnapDivision::Auto => self.get_grid_sub_beats(ppb),
            SnapDivision::Off => self.get_grid_sub_beats(ppb),
            SnapDivision::Bar1 => bpb,
            _ => self.grid_division.step_beats(bpb),
        };
        let mut sub = base_sub;
        while sub * ppb < 4.0 {
            sub *= 2.0;
        }
        let interval = self.get_grid_interval_beats(ppb);
        let eps = sub * 0.01;

        let start_beat = self.viewport.scroll_x / ppb;
        let end_beat = (self.viewport.scroll_x + viewport_width) / ppb;
        let first = (start_beat / sub).floor() * sub;

        let mut lines = Vec::new();
        let limit = end_beat + sub;
        let mut beat = first;
        while beat <= limit {
            let rb = (beat * 100000.0).round() / 100000.0;
            let x = (rb * ppb - self.viewport.scroll_x).round();

            // Bar boundary - beat is a multiple of bpb
            let mod_bar = ((rb % bpb) + bpb) % bpb;
            let is_bar = mod_bar < eps || mod_bar > bpb - eps;

            // Quarter-note beat boundary
            let mod_qn = ((rb % 1.0) + 1.0) % 1.0;
            let is_beat = !is_bar && (mod_qn < eps || mod_qn > 1.0 - eps);

            // Label
            let mod_lbl = ((rb % interval) + interval) % interval;
            let is_label = mod_lbl < eps || mod_lbl > interval - eps;

            lines.push(GridLine {
                x,
                beat: rb,
                level: if is_bar {
                    GridLineLevel::Bar
                } else if is_beat {
                    GridLineLevel::Beat
                } else {
                    GridLineLevel::Sub
                },
                show_label: is_label,
            });

            beat += sub;
        }
        lines
    }

    pub fn format_bar_beat(&self, beats: f32) -> String {
        let bpb = self.beats_per_bar();
        let bar = (beats / bpb).floor() as i32 + 1;
        let beat = (beats % bpb).floor() as i32 + 1;
        format!("{}.{}", bar, beat)
    }

    // ── Identity helpers ─────────────────────────────────────────────────────

    pub fn next_track_id(&self) -> String {
        // Find the highest numeric suffix on "track-N" ids, plus one.
        let mut n = 0u32;
        for t in &self.tracks {
            if let Some(rest) = t.id.strip_prefix("track-") {
                if let Ok(v) = rest.parse::<u32>() { if v > n { n = v; } }
            }
        }
        format!("track-{}", n + 1)
    }

    pub fn next_clip_id(&self) -> String {
        let mut n = 0u32;
        for t in &self.tracks {
            for c in &t.clips {
                if let Some(rest) = c.id.strip_prefix("clip-") {
                    if let Ok(v) = rest.parse::<u32>() { if v > n { n = v; } }
                }
            }
        }
        format!("clip-{}", n + 1)
    }

    pub fn track_index_at_y(&self, y: f32) -> Option<usize> {
        if y < 0.0 { return None; }
        let idx = (y / TRACK_HEIGHT).floor() as usize;
        if idx < self.tracks.len() { Some(idx) } else { None }
    }

    /// Snap a beat value to the current grid (or return it unchanged when snap is off).
    pub fn snap_beats(&self, beats: f32) -> f32 {
        let snapped_sec = self.snap_time(beats * self.seconds_per_beat());
        snapped_sec / self.seconds_per_beat()
    }

    /// Create a new audio track with auto-assigned id/color.
    pub fn create_audio_track(&mut self) -> String {
        let id = self.next_track_id();
        let palette = [0x56C7C9_u32, 0x7EDB9A, 0xF2C96D, 0xC290F0, 0xF49AC2, 0x83B8FF];
        let color = gpui::rgb(palette[self.tracks.len() % palette.len()]);
        let name = format!("Audio {}", self.tracks.len() + 1);
        self.tracks.push(TrackState {
            id: id.clone(),
            name,
            track_type: TrackType::Audio,
            color,
            volume: volume::db_to_norm(0.0),
            pan: 0.0,
            muted: false,
            solo: false,
            armed: false,
            input_monitor: false,
            meter_level_l: 0.0,
            meter_level_r: 0.0,
            clips: Vec::new(),
            automation_lanes: Vec::new(),
        });
        id
    }

    // ── Single-source-of-truth mutations ─────────────────────────────────────
    // These are the only paths that should mutate per-track UI state. Both the
    // timeline TrackHeader and the bottom-panel Mixer call into these, so the
    // two views can never drift apart.

    pub fn set_master_volume(&mut self, norm: f32) {
        self.master.volume = norm.clamp(0.0, 1.0);
    }

    pub fn set_track_volume(&mut self, track_id: &str, norm: f32) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.volume = norm.clamp(0.0, 1.0);
        }
    }

    pub fn set_track_pan(&mut self, track_id: &str, pan: f32) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.pan = pan.clamp(-1.0, 1.0);
        }
    }

    pub fn toggle_track_mute(&mut self, track_id: &str) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.muted = !t.muted;
        }
    }

    pub fn toggle_track_solo(&mut self, track_id: &str) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.solo = !t.solo;
        }
    }

    pub fn toggle_track_arm(&mut self, track_id: &str) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.armed = !t.armed;
        }
    }

    pub fn toggle_track_input_monitor(&mut self, track_id: &str) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.input_monitor = !t.input_monitor;
        }
    }

    pub fn select_track(&mut self, track_id: &str) {
        self.selection.selected_track_id = Some(track_id.to_string());
        self.selection.selected_clip_ids.clear();
    }

    pub fn select_clip(&mut self, clip_id: &str) {
        self.selection.selected_clip_ids = vec![clip_id.to_string()];
        if let Some(track) = self
            .tracks
            .iter()
            .find(|t| t.clips.iter().any(|c| c.id == clip_id))
        {
            self.selection.selected_track_id = Some(track.id.clone());
        }
    }

    pub fn find_track(&self, track_id: &str) -> Option<&TrackState> {
        self.tracks.iter().find(|t| t.id == track_id)
    }

    pub fn find_clip(&self, clip_id: &str) -> Option<(&TrackState, &ClipState)> {
        for t in &self.tracks {
            if let Some(c) = t.clips.iter().find(|c| c.id == clip_id) {
                return Some((t, c));
            }
        }
        None
    }

    /// Multiplicative zoom around a content-space x anchor. Updates
    /// `pixels_per_second` and shifts `scroll_x` so the time under `anchor_x`
    /// (a screen-space x inside the track lane, already net of header) stays
    /// fixed under the cursor. Smooth — accepts arbitrary positive `factor`.
    pub fn zoom_by(&mut self, factor: f32, anchor_x: f32) {
        let factor = factor.max(0.0001);
        let old_pps = self.viewport.pixels_per_second.max(0.0001);
        let new_pps = (old_pps * factor).clamp(4.0, 4000.0);
        if (new_pps - old_pps).abs() < 0.0001 {
            return;
        }
        // Time under anchor before the change.
        let anchor_time = (anchor_x + self.viewport.scroll_x) / old_pps;
        self.viewport.pixels_per_second = new_pps;
        // Re-solve scroll_x so the same anchor_time lands under anchor_x.
        let new_scroll = (anchor_time * new_pps - anchor_x).max(0.0);
        self.viewport.scroll_x = new_scroll;
    }

    /// Drop a clip onto the timeline. `drop_x` and `drop_y` are in the track
    /// area coordinate system (header_width and ruler_height already stripped).
    /// `duration_seconds` is used to compute clip length; if 0, falls back to 2 bars.
    pub fn import_audio_at(
        &mut self,
        source_path: String,
        clip_name: String,
        drop_x: f32,
        drop_y: f32,
        duration_seconds: f32,
    ) -> String {
        // Resolve target track: an existing lane under drop_y, otherwise create one.
        let track_id = match self.track_index_at_y(drop_y) {
            Some(idx) if matches!(self.tracks[idx].track_type, TrackType::Audio) => {
                self.tracks[idx].id.clone()
            }
            _ => self.create_audio_track(),
        };

        // Resolve start beat with snap.
        let raw_beats = self.x_to_beats(drop_x.max(0.0));
        let start_beat = self.snap_beats(raw_beats).max(0.0);

        // Length: prefer decoded duration; fallback to 8 beats.
        let duration_beats = if duration_seconds > 0.0 {
            self.seconds_to_beats(duration_seconds)
        } else {
            8.0
        };

        let clip_id = self.next_clip_id();
        let new_clip = ClipState {
            id: clip_id.clone(),
            name: clip_name,
            start_beat,
            duration_beats,
            offset_beats: 0.0,
            gain: 1.0,
            clip_type: ClipType::Audio {
                file_id: source_path.clone(),
                source_path: Some(source_path),
            },
            muted: false,
        };

        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            t.clips.push(new_clip);
        }

        // Select the new clip.
        self.selection.selected_track_id = Some(track_id);
        self.selection.selected_clip_ids = vec![clip_id.clone()];
        clip_id
    }
}
