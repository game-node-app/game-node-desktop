//! Bounded, path-safe extraction for managed plugin packages.
//!
//! Package validation remains in the host. This module accepts the manifest's
//! allowlist and extracts only the files selected for the current target.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek},
    path::{Component, Path, PathBuf},
};

use sha2::Digest;
use thiserror::Error;
use zip::ZipArchive;

pub const MAX_COMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 4096;
pub const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;
/// ASCII domain label followed by one NUL byte; signed before raw integrity.json bytes.
pub const SIGNATURE_DOMAIN_PREFIX: &[u8] = b"GameNode/.gnplugin/integrity/v1\0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IntegrityError {
    #[error("integrity metadata is malformed or exceeds the size limit")]
    InvalidMetadata,
    #[error("integrity file does not exactly match the package payload")]
    PayloadSetMismatch,
    #[error("integrity record contains an invalid SHA-256 digest")]
    InvalidDigest,
    #[error("a package payload hash does not match")]
    HashMismatch,
    #[error("publisher key or signature has an invalid length")]
    InvalidKeyOrSignature,
    #[error("Ed25519 integrity signature is invalid")]
    InvalidSignature,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrityManifest {
    pub schema: u32,
    pub files: UniqueFiles,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UniqueFiles(pub BTreeMap<String, String>);

impl<'de> serde::Deserialize<'de> for UniqueFiles {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct FilesVisitor;
        impl<'de> serde::de::Visitor<'de> for FilesVisitor {
            type Value = UniqueFiles;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an object mapping payload paths to SHA-256 digests")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut files = BTreeMap::new();
                while let Some((path, digest)) = map.next_entry::<String, String>()? {
                    if files.insert(path.clone(), digest).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate integrity path `{path}`"
                        )));
                    }
                }
                Ok(UniqueFiles(files))
            }
        }
        deserializer.deserialize_map(FilesVisitor)
    }
}

/// Parse the bounded integrity record after verifying its detached signature.
pub fn parse_integrity_json(raw: &[u8]) -> Result<IntegrityManifest, IntegrityError> {
    if raw.is_empty() || raw.len() as u64 > MAX_METADATA_BYTES {
        return Err(IntegrityError::InvalidMetadata);
    }
    let manifest: IntegrityManifest =
        serde_json::from_slice(raw).map_err(|_| IntegrityError::InvalidMetadata)?;
    if manifest.schema != 1 || manifest.files.0.is_empty() {
        return Err(IntegrityError::InvalidMetadata);
    }
    for (path, digest) in &manifest.files.0 {
        if validated_relative_path(path).is_err()
            || path.ends_with('/')
            || path.ends_with('\\')
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(IntegrityError::InvalidMetadata);
        }
    }
    Ok(manifest)
}

/// Verify that every supplied payload is listed exactly once and matches its
/// lowercase SHA-256 digest.
pub fn verify_payload_hashes(
    declared: &BTreeMap<String, String>,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Result<(), IntegrityError> {
    if declared.len() != payloads.len()
        || declared.keys().ne(payloads.keys())
        || declared.keys().any(|path| {
            validated_relative_path(path).is_err() || path.ends_with('/') || path.ends_with('\\')
        })
    {
        return Err(IntegrityError::PayloadSetMismatch);
    }
    for (path, bytes) in payloads {
        let expected = declared
            .get(path)
            .ok_or(IntegrityError::PayloadSetMismatch)?;
        if expected.len() != 64
            || !expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(IntegrityError::InvalidDigest);
        }
        let digest = sha2::Sha256::digest(bytes);
        if format!("{digest:x}") != *expected {
            return Err(IntegrityError::HashMismatch);
        }
    }
    Ok(())
}

