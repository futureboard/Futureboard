//! Deterministic, display-synced frame scheduler.
//!
//! GPUI repaints on demand (a `cx.notify()` / `app.notify(id)` schedules one
//! frame); there is no continuous render loop. The only thing that drives
//! *continuous* repaints is the audio poll loop in
//! [`crate::layout`] (`spawn_audio_poll`), which historically slept a hardcoded
//! 16 ms (~60 Hz). This module replaces that with a cadence that is a **pure
//! function** of `(mode, detected refresh rate, frame class)`:
//!
//! * default to the monitor refresh rate ([`FrameRateMode::DisplaySync`]),
//! * offer fixed caps + a battery saver for settings/debug,
//! * never feed measured frame timing back into the interval, so the cadence
//!   cannot oscillate / jitter.
//!
//! Idle is unchanged: the poll loop only notifies on state change, so when
//! nothing is dirty no frames are scheduled regardless of the configured rate.
//!
//! The refresh rate is queried once from the OS and cached. On Windows that is
//! `EnumDisplaySettingsW(...).dmDisplayFrequency`; everywhere else (and on any
//! query failure) it falls back to 60 Hz. The detected value is clamped to
//! `30..=240` Hz.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// Frame rate behaviour. `DisplaySync` is the default and tracks the monitor
/// refresh rate; the fixed modes and battery saver are for settings/debug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FrameRateMode {
    /// Track the detected monitor refresh rate (clamped 30..=240 Hz).
    DisplaySync,
    Fixed60,
    Fixed120,
    Fixed144,
    /// As fast as the poll loop allows (1 ms floor). Debug only.
    Unlimited,
    /// 30 FPS, or refresh/2 when that is lower. Reduces power on laptops.
    BatterySaver,
}

impl Default for FrameRateMode {
    fn default() -> Self {
        FrameRateMode::DisplaySync
    }
}

impl FrameRateMode {
    pub fn label(self) -> &'static str {
        match self {
            FrameRateMode::DisplaySync => "Display Sync",
            FrameRateMode::Fixed60 => "60 FPS",
            FrameRateMode::Fixed120 => "120 FPS",
            FrameRateMode::Fixed144 => "144 FPS",
            FrameRateMode::Unlimited => "Unlimited",
            FrameRateMode::BatterySaver => "Battery Saver",
        }
    }

    /// All modes in display order (settings dropdown / round-trip tests).
    pub fn all() -> [FrameRateMode; 6] {
        [
            FrameRateMode::DisplaySync,
            FrameRateMode::Fixed60,
            FrameRateMode::Fixed120,
            FrameRateMode::Fixed144,
            FrameRateMode::Unlimited,
            FrameRateMode::BatterySaver,
        ]
    }

    /// Debug override via `FUTUREBOARD_FRAME_RATE_MODE`
    /// (`displaysync|fixed60|fixed120|fixed144|unlimited|battery`). Takes
    /// precedence over the persisted setting so a session can be pinned without
    /// touching the settings file.
    pub fn from_env() -> Option<FrameRateMode> {
        let raw = std::env::var("FUTUREBOARD_FRAME_RATE_MODE").ok()?;
        match raw.trim().to_ascii_lowercase().as_str() {
            "displaysync" | "display" | "display-sync" | "refresh" => {
                Some(FrameRateMode::DisplaySync)
            }
            "fixed60" | "60" => Some(FrameRateMode::Fixed60),
            "fixed120" | "120" => Some(FrameRateMode::Fixed120),
            "fixed144" | "144" => Some(FrameRateMode::Fixed144),
            "unlimited" | "uncapped" | "max" => Some(FrameRateMode::Unlimited),
            "battery" | "batterysaver" | "battery-saver" | "saver" => {
                Some(FrameRateMode::BatterySaver)
            }
            other => {
                eprintln!(
                    "[frame-scheduler] ignoring unknown FUTUREBOARD_FRAME_RATE_MODE='{other}'"
                );
                None
            }
        }
    }
}

