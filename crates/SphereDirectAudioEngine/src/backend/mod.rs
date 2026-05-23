//! DAUx — cross-platform low-latency audio backend abstraction layer.
//!
//! The `IAudioBackend` trait decouples the engine from any specific
//! OS audio API.  Concrete implementations:
//!
//! | Backend                  | Platform     | Notes                              |
//! |--------------------------|--------------|-------------------------------------|
//! | `DauxCpalBackend`        | All          | cpal: WASAPI Shared / CoreAudio / ALSA |
//! | `DauxWasapiExclBackend`  | Windows only | Raw WASAPI exclusive + MMCSS        |
//! | `DauxMmeBackend`         | Windows only | Legacy MME stub (fallback only)     |
//!
//! Audio engine rule: all backends share `Arc<SharedState>` for meters/transport
//! and receive `EngineCommand` through a `crossbeam_channel::Receiver`.

pub mod cpal_backend;
pub mod render;

#[cfg(target_os = "windows")]
pub mod wasapi_exclusive;

// ── Backend kind ──────────────────────────────────────────────────────────────

/// Which DAUx audio backend to use for this session.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BackendKind {
    /// Platform best-effort via cpal (WASAPI Shared on Win, CoreAudio on Mac, ALSA on Linux).
    #[default]
    Auto,
    /// Windows: WASAPI Shared event-driven (lowest-common-denominator, safe).
    WasapiShared,
    /// Windows: WASAPI Exclusive event-driven (lowest practical latency without ASIO).
    WasapiExclusive,
    /// macOS: CoreAudio (same as Auto on macOS, explicit selection).
    CoreAudio,
    /// Linux: ALSA PCM (same as Auto on Linux, explicit selection).
    Alsa,
    /// Windows: MME legacy fallback — high latency, maximum compatibility.
    MmeFallback,
}

impl BackendKind {
    pub fn display_name(&self) -> &'static str {
        match self {
            BackendKind::Auto => "Auto",
            BackendKind::WasapiShared => "DAUx WASAPI Shared",
            BackendKind::WasapiExclusive => "DAUx WASAPI Exclusive",
            BackendKind::CoreAudio => "DAUx CoreAudio",
            BackendKind::Alsa => "DAUx ALSA",
            BackendKind::MmeFallback => "DAUx MME (Legacy Fallback)",
        }
    }

    pub fn id(&self) -> &'static str {
        match self {
            BackendKind::Auto => "auto",
            BackendKind::WasapiShared => "wasapi-shared",
            BackendKind::WasapiExclusive => "wasapi-exclusive",
            BackendKind::CoreAudio => "coreaudio",
            BackendKind::Alsa => "alsa",
            BackendKind::MmeFallback => "mme",
        }
    }

    pub fn from_id(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "wasapi-shared" | "wasapishared" => BackendKind::WasapiShared,
            "wasapi-exclusive" | "wasapiexclusive" => BackendKind::WasapiExclusive,
            "coreaudio" | "core-audio" => BackendKind::CoreAudio,
            "alsa" => BackendKind::Alsa,
            "mme" | "mmefallback" => BackendKind::MmeFallback,
            _ => BackendKind::Auto,
        }
    }
}

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for opening an audio device through the DAUx abstraction.
#[derive(Debug, Clone, Default)]
pub struct DauxDeviceConfig {
    /// Which backend to use.
    pub backend: BackendKind,
    /// Specific output device name/id, or None for the system default.
    pub output_device_id: Option<String>,
    /// Specific input device name/id (for future capture support).
    pub input_device_id: Option<String>,
    /// Requested sample rate (Hz).  None = use device default.
    pub sample_rate: Option<u32>,
    /// Requested buffer size (frames).  None = use driver default.
    pub buffer_size: Option<u32>,
    /// Request MMCSS "Pro Audio" thread priority on Windows.
    pub mmcss_priority: bool,
    /// Safe mode: use larger buffer for stability over latency.
    pub safe_mode: bool,
}

// ── Backend runtime status ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct DauxBackendStatus {
    /// Backend identifier string (e.g. "wasapi-shared", "coreaudio").
    pub backend_id: String,
    /// Human-readable backend name.
    pub backend_name: String,
    /// Active output device name.
    pub output_device: Option<String>,
    /// Active sample rate (Hz).
    pub sample_rate: u32,
    /// Active buffer size (frames).
    pub buffer_size: u32,
    /// Estimated round-trip output latency in milliseconds.
    pub estimated_latency_ms: f64,
    /// Number of audio buffer underruns / glitches since stream open.
    pub glitch_count: u64,
    /// Number of xruns (ALSA) / underruns since stream open.
    pub xrun_count: u64,
    /// Last measured callback duration (microseconds).
    pub last_callback_us: u64,
}

// ── Available backend list ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BackendInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub is_default: bool,
    pub description: String,
}

/// Return all backends available on the current platform.
pub fn list_available_backends() -> Vec<BackendInfo> {
    let mut list = vec![BackendInfo {
        id: "auto".into(),
        name: "Auto".into(),
        available: true,
        is_default: true,
        description: "Platform default (WASAPI Shared / CoreAudio / ALSA)".into(),
    }];

    #[cfg(target_os = "windows")]
    {
        list.push(BackendInfo {
            id: "wasapi-shared".into(),
            name: "DAUx WASAPI Shared".into(),
            available: true,
            is_default: false,
            description: "WASAPI Shared event-driven — compatible, ~10-30ms latency".into(),
        });
        list.push(BackendInfo {
            id: "wasapi-exclusive".into(),
            name: "DAUx WASAPI Exclusive".into(),
            available: true,
            is_default: false,
            description: "WASAPI Exclusive + MMCSS — lowest latency, requires device support"
                .into(),
        });
        list.push(BackendInfo {
            id: "mme".into(),
            name: "DAUx MME (Legacy Fallback)".into(),
            available: true,
            is_default: false,
            description: "Windows MME — maximum compatibility, high latency (~100ms+)".into(),
        });
    }

    #[cfg(target_os = "macos")]
    {
        list.push(BackendInfo {
            id: "coreaudio".into(),
            name: "DAUx CoreAudio".into(),
            available: true,
            is_default: false,
            description: "CoreAudio — native macOS low-latency backend".into(),
        });
    }

    #[cfg(target_os = "linux")]
    {
        list.push(BackendInfo {
            id: "alsa".into(),
            name: "DAUx ALSA".into(),
            available: true,
            is_default: false,
            description: "ALSA PCM — native Linux audio, configurable period/buffer size".into(),
        });
    }

    list
}
