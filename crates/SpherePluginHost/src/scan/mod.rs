//! Structured plug-in scan types, logging, subprocess isolation, and cache state.

pub mod cache;
pub mod isolation;
pub mod log;
pub mod types;

pub use cache::{load_au_cache_state, save_au_cache_state, AuScanCacheState, FormatCacheStatus};
pub use isolation::{
    locate_scanner_binary, run_isolated_format_scan, run_isolated_plugin_validation,
    IsolatedScanOutcome, IsolatedScanRequest, ScannerBinaryLocation,
};
pub use log::{
    scan_finished, scan_found, scan_plugin_failed, scan_plugin_start, scan_plugin_success,
    scan_process_crashed, scan_start,
};
pub use types::{
    PluginDescriptor, PluginScanError, PluginScanFormat, PluginScanStatus, ScanResultPayload,
};