/// What kind of work a scheduled frame serves. Each class has its own cap so
/// meters and background jobs never force the whole app to repaint at the
/// continuous (playback) rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameClass {
    /// Playback playhead, drag, scroll, zoom, active animations.
    Continuous,
    /// Meter / VU updates. Capped to 60 Hz.
    Meter,
    /// Progress bars / background jobs. Capped to 30 Hz, region-invalidation.
    Background,
}

const MIN_REFRESH_HZ: u32 = 30;
const MAX_REFRESH_HZ: u32 = 240;
const FALLBACK_REFRESH_HZ: u32 = 60;
const METER_CAP_HZ: u32 = 60;
const BACKGROUND_CAP_HZ: u32 = 30;
/// Floor for `Unlimited` so the poll loop never busy-spins.
const UNLIMITED_FLOOR: Duration = Duration::from_millis(1);

#[inline]
fn hz_to_interval(hz: u32) -> Duration {
    Duration::from_nanos(1_000_000_000u64 / hz.max(1) as u64)
}

/// Clamp a raw OS-reported refresh rate. `0` / `1` are the Windows "use
/// hardware default" sentinels and map to the fallback.
pub fn clamp_refresh(raw: u32) -> u32 {
    if raw <= 1 {
        FALLBACK_REFRESH_HZ
    } else {
        raw.clamp(MIN_REFRESH_HZ, MAX_REFRESH_HZ)
    }
}

/// Pure cadence function: the only inputs are the mode, the (already clamped)
/// refresh rate, and the frame class. No measured-timing feedback, so the
/// returned interval is stable for a given configuration.
pub fn frame_interval(mode: FrameRateMode, refresh_hz: u32, class: FrameClass) -> Duration {
    let refresh_hz = refresh_hz.clamp(MIN_REFRESH_HZ, MAX_REFRESH_HZ);
    let continuous = match mode {
        FrameRateMode::DisplaySync => hz_to_interval(refresh_hz),
        FrameRateMode::Fixed60 => hz_to_interval(60),
        FrameRateMode::Fixed120 => hz_to_interval(120),
        FrameRateMode::Fixed144 => hz_to_interval(144),
        FrameRateMode::Unlimited => UNLIMITED_FLOOR,
        // 30 FPS, or refresh/2 when that is slower (larger interval).
        FrameRateMode::BatterySaver => {
            hz_to_interval(BACKGROUND_CAP_HZ).max(hz_to_interval(refresh_hz) * 2)
        }
    };
    match class {
        FrameClass::Continuous => continuous,
        // Never faster than 60 Hz, but honour a slower continuous rate
        // (e.g. BatterySaver) so meters don't outrun the rest of the UI.
        FrameClass::Meter => continuous.max(hz_to_interval(METER_CAP_HZ)),
        FrameClass::Background => continuous.max(hz_to_interval(BACKGROUND_CAP_HZ)),
    }
}

/// Query the primary monitor refresh rate once and cache it. Clamped to
/// `30..=240`; falls back to 60 Hz on non-Windows or any query failure.
pub fn detect_refresh_hz() -> u32 {
    static CACHED: OnceLock<u32> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let raw = query_refresh_hz_os().unwrap_or(FALLBACK_REFRESH_HZ);
        let hz = clamp_refresh(raw);
        if frame_diag_enabled() {
            eprintln!("[frame-scheduler] detected refresh raw={raw}Hz -> clamped {hz}Hz");
        }
        hz
    })
}

#[cfg(windows)]
fn query_refresh_hz_os() -> Option<u32> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
    let mut devmode = DEVMODEW {
        dmSize: std::mem::size_of::<DEVMODEW>() as u16,
        ..Default::default()
    };
    // NULL device name → the current display device on the calling thread
    // (i.e. the primary monitor for the app).
    let ok = unsafe { EnumDisplaySettingsW(PCWSTR::null(), ENUM_CURRENT_SETTINGS, &mut devmode) };
    if ok.as_bool() {
        Some(devmode.dmDisplayFrequency)
    } else {
        None
    }
}