/// Re-hash the complete package payload from its immutable version directory.
/// This is intended for worker-side checks immediately before process launch.
pub fn verify_payload_files(
    version_root: &Path,
    declared: &BTreeMap<String, String>,
) -> Result<(), IntegrityError> {
    if declared.is_empty() {
        return Err(IntegrityError::PayloadSetMismatch);
    }
    let root = fs::canonicalize(version_root).map_err(|_| IntegrityError::PayloadSetMismatch)?;
    for (relative, expected) in declared {
        if validated_relative_path(relative).is_err()
            || relative.ends_with('/')
            || relative.ends_with('\\')
        {
            return Err(IntegrityError::PayloadSetMismatch);
        }
        if expected.len() != 64
            || !expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(IntegrityError::InvalidDigest);
        }
        let path = root.join(relative);
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| IntegrityError::PayloadSetMismatch)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(IntegrityError::PayloadSetMismatch);
        }
        let canonical = fs::canonicalize(&path).map_err(|_| IntegrityError::PayloadSetMismatch)?;
        if !canonical.starts_with(&root) {
            return Err(IntegrityError::PayloadSetMismatch);
        }
        let mut file = File::open(canonical).map_err(|_| IntegrityError::PayloadSetMismatch)?;
        let mut hasher = sha2::Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| IntegrityError::PayloadSetMismatch)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        if format!("{:x}", hasher.finalize()) != *expected {
            return Err(IntegrityError::HashMismatch);
        }
    }
    let mut actual_files = BTreeSet::new();
    collect_payload_files(&root, &root, &mut actual_files)?;
    if actual_files.len() != declared.len() || actual_files.iter().ne(declared.keys()) {
        return Err(IntegrityError::PayloadSetMismatch);
    }
    Ok(())
}

/// Validate the signed package directory produced by the bounded extractor. The detached
/// signature and integrity record are package metadata; every other regular file must be
/// declared and hashed by `integrity.json`.
pub fn verify_package_directory(
    package_root: &Path,
    publisher_key: &[u8],
) -> Result<IntegrityManifest, IntegrityError> {
    let integrity_path = package_root.join("integrity.json");
    let signature_path = package_root.join("signature.ed25519");
    let integrity_meta =
        fs::symlink_metadata(&integrity_path).map_err(|_| IntegrityError::InvalidMetadata)?;
    let signature_meta =
        fs::symlink_metadata(&signature_path).map_err(|_| IntegrityError::InvalidMetadata)?;
    if !integrity_meta.is_file()
        || integrity_meta.file_type().is_symlink()
        || !signature_meta.is_file()
        || signature_meta.file_type().is_symlink()
    {
        return Err(IntegrityError::InvalidMetadata);
    }
    let raw = read_bounded(&integrity_path, MAX_METADATA_BYTES as usize)
        .map_err(|_| IntegrityError::InvalidMetadata)?;
    let signature =
        read_bounded(&signature_path, 64).map_err(|_| IntegrityError::InvalidKeyOrSignature)?;
    verify_integrity_signature(publisher_key, &raw, &signature)?;
    let manifest = parse_integrity_json(&raw)?;
    verify_payload_directory_with_metadata(package_root, &manifest.files.0)?;
    Ok(manifest)
}

fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds limit",
        ));
    }
    Ok(bytes)
}

/// Revalidate an installed package immediately before launch using its pinned publisher key.
pub fn verify_installed_package(
    package_root: &Path,
    publisher_key: &[u8],
) -> Result<(), IntegrityError> {
    verify_package_directory(package_root, publisher_key).map(|_| ())
}

fn verify_payload_directory_with_metadata(
    root: &Path,
    declared: &BTreeMap<String, String>,
) -> Result<(), IntegrityError> {
    let mut payload_files = BTreeMap::new();
    for (path, digest) in declared {
        payload_files.insert(path.clone(), digest.clone());
    }
    let canonical_root = fs::canonicalize(root).map_err(|_| IntegrityError::PayloadSetMismatch)?;
    let mut actual = BTreeSet::new();
    collect_payload_files(&canonical_root, &canonical_root, &mut actual)?;
    actual.remove("integrity.json");
    actual.remove("signature.ed25519");
    if actual.len() != declared.len() || actual.iter().ne(declared.keys()) {
        return Err(IntegrityError::PayloadSetMismatch);
    }
    for (relative, expected) in &payload_files {
        if validated_relative_path(relative).is_err()
            || relative.ends_with('/')
            || relative.ends_with('\\')
            || expected.len() != 64
            || !expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(IntegrityError::InvalidMetadata);
        }
        let path = canonical_root.join(relative);
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| IntegrityError::PayloadSetMismatch)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(IntegrityError::PayloadSetMismatch);
        }
        let mut file = File::open(path).map_err(|_| IntegrityError::PayloadSetMismatch)?;
        let mut hasher = sha2::Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| IntegrityError::PayloadSetMismatch)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        if format!("{:x}", hasher.finalize()) != *expected {
            return Err(IntegrityError::HashMismatch);
        }
    }
    Ok(())
}

