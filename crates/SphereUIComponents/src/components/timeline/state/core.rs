use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct TransportState {
    pub playing: bool,
    pub recording: bool,
    pub metronome_enabled: bool,
    pub playhead_beats: f32,
    pub loop_enabled: bool,
    pub loop_start_beats: f32,
    pub loop_end_beats: f32,
    pub last_engine_frame: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MasterBusState {
    pub volume: f32,
    pub meter_level_l: f32,
    pub meter_level_r: f32,
    /// Held peak levels (slow release) for the master peak-hold tick. UI-only.
    pub meter_peak_hold_l: f32,
    pub meter_peak_hold_r: f32,
    /// Latched master clip indicator. UI-only.
    pub meter_clip: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineState {
    pub bpm: f32,
    /// Project-level tempo automation. Always active and owned by the project;
    /// the TempoTrack (when shown) is only a view/editor over this. When empty
    /// the project plays at the static `bpm`.
    pub tempo_map: TempoMap,
    /// Global time signature markers (authoritative for bar/beat layout).
    pub time_signature_map: TimeSignatureMap,
    /// Timeline markers shown on the arrangement ruler.
    pub markers: Vec<TimelineMarkerState>,
    /// Named timeline regions spanning a beat range.
    pub regions: Vec<TimelineRegionState>,
    /// Legacy single signature — kept in sync with the marker at beat 0 for
    /// templates and engine fallbacks.
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
    pub dragging_track_id: Option<TrackId>,
    pub drag_origin_index: Option<usize>,
    pub drag_current_y: f32,
    pub drag_target_index: Option<usize>,
    /// True when the timeline viewport should follow the playhead during
    /// playback. Toggled off temporarily when the user manually scrolls or
    /// drags the viewport; can be re-enabled from the Follow button.
    pub follow_playhead: bool,
    pub auto_scroll_mode: AutoScrollMode,
    /// Arrangement time-range selection in beats. UI-only; never marks the
    /// project or engine dirty by itself.
    pub arrangement_range: Option<TimelineRangeSelection>,
    /// When true, the global Tempo Track lane is shown below the ruler.
    pub show_tempo_track: bool,
    /// Compact collapsed height for the Tempo Track lane header/curve.
    pub tempo_track_collapsed: bool,
    /// Selected tempo marker on the Tempo Track (stable persisted id).
    pub selected_tempo_point_id: Option<String>,
    pub show_time_signature_track: bool,
    pub time_signature_track_collapsed: bool,
    pub selected_time_signature_point_id: Option<String>,
}

impl Default for TimelineState {
    /// Clean, empty project. No tracks, no clips, no MIDI — the real runtime
    /// startup state. Use [`TimelineState::demo_project`] when you explicitly
    /// want the seeded demo content (development / screenshots).
    fn default() -> Self {
        Self {
            bpm: 120.0,
            tempo_map: TempoMap::new(),
            time_signature_map: TimeSignatureMap::with_default_4_4(),
            markers: Vec::new(),
            regions: Vec::new(),
            time_signature_num: 4,
            time_signature_den: 4,
            viewport: TimelineViewport {
                scroll_x: 0.0,
                scroll_y: 0.0,
                target_scroll_x: 0.0,
                target_scroll_y: 0.0,
                pixels_per_second: 150.0,
                pixels_per_beat: 75.0,
                viewport_width: 0.0,
                viewport_height: 500.0,
                track_area_height: 500.0,
            },
            transport: TransportState {
                playing: false,
                recording: false,
                metronome_enabled: false,
                playhead_beats: 0.0,
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 16.0,
                last_engine_frame: 0,
            },
            tracks: Vec::new(),
            master: MasterBusState {
                volume: volume::db_to_norm(0.0),
                meter_level_l: 0.0,
                meter_level_r: 0.0,
                meter_peak_hold_l: 0.0,
                meter_peak_hold_r: 0.0,
                meter_clip: false,
            },
            selection: TimelineSelection {
                selected_track_id: None,
                selected_clip_ids: Vec::new(),
            },
            active_tool: TimelineTool::Pointer,
            snap_to_grid: true,
            grid_division: SnapDivision::Div1_16,
            dragging_track_id: None,
            drag_origin_index: None,
            drag_current_y: 0.0,
            drag_target_index: None,
            follow_playhead: true,
            auto_scroll_mode: AutoScrollMode::Page,
            arrangement_range: None,
            show_tempo_track: false,
            tempo_track_collapsed: false,
            selected_tempo_point_id: None,
            show_time_signature_track: false,
            time_signature_track_collapsed: false,
            selected_time_signature_point_id: None,
        }
    }
}

// ── Time conversions and coordinate helpers ───────────────────────────────────────

pub const HEADER_WIDTH: f32 = 320.0; // Keep it slightly wider for native controls

pub const TRACK_HEIGHT: f32 = 76.0;

pub const RULER_HEIGHT: f32 = 30.0;

pub type TrackId = String;

impl TimelineState {
    pub fn seconds_per_beat(&self) -> f32 {
        60.0 / self.bpm.max(1.0)
    }

    pub fn seconds_to_beats(&self, seconds: f64) -> f32 {
        (seconds * self.bpm.max(1.0) as f64 / 60.0) as f32
    }

    pub fn beats_to_seconds(&self, beats: f32) -> f32 {
        beats * self.seconds_per_beat()
    }

    /// Y offset from the timeline top to the track-list content area.
    pub fn arrangement_content_top(&self) -> f32 {
        RULER_HEIGHT + self.global_lanes_height()
    }
}
