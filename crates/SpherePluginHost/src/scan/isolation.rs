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
    if let Ok(path) = std::env::var("FUTUREBOARD_PLUGIN_SCANNER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(PluginScanError::ScannerBinaryMissing(path.display().to_string()));
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let candidate = scanner_binary_name(dir);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    let compile_time = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug/futureboard_plugin_scanner");
    if compile_time.is_file() {
        return Ok(compile_time);
    }

    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/release/futureboard_plugin_scanner");
    if release.is_file() {
        return Ok(release);
    }

    Err(PluginScanError::ScannerBinaryMissing(
        "futureboard_plugin_scanner".into(),
    ))
}

fn scanner_binary_name(dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        dir.join("futureboard_plugin_scanner.exe")
    }
    #[cfg(not(windows))]
    {
        dir.join("futureboard_plugin_scanner")
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

    if request.format == PluginScanFormat::AudioUnit {
        return run_subprocess_scan(request);
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

    let output = match command.stdout(Stdio::piped()).stderr(Stdio::piped()).output() {
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
        let error = PluginScanError::AudioUnitScannerCrashed { exit_code };
        scan_finished(request.format, 0, 0, 1);
        return IsolatedScanOutcome {
            payload: ScanResultPayload::process_crash(
                request.format,
                exit_code,
                error.message(),
            ),
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
