//! Registry-based ASIO driver enumeration (Windows, control thread only).
//!
//! Lists installed ASIO drivers **without instantiating any of them**. The
//! previous enumeration path walked `cpal::Host::devices()`, which COM-loads
//! and `ASIOInit`s every registered driver just to read its name — slow, able
//! to pop driver dialogs, and (because asio-sys keeps process-global callback
//! state) able to clobber an already-running ASIO stream. Registry reads have
//! none of those failure modes and cannot touch the realtime thread.
//!
//! Identity contract: `stable_id` reproduces the name the ASIO SDK's
//! `asiolist` produces for the same registration (the `Description` value when
//! present, otherwise the subkey name, truncated to the SDK's 32-byte buffer
//! rule). That is the exact string `asio-sys`/CPAL use to load a driver, so a
//! persisted `stable_id` keeps resolving to the same driver across refreshes
//! and restarts.
//!
//! A 64-bit process only loads drivers registered in the 64-bit registry view
//! (`HKLM\SOFTWARE\ASIO`); entries that exist only under `WOW6432Node` are
//! surfaced as diagnostics (`WrongRegistryView`), never as selectable devices.

use std::path::{Path, PathBuf};

/// One installed ASIO driver registration, resolved and validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsioDriverDescriptor {
    /// asiolist-compatible driver name — the id CPAL loads by and the id we
    /// persist in settings. Stable across refreshes.
    pub stable_id: String,
    /// Untruncated user-facing name (registry `Description`, or key name).
    pub display_name: String,
    /// `HKLM\SOFTWARE\ASIO` subkey name this entry came from.
    pub registry_key: String,
    /// Driver COM class id, normalized to uppercase `{...}` form.
    pub clsid: String,
    /// Resolved in-process COM server path, when the CLSID resolves.
    pub module_path: Option<PathBuf>,
    pub compatibility: AsioDriverCompatibility,
}

