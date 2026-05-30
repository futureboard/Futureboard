use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::plugin_db::database_dir;

const AU_CACHE_FILE: &str = "au_scan_state.json";
const SAFE_MODE_CRASH_THRESHOLD: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatCacheStatus {
    Fresh,
    Stale,
    Failed,
    Crashed,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuScanCacheState {
    pub status: FormatCacheStatus,
    pub crash_count: u32,
    pub auto_scan_disabled: bool,
    pub last_error: Option<String>,
    pub last_scan_at_ms: i64,
}

impl Default for AuScanCacheState {
    fn default() -> Self {
        Self {
            status: FormatCacheStatus::Stale,
            crash_count: 0,
            auto_scan_disabled: false,
            last_error: None,
            last_scan_at_ms: 0,
        }
    }
}

fn au_cache_path() -> PathBuf {
    database_dir().join(AU_CACHE_FILE)
}

pub fn load_au_cache_state() -> AuScanCacheState {
    let path = au_cache_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return AuScanCacheState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_au_cache_state(state: &AuScanCacheState) -> Result<(), String> {
    crate::plugin_db::ensure_database_dir()?;
    let path = au_cache_path();
    let json = serde_json::to_string_pretty(state)
        .map_err(|error| format!("serialize au cache state: {error}"))?;
    fs::write(path, json).map_err(|error| error.to_string())
}

pub fn record_au_scan_success(state: &mut AuScanCacheState, scanned_at_ms: i64) {
    state.status = FormatCacheStatus::Fresh;
    state.crash_count = 0;
    state.auto_scan_disabled = false;
    state.last_error = None;
    state.last_scan_at_ms = scanned_at_ms;
}

pub fn record_au_scan_failure(state: &mut AuScanCacheState, error: String, crashed: bool) {
    state.last_error = Some(error);
    state.status = if crashed {
        FormatCacheStatus::Crashed
    } else {
        FormatCacheStatus::Failed
    };
    if crashed {
        state.crash_count = state.crash_count.saturating_add(1);
        if state.crash_count >= SAFE_MODE_CRASH_THRESHOLD {
            state.auto_scan_disabled = true;
        }
    }
}

pub fn should_auto_scan_au(state: &AuScanCacheState) -> bool {
    cfg!(target_os = "macos") && !state.auto_scan_disabled
}
