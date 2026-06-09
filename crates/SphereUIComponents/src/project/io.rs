use super::{
    format::{decode_project, encode_project, ProjectError},
    now_secs, ClipSource, FutureboardProject, ProjectAsset,
};
use crate::paths::{FutureboardPaths, ProjectFolderLayout};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const PROJECT_FILE_EXT: &str = "fbproj";
pub const LEGACY_PROJECT_FILE_EXT: &str = "fbs";
pub const SUPPORTED_PROJECT_FILE_EXTS: &[&str] = &[PROJECT_FILE_EXT, LEGACY_PROJECT_FILE_EXT];

/// Temp path used for atomic saves: `<project>.fbproj.tmp`.
pub fn project_temp_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.tmp", path.display()))
}

/// Backup path written before each successful save: `<project>.fbproj.bak`.
pub fn project_backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.bak", path.display()))
}

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

fn project_save_log(args: std::fmt::Arguments<'_>) {
    eprintln!("[ProjectSave] {args}");
}

/// Atomically writes `project` to `path`:
/// serialize → temp file → flush/fsync → backup existing → rename.
pub fn save_project(project: &mut FutureboardProject, path: &Path) -> Result<(), ProjectError> {
    project_save_log(format_args!("serialize start"));
    prepare_portable_assets(project, path)?;
    project.modified_at = now_secs();
    let bytes = encode_project(project);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = project_temp_path(path);
    let backup_path = project_backup_path(path);

    project_save_log(format_args!("writing temp: {}", tmp_path.display()));
    {
        let mut file = File::create(&tmp_path)?;
        file.write_all(&bytes)?;
        file.flush()?;
        file.sync_all()?;
    }
    project_save_log(format_args!("temp bytes written: {}", bytes.len()));
    project_save_log(format_args!("fsync complete"));

    if path.exists() {
        fs::copy(path, &backup_path)?;
        project_save_log(format_args!("backup written: {}", backup_path.display()));
        // Windows cannot rename over an existing file.
        let _ = fs::remove_file(path);
    }

    match fs::rename(&tmp_path, path) {
        Ok(()) => {
            project_save_log(format_args!("atomic rename complete: {}", path.display()));
            let _ = fs::remove_file(&tmp_path);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&tmp_path);
            project_save_log(format_args!("save failed: {error}"));
            Err(ProjectError::Io(error))
        }
    }
}

/// Loads a `FutureboardProject` from `path`.
pub fn load_project(path: &Path) -> Result<FutureboardProject, ProjectError> {
    project_load_log(format_args!("opening: {}", path.display()));
    let bytes = fs::read(path).map_err(|error| {
        project_load_log(format_args!("failed: I/O error: {error}"));
        ProjectError::Io(error)
    })?;
    let mut project = decode_project(&bytes)?;
    resolve_project_relative_assets(&mut project, path);
    project_load_log(format_args!("loaded ok: {}", project.name));
    Ok(project)
}

/// Round-trip verify that `path` contains a loadable project file.
pub fn verify_project_file(path: &Path) -> Result<(), ProjectError> {
    load_project(path).map(|_| ())
}

/// Cheaply validate a project file on disk by reading only its header.
pub fn validate_project_file(path: &Path) -> Result<u32, ProjectError> {
    use std::io::Read;
    project_load_log(format_args!("validating header: {}", path.display()));
    let mut file = fs::File::open(path)?;
    let mut header = [0u8; 20];
    if file.read_exact(&mut header).is_err() {
        return Err(ProjectError::IncompleteFile {
            reason: "file too small for project header".to_string(),
        });
    }
    super::format::peek_project_header(&header)
}