fn collect_payload_files(
    root: &Path,
    current: &Path,
    files: &mut BTreeSet<String>,
) -> Result<(), IntegrityError> {
    for entry in fs::read_dir(current).map_err(|_| IntegrityError::PayloadSetMismatch)? {
        let entry = entry.map_err(|_| IntegrityError::PayloadSetMismatch)?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| IntegrityError::PayloadSetMismatch)?;
        if metadata.file_type().is_symlink() {
            return Err(IntegrityError::PayloadSetMismatch);
        }
        if metadata.is_dir() {
            collect_payload_files(root, &path, files)?;
        } else if metadata.is_file() {
            let canonical =
                fs::canonicalize(&path).map_err(|_| IntegrityError::PayloadSetMismatch)?;
            if !canonical.starts_with(root) {
                return Err(IntegrityError::PayloadSetMismatch);
            }
            let relative = canonical
                .strip_prefix(root)
                .map_err(|_| IntegrityError::PayloadSetMismatch)?
                .to_string_lossy()
                .replace('\\', "/");
            if !files.insert(relative) {
                return Err(IntegrityError::PayloadSetMismatch);
            }
        } else {
            return Err(IntegrityError::PayloadSetMismatch);
        }
    }
    Ok(())
}

/// Verify Ed25519 over the fixed domain prefix plus raw integrity JSON bytes,
/// without parsing or re-serializing the signed JSON.
pub fn verify_integrity_signature(
    public_key: &[u8],
    raw_integrity_json: &[u8],
    signature: &[u8],
) -> Result<(), IntegrityError> {
    if public_key.len() != 32 || signature.len() != 64 {
        return Err(IntegrityError::InvalidKeyOrSignature);
    }
    let mut signed = Vec::with_capacity(SIGNATURE_DOMAIN_PREFIX.len() + raw_integrity_json.len());
    signed.extend_from_slice(SIGNATURE_DOMAIN_PREFIX);
    signed.extend_from_slice(raw_integrity_json);
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
        .verify(&signed, signature)
        .map_err(|_| IntegrityError::InvalidSignature)
}

