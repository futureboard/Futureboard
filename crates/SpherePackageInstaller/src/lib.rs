use std::fmt;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use aes_gcm::aead::rand_core::{OsRng, RngCore};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use walkdir::WalkDir;

const MAGIC: &[u8; 5] = b"APAK\0";
const FORMAT_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const KDF_ROUNDS: u32 = 210_000;

#[derive(Debug, thiserror::Error)]
pub enum ApakError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("archive error: {0}")]
    Archive(String),
    #[error("compression error: {0}")]
    Compression(String),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("invalid .apak package: {0}")]
    InvalidPackage(String),
    #[error("invalid package template: {0}")]
    InvalidTemplate(String),
    #[error("unsupported package target: {0}")]
    UnsupportedTarget(String),
}

pub type Result<T> = std::result::Result<T, ApakError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageTarget {
    Sample,
    Preset,
    #[serde(alias = "Extension", alias = "Extensions")]
    Extentions,
}

impl fmt::Display for PackageTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sample => f.write_str("Sample"),
            Self::Preset => f.write_str("Preset"),
            Self::Extentions => f.write_str("Extentions"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallToml {
    pub package: PackageSection,
    #[serde(default)]
    pub install: InstallSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSection {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub target: PackageTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallSection {
    #[serde(default = "default_overwrite")]
    pub overwrite: bool,
}

impl Default for InstallSection {
    fn default() -> Self {
        Self {
            overwrite: default_overwrite(),
        }
    }
}

fn default_overwrite() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataToml {
    pub metadata: MetadataSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataSection {
    pub publisher: String,
    pub description: String,
    pub license: String,
}

#[derive(Debug, Clone)]
pub struct PackageSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub target: PackageTarget,
    pub publisher: String,
    pub description: String,
    pub license: String,
}

impl PackageSummary {
    fn from_manifests(install: &InstallToml, metadata: &MetadataToml) -> Self {
        Self {
            id: install.package.id.clone(),
            name: install.package.name.clone(),
            version: install.package.version.clone(),
            target: install.package.target,
            publisher: metadata.metadata.publisher.clone(),
            description: metadata.metadata.description.clone(),
            license: metadata.metadata.license.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallRoots {
    pub samples: PathBuf,
    pub presets: PathBuf,
    pub extentions: PathBuf,
}

impl InstallRoots {
    pub fn default_user() -> Result<Self> {
        let documents = dirs::document_dir()
            .ok_or_else(|| ApakError::InvalidPackage("could not resolve Documents".to_string()))?;
        let config = dirs::config_dir()
            .ok_or_else(|| ApakError::InvalidPackage("could not resolve AppData".to_string()))?;

        let library = documents.join("Futureboard Studio").join("Library");
        Ok(Self {
            samples: library.join("Samples"),
            presets: library.join("Presets"),
            extentions: config.join("Futureboard Studio").join("Extentions"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct PackOptions {
    pub source_dir: PathBuf,
    pub output_path: PathBuf,
    pub secret_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub package_path: PathBuf,
    pub secret_file: PathBuf,
    pub roots: InstallRoots,
}

#[derive(Debug, Clone)]
pub struct PackReport {
    pub summary: PackageSummary,
    pub output_path: PathBuf,
    pub asset_count: usize,
    pub byte_len: u64,
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub summary: PackageSummary,
    pub installed_files: Vec<PathBuf>,
}

pub fn default_secret_file() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".apak.secret")
}

pub fn pack_template(options: PackOptions) -> Result<PackReport> {
    let install_path = options.source_dir.join("install.toml");
    let metadata_path = options.source_dir.join("metadata.toml");
    let assets_dir = options.source_dir.join("assets");

    if !install_path.is_file() {
        return Err(ApakError::InvalidTemplate(
            "missing install.toml".to_string(),
        ));
    }
    if !metadata_path.is_file() {
        return Err(ApakError::InvalidTemplate(
            "missing metadata.toml".to_string(),
        ));
    }
    if !assets_dir.is_dir() {
        return Err(ApakError::InvalidTemplate(
            "missing assets directory".to_string(),
        ));
    }

    let install = read_install_toml(&install_path)?;
    let metadata = read_metadata_toml(&metadata_path)?;
    validate_install(&install)?;
    let summary = PackageSummary::from_manifests(&install, &metadata);

    let (tar_bytes, asset_count) = build_tar_payload(
        &options.source_dir,
        &install_path,
        &metadata_path,
        &assets_dir,
    )?;
    if asset_count == 0 {
        return Err(ApakError::InvalidTemplate(
            "assets directory contains no files".to_string(),
        ));
    }

    let compressed = lzma_compress(&tar_bytes)?;
    let secret = read_secret_file(&options.secret_file)?;
    let package_bytes = encrypt_payload(&compressed, &secret)?;

    if let Some(parent) = options.output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.output_path, package_bytes)?;
    let byte_len = fs::metadata(&options.output_path)?.len();

    Ok(PackReport {
        summary,
        output_path: options.output_path,
        asset_count,
        byte_len,
    })
}

pub fn read_package_info(package_path: &Path, secret_file: &Path) -> Result<PackageSummary> {
    let payload = decrypt_package(package_path, secret_file)?;
    let (install, metadata) = read_manifests_from_tar(&payload)?;
    validate_install(&install)?;
    Ok(PackageSummary::from_manifests(&install, &metadata))
}

pub fn install_package(options: InstallOptions) -> Result<InstallReport> {
    let payload = decrypt_package(&options.package_path, &options.secret_file)?;
    let (install, metadata) = read_manifests_from_tar(&payload)?;
    validate_install(&install)?;
    let summary = PackageSummary::from_manifests(&install, &metadata);

    let mut archive = tar::Archive::new(Cursor::new(payload));
    let mut installed_files = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(asset_rel) = asset_relative_path(&path)? else {
            continue;
        };

        if entry.header().entry_type().is_dir() {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(ApakError::Archive(format!(
                "unsupported archive entry type for {}",
                path.display()
            )));
        }

        let target = resolve_install_target(&install, &options.roots, &asset_rel)?;
        if target.exists() && !install.install.overwrite {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&target)?;
        installed_files.push(target);
    }

    Ok(InstallReport {
        summary,
        installed_files,
    })
}

pub fn write_template(destination: &Path) -> Result<()> {
    fs::create_dir_all(destination.join("assets"))?;
    write_if_missing(destination.join("install.toml"), TEMPLATE_INSTALL_TOML)?;
    write_if_missing(destination.join("metadata.toml"), TEMPLATE_METADATA_TOML)?;
    write_if_missing(
        destination.join("assets").join("README.md"),
        TEMPLATE_ASSETS_README,
    )?;
    Ok(())
}

fn write_if_missing(path: PathBuf, contents: &str) -> Result<()> {
    if !path.exists() {
        fs::write(path, contents)?;
    }
    Ok(())
}

fn read_install_toml(path: &Path) -> Result<InstallToml> {
    Ok(toml::from_str(&fs::read_to_string(path)?)?)
}

fn read_metadata_toml(path: &Path) -> Result<MetadataToml> {
    Ok(toml::from_str(&fs::read_to_string(path)?)?)
}

fn validate_install(install: &InstallToml) -> Result<()> {
    let fields = [
        ("package.id", install.package.id.as_str()),
        ("package.name", install.package.name.as_str()),
        ("package.version", install.package.version.as_str()),
    ];
    for (field, value) in fields {
        if value.trim().is_empty() {
            return Err(ApakError::InvalidTemplate(format!("{field} is empty")));
        }
    }
    Ok(())
}

fn build_tar_payload(
    source_dir: &Path,
    install_path: &Path,
    metadata_path: &Path,
    assets_dir: &Path,
) -> Result<(Vec<u8>, usize)> {
    let mut out = Vec::new();
    let mut builder = tar::Builder::new(&mut out);
    builder.append_path_with_name(install_path, "install.toml")?;
    builder.append_path_with_name(metadata_path, "metadata.toml")?;

    let mut files = Vec::new();
    for entry in WalkDir::new(assets_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            files.push(path.to_path_buf());
        }
    }
    files.sort();

    let mut count = 0usize;
    for path in files {
        let rel = path.strip_prefix(source_dir).map_err(|error| {
            ApakError::InvalidTemplate(format!("could not relativize asset path: {error}"))
        })?;
        let rel = normalize_archive_path(rel)?;
        builder.append_path_with_name(&path, rel)?;
        count += 1;
    }
    builder.finish()?;
    drop(builder);
    Ok((out, count))
}

fn normalize_archive_path(path: &Path) -> Result<PathBuf> {
    let safe = safe_components(path)?;
    Ok(safe.iter().collect())
}

fn safe_components(path: &Path) -> Result<Vec<PathBuf>> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(PathBuf::from(part)),
            Component::CurDir => {}
            _ => {
                return Err(ApakError::InvalidPackage(format!(
                    "unsafe path {}",
                    path.display()
                )));
            }
        }
    }
    if parts.is_empty() {
        return Err(ApakError::InvalidPackage("empty archive path".to_string()));
    }
    Ok(parts)
}

fn asset_relative_path(path: &Path) -> Result<Option<PathBuf>> {
    let parts = safe_components(path)?;
    let Some(first) = parts.first() else {
        return Ok(None);
    };
    if first.as_os_str() != "assets" {
        return Ok(None);
    }
    if parts.len() == 1 {
        return Ok(None);
    }
    Ok(Some(parts[1..].iter().collect()))
}

fn resolve_install_target(
    install: &InstallToml,
    roots: &InstallRoots,
    asset_rel: &Path,
) -> Result<PathBuf> {
    let parts = safe_components(asset_rel)?;
    let rel: PathBuf = parts.iter().collect();
    match install.package.target {
        PackageTarget::Sample => Ok(roots.samples.join(rel)),
        PackageTarget::Preset => Ok(roots.presets.join(rel)),
        PackageTarget::Extentions => {
            let first = parts
                .first()
                .and_then(|part| part.to_str())
                .unwrap_or_default();
            match first {
                "Themes" | "Plugins" | "Services" => Ok(roots.extentions.join(rel)),
                _ => Err(ApakError::UnsupportedTarget(format!(
                    "Extentions assets must start with Themes, Plugins, or Services: {}",
                    asset_rel.display()
                ))),
            }
        }
    }
}

fn decrypt_package(package_path: &Path, secret_file: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(package_path)?;
    let compressed = decrypt_payload(&bytes, &read_secret_file(secret_file)?)?;
    lzma_decompress(&compressed)
}

fn encrypt_payload(payload: &[u8], secret: &[u8]) -> Result<Vec<u8>> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let key = derive_key(secret, &salt);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|error| ApakError::Crypto(error.to_string()))?;
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&nonce), payload)
        .map_err(|error| ApakError::Crypto(error.to_string()))?;

    let mut out = Vec::with_capacity(MAGIC.len() + 1 + SALT_LEN + NONCE_LEN + encrypted.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&encrypted);
    Ok(out)
}