#[cfg(not(windows))]
fn query_refresh_hz_os() -> Option<u32> {
    // TODO(non-windows): query via the platform display API when available.
    // Until then the 60 Hz fallback applies.
    None
}

fn frame_diag_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| std::env::var_os("FUTUREBOARD_FRAME_DIAG").is_some())
}

/// Live scheduler used by the layout. Holds the resolved mode + cached refresh
/// rate and publishes the continuous interval through a lock-free `AtomicU64`
/// (nanoseconds) the detached poll loop reads each tick without locking the
/// entity.
pub struct FrameScheduler {
    mode: FrameRateMode,
    refresh_hz: u32,
    /// Set when `FUTUREBOARD_FRAME_RATE_MODE` is present; overrides settings.
    env_override: Option<FrameRateMode>,
    continuous_nanos: Arc<AtomicU64>,
}

impl FrameScheduler {
    pub fn new(settings_mode: FrameRateMode) -> Self {
        let refresh_hz = detect_refresh_hz();
        let env_override = FrameRateMode::from_env();
        let mode = env_override.unwrap_or(settings_mode);
        let continuous_nanos = Arc::new(AtomicU64::new(
            frame_interval(mode, refresh_hz, FrameClass::Continuous).as_nanos() as u64,
        ));
        let scheduler = Self {
            mode,
            refresh_hz,
            env_override,
            continuous_nanos,
        };
        if frame_diag_enabled() {
            eprintln!(
                "[frame-scheduler] init {} (continuous {:.2}ms, meter {:.2}ms, background {:.2}ms)",
                scheduler.describe(),
                scheduler.continuous_interval().as_secs_f32() * 1000.0,
                scheduler.meter_min_interval().as_secs_f32() * 1000.0,
                scheduler.background_interval().as_secs_f32() * 1000.0,
            );
        }
        scheduler
    }