#[derive(Debug, Error)]
pub enum PackageArchiveError {
    #[error("package archive exceeds the compressed size limit")]
    CompressedLimit,
    #[error("package archive has too many entries")]
    EntryLimit,
    #[error("package archive expands beyond the size limit")]
    ExpandedLimit,
    #[error("package metadata exceeds the size limit")]
    MetadataLimit,
    #[error("package archive contains an invalid path")]
    InvalidPath,
    #[error("package archive contains duplicate or case-colliding paths")]
    PathCollision,
    #[error("package archive contains a link or special file")]
    SpecialFile,
    #[error("package archive contains an unlisted payload")]
    UnlistedPayload,
    #[error("selected target payload is missing")]
    MissingTargetPayload,
    #[error("staging directory is not empty or is unsafe")]
    InvalidStagingDirectory,
    #[error("invalid ZIP package")]
    InvalidZip(#[source] zip::result::ZipError),
    #[error("failed to read or write package staging files")]
    Io(#[from] io::Error),
}

/// Extract one selected target into a newly-created, empty staging directory.
///
/// `expected_files` is the validated package payload allowlist after selecting
/// the current target. `target_files` must contain at least one executable or
/// other target-specific payload and is checked independently so an archive
/// for a different architecture cannot be staged by accident.
pub fn extract_selected_target(
    archive_path: &Path,
    staging_dir: &Path,
    expected_files: &BTreeSet<String>,
    metadata_files: &BTreeSet<String>,
    target_files: &BTreeSet<String>,
) -> Result<(), PackageArchiveError> {
    if expected_files.is_empty()
        || target_files.is_empty()
        || !target_files.is_subset(expected_files)
    {
        return Err(PackageArchiveError::MissingTargetPayload);
    }
    let file = File::open(archive_path)?;
    if file.metadata()?.len() > MAX_COMPRESSED_BYTES {
        return Err(PackageArchiveError::CompressedLimit);
    }
    let central_attributes = read_central_attributes(archive_path)?;
    let archive = ZipArchive::new(file).map_err(PackageArchiveError::InvalidZip)?;
    match fs::symlink_metadata(staging_dir) {
        Ok(_) => return Err(PackageArchiveError::InvalidStagingDirectory),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir(staging_dir)?;
    if let Err(error) = restrict_directory(staging_dir) {
        let _ = fs::remove_dir_all(staging_dir);
        return Err(error.into());
    }

    let result = extract_archive(
        archive,
        &central_attributes,
        staging_dir,
        expected_files,
        metadata_files,
        target_files,
    );
    if result.is_err() {
        let _ = fs::remove_dir_all(staging_dir);
    }
    result
}

fn extract_archive<R: Read + Seek>(
    mut archive: ZipArchive<R>,
    central_attributes: &[(u8, u32)],
    staging_dir: &Path,
    expected_files: &BTreeSet<String>,
    metadata_files: &BTreeSet<String>,
    target_files: &BTreeSet<String>,
) -> Result<(), PackageArchiveError> {
    if expected_files.is_empty()
        || target_files.is_empty()
        || !target_files.is_subset(expected_files)
    {
        return Err(PackageArchiveError::MissingTargetPayload);
    }

    let staging_root = fs::canonicalize(staging_dir)?;
    let mut seen = BTreeMap::<String, String>::new();
    let mut found = BTreeSet::new();
    let mut expanded_total = 0u64;
    let mut metadata_total = 0u64;

    if archive.len() > MAX_ENTRIES {
        return Err(PackageArchiveError::EntryLimit);
    }
    if central_attributes.len() != archive.len() {
        return Err(PackageArchiveError::InvalidStagingDirectory);
    }
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(PackageArchiveError::InvalidZip)?;
        let raw_name = entry.name().to_owned();
        let (relative, is_directory) = validated_relative_path(&raw_name)?;
        let normalized = relative.to_string_lossy().replace('\\', "/");
        let folded = normalized.to_lowercase();
        if let Some(previous) = seen.insert(folded, normalized.clone()) {
            if previous != normalized {
                return Err(PackageArchiveError::PathCollision);
            }
            return Err(PackageArchiveError::PathCollision);
        }

        let (creator_system, external_attributes) = central_attributes[index];
        if entry.is_symlink()
            || has_special_unix_type(entry.unix_mode(), is_directory)
            || (creator_system == 0 && external_attributes as u16 & 0x0440 != 0)
        {
            return Err(PackageArchiveError::SpecialFile);
        }
        if is_directory {
            if !expected_files
                .iter()
                .any(|p| p.starts_with(&format!("{normalized}/")))
            {
                return Err(PackageArchiveError::UnlistedPayload);
            }
            let destination = staging_root.join(&relative);
            ensure_contained(&staging_root, &destination)?;
            fs::create_dir_all(destination)?;
            continue;
        }

        if !expected_files.contains(&normalized) {
            return Err(PackageArchiveError::UnlistedPayload);
        }
        found.insert(normalized.clone());
        expanded_total = expanded_total
            .checked_add(entry.size())
            .ok_or(PackageArchiveError::ExpandedLimit)?;
        if expanded_total > MAX_EXPANDED_BYTES {
            return Err(PackageArchiveError::ExpandedLimit);
        }
        if metadata_files.contains(&normalized) {
            metadata_total = metadata_total
                .checked_add(entry.size())
                .ok_or(PackageArchiveError::MetadataLimit)?;
            if metadata_total > MAX_METADATA_BYTES {
                return Err(PackageArchiveError::MetadataLimit);
            }
        }

        let destination = staging_root.join(&relative);
        ensure_contained(&staging_root, &destination)?;
        let parent = destination
            .parent()
            .ok_or(PackageArchiveError::InvalidPath)?;
        fs::create_dir_all(parent)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        let declared_size = entry.size();
        let copied = {
            let mut bounded_entry = (&mut entry).take(declared_size);
            io::copy(&mut bounded_entry, &mut output)?
        };
        let mut extra = [0u8; 1];
        if copied != declared_size || entry.read(&mut extra)? != 0 {
            return Err(PackageArchiveError::InvalidZip(zip::result::ZipError::Io(
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "ZIP entry length does not match its metadata",
                ),
            )));
        }
        output.sync_all()?;
    }

    if !target_files.is_subset(&found) || !expected_files.is_subset(&found) {
        return Err(PackageArchiveError::MissingTargetPayload);
    }
    Ok(())
}

/// Read central-directory creator-system and external attributes. The ZIP crate
/// normalizes DOS attributes, so inspect the raw record to reject Windows
/// device and reparse-point flags before extraction.
fn read_central_attributes(path: &Path) -> Result<Vec<(u8, u32)>, PackageArchiveError> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let tail_len = length.min(65_557) as usize;
    let mut tail = vec![0; tail_len];
    file.seek(io::SeekFrom::End(-(tail_len as i64)))?;
    file.read_exact(&mut tail)?;
    let eocd = tail
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .ok_or(PackageArchiveError::InvalidStagingDirectory)?;
    if eocd + 22 > tail.len() {
        return Err(PackageArchiveError::InvalidStagingDirectory);
    }
    let disk = u16::from_le_bytes([tail[eocd + 4], tail[eocd + 5]]);
    let central_disk = u16::from_le_bytes([tail[eocd + 6], tail[eocd + 7]]);
    let entries = u16::from_le_bytes([tail[eocd + 10], tail[eocd + 11]]);
    let central_size = u32::from_le_bytes(tail[eocd + 12..eocd + 16].try_into().unwrap());
    let central_offset = u32::from_le_bytes(tail[eocd + 16..eocd + 20].try_into().unwrap());
    if disk != 0
        || central_disk != 0
        || entries == u16::MAX
        || central_size == u32::MAX
        || central_offset == u32::MAX
        || entries as usize > MAX_ENTRIES
        || central_offset as u64 + central_size as u64 > length
    {
        return Err(PackageArchiveError::InvalidStagingDirectory);
    }

