#![allow(dead_code)]

/// Represents a physical display rectangle.
#[derive(Debug, Clone)]
pub struct DisplayRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl DisplayRect {
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        x >= self.x
            && x < self.x + self.width
            && y >= self.y
            && y < self.y + self.height
    }

    /// Clamp a window top-left so it remains partially visible on this display.
    pub fn clamp_window(&self, wx: f32, wy: f32) -> (f32, f32) {
        let margin = 50.0;
        let x = wx
            .max(self.x - margin)
            .min(self.x + self.width - margin);
        let y = wy.max(self.y).min(self.y + self.height - margin);
        (x, y)
    }
}

/// Return a conservative primary display estimate.
/// In the MVP we can't easily query monitor info from egui/winit without
/// deeper integration, so we fall back to a large safe region.
pub fn primary_display() -> DisplayRect {
    DisplayRect {
        x: 0.0,
        y: 0.0,
        width: 3840.0,
        height: 2160.0,
    }
}

/// Clamp window position so it is never fully off-screen.
/// Supports negative x (left of primary, second monitor on left).
pub fn clamp_to_safe_area(x: f32, y: f32) -> (f32, f32) {
    let x = x.max(-3840.0).min(7680.0);
    let y = y.max(-100.0).min(4320.0);
    (x, y)
}
