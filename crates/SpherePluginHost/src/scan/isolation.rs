use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::scan::log::{
    scan_finished, scan_found, scan_plugin_failed, scan_plugin_start, scan_plugin_success,
    scan_process_crashed, scan_start,
};
use crate::scan::types::{
    PluginDescriptor, PluginScanError, PluginScanFormat, PluginScanStatus, ScanFailureRecord,
    ScanResultPayload,
};
use crate::scanner::{scan_clap_paths, scan_vst3_paths};
use crate::types::PluginInfo;

#[derive(Debug, Clone)]
pub struct IsolatedScanRequest {
    pub format: PluginScanFormat,
    pub paths: Vec<PathBuf>,
    pub validate_plugins: bool,
}

#[derive(Debug, Clone)]
pub struct IsolatedScanOutcome {
    pub payload: ScanResultPayload,
    pub error: Option<PluginScanError>,
}

#[derive(Debug, Clone)]
pub enum ScannerBinaryLocation {
    EnvOverride(PathBuf),
    AdjacentToCurrentExe(PathBuf),
    CompileTime(PathBuf),
}

pub fn locate_scanner_binary() -> Result<PathBuf, PluginScanError> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = scanner_binary_name(dir);
            if candidate.is_file() {
                return Ok(candidate);
            }
            return Err(PluginScanError::ScannerBinaryMissing(
                candidate.display().to_string(),
            ));
        }
    }

    Err(PluginScanError::ScannerBinaryMissing(
        "{appdir}/FutureboardPluginScanner.exe".into(),
    ))
}