impl AsioDriverDescriptor {
    /// Whether this driver can actually be loaded by the current process.
    pub fn is_loadable(&self) -> bool {
        self.compatibility == AsioDriverCompatibility::Compatible
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsioDriverCompatibility {
    Compatible,
    /// Registration has no `CLSID` value.
    MissingClsid,
    /// `CLSID` value is not a well-formed GUID.
    InvalidClsid,
    /// CLSID has no `InprocServer32` registration in this process's view.
    ClsidNotRegistered,
    /// The registered module path does not exist on disk.
    ModuleMissing,
    /// The module's PE machine type does not match this process.
    WrongArchitecture { machine: u16 },
    /// Registered only in the 32-bit (`WOW6432Node`) view — invisible to the
    /// ASIO loader in a 64-bit process.
    WrongRegistryView,
    /// Same display name as an earlier registration: the ASIO loader matches
    /// drivers by (truncated) name, so this entry can never be the one loaded.
    ShadowedByDuplicateName,
}

impl AsioDriverCompatibility {
    pub fn describe(&self) -> String {
        match self {
            Self::Compatible => "compatible".into(),
            Self::MissingClsid => "registration has no CLSID value".into(),
            Self::InvalidClsid => "registration has a malformed CLSID".into(),
            Self::ClsidNotRegistered => {
                "CLSID has no InprocServer32 entry in this registry view".into()
            }
            Self::ModuleMissing => "registered driver module not found on disk".into(),
            Self::WrongArchitecture { machine } => format!(
                "driver module architecture (PE machine 0x{machine:04X}) does not match this \
                 process (0x{:04X})",
                current_process_machine()
            ),
            Self::WrongRegistryView => {
                "registered only in the 32-bit registry view (WOW6432Node)".into()
            }
            Self::ShadowedByDuplicateName => {
                "shadowed by an earlier driver with the same name".into()
            }
        }
    }
}

/// Raw registry facts for one `SOFTWARE\ASIO` subkey, before validation.
#[derive(Debug, Clone)]
pub struct RawAsioRegistration {
    pub key_name: String,
    pub description: Option<String>,
    pub clsid: Option<String>,
    /// `true` when the entry was found only under `WOW6432Node`.
    pub from_wow64_view: bool,
}

/// Lookup surface for the non-registry-list parts of validation. Split out so
/// the resolution logic is testable without a live registry/filesystem.
pub trait ClsidResolver {
    /// Resolve `HKCR\CLSID\{clsid}\InprocServer32` (default value) in the
    /// current process's registry view.
    fn inproc_server_path(&self, clsid: &str) -> Option<PathBuf>;
    /// PE `Machine` field of the module, or `None` if unreadable.
    fn module_machine(&self, path: &Path) -> Option<u16>;
}

// ── asiolist name contract ────────────────────────────────────────────────────

/// Byte budget of the SDK's driver-name buffers (`asioGetDriverName` is always
/// called with a 32-byte buffer by asio-sys and by `AsioDrivers::loadDriver`).
const ASIOLIST_NAME_BUF: usize = 32;

/// Reproduce `AsioDriverList::asioGetDriverName`'s truncation: names shorter
/// than the buffer pass through; longer names become the first
/// `buf - 4` bytes followed by `"..."`. Cutting lands on a `char` boundary so
/// the result stays valid UTF-8 (the SDK operates on ANSI bytes; for the ASCII
/// names real drivers use, the two agree).
pub fn asiolist_driver_name(full_name: &str) -> String {
    if full_name.len() < ASIOLIST_NAME_BUF {
        return full_name.to_string();
    }
    let budget = ASIOLIST_NAME_BUF - 4;
    let mut cut = budget;
    while cut > 0 && !full_name.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}...", &full_name[..cut])
}

/// The name asiolist derives for a registration: `Description` value when
/// present, key name otherwise.
pub fn asiolist_source_name(raw: &RawAsioRegistration) -> &str {
    match raw.description.as_deref() {
        Some(description) if !description.trim().is_empty() => description,
        _ => raw.key_name.as_str(),
    }
}

// ── Resolution (pure given a resolver) ────────────────────────────────────────

fn normalize_clsid(clsid: &str) -> Option<String> {
    let trimmed = clsid.trim();
    let inner = trimmed
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .unwrap_or(trimmed);
    // 8-4-4-4-12 hex groups.
    let groups: Vec<&str> = inner.split('-').collect();
    let expected = [8usize, 4, 4, 4, 12];
    if groups.len() != expected.len() {
        return None;
    }
    for (group, expected_len) in groups.iter().zip(expected) {
        if group.len() != expected_len || !group.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
    }
    Some(format!("{{{}}}", inner.to_ascii_uppercase()))
}

pub fn current_process_machine() -> u16 {
    #[cfg(target_arch = "x86_64")]
    {
        0x8664 // IMAGE_FILE_MACHINE_AMD64
    }
    #[cfg(target_arch = "aarch64")]
    {
        0xAA64 // IMAGE_FILE_MACHINE_ARM64
    }
    #[cfg(target_arch = "x86")]
    {
        0x014C // IMAGE_FILE_MACHINE_I386
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "x86")))]
    {
        0
    }
}

