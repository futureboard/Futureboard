//! SQLite-backed plug-in catalog cache.
//!
//! Stored at:
//! * Windows  — `%APPDATA%\Futureboard Studio\Plugin Database\index.dat`
//! * macOS    — `~/Library/Application Support/Futureboard Studio/Plugin Database/index.dat`
//! * Linux    — `~/.config/Futureboard Studio/Plugin Database/index.dat`
//!
//! The file is SQLite internally (extension `.dat` is deliberate — we own this
//! database, the suffix keeps it from being treated as a generic SQLite file
//! by file managers).
//!
//! This module is the only place that talks to `rusqlite`; the rest of the
//! crate sees `PluginCatalogEntry` / `PluginCatalog` and lets the DB module
//! handle schema, transactions, and upsert. The catalog is read on a worker
//! thread by [`crate::registry::PluginRegistry::load_catalog`] and written
//! transactionally by [`crate::registry::PluginRegistry::scan_with_progress`].

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::registry::{
    classify_kind, display_category, PluginFormat, PluginKind, PluginStatus, RegistryPlugin,
};

const SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginScanStatus {
    Pending,
    Scanning,
    Success,
    Failed,
    Crashed,
    Skipped,
    /// Legacy alias for [`Self::Success`].
    Ok,
    /// Legacy alias for [`Self::Failed`].
    MetadataOnly,
    /// Legacy alias for [`Self::Skipped`].
    Disabled,
}

impl PluginScanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginScanStatus::Pending => "pending",
            PluginScanStatus::Scanning => "scanning",
            PluginScanStatus::Success | PluginScanStatus::Ok => "success",
            PluginScanStatus::Failed | PluginScanStatus::MetadataOnly => "failed",
            PluginScanStatus::Crashed => "crashed",
            PluginScanStatus::Skipped | PluginScanStatus::Disabled => "skipped",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "pending" => PluginScanStatus::Pending,
            "scanning" => PluginScanStatus::Scanning,
            "success" | "ok" => PluginScanStatus::Success,
            "failed" | "metadata_only" => PluginScanStatus::Failed,
            "crashed" => PluginScanStatus::Crashed,
            "skipped" | "disabled" => PluginScanStatus::Skipped,
            _ => PluginScanStatus::Success,
        }
    }

    pub fn is_usable(self) -> bool {
        matches!(self, PluginScanStatus::Success | PluginScanStatus::Ok)
    }
}

/// One row in the SQLite catalog (1:1 with the spec).
#[derive(Debug, Clone)]
pub struct PluginCatalogEntry {
    pub id: String,
    pub format: PluginFormat,
    pub name: String,
    pub vendor: Option<String>,
    pub category: Option<String>,
    pub path: PathBuf,
    pub class_id: Option<String>,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub is_instrument: bool,
    pub is_effect: bool,
    pub scan_status: PluginScanStatus,
    pub validation_level: Option<String>,
    pub disabled: bool,
    pub favorite: bool,
    pub file_modified_at: Option<String>,
    pub file_size: Option<i64>,
    pub last_scanned_at: Option<String>,
    pub error: Option<String>,
    pub metadata_json: Option<String>,
    /// Precomputed lowercased `name + vendor + category + format` string for
    /// substring search.
    pub search_text: String,
}

impl PluginCatalogEntry {
    /// Project a catalog row back into the legacy [`RegistryPlugin`] shape the
    /// rest of the UI consumes. `preset_path` is derived for completeness; the
    /// picker never touches `.pst` files so it can be a zero-cost placeholder.
    pub fn to_registry_plugin(&self) -> RegistryPlugin {
        let raw_category = self.category.clone();
        let category = display_category(
            self.format,
            self.category.as_deref().unwrap_or(""),
            raw_category.as_deref(),
            None,
        );
        let kind = if self.is_instrument {
            PluginKind::Instrument
        } else {
            PluginKind::Effect
        };
        let status = match self.scan_status {
            PluginScanStatus::Success | PluginScanStatus::Ok => PluginStatus::PresetReady,
            _ => PluginStatus::MissingPreset,
        };
        RegistryPlugin {
            id: self.id.clone(),
            name: self.name.clone(),
            vendor: self.vendor.clone().unwrap_or_default(),
            format: self.format,
            category,
            raw_category,
            sub_categories: None,
            kind,
            path: self.path.clone(),
            class_id: self.class_id.clone(),
            version: self.version.clone(),
            sdk_metadata_loaded: self.scan_status.is_usable(),
            preset_path: PathBuf::new(),
            scanned_at_ms: parse_iso8601_to_ms(self.last_scanned_at.as_deref()).unwrap_or(0),
            status,
            scan_status: self.scan_status,
            error_message: self.error.clone(),
        }
    }
}

