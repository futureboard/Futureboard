use super::*;

pub fn beat_to_x(beat: f64, viewport: &TimelineViewport) -> f32 {
    ((beat.max(0.0) as f32) * viewport.pixels_per_beat - viewport.scroll_x).round()
}

pub fn x_to_beat(x: f32, viewport: &TimelineViewport) -> f64 {
    ((x + viewport.scroll_x) / viewport.pixels_per_beat.max(0.0001)).max(0.0) as f64
}

pub fn snap_beat(beat: f64, snap: SnapSettings) -> f64 {
    if !snap.enabled || snap.division == SnapDivision::Off {
        return beat.max(0.0);
    }
    let step = match snap.division {
        SnapDivision::Auto => snap.auto_step_beats,
        SnapDivision::Bar1 => snap.beats_per_bar,
        other => other.step_beats(snap.beats_per_bar as f32) as f64,
    };
    if step <= 0.0 {
        return beat.max(0.0);
    }
    ((beat / step).round() * step).max(0.0)
}

pub fn track_at_y(y: f32, layout: &TrackLayout) -> Option<TrackId> {
    if layout.track_height <= 0.0 {
        return None;
    }
    let index = ((y + layout.scroll_y).max(0.0) / layout.track_height).floor() as usize;
    layout.track_ids.get(index).cloned()
}

pub fn clip_rect(
    clip: &ClipState,
    viewport: &TimelineViewport,
    layout: &TrackLayout,
) -> gpui::Bounds<gpui::Pixels> {
    let x = beat_to_x(clip.start_beat as f64, viewport);
    let w =
        ((clip.duration_beats.max(0.0) as f64 * viewport.pixels_per_beat as f64) as f32).max(1.0);
    let y = -layout.scroll_y;
    gpui::bounds(
        gpui::point(gpui::px(x), gpui::px(y)),
        gpui::size(gpui::px(w), gpui::px(layout.track_height)),
    )
}

impl TimelineState {
    pub fn time_to_content_x(&self, time_sec: f32) -> f32 {
        (time_sec * self.viewport.pixels_per_second - self.viewport.scroll_x).round()
    }

    pub fn content_x_to_time(&self, x: f32) -> f32 {
        ((x + self.viewport.scroll_x) / self.viewport.pixels_per_second).max(0.0)
    }

    pub fn beats_to_x(&self, beats: f32) -> f32 {
        beat_to_x(beats as f64, &self.viewport)
    }

    pub fn x_to_beats(&self, x: f32) -> f32 {
        x_to_beat(x, &self.viewport) as f32
    }

    pub fn beat_to_x(&self, beat: f32) -> f32 {
        self.beats_to_x(beat)
    }

    pub fn x_to_beat(&self, x: f32) -> f64 {
        x_to_beat(x, &self.viewport)
    }

    pub fn lane_y_to_track_id(&self, y: f32) -> Option<TrackId> {
        track_at_y(
            y,
            &TrackLayout::from_tracks(&self.tracks, self.viewport.scroll_y),
        )
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

    /// Snap a beat value to the current grid (or return it unchanged when snap is off).
    pub fn snap_beats(&self, beats: f32) -> f32 {
        let mut snap = SnapSettings::from_timeline(self);
        snap.beats_per_bar = self.beats_per_bar_at_beat(beats as f64);
        snap_beat(beats as f64, snap) as f32
    }
}
