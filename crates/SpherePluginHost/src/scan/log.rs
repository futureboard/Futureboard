use crate::scan::types::PluginScanFormat;

pub fn scan_start(format: PluginScanFormat) {
    eprintln!("[plugin-scan] scan_start(format={})", format.cli_arg());
}

pub fn scan_found(format: PluginScanFormat, count: usize) {
    eprintln!(
        "[plugin-scan] scan_found(format={}, count={count})",
        format.cli_arg()
    );
}

pub fn scan_plugin_start(format: PluginScanFormat, identifier: &str) {
    eprintln!(
        "[plugin-scan] scan_plugin_start(format={}, identifier={identifier})",
        format.cli_arg()
    );
}

pub fn scan_plugin_success(format: PluginScanFormat, identifier: &str) {
    eprintln!(
        "[plugin-scan] scan_plugin_success(format={}, identifier={identifier})",
        format.cli_arg()
    );
}

pub fn scan_plugin_failed(format: PluginScanFormat, identifier: &str, error: &str) {
    eprintln!(
        "[plugin-scan] scan_plugin_failed(format={}, identifier={identifier}, error={error})",
        format.cli_arg()
    );
}

pub fn scan_process_crashed(format: PluginScanFormat, exit_code: Option<i32>) {
    match exit_code {
        Some(code) => eprintln!(
            "[plugin-scan] scan_process_crashed(format={}, exit_code={code})",
            format.cli_arg()
        ),
        None => eprintln!(
            "[plugin-scan] scan_process_crashed(format={})",
            format.cli_arg()
        ),
    }
}

pub fn scan_finished(
    format: PluginScanFormat,
    success_count: usize,
    failed_count: usize,
    crashed_count: usize,
) {
    eprintln!(
        "[plugin-scan] scan_finished(format={}, success_count={success_count}, failed_count={failed_count}, crashed_count={crashed_count})",
        format.cli_arg()
    );
}
