use super::{
    format::{decode_project, encode_project, ProjectError},
    now_secs, FutureboardProject,
};
use std::fs;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE_EXT: &str = "fbproj";

/// Platform-aware default projects directory: `~/Documents/Futureboard Studio/Projects/`.
pub fn default_projects_dir() -> PathBuf {
    dirs::document_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Futureboard Studio")
        .join("Projects")
}

/// Strips characters that are illegal in file/folder names on Windows, macOS, and Linux.
pub fn sanitize_project_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = sanitized.trim_matches(|c: char| c == ' ' || c == '.');
    if trimmed.is_empty() {
        "Untitled Project".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Creates the project folder tree under `base_dir/project_name/`.
/// Returns the root folder path.
///
/// Folder layout:
/// ```
/// <base_dir>/<project_name>/
///   <project_name>.fbproj
///   Media/Audio/
///   Media/MIDI/
///   Media/Samples/
///   Cache/Waveform/
///   Cache/Peaks/
///   Cache/Processed/
///   Cache/Analysis/
///   Rendered/Mixdowns/
///   Rendered/Stems/
///   Rendered/Bounces/
/// ```
pub fn create_project_folder(base_dir: &Path, project_name: &str) -> Result<PathBuf, ProjectError> {
    let safe_name = sanitize_project_name(project_name);
    let root = base_dir.join(&safe_name);

    let dirs_to_create = [
        root.join("Media").join("Audio"),
        root.join("Media").join("MIDI"),
        root.join("Media").join("Samples"),
        root.join("Cache").join("Waveform"),
        root.join("Cache").join("Peaks"),
        root.join("Cache").join("Processed"),
        root.join("Cache").join("Analysis"),
        root.join("Rendered").join("Mixdowns"),
        root.join("Rendered").join("Stems"),
        root.join("Rendered").join("Bounces"),
    ];

    for dir in &dirs_to_create {
        fs::create_dir_all(dir)?;
    }

    Ok(root)
}

/// Atomically writes `project` to `path` using a `.tmp` file + rename.
pub fn save_project(project: &mut FutureboardProject, path: &Path) -> Result<(), ProjectError> {
    project.modified_at = now_secs();
    let bytes = encode_project(project);

    let tmp_path = path.with_extension("fbproj.tmp");
    fs::write(&tmp_path, &bytes)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Loads a `FutureboardProject` from `path`.
pub fn load_project(path: &Path) -> Result<FutureboardProject, ProjectError> {
    let bytes = fs::read(path)?;
    decode_project(&bytes)
}