    /// Lock-free handle to the continuous interval (nanoseconds) for the poll
    /// loop. The loop reads this each iteration so a mode change applies on the
    /// next tick.
    pub fn continuous_nanos_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.continuous_nanos)
    }

    /// Re-resolve the mode from the latest persisted setting (env override still
    /// wins) and republish the continuous interval. Cheap — call from `render`.
    pub fn refresh_from_settings(&mut self, settings_mode: FrameRateMode) {
        let mode = self.env_override.unwrap_or(settings_mode);
        if mode != self.mode {
            self.mode = mode;
            self.continuous_nanos.store(
                frame_interval(mode, self.refresh_hz, FrameClass::Continuous).as_nanos() as u64,
                Ordering::Relaxed,
            );
            if frame_diag_enabled() {
                eprintln!("[frame-scheduler] mode -> {}", self.describe());
            }
        }
    }

    pub fn mode(&self) -> FrameRateMode {
        self.mode
    }

    pub fn refresh_hz(&self) -> u32 {
        self.refresh_hz
    }

    pub fn continuous_interval(&self) -> Duration {
        frame_interval(self.mode, self.refresh_hz, FrameClass::Continuous)
    }

    pub fn meter_min_interval(&self) -> Duration {
        frame_interval(self.mode, self.refresh_hz, FrameClass::Meter)
    }

    pub fn background_interval(&self) -> Duration {
        frame_interval(self.mode, self.refresh_hz, FrameClass::Background)
    }

    /// Effective continuous FPS for the HUD (e.g. `144` for DisplaySync@144).
    pub fn effective_fps(&self) -> u32 {
        let nanos = self.continuous_interval().as_nanos().max(1) as u64;
        (1_000_000_000u64 / nanos) as u32
    }

    /// Status-bar / log label, e.g. `"Display Sync 144Hz"`.
    pub fn describe(&self) -> String {
        match self.mode {
            FrameRateMode::DisplaySync => {
                format!("Display Sync {}Hz", self.refresh_hz)
            }
            FrameRateMode::Unlimited => "Unlimited".to_string(),
            other => format!("{} ({} FPS)", other.label(), self.effective_fps()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    #[test]
    fn clamp_refresh_bounds_and_sentinels() {
        assert_eq!(clamp_refresh(10), 30, "below floor clamps up");
        assert_eq!(clamp_refresh(300), 240, "above ceiling clamps down");
        assert_eq!(clamp_refresh(0), 60, "0 is the fallback sentinel");
        assert_eq!(clamp_refresh(1), 60, "1 is the fallback sentinel");
        assert_eq!(clamp_refresh(144), 144);
        assert_eq!(clamp_refresh(30), 30);
        assert_eq!(clamp_refresh(240), 240);
    }

    #[test]
    fn continuous_interval_per_mode() {
        let near = |a: Duration, want_ms: f64| (ms(a) - want_ms).abs() < 0.2;
        assert!(near(
            frame_interval(FrameRateMode::DisplaySync, 144, FrameClass::Continuous),
            1000.0 / 144.0
        ));
        assert!(near(
            frame_interval(FrameRateMode::DisplaySync, 60, FrameClass::Continuous),
            1000.0 / 60.0
        ));
        assert!(near(
            frame_interval(FrameRateMode::Fixed60, 144, FrameClass::Continuous),
            1000.0 / 60.0
        ));
        assert!(near(
            frame_interval(FrameRateMode::Fixed120, 60, FrameClass::Continuous),
            1000.0 / 120.0
        ));
        assert!(near(
            frame_interval(FrameRateMode::Fixed144, 60, FrameClass::Continuous),
            1000.0 / 144.0
        ));
        assert_eq!(
            frame_interval(FrameRateMode::Unlimited, 240, FrameClass::Continuous),
            UNLIMITED_FLOOR
        );
    }

    #[test]
    fn meter_is_capped_at_60_but_respects_slower_continuous() {
        // 144 Hz DisplaySync: continuous ~6.9ms, meter capped to 60 Hz (~16.6ms).
        let meter = frame_interval(FrameRateMode::DisplaySync, 144, FrameClass::Meter);
        assert!(
            (ms(meter) - 1000.0 / 60.0).abs() < 0.2,
            "meter was {}ms",
            ms(meter)
        );
        // Battery saver continuous (~33ms) is slower than 60 Hz → meter follows it.
        let bs_meter = frame_interval(FrameRateMode::BatterySaver, 144, FrameClass::Meter);
        assert!(
            (ms(bs_meter) - 1000.0 / 30.0).abs() < 0.2,
            "bs meter was {}ms",
            ms(bs_meter)
        );
    }

    #[test]
    fn background_capped_at_30() {
        for mode in FrameRateMode::all() {
            let bg = frame_interval(mode, 144, FrameClass::Background);
            assert!(
                ms(bg) >= 1000.0 / 30.0 - 0.2,
                "{mode:?} background {}ms too fast",
                ms(bg)
            );
        }
    }

    #[test]
    fn battery_saver_is_30_or_refresh_over_two() {
        // 144 Hz: 30 FPS dominates (33ms > 13.9ms).
        let at_144 = frame_interval(FrameRateMode::BatterySaver, 144, FrameClass::Continuous);
        assert!((ms(at_144) - 1000.0 / 30.0).abs() < 0.2);
        // 50 Hz: refresh/2 = 25 FPS dominates (40ms > 33ms).
        let at_50 = frame_interval(FrameRateMode::BatterySaver, 50, FrameClass::Continuous);
        assert!(
            (ms(at_50) - 1000.0 / 25.0).abs() < 0.5,
            "bs@50 was {}ms",
            ms(at_50)
        );
    }

    #[test]
    fn detect_is_clamped_and_nonzero() {
        let hz = detect_refresh_hz();
        assert!((MIN_REFRESH_HZ..=MAX_REFRESH_HZ).contains(&hz));
    }
}