/// Validate + dedupe raw registrations into stable, ordered descriptors.
///
/// Rules:
/// * dedupe by normalized CLSID (first entry in sort order wins; the 64-bit
///   view is sorted ahead of `WOW6432Node`),
/// * `WOW6432Node`-only entries are kept as `WrongRegistryView` diagnostics,
/// * a later entry whose asiolist name collides with an earlier loadable one is
///   `ShadowedByDuplicateName` (the SDK loader matches by name — only the first
///   can ever load),
/// * output is sorted by `stable_id` (case-insensitive) then key name, so the
///   list and every entry's identity are stable across refreshes.
pub fn resolve_descriptors(
    mut raw: Vec<RawAsioRegistration>,
    resolver: &dyn ClsidResolver,
) -> Vec<AsioDriverDescriptor> {
    raw.sort_by(|a, b| {
        a.from_wow64_view
            .cmp(&b.from_wow64_view)
            .then_with(|| a.key_name.to_lowercase().cmp(&b.key_name.to_lowercase()))
    });

    let mut seen_clsids: Vec<String> = Vec::new();
    let mut seen_loadable_names: Vec<String> = Vec::new();
    let mut out: Vec<AsioDriverDescriptor> = Vec::new();

    for entry in raw {
        let source_name = asiolist_source_name(&entry).to_string();
        let stable_id = asiolist_driver_name(&source_name);

        let (clsid, clsid_state) = match entry.clsid.as_deref() {
            None => (String::new(), Some(AsioDriverCompatibility::MissingClsid)),
            Some(raw_clsid) => match normalize_clsid(raw_clsid) {
                None => (
                    raw_clsid.trim().to_string(),
                    Some(AsioDriverCompatibility::InvalidClsid),
                ),
                Some(normalized) => (normalized, None),
            },
        };

        // Dedupe by identity (CLSID), not display name: the same driver is
        // sometimes registered under several subkeys.
        if clsid_state.is_none() {
            if seen_clsids.iter().any(|seen| *seen == clsid) {
                continue;
            }
            seen_clsids.push(clsid.clone());
        }

        let mut module_path = None;
        let compatibility = if let Some(state) = clsid_state {
            state
        } else if entry.from_wow64_view {
            AsioDriverCompatibility::WrongRegistryView
        } else {
            match resolver.inproc_server_path(&clsid) {
                None => AsioDriverCompatibility::ClsidNotRegistered,
                Some(path) => match resolver.module_machine(&path) {
                    None => {
                        module_path = Some(path);
                        AsioDriverCompatibility::ModuleMissing
                    }
                    Some(machine) => {
                        module_path = Some(path);
                        if machine == current_process_machine() {
                            AsioDriverCompatibility::Compatible
                        } else {
                            AsioDriverCompatibility::WrongArchitecture { machine }
                        }
                    }
                },
            }
        };

        // The SDK loader resolves drivers by (truncated) name — a duplicate
        // name can never load, whichever CLSID it carries.
        let compatibility = if compatibility == AsioDriverCompatibility::Compatible {
            if seen_loadable_names
                .iter()
                .any(|seen| seen.eq_ignore_ascii_case(&stable_id))
            {
                AsioDriverCompatibility::ShadowedByDuplicateName
            } else {
                seen_loadable_names.push(stable_id.clone());
                AsioDriverCompatibility::Compatible
            }
        } else {
            compatibility
        };

        out.push(AsioDriverDescriptor {
            stable_id,
            display_name: source_name,
            registry_key: entry.key_name,
            clsid,
            module_path,
            compatibility,
        });
    }

    out.sort_by(|a, b| {
        a.stable_id
            .to_lowercase()
            .cmp(&b.stable_id.to_lowercase())
            .then_with(|| a.registry_key.cmp(&b.registry_key))
    });
    out
}

// ── Live registry / filesystem shell ─────────────────────────────────────────

fn read_view(root: &windows_registry::Key, path: &str, wow64: bool) -> Vec<RawAsioRegistration> {
    let Ok(key) = root.open(path) else {
        return Vec::new();
    };
    let Ok(subkeys) = key.keys() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in subkeys {
        let (description, clsid) = match key.open(&name) {
            Ok(sub) => (
                sub.get_string("Description").ok(),
                sub.get_string("CLSID").ok(),
            ),
            Err(_) => (None, None),
        };
        out.push(RawAsioRegistration {
            key_name: name,
            description,
            clsid,
            from_wow64_view: wow64,
        });
    }
    out
}

/// Read both registry views. The 64-bit view is what the loader actually uses;
/// `WOW6432Node` rows exist only so a 32-bit-only install is diagnosable
/// instead of invisibly absent.
pub fn read_registrations() -> Vec<RawAsioRegistration> {
    let mut raw = read_view(windows_registry::LOCAL_MACHINE, "SOFTWARE\\ASIO", false);
    let wow = read_view(
        windows_registry::LOCAL_MACHINE,
        "SOFTWARE\\WOW6432Node\\ASIO",
        true,
    );
    for entry in wow {
        // Only keep 32-bit rows that are not also (correctly) registered in
        // the 64-bit view.
        if !raw
            .iter()
            .any(|native| native.key_name.eq_ignore_ascii_case(&entry.key_name))
        {
            raw.push(entry);
        }
    }
    raw
}

struct LiveClsidResolver;

impl ClsidResolver for LiveClsidResolver {
    fn inproc_server_path(&self, clsid: &str) -> Option<PathBuf> {
        let path = format!("CLSID\\{clsid}\\InprocServer32");
        let key = windows_registry::CLASSES_ROOT.open(&path).ok()?;
        // Default value = module path. Some drivers quote it or append args.
        let value = key.get_string("").ok()?;
        let trimmed = value.trim().trim_matches('"').to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    }

    fn module_machine(&self, path: &Path) -> Option<u16> {
        pe_machine_of_file(path)
    }
}

/// Read the PE `Machine` field from a DLL header. Plain `std::fs` reads on the
/// control thread; never called from audio code.
fn pe_machine_of_file(path: &Path) -> Option<u16> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; 4096];
    let read = file.read(&mut header).ok()?;
    let header = &header[..read];
    if header.len() < 0x40 || &header[0..2] != b"MZ" {
        return None;
    }
    let e_lfanew = u32::from_le_bytes(header[0x3C..0x40].try_into().ok()?) as usize;
    let machine_off = e_lfanew.checked_add(4)?;
    if header.len() < machine_off + 2 || &header[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes(
        header[machine_off..machine_off + 2].try_into().ok()?,
    ))
}

