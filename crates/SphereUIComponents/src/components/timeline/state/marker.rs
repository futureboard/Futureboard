use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineMarkerState {
    pub id: String,
    pub beat: f64,
    pub name: String,
    pub color_hex: String,
}

impl TimelineMarkerState {
    pub fn new(beat: f64, name: impl Into<String>, color_hex: impl Into<String>) -> Self {
        Self::with_id("", beat, name, color_hex)
    }

    pub fn with_id(
        id: impl Into<String>,
        beat: f64,
        name: impl Into<String>,
        color_hex: impl Into<String>,
    ) -> Self {
        let mut id = id.into();
        if id.is_empty() {
            id = next_timeline_marker_id();
        }
        Self {
            id,
            beat: beat.max(0.0),
            name: name.into(),
            color_hex: color_hex.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineRegionState {
    pub id: String,
    pub start_beat: f64,
    pub end_beat: f64,
    pub name: String,
    pub color_hex: String,
}

impl TimelineRegionState {
    pub fn new(
        start_beat: f64,
        end_beat: f64,
        name: impl Into<String>,
        color_hex: impl Into<String>,
    ) -> Self {
        Self::with_id("", start_beat, end_beat, name, color_hex)
    }

    pub fn with_id(
        id: impl Into<String>,
        start_beat: f64,
        end_beat: f64,
        name: impl Into<String>,
        color_hex: impl Into<String>,
    ) -> Self {
        let mut id = id.into();
        if id.is_empty() {
            id = next_timeline_region_id();
        }
        let (start, end) = if start_beat <= end_beat {
            (start_beat.max(0.0), end_beat.max(0.0))
        } else {
            (end_beat.max(0.0), start_beat.max(0.0))
        };
        Self {
            id,
            start_beat: start,
            end_beat: end.max(start + 1.0e-3),
            name: name.into(),
            color_hex: color_hex.into(),
        }
    }

    pub fn normalized_range(&self) -> (f64, f64) {
        if self.start_beat <= self.end_beat {
            (self.start_beat, self.end_beat)
        } else {
            (self.end_beat, self.start_beat)
        }
    }
}

impl TimelineState {
    pub fn add_marker_at_beat(&mut self, beat: f64) -> String {
        let label = format!("Marker {}", self.markers.len() + 1);
        let marker = TimelineMarkerState::new(beat, label, "#7C5CFF");
        let id = marker.id.clone();
        self.markers.push(marker);
        self.markers
            .sort_by(|a, b| a.beat.total_cmp(&b.beat).then_with(|| a.id.cmp(&b.id)));
        id
    }

    pub fn add_region_at_beat(&mut self, beat: f64) -> String {
        let start = beat.max(0.0);
        let length = self.beats_per_bar_at_beat(start).max(1.0);
        let label = format!("Region {}", self.regions.len() + 1);
        let region = TimelineRegionState::new(start, start + length, label, "#42C7A3");
        let id = region.id.clone();
        self.regions.push(region);
        self.regions.sort_by(|a, b| {
            a.start_beat
                .total_cmp(&b.start_beat)
                .then_with(|| a.id.cmp(&b.id))
        });
        id
    }

    pub fn delete_marker(&mut self, id: &str) -> bool {
        let before = self.markers.len();
        self.markers.retain(|marker| marker.id != id);
        before != self.markers.len()
    }

    pub fn delete_region(&mut self, id: &str) -> bool {
        let before = self.regions.len();
        self.regions.retain(|region| region.id != id);
        before != self.regions.len()
    }

    pub fn update_region_range(&mut self, id: &str, start_beat: f64, end_beat: f64) -> bool {
        let Some(region) = self.regions.iter_mut().find(|region| region.id == id) else {
            return false;
        };
        let updated = TimelineRegionState::with_id(
            region.id.clone(),
            start_beat,
            end_beat,
            region.name.clone(),
            region.color_hex.clone(),
        );
        if (region.start_beat - updated.start_beat).abs() < 1.0e-6
            && (region.end_beat - updated.end_beat).abs() < 1.0e-6
        {
            return false;
        }
        region.start_beat = updated.start_beat;
        region.end_beat = updated.end_beat;
        self.regions.sort_by(|a, b| {
            a.start_beat
                .total_cmp(&b.start_beat)
                .then_with(|| a.id.cmp(&b.id))
        });
        true
    }
}
