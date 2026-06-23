use std::path::PathBuf;

use apak::{
    InstallOptions, InstallRoots, PackOptions, default_secret_file, install_package, pack_template,
    read_package_info, write_template,
};
use clap::{Parser, Subcommand};

use crate::platform::{ELEVATED_WARNING_CLI, is_process_elevated};

#[derive(Parser, Debug)]
#[command(name = "apak", version, about = ".apak audio package installer")]
pub struct ApakArgs {
    #[command(subcommand)]
    command: ApakCommand,
}

#[derive(Subcommand, Debug)]
enum ApakCommand {
    Init {
        #[arg(default_value = ".")]
        directory: PathBuf,
    },
    Pack {
        source: PathBuf,
        output: PathBuf,
        #[arg(long, value_name = "FILE")]
        secret_file: Option<PathBuf>,
    },
    Install {
        package: PathBuf,
        #[arg(long, value_name = "FILE")]
        secret_file: Option<PathBuf>,
    },
    Info {
        package: PathBuf,
        #[arg(long, value_name = "FILE")]
        secret_file: Option<PathBuf>,
    },
    Roots,
}

#[derive(Parser, Debug)]
#[command(name = "makeapak", version, about = "Build a .apak package")]
pub struct MakeApakArgs {
    source: PathBuf,
    output: PathBuf,
    #[arg(long, value_name = "FILE")]
    secret_file: Option<PathBuf>,
}

pub fn run_apak_cli() -> i32 {
    warn_if_elevated();
    match run_apak_command(ApakArgs::parse()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("apak: {error}");
            1
        }
    }
}

pub fn run_makeapak_cli() -> i32 {
    warn_if_elevated();
    match run_makeapak_command(MakeApakArgs::parse()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("makeapak: {error}");
            1
        }
    }
}

fn warn_if_elevated() {
    if is_process_elevated() {
        eprintln!("{ELEVATED_WARNING_CLI}");
    }
}

fn run_apak_command(args: ApakArgs) -> apak::Result<()> {
    match args.command {
        ApakCommand::Init { directory } => {
            write_template(&directory)?;
            println!("Template written to {}", directory.display());
        }
        ApakCommand::Pack {
            source,
            output,
            secret_file,
        } => {
            pack(source, output, secret_file)?;
        }
        ApakCommand::Install {
            package,
            secret_file,
        } => {
            install(package, secret_file)?;
        }
        ApakCommand::Info {
            package,
            secret_file,
        } => {
            let summary =
                read_package_info(&package, &secret_file.unwrap_or_else(default_secret_file))?;
            print_summary(&summary);
        }
        ApakCommand::Roots => {
            let roots = InstallRoots::default_user()?;
            println!("Samples: {}", roots.samples.display());
            println!("Presets: {}", roots.presets.display());
            println!("Extentions: {}", roots.extentions.display());
        }
    }
    Ok(())
}

fn run_makeapak_command(args: MakeApakArgs) -> apak::Result<()> {
    pack(args.source, args.output, args.secret_file)
}

fn pack(source: PathBuf, output: PathBuf, secret_file: Option<PathBuf>) -> apak::Result<()> {
    let report = pack_template(PackOptions {
        source_dir: source,
        output_path: output,
        secret_file: secret_file.unwrap_or_else(default_secret_file),
    })?;
    print_summary(&report.summary);
    println!("Assets: {}", report.asset_count);
    println!("Output: {}", report.output_path.display());
    println!("Bytes: {}", report.byte_len);
    Ok(())
}

fn install(package: PathBuf, secret_file: Option<PathBuf>) -> apak::Result<()> {
    let report = install_package(InstallOptions {
        package_path: package,
        secret_file: secret_file.unwrap_or_else(default_secret_file),
        roots: InstallRoots::default_user()?,
    })?;
    print_summary(&report.summary);
    println!("Installed files: {}", report.installed_files.len());
    for path in report.installed_files {
        println!("  {}", path.display());
    }
    Ok(())
}

fn print_summary(summary: &apak::PackageSummary) {
    println!("Package: {} ({})", summary.name, summary.id);
    println!("Version: {}", summary.version);
    println!("Target: {}", summary.target);
    println!("Publisher: {}", summary.publisher);
    println!("License: {}", summary.license);
    if !summary.description.trim().is_empty() {
        println!("Description: {}", summary.description);
    }
}