    file.seek(io::SeekFrom::Start(central_offset as u64))?;
    let mut attributes = Vec::with_capacity(entries as usize);
    for _ in 0..entries {
        let mut fixed = [0u8; 46];
        file.read_exact(&mut fixed)?;
        if &fixed[..4] != b"PK\x01\x02" {
            return Err(PackageArchiveError::InvalidStagingDirectory);
        }
        let name_len = u16::from_le_bytes([fixed[28], fixed[29]]) as u64;
        let extra_len = u16::from_le_bytes([fixed[30], fixed[31]]) as u64;
        let comment_len = u16::from_le_bytes([fixed[32], fixed[33]]) as u64;
        let skip = name_len + extra_len + comment_len;
        if file.stream_position()?.saturating_add(skip) > length {
            return Err(PackageArchiveError::InvalidStagingDirectory);
        }
        file.seek(io::SeekFrom::Current(skip as i64))?;
        attributes.push((
            fixed[5],
            u32::from_le_bytes(fixed[38..42].try_into().unwrap()),
        ));
    }
    Ok(attributes)
}

fn validated_relative_path(name: &str) -> Result<(PathBuf, bool), PackageArchiveError> {
    if name.is_empty() || name.contains('\0') || name.starts_with('/') || name.starts_with('\\') {
        return Err(PackageArchiveError::InvalidPath);
    }
    let directory = name.ends_with('/') || name.ends_with('\\');
    let normalized = name.replace('\\', "/");
    let path_text = if directory {
        normalized.strip_suffix('/').unwrap_or(&normalized)
    } else {
        normalized.as_str()
    };
    if normalized.starts_with("//")
        || normalized.as_bytes().get(1) == Some(&b':')
        || path_text.split('/').any(str::is_empty)
    {
        return Err(PackageArchiveError::InvalidPath);
    }
    let path = Path::new(path_text);
    if path.components().any(|component| {
        !matches!(component, Component::Normal(_))
            || component.as_os_str().to_string_lossy().contains(':')
    }) {
        return Err(PackageArchiveError::InvalidPath);
    }
    Ok((path.to_path_buf(), directory))
}

