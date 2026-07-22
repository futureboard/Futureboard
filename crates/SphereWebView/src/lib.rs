//! Native, windowed Chromium Embedded Framework views for Futureboard Studio.
//!
//! The CEF SDK is intentionally not downloaded by normal workspace builds.
//! Run the `install_cef` example with the `installer` feature to populate the
//! workspace-local `build/cef` directory, then enable `cef-runtime` in the
//! executable that owns the CEF process lifecycle. Windowless/off-screen
//! rendering is never enabled by this crate.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// CEF release selected for every supported desktop target.
pub const CEF_VERSION: &str = "150.0.11+gb887805";
pub const CHROMIUM_VERSION: &str = "150.0.7871.115";

pub const WINDOWS_X86_64_URL: &str = "https://cef-builds.spotifycdn.com/cef_binary_150.0.11%2Bgb887805%2Bchromium-150.0.7871.115_windows64.tar.bz2";
pub const LINUX_X86_64_URL: &str = "https://cef-builds.spotifycdn.com/cef_binary_150.0.11%2Bgb887805%2Bchromium-150.0.7871.115_linux64.tar.bz2";
pub const MACOS_X86_64_URL: &str = "https://cef-builds.spotifycdn.com/cef_binary_150.0.11%2Bgb887805%2Bchromium-150.0.7871.115_macosx64.tar.bz2";
pub const MACOS_AARCH64_URL: &str = "https://cef-builds.spotifycdn.com/cef_binary_150.0.11%2Bgb887805%2Bchromium-150.0.7871.115_macosarm64.tar.bz2";

/// Desktop CEF distributions currently pinned by Futureboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CefTarget {
    WindowsX86_64,
    LinuxX86_64,
    MacOsX86_64,
    MacOsAarch64,
}

impl CefTarget {
    pub const fn target_triple(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "x86_64-pc-windows-msvc",
            Self::LinuxX86_64 => "x86_64-unknown-linux-gnu",
            Self::MacOsX86_64 => "x86_64-apple-darwin",
            Self::MacOsAarch64 => "aarch64-apple-darwin",
        }
    }

    pub const fn archive_url(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => WINDOWS_X86_64_URL,
            Self::LinuxX86_64 => LINUX_X86_64_URL,
            Self::MacOsX86_64 => MACOS_X86_64_URL,
            Self::MacOsAarch64 => MACOS_AARCH64_URL,
        }
    }

    pub const fn archive_name(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => {
                "cef_binary_150.0.11+gb887805+chromium-150.0.7871.115_windows64.tar.bz2"
            }
            Self::LinuxX86_64 => {
                "cef_binary_150.0.11+gb887805+chromium-150.0.7871.115_linux64.tar.bz2"
            }
            Self::MacOsX86_64 => {
                "cef_binary_150.0.11+gb887805+chromium-150.0.7871.115_macosx64.tar.bz2"
            }
            Self::MacOsAarch64 => {
                "cef_binary_150.0.11+gb887805+chromium-150.0.7871.115_macosarm64.tar.bz2"
            }
        }
    }

    pub fn from_target_triple(target: &str) -> Result<Self, CefDistributionError> {
        match target {
            "x86_64-pc-windows-msvc" => Ok(Self::WindowsX86_64),
            "x86_64-unknown-linux-gnu" => Ok(Self::LinuxX86_64),
            "x86_64-apple-darwin" => Ok(Self::MacOsX86_64),
            "aarch64-apple-darwin" => Ok(Self::MacOsAarch64),
            _ => Err(CefDistributionError::UnsupportedTarget(target.to_owned())),
        }
    }

    pub fn current() -> Result<Self, CefDistributionError> {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return Ok(Self::WindowsX86_64);
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return Ok(Self::LinuxX86_64);
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return Ok(Self::MacOsX86_64);
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return Ok(Self::MacOsAarch64);
        #[allow(unreachable_code)]
        Err(CefDistributionError::UnsupportedTarget(format!(
            "{}-{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        )))
    }

    fn runtime_library(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "libcef.dll",
            Self::LinuxX86_64 => "libcef.so",
            Self::MacOsX86_64 | Self::MacOsAarch64 => {
                "Chromium Embedded Framework.framework/Chromium Embedded Framework"
            }
        }
    }
}

/// Returns `<workspace>/build/cef`, the path consumed by `cef-dll-sys`.
pub fn cef_path(workspace_root: impl AsRef<Path>) -> PathBuf {
    workspace_root.as_ref().join("build").join("cef")
}

/// Resolves the Futureboard workspace from this crate's compile-time location.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("SphereWebView must remain under <workspace>/crates")
        .to_path_buf()
}

pub fn workspace_cef_path() -> PathBuf {
    cef_path(workspace_root())
}

/// Verifies that a prepared SDK has the headers, CMake metadata, archive
/// manifest and target runtime required by cef-rs.
pub fn validate_cef_path(
    path: impl AsRef<Path>,
    target: CefTarget,
) -> Result<(), CefDistributionError> {
    let path = path.as_ref();
    for relative in [
        Path::new("archive.json"),
        Path::new("CMakeLists.txt"),
        Path::new("include/cef_app.h"),
        Path::new(target.runtime_library()),
    ] {
        let candidate = path.join(relative);
        if !candidate.is_file() {
            return Err(CefDistributionError::MissingFile(candidate));
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum CefDistributionError {
    #[error("unsupported CEF target: {0}")]
    UnsupportedTarget(String),
    #[error("CEF distribution is missing required file: {0}")]
    MissingFile(PathBuf),
    #[error("CEF destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("CEF install I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[cfg(feature = "installer")]
    #[error("CEF download or extraction failed: {0}")]
    Download(#[from] download_cef::Error),
    #[cfg(feature = "installer")]
    #[error("CEF HTTP request failed: {0}")]
    Http(#[from] Box<ureq::Error>),
}

#[cfg(feature = "installer")]
mod installer;
#[cfg(feature = "installer")]
pub use installer::install_cef;

#[cfg(feature = "cef-runtime")]
pub mod client;

#[cfg(feature = "cef-runtime")]
pub mod runtime;

#[cfg(feature = "cef-runtime")]
pub mod scheme;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_targets_to_the_pinned_archives() {
        let cases = [
            ("x86_64-pc-windows-msvc", "windows64"),
            ("x86_64-unknown-linux-gnu", "linux64"),
            ("x86_64-apple-darwin", "macosx64"),
            ("aarch64-apple-darwin", "macosarm64"),
        ];
        for (triple, archive_platform) in cases {
            let target = CefTarget::from_target_triple(triple).unwrap();
            assert_eq!(target.target_triple(), triple);
            assert!(target.archive_url().contains(archive_platform));
            assert!(target.archive_name().contains(archive_platform));
        }
    }

    #[test]
    fn cef_path_is_workspace_local() {
        assert_eq!(
            cef_path(Path::new("workspace")),
            Path::new("workspace").join("build").join("cef")
        );
    }

    #[test]
    fn unsupported_target_is_explicit() {
        assert!(matches!(
            CefTarget::from_target_triple("wasm32-unknown-unknown"),
            Err(CefDistributionError::UnsupportedTarget(_))
        ));
    }
}
