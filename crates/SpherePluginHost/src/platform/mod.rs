//! Platform-specific plugin-host process helpers.

#[cfg(windows)]
pub mod windows_process;

#[cfg(windows)]
pub use windows_process::PluginHostJob;

#[cfg(not(windows))]
pub struct PluginHostJob;

#[cfg(not(windows))]
impl Default for PluginHostJob {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(windows))]
impl PluginHostJob {
    pub fn new() -> Self {
        Self
    }

    pub fn assign_child(&self, _child: &std::process::Child) {}
}
