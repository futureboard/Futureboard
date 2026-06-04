use super::{
    format::{decode_project, encode_project, ProjectError},
    now_secs, ClipSource, FutureboardProject, ProjectAsset,
};
use crate::paths::{FutureboardPaths, ProjectFolderLayout};
use std::fs;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE_EXT: &str = "fbproj";
pub const LEGACY_PROJECT_FILE_EXT: &str = "fbs";
pub const SUPPORTED_PROJECT_FILE_EXTS: &[&str] = &[PROJECT_FILE_EXT, LEGACY_PROJECT_FILE_EXT];

/// Platform-aware default projects directory: `~/Documents/Futureboard Studio/Projects/`.
///
/// Delegates to [`FutureboardPaths::resolve()`] so the path string is defined
/// in exactly one place.
pub fn default_projects_dir() -> PathBuf {
    FutureboardPaths::resolve().projects
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
/// Delegates to [`ProjectFolderLayout`] for the actual subfolder structure.
///
/// Folder layout:
/// ```text
/// <base_dir>/<project_name>/
///   <project_name>.fbproj
///   Assets/Audio/
///   Assets/MIDI/
///   Assets/Samples/
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
    let mut root = base_dir.join(&safe_name);
    if root.exists() {
        for index in 1..=999 {
            let candidate = base_dir.join(format!("{safe_name}-{index}"));
            if !candidate.exists() {
                root = candidate;
                break;
            }
        }
    }

    let layout = ProjectFolderLayout::from_root(root.clone());
    layout.ensure_dirs()?;

    Ok(root)
}

/// Atomically writes `project` to `path` using a `.tmp` file + rename.
pub fn save_project(project: &mut FutureboardProject, path: &Path) -> Result<(), ProjectError> {
    project_save_debug(format_args!("save requested -> {}", path.display()));
    prepare_portable_assets(project, path)?;
    project.modified_at = now_secs();
    let bytes = encode_project(project);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("fbproj.tmp");
    fs::write(&tmp_path, &bytes)?;
    fs::rename(&tmp_path, path)?;
    project_save_debug(format_args!("project file written -> {}", path.display()));
    Ok(())
}

/// Loads a `FutureboardProject` from `path`.
pub fn load_project(path: &Path) -> Result<FutureboardProject, ProjectError> {
    let bytes = fs::read(path)?;
    let mut project = decode_project(&bytes)?;
    resolve_project_relative_assets(&mut project, path);
    Ok(project)
}

/// Cheaply validate a project file on disk by reading only its 20-byte header
/// (magic + version). Returns the format version on success, or a
/// [`ProjectError`] describing why it is not a valid/supported Futureboard
/// project. Does not decode the body or verify the checksum — use
/// [`load_project`] for that. Never panics on I/O errors.
pub fn validate_project_file(path: &Path) -> Result<u32, ProjectError> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut header = [0u8; 20];
    if file.read_exact(&mut header).is_err() {
        return Err(ProjectError::Corrupted("file too small".into()));
    }
    super::format::peek_project_header(&header)
}

fn project_save_debug(args: std::fmt::Arguments<'_>) {
    if std::env::var("FUTUREBOARD_PROJECT_SAVE_DEBUG").as_deref() == Ok("1")
        || std::env::var("FUTUREBOARD_ASSET_COPY_DEBUG").as_deref() == Ok("1")
    {
        eprintln!("[project-save] {args}");
    }
}

macro_rules! project_save_debug {
    ($($arg:tt)*) => { project_save_debug(format_args!($($arg)*)) };
}

fn prepare_portable_assets(
    project: &mut FutureboardProject,
    project_file: &Path,
) -> Result<(), ProjectError> {
    let Some(project_root) = project_file.parent() else {
        return Ok(());
    };
    let layout = ProjectFolderLayout::from_root(project_root.to_path_buf());
    layout.ensure_dirs()?;

    let mut copied: Vec<(PathBuf, String)> = Vec::new();
    let mut assets: Vec<ProjectAsset> = Vec::new();
    project_save_debug!("asset copy plan root={}", project_root.display());

    for track in &mut project.tracks {
        for clip in &mut track.clips {
            let ClipSource::Audio {
                asset_id,
                source_path: Some(source_path),
            } = &mut clip.source
            else {
                continue;
            };

            let source_abs = resolve_source_for_save(source_path, project_root);
            if !source_abs.exists() {
                return Err(ProjectError::Corrupted(format!(
                    "missing audio asset: {}",
                    source_abs.display()
                )));
            }

            if let Some(relative) = path_relative_to_project(&source_abs, project_root) {
                let relative_string = path_to_project_string(&relative);
                project_save_debug!("asset already inside project -> {}", relative_string);
                *source_path = PathBuf::from(&relative_string);
                *asset_id = relative_string.clone();
                assets.push(asset_record(
                    asset_id.clone(),
                    &source_abs,
                    relative_string,
                    None,
                )?);
                continue;
            }

            if let Some((_, relative_string)) = copied
                .iter()
                .find(|(known_source, _)| same_source(known_source, &source_abs))
            {
                project_save_debug!("asset reused -> {}", relative_string);
                *source_path = PathBuf::from(relative_string);
                *asset_id = relative_string.clone();
                continue;
            }

            let dest = unique_asset_destination(&layout.media_audio, &source_abs)?;
            fs::copy(&source_abs, &dest)?;
            let relative = path_relative_to_project(&dest, project_root).ok_or_else(|| {
                ProjectError::Corrupted(format!(
                    "copied asset escaped project folder: {}",
                    dest.display()
                ))
            })?;
            let relative_string = path_to_project_string(&relative);
            project_save_debug!(
                "asset copied {} -> {}",
                source_abs.display(),
                dest.display()
            );

            *source_path = PathBuf::from(&relative_string);
            *asset_id = relative_string.clone();
            copied.push((source_abs.clone(), relative_string.clone()));
            assets.push(asset_record(
                asset_id.clone(),
                &dest,
                relative_string,
                Some(source_abs),
            )?);
        }
    }

    if !assets.is_empty() {
        project.assets = assets;
    }
    Ok(())
}