fn decrypt_payload(package: &[u8], secret: &[u8]) -> Result<Vec<u8>> {
    let header_len = MAGIC.len() + 1 + SALT_LEN + NONCE_LEN;
    if package.len() <= header_len {
        return Err(ApakError::InvalidPackage("file is too small".to_string()));
    }
    if &package[..MAGIC.len()] != MAGIC {
        return Err(ApakError::InvalidPackage("bad magic".to_string()));
    }
    let version = package[MAGIC.len()];
    if version != FORMAT_VERSION {
        return Err(ApakError::InvalidPackage(format!(
            "unsupported version {version}"
        )));
    }

    let salt_start = MAGIC.len() + 1;
    let nonce_start = salt_start + SALT_LEN;
    let cipher_start = nonce_start + NONCE_LEN;
    let salt = &package[salt_start..nonce_start];
    let nonce = &package[nonce_start..cipher_start];
    let ciphertext = &package[cipher_start..];

    let key = derive_key(secret, salt);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|error| ApakError::Crypto(error.to_string()))?;
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| ApakError::Crypto("could not decrypt package".to_string()))
}

fn derive_key(secret: &[u8], salt: &[u8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(secret, salt, KDF_ROUNDS, &mut key);
    key
}

fn read_secret_file(path: &Path) -> Result<Vec<u8>> {
    let contents = fs::read_to_string(path).map_err(|error| {
        ApakError::Crypto(format!(
            "could not read secret file {}: {error}",
            path.display()
        ))
    })?;
    let value = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .next()
        .ok_or_else(|| ApakError::Crypto("secret file is empty".to_string()))?;
    let value = value
        .split_once('=')
        .map(|(_, right)| right.trim())
        .unwrap_or(value);
    decode_secret_value(value)
}

fn decode_secret_value(value: &str) -> Result<Vec<u8>> {
    if let Some(bytes) = decode_hex_32(value)? {
        return Ok(bytes);
    }
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(value) {
        if bytes.len() == KEY_LEN {
            return Ok(bytes);
        }
    }
    Ok(value.as_bytes().to_vec())
}

fn decode_hex_32(value: &str) -> Result<Option<Vec<u8>>> {
    let cleaned: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() != KEY_LEN * 2 {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(KEY_LEN);
    let bytes = cleaned.as_bytes();
    for pair in bytes.chunks_exact(2) {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(Some(out))
}

fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ApakError::Crypto(
            "secret hex value contains non-hex characters".to_string(),
        )),
    }
}