pub fn run_isolated_bundle_scan(bundle: &Path) -> Result<Vec<PluginInfo>, String> {
    let format = bundle_scan_format(bundle)
        .ok_or_else(|| format!("Unsupported plug-in bundle: {}", bundle.display()))?;
    let scanner = locate_scanner_binary().map_err(|error| error.message())?;
    let output = Command::new(&scanner)
        .arg("--format")
        .arg(format.cli_arg())
        .arg("--json")
        .arg("--path")
        .arg(bundle)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| PluginScanError::ScannerLaunchFailed(error.to_string()).message())?;

    if !output.status.success() {
        let exit_code = output.status.code();
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        return Err(match (exit_code, detail.is_empty()) {
            (Some(code), true) => {
                format!("{} scanner process crashed (exit {code})", format.cli_arg())
            }
            (Some(code), false) => {
                format!(
                    "{} scanner process failed (exit {code}): {detail}",
                    format.cli_arg()
                )
            }
            (None, true) => format!("{} scanner process crashed", format.cli_arg()),
            (None, false) => format!("{} scanner process crashed: {detail}", format.cli_arg()),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: ScanResultPayload = serde_json::from_str(&stdout)
        .map_err(|error| PluginScanError::ScannerOutputInvalid(error.to_string()).message())?;
    if payload.process_crashed {
        return Err(payload
            .error
            .unwrap_or_else(|| format!("{} scanner process crashed", format.cli_arg())));
    }
    if let Some(error) = payload.error {
        return Err(error);
    }
    Ok(payload
        .plugins
        .iter()
        .map(plugin_info_from_descriptor)
        .collect())
}

fn bundle_scan_format(bundle: &Path) -> Option<PluginScanFormat> {
    let ext = bundle.extension()?.to_str()?;
    match ext.to_ascii_lowercase().as_str() {
        "vst3" => Some(PluginScanFormat::Vst3),
        "clap" => Some(PluginScanFormat::Clap),
        _ => None,
    }
}

fn scanner_binary_name(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        dir.join("FutureboardPluginScanner.exe")
    }
    #[cfg(not(windows))]
    {
        dir.join("FutureboardPluginScanner")
    }
}

pub fn run_isolated_format_scan(request: IsolatedScanRequest) -> IsolatedScanOutcome {
    scan_start(request.format);

    if !request.format.available_on_current_platform() {
        let error = PluginScanError::UnsupportedPlatform;
        return IsolatedScanOutcome {
            payload: ScanResultPayload {
                format: request.format,
                success: true,
                plugins: Vec::new(),
                failures: Vec::new(),
                crashed_plugins: Vec::new(),
                process_crashed: false,
                exit_code: None,
                error: Some(error.message()),
                scanned_paths: request.paths,
            },
            error: Some(error),
        };
    }

    // Prefer the out-of-process scanner for EVERY format, not just AudioUnit, so
    // a crashing or malicious plugin takes down the scanner child rather than the
    // host process. (Previously VST3/CLAP loaded plugin binaries directly
    // in-process here — `catch_unwind` cannot stop a C++ access violation, so a
    // single bad plugin crashed the app despite the "isolated" name.) The
    // in-process branch below is a best-effort fallback for builds shipped
    // without the scanner binary.
    if locate_scanner_binary().is_ok() {
        return run_subprocess_scan(request);
    }

    if request.format == PluginScanFormat::AudioUnit {
        return run_inprocess_au_scan(request);
    }

    match run_inprocess_scan(request.format, &request.paths) {
        Ok(payload) => {
            scan_found(request.format, payload.plugins.len());
            scan_finished(
                request.format,
                payload.plugins.len(),
                payload.failures.len(),
                payload.crashed_plugins.len(),
            );
            IsolatedScanOutcome {
                payload,
                error: None,
            }
        }
        Err(error) => {
            scan_finished(request.format, 0, 1, 0);
            IsolatedScanOutcome {
                payload: ScanResultPayload {
                    format: request.format,
                    success: false,
                    plugins: Vec::new(),
                    failures: request
                        .paths
                        .iter()
                        .map(|path| ScanFailureRecord {
                            path: path.display().to_string(),
                            error: error.message(),
                            scan_status: PluginScanStatus::Failed,
                        })
                        .collect(),
                    crashed_plugins: Vec::new(),
                    process_crashed: false,
                    exit_code: None,
                    error: Some(error.message()),
                    scanned_paths: request.paths,
                },
                error: Some(error),
            }
        }
    }
}

pub fn run_isolated_plugin_validation(
    format: PluginScanFormat,
    component_id: &str,
) -> Result<bool, PluginScanError> {
    if format != PluginScanFormat::AudioUnit {
        return Ok(true);
    }
    if !cfg!(target_os = "macos") {
        return Err(PluginScanError::UnsupportedPlatform);
    }

    let scanner = locate_scanner_binary();
    if scanner.is_ok() {
        let scanner = scanner?;
        let output = Command::new(&scanner)
            .arg("--format")
            .arg(format.cli_arg())
            .arg("--json")
            .arg("--validate")
            .arg(component_id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| PluginScanError::ScannerLaunchFailed(error.to_string()))?;
        if !output.status.success() {
            return Ok(false);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(stdout.contains("\"ok\":true"));
    }

    crate::au_scanner::validate_au_component(component_id)
}

fn run_subprocess_scan(request: IsolatedScanRequest) -> IsolatedScanOutcome {
    let scanner = match locate_scanner_binary() {
        Ok(path) => path,
        Err(error) => {
            if request.format == PluginScanFormat::AudioUnit {
                return run_inprocess_au_scan(request);
            }
            return IsolatedScanOutcome {
                payload: ScanResultPayload {
                    format: request.format,
                    success: false,
                    plugins: Vec::new(),
                    failures: Vec::new(),
                    crashed_plugins: Vec::new(),
                    process_crashed: false,
                    exit_code: None,
                    error: Some(error.message()),
                    scanned_paths: request.paths,
                },
                error: Some(error),
            };
        }
    };

    let mut command = Command::new(&scanner);
    command
        .arg("--format")
        .arg(request.format.cli_arg())
        .arg("--json");
    if request.validate_plugins {
        command.arg("--validate-plugins");
    }
    for path in &request.paths {
        command.arg("--path").arg(path);
    }

    let output = match command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            let scan_error = PluginScanError::ScannerLaunchFailed(error.to_string());
            return IsolatedScanOutcome {
                payload: ScanResultPayload {
                    format: request.format,
                    success: false,
                    plugins: Vec::new(),
                    failures: Vec::new(),
                    crashed_plugins: Vec::new(),
                    process_crashed: false,
                    exit_code: None,
                    error: Some(scan_error.message()),
                    scanned_paths: request.paths,
                },
                error: Some(scan_error),
            };
        }
    };

    let exit_code = output.status.code();
    if !output.status.success() {
        scan_process_crashed(request.format, exit_code);
        let error = PluginScanError::ScannerProcessCrashed {
            format: request.format,
            exit_code,
        };
        scan_finished(request.format, 0, 0, 1);
        return IsolatedScanOutcome {
            payload: ScanResultPayload::process_crash(request.format, exit_code, error.message()),
            error: Some(error),
        };
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<ScanResultPayload>(&stdout) {
        Ok(mut payload) => {
            if payload.scanned_paths.is_empty() {
                payload.scanned_paths = request.paths;
            }
            scan_found(request.format, payload.plugins.len());
            scan_finished(
                request.format,
                payload.plugins.len(),
                payload.failures.len(),
                payload.crashed_plugins.len(),
            );
            IsolatedScanOutcome {
                payload,
                error: None,
            }
        }
        Err(error) => {
            let scan_error =
                PluginScanError::ScannerOutputInvalid(format!("{error}; stdout={stdout}"));
            IsolatedScanOutcome {
                payload: ScanResultPayload {
                    format: request.format,
                    success: false,
                    plugins: Vec::new(),
                    failures: Vec::new(),
                    crashed_plugins: Vec::new(),
                    process_crashed: false,
                    exit_code,
                    error: Some(scan_error.message()),
                    scanned_paths: request.paths,
                },
                error: Some(scan_error),
            }
        }
    }
}

fn run_inprocess_au_scan(request: IsolatedScanRequest) -> IsolatedScanOutcome {
    match crate::au_scanner::scan_audio_units(request.validate_plugins) {
        Ok(plugins) => {
            scan_found(request.format, plugins.len());
            scan_finished(request.format, plugins.len(), 0, 0);
            IsolatedScanOutcome {
                payload: ScanResultPayload {
                    format: request.format,
                    success: true,
                    plugins,
                    failures: Vec::new(),
                    crashed_plugins: Vec::new(),
                    process_crashed: false,
                    exit_code: Some(0),
                    error: None,
                    scanned_paths: request.paths,
                },
                error: None,
            }
        }
        Err(error) => {
            scan_finished(request.format, 0, 1, 0);
            IsolatedScanOutcome {
                payload: ScanResultPayload {
                    format: request.format,
                    success: false,
                    plugins: Vec::new(),
                    failures: Vec::new(),
                    crashed_plugins: Vec::new(),
                    process_crashed: false,
                    exit_code: None,
                    error: Some(error.message()),
                    scanned_paths: request.paths,
                },
                error: Some(error),
            }
        }
    }
}

fn run_inprocess_scan(
    format: PluginScanFormat,
    paths: &[PathBuf],
) -> Result<ScanResultPayload, PluginScanError> {
    let path_strings: Vec<String> = paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let infos = match format {
        PluginScanFormat::Vst3 => scan_vst3_paths(&path_strings),
        PluginScanFormat::Clap => scan_clap_paths(&path_strings),
        PluginScanFormat::AudioUnit => {
            return Err(PluginScanError::AudioUnitUnavailable);
        }
    }
    .map_err(PluginScanError::NativeScanFailed)?;

    let plugins = infos
        .into_iter()
        .map(plugin_descriptor_from_info)
        .collect::<Vec<_>>();
    Ok(ScanResultPayload {
        format,
        success: true,
        plugins,
        failures: Vec::new(),
        crashed_plugins: Vec::new(),
        process_crashed: false,
        exit_code: Some(0),
        error: None,
        scanned_paths: paths.to_vec(),
    })
}

pub fn plugin_descriptor_from_info(info: PluginInfo) -> PluginDescriptor {
    let format = info.format.to_ascii_uppercase();
    let is_instrument = info.category.to_ascii_lowercase().contains("instrument")
        || info
            .sub_categories
            .as_deref()
            .is_some_and(|tags| tags.to_ascii_lowercase().contains("instrument"));
    PluginDescriptor {
        id: info.id,
        format,
        name: info.name.clone(),
        vendor: info.vendor,
        version: info.version,
        path_or_identifier: info.path,
        category: info.category,
        is_instrument,
        is_effect: !is_instrument,
        scan_status: if info.sdk_metadata_loaded {
            PluginScanStatus::Success
        } else {
            PluginScanStatus::Failed
        },
        error_message: None,
        class_id: info.class_id,
        sub_categories: info.sub_categories,
        sdk_metadata_loaded: info.sdk_metadata_loaded,
    }
}

pub fn plugin_info_from_descriptor(descriptor: &PluginDescriptor) -> PluginInfo {
    PluginInfo {
        id: descriptor.id.clone(),
        name: descriptor.name.clone(),
        vendor: descriptor.vendor.clone(),
        category: descriptor.category.clone(),
        sub_categories: descriptor.sub_categories.clone(),
        format: descriptor.format.clone(),
        path: descriptor.path_or_identifier.clone(),
        module_path: Some(descriptor.path_or_identifier.clone()),
        class_id: descriptor.class_id.clone(),
        version: descriptor.version.clone(),
        sdk_version: None,
        is_shell_child: false,
        sdk_metadata_loaded: descriptor.sdk_metadata_loaded,
    }
}

pub fn run_direct_format_scan_for_cli(
    format: PluginScanFormat,
    paths: &[PathBuf],
    validate_plugins: bool,
) -> ScanResultPayload {
    if format == PluginScanFormat::AudioUnit {
        return run_direct_au_scan_for_cli(validate_plugins);
    }

    match run_inprocess_scan(format, paths) {
        Ok(payload) => payload,
        Err(error) => ScanResultPayload {
            format,
            success: false,
            plugins: Vec::new(),
            failures: paths
                .iter()
                .map(|path| ScanFailureRecord {
                    path: path.display().to_string(),
                    error: error.message(),
                    scan_status: PluginScanStatus::Failed,
                })
                .collect(),
            crashed_plugins: Vec::new(),
            process_crashed: false,
            exit_code: None,
            error: Some(error.message()),
            scanned_paths: paths.to_vec(),
        },
    }
}

fn run_direct_au_scan_for_cli(validate_plugins: bool) -> ScanResultPayload {
    if !cfg!(target_os = "macos") {
        return ScanResultPayload {
            format: PluginScanFormat::AudioUnit,
            success: true,
            plugins: Vec::new(),
            failures: Vec::new(),
            crashed_plugins: Vec::new(),
            process_crashed: false,
            exit_code: Some(0),
            error: Some(PluginScanError::UnsupportedPlatform.message()),
            scanned_paths: Vec::new(),
        };
    }

    let enumerated = match crate::au_scanner::scan_audio_units(false) {
        Ok(plugins) => plugins,
        Err(error) => {
            return ScanResultPayload {
                format: PluginScanFormat::AudioUnit,
                success: false,
                plugins: Vec::new(),
                failures: Vec::new(),
                crashed_plugins: Vec::new(),
                process_crashed: false,
                exit_code: None,
                error: Some(error.message()),
                scanned_paths: Vec::new(),
            };
        }
    };

    if !validate_plugins {
        scan_found(PluginScanFormat::AudioUnit, enumerated.len());
        scan_finished(PluginScanFormat::AudioUnit, enumerated.len(), 0, 0);
        return ScanResultPayload {
            format: PluginScanFormat::AudioUnit,
            success: true,
            plugins: enumerated,
            failures: Vec::new(),
            crashed_plugins: Vec::new(),
            process_crashed: false,
            exit_code: Some(0),
            error: None,
            scanned_paths: Vec::new(),
        };
    }

    let mut validated = Vec::new();
    let mut failures = Vec::new();
    let mut crashed = Vec::new();

    for plugin in enumerated {
        let identifier = plugin
            .class_id
            .clone()
            .unwrap_or_else(|| plugin.path_or_identifier.clone());
        scan_plugin_start(PluginScanFormat::AudioUnit, &identifier);

        match validate_au_in_child(&identifier) {
            Ok(true) => {
                scan_plugin_success(PluginScanFormat::AudioUnit, &identifier);
                validated.push(plugin);
            }
            Ok(false) => {
                scan_plugin_failed(
                    PluginScanFormat::AudioUnit,
                    &identifier,
                    "validation failed",
                );
                failures.push(ScanFailureRecord {
                    path: identifier.clone(),
                    error: PluginScanError::AudioUnitInstantiationFailed(
                        "validation failed".into(),
                    )
                    .message(),
                    scan_status: PluginScanStatus::Failed,
                });
            }
            Err(PluginScanError::AudioUnitScannerCrashed { exit_code }) => {
                scan_process_crashed(PluginScanFormat::AudioUnit, exit_code);
                crashed.push(ScanFailureRecord {
                    path: identifier,
                    error: PluginScanError::AudioUnitScannerCrashed { exit_code }.message(),
                    scan_status: PluginScanStatus::Crashed,
                });
            }
            Err(error) => {
                scan_plugin_failed(PluginScanFormat::AudioUnit, &identifier, &error.message());
                failures.push(ScanFailureRecord {
                    path: identifier,
                    error: error.message(),
                    scan_status: PluginScanStatus::Failed,
                });
            }
        }
    }

    scan_finished(
        PluginScanFormat::AudioUnit,
        validated.len(),
        failures.len(),
        crashed.len(),
    );

    ScanResultPayload {
        format: PluginScanFormat::AudioUnit,
        success: crashed.is_empty(),
        plugins: validated,
        failures,
        crashed_plugins: crashed,
        process_crashed: false,
        exit_code: Some(0),
        error: None,
        scanned_paths: Vec::new(),
    }
}

fn validate_au_in_child(component_id: &str) -> Result<bool, PluginScanError> {
    if let Ok(scanner) = locate_scanner_binary() {
        let output = Command::new(scanner)
            .arg("--format")
            .arg("audiounit")
            .arg("--json")
            .arg("--validate")
            .arg(component_id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| PluginScanError::ScannerLaunchFailed(error.to_string()))?;
        if !output.status.success() {
            return Err(PluginScanError::AudioUnitScannerCrashed {
                exit_code: output.status.code(),
            });
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(stdout.contains("\"ok\":true"));
    }
    crate::au_scanner::validate_au_component(component_id)
}