fn resolve_project_relative_assets(project: &mut FutureboardProject, project_file: &Path) {
    let Some(project_root) = project_file.parent() else {
        return;
    };
    for track in &mut project.tracks {
        for clip in &mut track.clips {
            let ClipSource::Audio {
                source_path: Some(source_path),
                ..
            } = &mut clip.source
            else {
                continue;
            };
            if source_path.is_relative() {
                *source_path = project_root.join(&source_path);
            }
        }
    }
}

fn resolve_source_for_save(source_path: &Path, project_root: &Path) -> PathBuf {
    if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        project_root.join(source_path)
    }
}

fn path_relative_to_project(path: &Path, project_root: &Path) -> Option<PathBuf> {
    let path = fs::canonicalize(path).ok()?;
    let root = fs::canonicalize(project_root).ok()?;
    path.strip_prefix(root).ok().map(Path::to_path_buf)
}

fn unique_asset_destination(asset_dir: &Path, source: &Path) -> Result<PathBuf, ProjectError> {
    fs::create_dir_all(asset_dir)?;
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_project_name)
        .unwrap_or_else(|| "audio".to_string());
    let stem = Path::new(&file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    let ext = Path::new(&file_name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let mut candidate = asset_dir.join(&file_name);
    if !candidate.exists() {
        return Ok(candidate);
    }
    for index in 1..=999 {
        let name = if ext.is_empty() {
            format!("{stem}-{index}")
        } else {
            format!("{stem}-{index}.{ext}")
        };
        candidate = asset_dir.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(ProjectError::Corrupted(format!(
        "could not create a unique asset filename for {}",
        source.display()
    )))
}

fn asset_record(
    id: String,
    copied_path: &Path,
    relative_path: String,
    original_path: Option<PathBuf>,
) -> Result<ProjectAsset, ProjectError> {
    Ok(ProjectAsset {
        id,
        original_filename: copied_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "audio".to_string()),
        relative_path: Some(relative_path),
        absolute_path: original_path,
        duration_secs: None,
        sample_rate: None,
        channels: None,
    })
}

fn same_source(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn path_to_project_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ClipSource, ProjectClip, ProjectTrack, ProjectTrackType, TrackRouting};

    fn temp_dir(label: &str) -> PathBuf {
        let unique = format!(
            "futureboard-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn save_project_copies_external_audio_to_assets_audio() {
        let root = temp_dir("asset-copy");
        let external = temp_dir("external-audio");
        fs::create_dir_all(&external).unwrap();
        let source = external.join("loop.wav");
        fs::write(&source, b"fake wav bytes").unwrap();

        let mut project = FutureboardProject::new("Portable");
        project.tracks.push(ProjectTrack {
            id: "track-1".to_string(),
            name: "Audio 1".to_string(),
            track_type: ProjectTrackType::Audio,
            color_hex: "#56C7C9".to_string(),
            volume_norm: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            record_arm: false,
            input_monitor: crate::project::InputMonitorMode::Off,
            routing: TrackRouting::default(),
            inserts: Vec::new(),
            automation_lanes: Vec::new(),
            clips: vec![ProjectClip {
                id: "clip-1".to_string(),
                name: "loop".to_string(),
                start_beat: 0.0,
                duration_beats: 4.0,
                offset_beats: 0.0,
                gain: 1.0,
                muted: false,
                source: ClipSource::Audio {
                    asset_id: source.to_string_lossy().into_owned(),
                    source_path: Some(source.clone()),
                },
            }],
        });

        let project_file = root.join("Portable.fbproj");
        save_project(&mut project, &project_file).unwrap();

        let copied = root.join("Assets").join("Audio").join("loop.wav");
        assert!(copied.exists());
        let ClipSource::Audio {
            asset_id,
            source_path: Some(source_path),
        } = &project.tracks[0].clips[0].source
        else {
            panic!("expected audio clip source");
        };
        assert_eq!(asset_id, "Assets/Audio/loop.wav");
        assert_eq!(source_path, &PathBuf::from("Assets/Audio/loop.wav"));
        assert_eq!(project.assets.len(), 1);

        let loaded = load_project(&project_file).unwrap();
        let ClipSource::Audio {
            source_path: Some(loaded_path),
            ..
        } = &loaded.tracks[0].clips[0].source
        else {
            panic!("expected loaded audio clip source");
        };
        assert_eq!(loaded_path, &copied);

        let moved_root = temp_dir("asset-copy-moved");
        fs::rename(&root, &moved_root).unwrap();
        let moved_project_file = moved_root.join("Portable.fbproj");
        let moved_copied = moved_root.join("Assets").join("Audio").join("loop.wav");
        let moved_loaded = load_project(&moved_project_file).unwrap();
        let ClipSource::Audio {
            source_path: Some(moved_loaded_path),
            ..
        } = &moved_loaded.tracks[0].clips[0].source
        else {
            panic!("expected moved loaded audio clip source");
        };
        assert_eq!(moved_loaded_path, &moved_copied);

        let _ = fs::remove_dir_all(moved_root);
        let _ = fs::remove_dir_all(external);
    }
}