fn ensure_contained(root: &Path, destination: &Path) -> Result<(), PackageArchiveError> {
    if !destination.starts_with(root) {
        return Err(PackageArchiveError::InvalidPath);
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn has_special_unix_type(mode: Option<u32>, is_directory: bool) -> bool {
    let Some(mode) = mode else { return false };
    let kind = mode & 0o170000;
    if kind == 0 {
        return false;
    }
    if is_directory {
        kind != 0o040000
    } else {
        kind != 0o100000
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs::File, io::Write};

    use tempfile::tempdir;
    use zip::{write::SimpleFileOptions, ZipWriter};

    use super::*;

    fn archive(entries: &[(&str, &[u8])], symlink: Option<&str>) -> (tempfile::TempDir, PathBuf) {
        let temp = tempdir().unwrap();
        let path = temp.path().join("package.zip");
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        for (name, contents) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(contents).unwrap();
        }
        if let Some(name) = symlink {
            let opts = SimpleFileOptions::default().unix_permissions(0o120777);
            zip.start_file(name, opts).unwrap();
            zip.write_all(b"../../outside").unwrap();
        }
        zip.finish().unwrap();
        if let Some(name) = symlink {
            // The high-level writer masks the POSIX type bits. Encode a real
            // Unix symlink marker in the central-directory fixture metadata.
            let mut bytes = fs::read(&path).unwrap();
            for offset in 0..bytes.len().saturating_sub(46) {
                if &bytes[offset..offset + 4] == b"PK\x01\x02" {
                    let name_len =
                        u16::from_le_bytes([bytes[offset + 28], bytes[offset + 29]]) as usize;
                    let entry_name = &bytes[offset + 46..offset + 46 + name_len];
                    if entry_name == name.as_bytes() {
                        bytes[offset + 4] = 20;
                        bytes[offset + 5] = 3;
                        bytes[offset + 38..offset + 42]
                            .copy_from_slice(&((0o120777u32) << 16).to_le_bytes());
                    }
                }
            }
            fs::write(&path, bytes).unwrap();
        }
        (temp, path)
    }

    fn set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extracts_only_allowlisted_selected_target_files() {
        let (temp, zip) = archive(
            &[("plugin.json", b"{}"), ("bin/linux-x64/plugin", b"bin")],
            None,
        );
        let staging = temp.path().join("stage");
        extract_selected_target(
            &zip,
            &staging,
            &set(&["plugin.json", "bin/linux-x64/plugin"]),
            &set(&["plugin.json"]),
            &set(&["bin/linux-x64/plugin"]),
        )
        .unwrap();
        assert_eq!(fs::read(staging.join("plugin.json")).unwrap(), b"{}");
    }

    #[test]
    fn rejects_traversal_links_unlisted_files_and_case_collisions() {
        for (entries, link, expected, target) in [
            (
                vec![("../escape", b"x".as_slice())],
                None,
                set(&["../escape"]),
                set(&["../escape"]),
            ),
            (
                vec![("plugin.json", b"{}".as_slice())],
                Some("link"),
                set(&["plugin.json", "link"]),
                set(&["plugin.json"]),
            ),
            (
                vec![
                    ("plugin.json", b"{}".as_slice()),
                    ("extra", b"x".as_slice()),
                ],
                None,
                set(&["plugin.json"]),
                set(&["plugin.json"]),
            ),
            (
                vec![("Foo", b"x".as_slice()), ("foo", b"y".as_slice())],
                None,
                set(&["Foo", "foo"]),
                set(&["Foo"]),
            ),
        ] {
            let (temp, zip) = archive(&entries, link);
            let staging = temp.path().join("stage");
            assert!(
                extract_selected_target(&zip, &staging, &expected, &BTreeSet::new(), &target,)
                    .is_err()
            );
            assert!(!temp.path().join("escape").exists());
        }
    }

    #[test]
    fn rejects_missing_target_and_nonempty_staging_directory() {
        let (temp, zip) = archive(&[("plugin.json", b"{}")], None);
        let staging = temp.path().join("stage");
        assert!(matches!(
            extract_selected_target(
                &zip,
                &staging,
                &set(&["plugin.json", "bin/windows-x64/plugin.exe"]),
                &BTreeSet::new(),
                &set(&["bin/windows-x64/plugin.exe"]),
            ),
            Err(PackageArchiveError::MissingTargetPayload)
        ));
        assert!(!staging.exists());
        fs::create_dir(&staging).unwrap();
        File::create(staging.join("existing")).unwrap();
        assert!(matches!(
            extract_selected_target(
                &zip,
                &staging,
                &set(&["plugin.json"]),
                &BTreeSet::new(),
                &set(&["plugin.json"]),
            ),
            Err(PackageArchiveError::InvalidStagingDirectory)
        ));
    }

    #[test]
    fn rejects_windows_reparse_point_attributes() {
        let (temp, zip) = archive(&[("plugin.json", b"{}")], None);
        let mut bytes = fs::read(&zip).unwrap();
        for offset in 0..bytes.len().saturating_sub(46) {
            if &bytes[offset..offset + 4] == b"PK\x01\x02" {
                let name_len =
                    u16::from_le_bytes([bytes[offset + 28], bytes[offset + 29]]) as usize;
                let name = &bytes[offset + 46..offset + 46 + name_len];
                if name == b"plugin.json" {
                    bytes[offset + 4] = 20;
                    bytes[offset + 5] = 0; // DOS creator system.
                    bytes[offset + 38..offset + 42].copy_from_slice(&0x0400u32.to_le_bytes());
                    // REPARSE_POINT.
                }
            }
        }
        fs::write(&zip, bytes).unwrap();

        assert!(matches!(
            extract_selected_target(
                &zip,
                &temp.path().join("stage"),
                &set(&["plugin.json"]),
                &BTreeSet::new(),
                &set(&["plugin.json"]),
            ),
            Err(PackageArchiveError::SpecialFile)
        ));
    }

    #[test]
    fn verifies_exact_payload_hash_sets_and_rejects_changed_bytes() {
        let bytes = b"license notice and binary".to_vec();
        let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
        let declared = BTreeMap::from([("licenses/LICENSE".to_owned(), digest)]);
        let payloads = BTreeMap::from([("licenses/LICENSE".to_owned(), bytes.clone())]);
        verify_payload_hashes(&declared, &payloads).unwrap();

        assert_eq!(
            verify_payload_hashes(
                &declared,
                &BTreeMap::from([("licenses/LICENSE".to_owned(), b"changed".to_vec())]),
            ),
            Err(IntegrityError::HashMismatch)
        );
        assert_eq!(
            verify_payload_hashes(
                &declared,
                &BTreeMap::from([
                    ("licenses/LICENSE".to_owned(), bytes),
                    ("extra".to_owned(), b"unlisted".to_vec()),
                ]),
            ),
            Err(IntegrityError::PayloadSetMismatch)
        );
        assert_eq!(
            verify_payload_hashes(
                &BTreeMap::from([("licenses/LICENSE".to_owned(), "A".repeat(64))]),
                &payloads,
            ),
            Err(IntegrityError::InvalidDigest)
        );
    }

    #[test]
    fn integrity_json_rejects_duplicate_paths_unsafe_names_and_bad_digests() {
        let digest = "a".repeat(64);
        let valid = format!(r#"{{"schema":1,"files":{{"plugin.json":"{digest}"}}}}"#);
        let parsed = parse_integrity_json(valid.as_bytes()).unwrap();
        assert_eq!(parsed.schema, 1);
        assert_eq!(parsed.files.0.len(), 1);

        for malformed in [
            format!(r#"{{"schema":1,"schema":1,"files":{{"plugin.json":"{digest}"}}}}"#),
            format!(
                r#"{{"schema":1,"files":{{"plugin.json":"{digest}","plugin.json":"{digest}"}}}}"#
            ),
            format!(r#"{{"schema":1,"files":{{"../escape":"{digest}"}}}}"#),
            r#"{"schema":1,"files":{"plugin.json":"ABC"}}"#.into(),
        ] {
            assert_eq!(
                parse_integrity_json(malformed.as_bytes()),
                Err(IntegrityError::InvalidMetadata)
            );
        }
    }

    #[test]
    fn rechecks_all_payload_files_before_launch_and_rejects_extras_or_links() {
        let temp = tempdir().unwrap();
        let root = temp.path().join("version");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("plugin.json"), b"manifest").unwrap();
        fs::write(root.join("bin/plugin"), b"executable").unwrap();
        let declared = BTreeMap::from([
            (
                "plugin.json".to_owned(),
                format!("{:x}", sha2::Sha256::digest(b"manifest")),
            ),
            (
                "bin/plugin".to_owned(),
                format!("{:x}", sha2::Sha256::digest(b"executable")),
            ),
        ]);
        verify_payload_files(&root, &declared).unwrap();

        fs::write(root.join("plugin.json"), b"changed manifest").unwrap();
        assert_eq!(
            verify_payload_files(&root, &declared),
            Err(IntegrityError::HashMismatch)
        );
        fs::write(root.join("plugin.json"), b"manifest").unwrap();
        fs::write(root.join("bin/plugin"), b"modified").unwrap();
        assert_eq!(
            verify_payload_files(&root, &declared),
            Err(IntegrityError::HashMismatch)
        );
        fs::write(root.join("bin/plugin"), b"executable").unwrap();
        fs::write(root.join("extra"), b"not signed").unwrap();
        assert_eq!(
            verify_payload_files(&root, &declared),
            Err(IntegrityError::PayloadSetMismatch)
        );

        fs::remove_file(root.join("extra")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("plugin.json"), root.join("link")).unwrap();
            assert_eq!(
                verify_payload_files(&root, &declared),
                Err(IntegrityError::PayloadSetMismatch)
            );
        }
    }

    #[test]
    fn verifies_domain_separated_ed25519_signature_over_exact_bytes() {
        use ring::signature::KeyPair;

        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7u8; 32]).unwrap();
        let raw = br#"{"schema":1,"files":{}}"#;
        let mut signed = SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(raw);
        let signature = key.sign(&signed);
        verify_integrity_signature(key.public_key().as_ref(), raw, signature.as_ref()).unwrap();
        let unsigned_domain_signature = key.sign(raw);
        assert_eq!(
            verify_integrity_signature(
                key.public_key().as_ref(),
                raw,
                unsigned_domain_signature.as_ref(),
            ),
            Err(IntegrityError::InvalidSignature)
        );
        assert_eq!(
            verify_integrity_signature(
                key.public_key().as_ref(),
                b"{\"schema\":1,\"files\":{}} ",
                signature.as_ref(),
            ),
            Err(IntegrityError::InvalidSignature)
        );
        assert_eq!(
            verify_integrity_signature(&[0; 31], raw, signature.as_ref()),
            Err(IntegrityError::InvalidKeyOrSignature)
        );
    }

    #[test]
    fn validates_signed_package_and_rechecks_notices_and_payload_files() {
        use ring::signature::KeyPair;
        let temp = tempdir().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("licenses")).unwrap();
        fs::write(root.join("plugin.json"), b"manifest").unwrap();
        fs::write(root.join("licenses/NOTICE"), b"notice").unwrap();
        fs::write(root.join("bin"), b"executable").unwrap();
        let files = BTreeMap::from([
            (
                "plugin.json".to_owned(),
                format!("{:x}", sha2::Sha256::digest(b"manifest")),
            ),
            (
                "licenses/NOTICE".to_owned(),
                format!("{:x}", sha2::Sha256::digest(b"notice")),
            ),
            (
                "bin".to_owned(),
                format!("{:x}", sha2::Sha256::digest(b"executable")),
            ),
        ]);
        let raw = serde_json::to_vec(&serde_json::json!({"schema": 1, "files": files})).unwrap();
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[9u8; 32]).unwrap();
        let mut signed = SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(&raw);
        fs::write(root.join("integrity.json"), &raw).unwrap();
        fs::write(root.join("signature.ed25519"), key.sign(&signed).as_ref()).unwrap();

        verify_package_directory(root, key.public_key().as_ref()).unwrap();
        verify_installed_package(root, key.public_key().as_ref()).unwrap();
        fs::write(root.join("licenses/NOTICE"), b"changed notice").unwrap();
        assert_eq!(
            verify_installed_package(root, key.public_key().as_ref()),
            Err(IntegrityError::HashMismatch)
        );
        assert!(matches!(
            verify_package_directory(root, &[0; 31]),
            Err(IntegrityError::InvalidKeyOrSignature)
        ));
    }
}