impl From<&RegistryPlugin> for PluginCatalogEntry {
    fn from(p: &RegistryPlugin) -> Self {
        let kind_is_instrument = matches!(p.kind, PluginKind::Instrument);
        let category = if p.category.is_empty() {
            p.raw_category.clone()
        } else {
            Some(p.category.clone())
        };
        let search_text = format!(
            "{} {} {} {}",
            p.name,
            p.vendor,
            p.display_category(),
            p.format.label()
        )
        .to_lowercase();
        let last_scanned_at = ms_to_iso8601(p.scanned_at_ms);
        Self {
            id: p.id.clone(),
            format: p.format,
            name: p.name.clone(),
            vendor: if p.vendor.is_empty() {
                None
            } else {
                Some(p.vendor.clone())
            },
            category,
            path: p.path.clone(),
            class_id: p.class_id.clone(),
            bundle_id: None,
            version: p.version.clone(),
            is_instrument: kind_is_instrument,
            is_effect: !kind_is_instrument,
            scan_status: p.scan_status,
            validation_level: None,
            disabled: false,
            favorite: false,
            file_modified_at: None,
            file_size: None,
            last_scanned_at,
            error: p.error_message.clone(),
            metadata_json: None,
            search_text,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginCatalog {
    pub plugins: Vec<PluginCatalogEntry>,
    pub loaded_at: std::time::Instant,
    pub source_path: PathBuf,
}

impl PluginCatalog {
    pub fn empty(source_path: PathBuf) -> Self {
        Self {
            plugins: Vec::new(),
            loaded_at: std::time::Instant::now(),
            source_path,
        }
    }
}

/// Resolve the on-disk path of the catalog DB. Always returns a path even if
/// the parent directory does not yet exist — call [`ensure_database_dir`]
/// before opening when you intend to write.
pub fn database_path() -> PathBuf {
    let root = root_dir();
    root.join("Futureboard Studio")
        .join("Plugin Database")
        .join("index.dat")
}

#[cfg(target_os = "windows")]
fn root_dir() -> PathBuf {
    // `dirs::config_dir()` on Windows resolves to `%APPDATA%` (Roaming).
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
fn root_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn root_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn database_dir() -> PathBuf {
    database_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn database_exists() -> bool {
    database_path().is_file()
}

pub fn ensure_database_dir() -> Result<(), String> {
    let dir = database_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())
}

/// Open (or create) the SQLite catalog DB. Always runs schema migrations.
pub fn open_database() -> Result<Connection, String> {
    ensure_database_dir()?;
    let path = database_path();
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )
    .map_err(|e| format!("open {}: {e}", path.display()))?;
    init_schema(&conn).map_err(|e| format!("init schema: {e}"))?;
    Ok(conn)
}

/// Open in read-only mode (returns Err if DB does not exist).
pub fn open_database_readonly() -> Result<Connection, String> {
    let path = database_path();
    if !path.is_file() {
        return Err(format!("Plugin database not found at {}", path.display()));
    }
    Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("open ro {}: {e}", path.display()))
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS schema_meta (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS plugins (
             id TEXT PRIMARY KEY,
             format TEXT NOT NULL,
             name TEXT NOT NULL,
             vendor TEXT,
             category TEXT,
             path TEXT NOT NULL,
             class_id TEXT,
             bundle_id TEXT,
             version TEXT,
             is_instrument INTEGER NOT NULL DEFAULT 0,
             is_effect INTEGER NOT NULL DEFAULT 1,
             scan_status TEXT NOT NULL,
             validation_level TEXT,
             disabled INTEGER NOT NULL DEFAULT 0,
             favorite INTEGER NOT NULL DEFAULT 0,
             file_modified_at TEXT,
             file_size INTEGER,
             last_scanned_at TEXT,
             error TEXT,
             metadata_json TEXT,
             search_text TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS plugin_scan_runs (
             id TEXT PRIMARY KEY,
             started_at TEXT,
             finished_at TEXT,
             status TEXT,
             plugins_found INTEGER,
             plugins_valid INTEGER,
             plugins_failed INTEGER,
             plugins_metadata_only INTEGER
         );
         CREATE TABLE IF NOT EXISTS plugin_scan_failures (
             id TEXT PRIMARY KEY,
             plugin_path TEXT,
             format TEXT,
             reason TEXT,
             scanned_at TEXT,
             stderr TEXT,
             crash_code TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_plugins_name ON plugins(name);
         CREATE INDEX IF NOT EXISTS idx_plugins_vendor ON plugins(vendor);
         CREATE INDEX IF NOT EXISTS idx_plugins_format ON plugins(format);
         CREATE INDEX IF NOT EXISTS idx_plugins_category ON plugins(category);
         CREATE INDEX IF NOT EXISTS idx_plugins_search ON plugins(search_text);
         CREATE INDEX IF NOT EXISTS idx_plugins_status ON plugins(scan_status);",
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO schema_meta(key, value) VALUES('version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

pub fn count_rows(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM plugins", [], |r| r.get::<_, i64>(0))
}

pub fn read_all(conn: &Connection) -> rusqlite::Result<Vec<PluginCatalogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, format, name, vendor, category, path, class_id, bundle_id, version,
                is_instrument, is_effect, scan_status, validation_level, disabled, favorite,
                file_modified_at, file_size, last_scanned_at, error, metadata_json, search_text
           FROM plugins
          ORDER BY favorite DESC, vendor COLLATE NOCASE ASC, name COLLATE NOCASE ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let format: String = row.get(1)?;
        let path: String = row.get(5)?;
        let scan_status: String = row.get(11)?;
        Ok(PluginCatalogEntry {
            id: row.get(0)?,
            format: PluginFormat::from_str_lossy(&format),
            name: row.get(2)?,
            vendor: row.get(3)?,
            category: row.get(4)?,
            path: PathBuf::from(path),
            class_id: row.get(6)?,
            bundle_id: row.get(7)?,
            version: row.get(8)?,
            is_instrument: row.get::<_, i64>(9)? != 0,
            is_effect: row.get::<_, i64>(10)? != 0,
            scan_status: PluginScanStatus::from_str_lossy(&scan_status),
            validation_level: row.get(12)?,
            disabled: row.get::<_, i64>(13)? != 0,
            favorite: row.get::<_, i64>(14)? != 0,
            file_modified_at: row.get(15)?,
            file_size: row.get(16)?,
            last_scanned_at: row.get(17)?,
            error: row.get(18)?,
            metadata_json: row.get(19)?,
            search_text: row.get(20)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Read `last_scanned_at` of the most recently updated row, returned as
/// milliseconds since epoch. `0` if the table is empty.
pub fn last_scan_ms(conn: &Connection) -> rusqlite::Result<i64> {
    let raw: Option<String> = conn
        .query_row("SELECT MAX(last_scanned_at) FROM plugins", [], |r| r.get(0))
        .optional()?
        .flatten();
    Ok(parse_iso8601_to_ms(raw.as_deref()).unwrap_or(0))
}

/// Upsert every row in `entries` inside a single transaction. Existing rows
/// keyed by `id` are replaced (favorite flag is preserved).
pub fn upsert_plugins(
    conn: &mut Connection,
    entries: &[PluginCatalogEntry],
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO plugins
                (id, format, name, vendor, category, path, class_id, bundle_id, version,
                 is_instrument, is_effect, scan_status, validation_level, disabled, favorite,
                 file_modified_at, file_size, last_scanned_at, error, metadata_json, search_text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                     ?16, ?17, ?18, ?19, ?20, ?21)
             ON CONFLICT(id) DO UPDATE SET
                format = excluded.format,
                name = excluded.name,
                vendor = excluded.vendor,
                category = excluded.category,
                path = excluded.path,
                class_id = excluded.class_id,
                bundle_id = excluded.bundle_id,
                version = excluded.version,
                is_instrument = excluded.is_instrument,
                is_effect = excluded.is_effect,
                scan_status = excluded.scan_status,
                validation_level = excluded.validation_level,
                disabled = excluded.disabled,
                file_modified_at = excluded.file_modified_at,
                file_size = excluded.file_size,
                last_scanned_at = excluded.last_scanned_at,
                error = excluded.error,
                metadata_json = excluded.metadata_json,
                search_text = excluded.search_text",
        )?;
        for e in entries {
            stmt.execute(params![
                e.id,
                e.format.label(),
                e.name,
                e.vendor,
                e.category,
                e.path.to_string_lossy(),
                e.class_id,
                e.bundle_id,
                e.version,
                e.is_instrument as i64,
                e.is_effect as i64,
                e.scan_status.as_str(),
                e.validation_level,
                e.disabled as i64,
                e.favorite as i64,
                e.file_modified_at,
                e.file_size,
                e.last_scanned_at,
                e.error,
                e.metadata_json,
                e.search_text,
            ])?;
        }
    }
    tx.commit()
}

/// Record a scan run summary.
#[allow(clippy::too_many_arguments)]
pub fn record_scan_run(
    conn: &Connection,
    id: &str,
    started_at: &str,
    finished_at: &str,
    status: &str,
    plugins_found: i64,
    plugins_valid: i64,
    plugins_failed: i64,
    plugins_metadata_only: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO plugin_scan_runs
            (id, started_at, finished_at, status, plugins_found, plugins_valid,
             plugins_failed, plugins_metadata_only)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            started_at,
            finished_at,
            status,
            plugins_found,
            plugins_valid,
            plugins_failed,
            plugins_metadata_only
        ],
    )?;
    Ok(())
}

/// Delete every plug-in row. Scan-run / failure history is preserved.
pub fn clear_plugins(conn: &Connection) -> rusqlite::Result<u32> {
    let n = conn.execute("DELETE FROM plugins", [])?;
    Ok(n as u32)
}

/// Delete the SQLite file outright (and the WAL/SHM siblings if present).
pub fn delete_database_file() -> Result<(), String> {
    let path = database_path();
    for ext in ["", "-wal", "-shm"] {
        let target = if ext.is_empty() {
            path.clone()
        } else {
            let mut p = path.clone().into_os_string();
            p.push(ext);
            PathBuf::from(p)
        };
        if target.exists() {
            std::fs::remove_file(&target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn now_iso8601() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    epoch_secs_to_iso8601(now)
}

fn ms_to_iso8601(ms: i64) -> Option<String> {
    if ms <= 0 {
        return None;
    }
    Some(epoch_secs_to_iso8601(ms / 1000))
}

fn epoch_secs_to_iso8601(secs: i64) -> String {
    // Plain ISO-8601-ish UTC stamp. Picked manually so we don't pull in
    // `chrono`/`time` just for the cache.
    let days_from_epoch = secs.div_euclid(86_400);
    let secs_in_day = secs.rem_euclid(86_400);
    let (year, month, day) = days_from_epoch_to_ymd(days_from_epoch);
    let h = secs_in_day / 3600;
    let m = (secs_in_day % 3600) / 60;
    let s = secs_in_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_from_epoch_to_ymd(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp.wrapping_sub(9) } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d)
}

fn parse_iso8601_to_ms(s: Option<&str>) -> Option<i64> {
    let s = s?;
    // Expect YYYY-MM-DDTHH:MM:SSZ; tolerate trailing 'Z' missing.
    if s.len() < 19 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let min: i64 = s.get(14..16)?.parse().ok()?;
    let sec: i64 = s.get(17..19)?.parse().ok()?;
    let days = ymd_to_days_from_epoch(year, month, day)?;
    let secs = days * 86_400 + hour * 3600 + min * 60 + sec;
    Some(secs * 1000)
}

fn ymd_to_days_from_epoch(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let doy = ((153 * m as u64 + 2) / 5 + day as u64 - 1) as i64;
    let doe = yoe as i64 * 365 + (yoe / 4) as i64 - (yoe / 100) as i64 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Convenience: clear the in-DB plug-in table and bookkeep a "user cleared"
/// scan run.
pub fn clear_with_run_record(conn: &mut Connection) -> rusqlite::Result<u32> {
    let removed = clear_plugins(conn)?;
    let stamp = now_iso8601();
    record_scan_run(conn, &stamp, &stamp, &stamp, "cleared", 0, 0, 0, 0)?;
    Ok(removed)
}

/// Re-export of the catalog helper that classifies missing-binary rows. Kept
/// here so callers don't have to import `registry::classify_kind` separately.
#[allow(dead_code)]
pub(crate) fn classify_kind_compat(category: &str, name: &str) -> PluginKind {
    classify_kind(category, name, None)
}
