#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        apakinstaller_app::gui::run(None);
        return;
    }

    if args.len() == 1 {
        let path = PathBuf::from(&args[0]);
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("apak"))
        {
            apakinstaller_app::gui::run(Some(path));
            return;
        }
    }

    std::process::exit(apakinstaller_app::cli::run_apak_cli());
}