fn lzma_compress(input: &[u8]) -> Result<Vec<u8>> {
    let mut reader = Cursor::new(input);
    let mut out = Vec::new();
    lzma_rs::lzma_compress(&mut reader, &mut out)
        .map_err(|error| ApakError::Compression(error.to_string()))?;
    Ok(out)
}

fn lzma_decompress(input: &[u8]) -> Result<Vec<u8>> {
    let mut reader = Cursor::new(input);
    let mut out = Vec::new();
    lzma_rs::lzma_decompress(&mut reader, &mut out)
        .map_err(|error| ApakError::Compression(error.to_string()))?;
    Ok(out)
}

fn read_manifests_from_tar(payload: &[u8]) -> Result<(InstallToml, MetadataToml)> {
    let mut archive = tar::Archive::new(Cursor::new(payload));
    let mut install = None;
    let mut metadata = None;

    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?.into_owned();
        let mut text = String::new();
        match path.to_string_lossy().as_ref() {
            "install.toml" => {
                entry.read_to_string(&mut text)?;
                install = Some(toml::from_str(&text)?);
            }
            "metadata.toml" => {
                entry.read_to_string(&mut text)?;
                metadata = Some(toml::from_str(&text)?);
            }
            _ => {}
        }
    }

    let install =
        install.ok_or_else(|| ApakError::InvalidPackage("missing install.toml".to_string()))?;
    let metadata =
        metadata.ok_or_else(|| ApakError::InvalidPackage("missing metadata.toml".to_string()))?;
    Ok((install, metadata))
}

