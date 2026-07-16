//! Cross-edition task runner (cargo-xtask pattern).
//!
//! Cargo aliases cannot chain commands, and the two editions must build into
//! separate target directories (cargo unifies features within one build
//! graph, so a shared directory would overwrite or reuse app artifacts).
//! `cargo xtask build-all` runs each edition's existing alias from
//! `.cargo/config.toml` in sequence, keeping those aliases the single source
//! of truth for edition flags.

use std::env;
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <command> [cargo args...]

Commands:
  build-all   cargo build-ce, then cargo build-exclusive-win
  check-all   cargo check-ce, then cargo check-exclusive-win

Extra arguments are forwarded to every underlying cargo invocation,
e.g. `cargo xtask build-all --release`.";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let forwarded: Vec<String> = args.collect();
    let aliases: &[&str] = match command.as_str() {
        "build-all" => &["build-ce", "build-exclusive-win"],
        "check-all" => &["check-ce", "check-exclusive-win"],
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => {
            eprintln!("error: unknown xtask command `{command}`\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    for alias in aliases {
        if let Err(code) = run_cargo_alias(alias, &forwarded) {
            return code;
        }
    }
    ExitCode::SUCCESS
}

fn run_cargo_alias(alias: &str, forwarded: &[String]) -> Result<(), ExitCode> {
    // Run from the workspace root so the aliases in .cargo/config.toml
    // resolve regardless of where `cargo xtask` was invoked.
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    // Cargo exports its own path as $CARGO for subprocesses; fall back to
    // PATH lookup when the binary is run directly.
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    eprintln!("[xtask] cargo {} {}", alias, forwarded.join(" "));
    let status = Command::new(&cargo)
        .arg(alias)
        .args(forwarded)
        .current_dir(root)
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            eprintln!("[xtask] cargo {alias} failed with {status}");
            let code = status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(1);
            Err(ExitCode::from(code))
        }
        Err(error) => {
            eprintln!("[xtask] failed to spawn `{cargo} {alias}`: {error}");
            Err(ExitCode::FAILURE)
        }
    }
}