/// Enumerate installed ASIO drivers from the registry. No COM instantiation,
/// no driver loads, no effect on any running stream. Control thread only.
pub fn enumerate_asio_drivers() -> Vec<AsioDriverDescriptor> {
    let descriptors = resolve_descriptors(read_registrations(), &LiveClsidResolver);
    log_enumeration(&descriptors);
    descriptors
}

fn log_enumeration(descriptors: &[AsioDriverDescriptor]) {
    let debug = std::env::var_os("FUTUREBOARD_AUDIO_DEVICE_DEBUG").is_some();
    for d in descriptors {
        if d.is_loadable() {
            if debug {
                eprintln!(
                    "[asio-enum] ok id={:?} clsid={} module={:?}",
                    d.stable_id, d.clsid, d.module_path
                );
            }
        } else {
            // Broken registrations are always visible in logs (short, bounded
            // by installed-driver count) so a missing driver is diagnosable.
            eprintln!(
                "[asio-enum] skipped id={:?} key={:?} clsid={:?} module={:?}: {}",
                d.stable_id,
                d.registry_key,
                d.clsid,
                d.module_path,
                d.compatibility.describe()
            );
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeResolver {
        servers: HashMap<String, PathBuf>,
        machines: HashMap<PathBuf, u16>,
    }

    impl FakeResolver {
        fn new() -> Self {
            Self {
                servers: HashMap::new(),
                machines: HashMap::new(),
            }
        }

        fn with_driver(mut self, clsid: &str, path: &str, machine: u16) -> Self {
            let path_buf = PathBuf::from(path);
            self.servers
                .insert(normalize_clsid(clsid).unwrap(), path_buf.clone());
            self.machines.insert(path_buf, machine);
            self
        }
    }

    impl ClsidResolver for FakeResolver {
        fn inproc_server_path(&self, clsid: &str) -> Option<PathBuf> {
            self.servers.get(clsid).cloned()
        }
        fn module_machine(&self, path: &Path) -> Option<u16> {
            self.machines.get(path).copied()
        }
    }

    fn raw(key: &str, description: Option<&str>, clsid: Option<&str>) -> RawAsioRegistration {
        RawAsioRegistration {
            key_name: key.to_string(),
            description: description.map(str::to_string),
            clsid: clsid.map(str::to_string),
            from_wow64_view: false,
        }
    }

    const CLSID_A: &str = "{11111111-2222-3333-4444-555555555555}";
    const CLSID_B: &str = "{AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE}";

    #[test]
    fn asiolist_name_matches_sdk_truncation() {
        assert_eq!(asiolist_driver_name("Focusrite USB ASIO"), "Focusrite USB ASIO");
        // 31 bytes: passes through untouched (fits the 32-byte buffer).
        let thirty_one = "a".repeat(31);
        assert_eq!(asiolist_driver_name(&thirty_one), thirty_one);
        // 32+ bytes: 28 bytes + "...".
        let long = "b".repeat(40);
        let truncated = asiolist_driver_name(&long);
        assert_eq!(truncated.len(), 31);
        assert_eq!(truncated, format!("{}...", "b".repeat(28)));
    }

    #[test]
    fn description_wins_over_key_name() {
        let entry = raw("KeyName", Some("Nice Driver Name"), Some(CLSID_A));
        assert_eq!(asiolist_source_name(&entry), "Nice Driver Name");
        let entry = raw("KeyName", Some("   "), Some(CLSID_A));
        assert_eq!(asiolist_source_name(&entry), "KeyName");
        let entry = raw("KeyName", None, Some(CLSID_A));
        assert_eq!(asiolist_source_name(&entry), "KeyName");
    }

    #[test]
    fn compatible_driver_resolves() {
        let resolver = FakeResolver::new().with_driver(CLSID_A, "C:/asio/a.dll", 0x8664);
        let out = resolve_descriptors(vec![raw("DriverA", None, Some(CLSID_A))], &resolver);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].stable_id, "DriverA");
        assert_eq!(out[0].compatibility, AsioDriverCompatibility::Compatible);
        assert_eq!(out[0].module_path.as_deref(), Some(Path::new("C:/asio/a.dll")));
    }

    #[test]
    fn missing_metadata_is_diagnosed_not_hidden() {
        let resolver = FakeResolver::new();
        let out = resolve_descriptors(
            vec![
                raw("NoClsid", Some("No Clsid"), None),
                raw("BadClsid", None, Some("not-a-guid")),
                raw("Unregistered", None, Some(CLSID_A)),
            ],
            &resolver,
        );
        assert_eq!(out.len(), 3);
        let by_key = |key: &str| out.iter().find(|d| d.registry_key == key).unwrap();
        assert_eq!(
            by_key("NoClsid").compatibility,
            AsioDriverCompatibility::MissingClsid
        );
        assert_eq!(
            by_key("BadClsid").compatibility,
            AsioDriverCompatibility::InvalidClsid
        );
        assert_eq!(
            by_key("Unregistered").compatibility,
            AsioDriverCompatibility::ClsidNotRegistered
        );
        assert!(out.iter().all(|d| !d.is_loadable()));
    }

    #[test]
    fn missing_module_and_wrong_arch_are_flagged() {
        let mut resolver = FakeResolver::new().with_driver(CLSID_B, "C:/asio/x86.dll", 0x014C);
        resolver
            .servers
            .insert(normalize_clsid(CLSID_A).unwrap(), PathBuf::from("C:/gone.dll"));
        let out = resolve_descriptors(
            vec![
                raw("Gone", None, Some(CLSID_A)),
                raw("ThirtyTwoBit", None, Some(CLSID_B)),
            ],
            &resolver,
        );
        let by_key = |key: &str| out.iter().find(|d| d.registry_key == key).unwrap();
        assert_eq!(
            by_key("Gone").compatibility,
            AsioDriverCompatibility::ModuleMissing
        );
        assert_eq!(
            by_key("ThirtyTwoBit").compatibility,
            AsioDriverCompatibility::WrongArchitecture { machine: 0x014C }
        );
    }

    #[test]
    fn duplicate_clsid_dedupes_and_duplicate_name_shadows() {
        let resolver = FakeResolver::new()
            .with_driver(CLSID_A, "C:/asio/a.dll", 0x8664)
            .with_driver(CLSID_B, "C:/asio/b.dll", 0x8664);
        let out = resolve_descriptors(
            vec![
                raw("KeyOne", Some("Same Device"), Some(CLSID_A)),
                // Same CLSID again under another key: deduped entirely.
                raw("KeyTwo", Some("Same Device Again"), Some(CLSID_A)),
                // Different CLSID but same asiolist name as KeyOne: shadowed.
                raw("KeyThree", Some("Same Device"), Some(CLSID_B)),
            ],
            &resolver,
        );
        assert_eq!(out.len(), 2);
        let loadable: Vec<_> = out.iter().filter(|d| d.is_loadable()).collect();
        assert_eq!(loadable.len(), 1);
        assert_eq!(loadable[0].registry_key, "KeyOne");
        assert_eq!(
            out.iter()
                .find(|d| d.registry_key == "KeyThree")
                .unwrap()
                .compatibility,
            AsioDriverCompatibility::ShadowedByDuplicateName
        );
    }

    #[test]
    fn wow64_only_entries_are_diagnostic_not_selectable() {
        let resolver = FakeResolver::new().with_driver(CLSID_A, "C:/asio/a.dll", 0x8664);
        let mut entry = raw("Legacy32", Some("Legacy 32-bit"), Some(CLSID_A));
        entry.from_wow64_view = true;
        let out = resolve_descriptors(vec![entry], &resolver);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].compatibility,
            AsioDriverCompatibility::WrongRegistryView
        );
        assert!(!out[0].is_loadable());
    }

    #[test]
    fn ordering_and_identity_are_stable_across_refreshes() {
        let resolver = FakeResolver::new()
            .with_driver(CLSID_A, "C:/asio/a.dll", 0x8664)
            .with_driver(CLSID_B, "C:/asio/b.dll", 0x8664);
        let build = |shuffled: bool| {
            let mut entries = vec![
                raw("Zeta", Some("Zeta ASIO"), Some(CLSID_B)),
                raw("Alpha", Some("Alpha ASIO"), Some(CLSID_A)),
            ];
            if shuffled {
                entries.reverse();
            }
            resolve_descriptors(entries, &resolver)
        };
        let first = build(false);
        let second = build(true);
        assert_eq!(first, second);
        assert_eq!(first[0].stable_id, "Alpha ASIO");
        assert_eq!(first[1].stable_id, "Zeta ASIO");
    }
}