pub const TEMPLATE_INSTALL_TOML: &str = r#"[package]
id = "publisher.package-id"
name = "Package Name"
version = "0.1.0"
type = "Sample"

[install]
overwrite = true
"#;

pub const TEMPLATE_METADATA_TOML: &str = r#"[metadata]
publisher = "Publisher"
description = "Describe the package contents."
license = "Proprietary"
"#;

pub const TEMPLATE_ASSETS_README: &str = r#"# Apak Assets

Place package files here before running `makeapak`.

Targets:
- Sample: files install into Documents/Futureboard Studio/Library/Samples
- Preset: files install into Documents/Futureboard Studio/Library/Presets
- Extentions: files must start with Themes, Plugins, or Services
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_package_roundtrips_to_sample_library() {
        let temp = tempfile::tempdir().expect("tempdir");
        let template = temp.path().join("template");
        fs::create_dir_all(template.join("assets/Drums")).expect("assets");
        fs::write(
            template.join("install.toml"),
            r#"[package]
id = "futureboard.test-samples"
name = "Test Samples"
version = "0.1.0"
type = "Sample"

[install]
overwrite = true
"#,
        )
        .expect("install");
        fs::write(
            template.join("metadata.toml"),
            r#"[metadata]
publisher = "Futureboard"
description = "Roundtrip test"
license = "MIT"
"#,
        )
        .expect("metadata");
        fs::write(template.join("assets/Drums/kick.txt"), "kick").expect("asset");
        fs::write(temp.path().join(".apak.secret"), "APAK_SECRET=test-secret").expect("secret");

        let package_path = temp.path().join("test.apak");
        let report = pack_template(PackOptions {
            source_dir: template,
            output_path: package_path.clone(),
            secret_file: temp.path().join(".apak.secret"),
        })
        .expect("pack");
        assert_eq!(report.asset_count, 1);

        let roots = InstallRoots {
            samples: temp.path().join("Samples"),
            presets: temp.path().join("Presets"),
            extentions: temp.path().join("Extentions"),
        };
        let install = install_package(InstallOptions {
            package_path,
            secret_file: temp.path().join(".apak.secret"),
            roots,
        })
        .expect("install");

        assert_eq!(install.installed_files.len(), 1);
        assert_eq!(
            fs::read_to_string(temp.path().join("Samples/Drums/kick.txt")).expect("installed"),
            "kick"
        );
    }

    #[test]
    fn extentions_rejects_unknown_top_level_asset() {
        let install = InstallToml {
            package: PackageSection {
                id: "x".to_string(),
                name: "x".to_string(),
                version: "0.1.0".to_string(),
                target: PackageTarget::Extentions,
            },
            install: InstallSection::default(),
        };
        let roots = InstallRoots {
            samples: PathBuf::from("Samples"),
            presets: PathBuf::from("Presets"),
            extentions: PathBuf::from("Extentions"),
        };
        let error = resolve_install_target(&install, &roots, Path::new("Other/file.txt"))
            .expect_err("unknown root should fail");
        assert!(matches!(error, ApakError::UnsupportedTarget(_)));
    }
}