fn project_load_log(args: std::fmt::Arguments<'_>) {
    eprintln!("[ProjectLoad] {args}");
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
    use crate::project::{
        format::{encode_project, ProjectError, PROJECT_HEADER_SIZE},
        ClipSource, FutureboardProject, ProjectClip, ProjectSession, ProjectTrack, ProjectTrackType,
        TrackRouting,
    };

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
    fn load_empty_file_reports_incomplete_project() {
        let dir = temp_dir("empty-file");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Empty.fbproj");
        fs::write(&path, &[]).unwrap();
        let err = load_project(&path).unwrap_err();
        assert_eq!(
            err.user_message(),
            "Could not open this project because the file appears to be incomplete or corrupted."
        );
        assert!(matches!(err, ProjectError::IncompleteFile { .. }));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_truncated_header_reports_incomplete_project() {
        let dir = temp_dir("trunc-header");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Trunc.fbproj");
        fs::write(&path, &[0u8; PROJECT_HEADER_SIZE - 1]).unwrap();
        let err = load_project(&path).unwrap_err();
        assert!(matches!(err, ProjectError::IncompleteFile { .. }));
        assert_eq!(
            err.user_message(),
            "Could not open this project because the file appears to be incomplete or corrupted."
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_truncated_payload_reports_unexpected_eof() {
        let dir = temp_dir("trunc-payload");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("TruncBody.fbproj");
        let mut bytes = encode_project(&FutureboardProject::new("TruncBody"));
        bytes.truncate(PROJECT_HEADER_SIZE + 2);
        fs::write(&path, &bytes).unwrap();
        let err = load_project(&path).unwrap_err();
        assert!(
            matches!(err, ProjectError::IncompleteFile { .. })
                || matches!(err, ProjectError::UnexpectedEof { .. })
                || matches!(err, ProjectError::ChecksumMismatch { .. })
        );
        assert_eq!(
            err.user_message(),
            "Could not open this project because the file appears to be incomplete or corrupted."
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn atomic_save_keeps_original_when_temp_is_invalid() {
        let dir = temp_dir("atomic-save");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Song.fbproj");
        let mut project = FutureboardProject::new("Song");
        save_project(&mut project, &path).unwrap();
        let original = fs::read(&path).unwrap();
        verify_project_file(&path).unwrap();

        let tmp = project_temp_path(&path);
        fs::write(&tmp, &[1, 2, 3]).unwrap();
        project.name = "Song Updated".to_string();
        save_project(&mut project, &path).unwrap();
        verify_project_file(&path).unwrap();
        let updated = fs::read(&path).unwrap();
        assert_ne!(updated, original);
        assert!(project_backup_path(&path).exists());
        let backup = load_project(&project_backup_path(&path)).unwrap();
        assert_eq!(backup.name, "Song");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn new_project_save_and_reopen_roundtrip() {
        let base = temp_dir("new-project-reopen");
        fs::create_dir_all(&base).unwrap();
        let folder = create_project_folder(&base, "Test Song").unwrap();
        let project_file = folder.join("Test Song.fbproj");
        let mut project = FutureboardProject::new("Test Song");
        save_project(&mut project, &project_file).unwrap();
        verify_project_file(&project_file).unwrap();
        project.name = "Test Song Updated".to_string();
        save_project(&mut project, &project_file).unwrap();

        let loaded = load_project(&project_file).unwrap();
        assert_eq!(loaded.name, "Test Song Updated");
        assert!(project_backup_path(&project_file).exists());

        let mut session = ProjectSession::untitled();
        session.bind_saved(
            loaded.id.clone(),
            loaded.name.clone(),
            Some(folder.clone()),
            project_file.clone(),
            loaded.created_at,
            loaded.modified_at,
        );
        assert!(!session.needs_save_as());

        let _ = fs::remove_dir_all(base);
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
        let loaded = load_project(&project_file).unwrap();
        let ClipSource::Audio {
            source_path: Some(loaded_path),
            ..
        } = &loaded.tracks[0].clips[0].source
        else {
            panic!("expected loaded audio clip source");
        };
        assert_eq!(loaded_path, &copied);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(external);
    }

    #[test]
    fn user_message_maps_invalid_magic() {
        let err = ProjectError::InvalidMagic;
        assert_eq!(err.user_message(), "This file is not a Futureboard project.");
    }

    #[test]
    fn user_message_maps_unexpected_eof() {
        let err = ProjectError::UnexpectedEof {
            needed: 4,
            remaining: 1,
            field: "u32",
        };
        assert_eq!(
            err.user_message(),
            "Could not open this project because the file appears to be incomplete or corrupted."
        );
        assert!(err.technical_detail().contains("u32"));
    }
}
