//! Native audio engine lifecycle owner for the Rust shell.
//!
//! `NativeAudioState` proves the Rust-Native Futureboard Studio binary can
//! own the [`AudioEngine`] lifecycle directly, without going through NAPI.
//!
//! Stage 1 scope: build, start, stop, poll stats, enumerate devices. No
//! timeline scheduling, mixer routing, or plugin processing yet.
//!
//! Failures never panic — errors are captured into `last_error` so the
//! status bar / inspector can surface them.

// The crate is published with `[lib] name = "DAUx"` so the N-API output
// is `DAUx.node`. Rust consumers import its symbols through that same
// name — alias it locally for readability.
use DAUx::{
    AudioBackend, AudioEngine, EngineConfig, EngineDeviceInfo, EngineStats, SphereAudioError,
};

pub struct NativeAudioState {
    pub config: EngineConfig,
    pub engine: Option<AudioEngine>,
    pub running: bool,
    pub last_error: Option<String>,
    pub stats: Option<EngineStats>,
}

impl Default for NativeAudioState {
    fn default() -> Self {
        Self {
            config: AudioEngine::default_config(),
            engine: None,
            running: false,
            last_error: None,
            stats: None,
        }
    }
}

// Stage 1: `start` / `stop` / `toggle_transport` / `set_backend` are part of
// the public API the next stages will wire into the UI/command dispatcher.
// They are intentionally unused right now.
#[allow(dead_code)]
impl NativeAudioState {
    /// Build a fresh state with default configuration. Does **not** create
    /// the engine yet — call [`Self::initialize_engine`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the [`AudioEngine`] handle. Idempotent: re-calling rebuilds
    /// the handle (e.g. after a config change).
    pub fn initialize_engine(&mut self) -> Result<(), SphereAudioError> {
        match AudioEngine::new(self.config.clone()) {
            Ok(engine) => {
                self.engine = Some(engine);
                self.last_error = None;
                Ok(())
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    /// Open the audio device and start the stream. Lazily initializes the
    /// engine handle if needed.
    pub fn start(&mut self) -> Result<(), SphereAudioError> {
        if self.engine.is_none() {
            self.initialize_engine()?;
        }
        let engine = self
            .engine
            .as_mut()
            .ok_or(SphereAudioError::EngineNotOpen)?;
        match engine.start() {
            Ok(()) => {
                self.running = true;
                self.last_error = None;
                self.refresh_stats();
                Ok(())
            }
            Err(e) => {
                self.running = false;
                self.last_error = Some(e.to_string());
                Err(e)
            }
        }
    }

    /// Stop the stream and close the device. Safe to call when already
    /// stopped — does nothing.
    pub fn stop(&mut self) -> Result<(), SphereAudioError> {
        if let Some(engine) = self.engine.as_mut() {
            if let Err(e) = engine.stop() {
                self.last_error = Some(e.to_string());
                return Err(e);
            }
        }
        self.running = false;
        self.refresh_stats();
        Ok(())
    }

    /// Toggle the transport play/pause if the engine is running. Returns
    /// the new transport state, or `None` if the engine is not started.
    pub fn toggle_transport(&mut self) -> Option<bool> {
        let engine = self.engine.as_ref()?;
        match engine.toggle_transport() {
            Ok(playing) => {
                self.last_error = None;
                Some(playing)
            }
            Err(e) => {
                self.last_error = Some(e.to_string());
                None
            }
        }
    }

    /// Refresh the cached [`EngineStats`] snapshot. Cheap — used by status
    /// bar polling.
    pub fn refresh_stats(&mut self) -> Option<&EngineStats> {
        if let Some(engine) = self.engine.as_ref() {
            let s = engine.stats();
            self.running = s.running;
            self.stats = Some(s);
        }
        self.stats.as_ref()
    }

    /// Enumerate output devices. Returns an empty list if the engine is
    /// not built or the backend errors.
    pub fn list_devices(&self) -> Vec<EngineDeviceInfo> {
        self.engine
            .as_ref()
            .map(|e| e.list_output_devices())
            .unwrap_or_default()
    }

    /// Engine semver — useful for the About box.
    pub fn version(&self) -> Option<String> {
        self.engine.as_ref().map(|e| e.version())
    }

    /// Force the backend selection. Caller must re-`start` for it to take effect.
    pub fn set_backend(&mut self, backend: AudioBackend) {
        self.config.backend = backend;
    }
}

impl Drop for NativeAudioState {
    fn drop(&mut self) {
        if self.running {
            let _ = self.stop();
        }
    }
}
