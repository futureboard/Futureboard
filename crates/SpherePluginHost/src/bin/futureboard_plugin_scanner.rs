#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::env;
use std::path::PathBuf;
use std::process;

use SpherePluginHost::au_scanner;
use SpherePluginHost::scan::isolation::run_direct_format_scan_for_cli;
use SpherePluginHost::scan::types::PluginScanFormat;

fn main() {
    let mut format: Option<PluginScanFormat> = None;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut validate_plugins = false;
    let mut validate_component: Option<String> = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--format" => {
                let value = args.next().unwrap_or_default();
                format = PluginScanFormat::from_cli(&value);
                if format.is_none() {
                    eprintln!("Unknown format: {value}");
                    process::exit(2);
                }
            }
            "--json" => {}
            "--path" => {
                if let Some(path) = args.next() {
                    paths.push(PathBuf::from(path));
                }
            }
            "--validate-plugins" => validate_plugins = true,
            "--validate" => {
                validate_component = args.next();
            }
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_help();
                process::exit(2);
            }
        }
    }

    let Some(format) = format else {
        eprintln!("Missing required --format");
        print_help();
        process::exit(2);
    };

    if let Some(component_id) = validate_component {
        if format != PluginScanFormat::AudioUnit {
            eprintln!("--validate is only supported for audiounit");
            process::exit(2);
        }
        match au_scanner::validate_au_component(&component_id) {
            Ok(ok) => {
                if ok {
                    println!("{{\"ok\":true}}");
                    process::exit(0);
                }
                println!("{{\"ok\":false}}");
                process::exit(1);
            }
            Err(error) => {
                println!(
                    "{{\"ok\":false,\"error\":{}}}",
                    serde_json::to_string(&error.message()).unwrap_or_else(|_| "\"error\"".into())
                );
                process::exit(1);
            }
        }
    }

    let payload = run_direct_format_scan_for_cli(format, &paths, validate_plugins);
    match serde_json::to_string(&payload) {
        Ok(json) => {
            println!("{json}");
            process::exit(if payload.process_crashed { 1 } else { 0 });
        }
        Err(error) => {
            eprintln!("Failed to serialize scan result: {error}");
            process::exit(1);
        }
    }
}

fn print_help() {
    eprintln!(
        "FutureboardPluginScanner\n\
         Usage:\n\
           FutureboardPluginScanner --format vst3|clap|audiounit --json [--path <dir>]...\n\
           FutureboardPluginScanner --format audiounit --json --validate <component-id>\n\
           FutureboardPluginScanner --format audiounit --json --validate-plugins"
    );
}
