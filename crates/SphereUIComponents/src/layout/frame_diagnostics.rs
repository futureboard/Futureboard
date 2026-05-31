use std::time::{Duration, Instant};

/// Rolling UI repaint diagnostics.
///
/// Counts how often `Render` runs and how far apart those calls are
/// — i.e. effective UI frame cadence, not unconditional display
/// refresh. When the app is idle (nothing dirty), `Render` is not
/// called and the readout stops updating; the `idle_after` check
/// in `hud` decays the displayed FPS to 0.
pub(super) struct FrameDiagnostics {
    last_frame: Option<Instant>,
    /// Start of the current 1-second accumulation window.
    window_start: Instant,
    /// Frame samples + frame-time aggregates collected this window.
    window_frames: u64,
    window_total_ms: f32,
    window_max_ms: f32,
    /// Stable readout refreshed once per second (the status-bar perf monitor).
    /// Updating only at the window boundary keeps the numbers from jittering
    /// every frame.
    displayed_fps: f32,
    displayed_avg_ms: f32,
    displayed_peak_ms: f32,
    has_sample: bool,
    log_to_stderr: bool,
}

impl FrameDiagnostics {
    /// How often the displayed perf readout refreshes.
    const WINDOW: Duration = Duration::from_secs(1);

    pub(super) fn new() -> Self {
        Self {
            last_frame: None,
            window_start: Instant::now(),
            window_frames: 0,
            window_total_ms: 0.0,
            window_max_ms: 0.0,
            displayed_fps: 0.0,
            displayed_avg_ms: 0.0,
            displayed_peak_ms: 0.0,
            has_sample: false,
            log_to_stderr: std::env::var_os("FUTUREBOARD_FRAME_DIAG").is_some(),
        }
    }

    pub(super) fn tick(&mut self, reason: &str) {
        let now = Instant::now();
        if let Some(prev) = self.last_frame {
            let dt = now.duration_since(prev).as_secs_f32() * 1000.0;
            // Drop absurd intervals: first frame after a long idle, or a
            // debugger pause. Anything > 1 s is not a repaint cadence sample.
            if dt > 0.0 && dt < 1000.0 {
                self.window_frames += 1;
                self.window_total_ms += dt;
                if dt > self.window_max_ms {
                    self.window_max_ms = dt;
                }
            }
        }
        self.last_frame = Some(now);

        // Roll the window once per second: recompute the displayed fps / avg /
        // peak from this window's samples, then reset. Render is only called
        // when something is dirty, so during idle the window simply doesn't roll
        // and the last readout stays put (no false 0-fps flicker mid-window).
        let elapsed = now.duration_since(self.window_start);
        if elapsed >= Self::WINDOW {
            let secs = elapsed.as_secs_f32().max(0.001);
            if self.window_frames > 0 {
                self.displayed_fps = self.window_frames as f32 / secs;
                self.displayed_avg_ms = self.window_total_ms / self.window_frames as f32;
                self.displayed_peak_ms = self.window_max_ms;
                self.has_sample = true;
            } else {
                self.displayed_fps = 0.0;
            }
            if self.log_to_stderr {
                eprintln!(
                    "[frame] {:.1} fps  {:.2} ms avg  {:.2} ms peak  reason={}  frames={}",
                    self.displayed_fps,
                    self.displayed_avg_ms,
                    self.displayed_peak_ms,
                    reason,
                    self.window_frames
                );
            }
            self.window_start = now;
            self.window_frames = 0;
            self.window_total_ms = 0.0;
            self.window_max_ms = 0.0;
        }
    }

    /// Status-bar perf monitor: fps, average frame time, and the worst frame
    /// (peak) over the last second. Refreshes at 1 Hz.
    pub(super) fn hud(&self) -> String {
        if !self.has_sample {
            return "— fps".to_string();
        }
        format!(
            "{:.0} fps  {:.1} ms  peak {:.1} ms",
            self.displayed_fps, self.displayed_avg_ms, self.displayed_peak_ms
        )
    }
}
