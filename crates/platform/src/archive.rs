//! Archive detection, safe entry-path validation and in-process extraction shared by game tools
//! and PC port installs.
//!
//! All functions are blocking and must run on a worker thread. Extraction enforces a caller-set
//! uncompressed-size cap so a decompression bomb cannot fill the disk.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use gamenode_core::tools::{ArchiveEntry, ToolError};

/// Default maximum uncompressed bytes allowed during archive extraction (2 GiB).
pub const MAX_TOTAL_EXTRACTED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Extracted-size limit for PC port release assets (16 GiB).
///
/// Ports sometimes bundle large game assets, so they use a higher cap than game tools.
pub const PORTS_MAX_TOTAL_EXTRACTED_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveType {
    SevenZip,
    Zip,
    /// A tar stream, optionally compressed with gzip ([`ArchiveType::TarGz`]).
    Tar,
    TarGz,
}

/// Kind of release asset that can be placed into an install folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetType {
    SevenZip,
    Zip,
    Tar,
    TarGz,
    /// A bare executable that is copied into the folder as-is.
    BareExecutable,
}

/// Detects an archive from its magic bytes, falling back to the file extension.
pub fn detect_archive_type(path: &Path) -> Result<ArchiveType, ToolError> {
    if let Ok(mut file) = File::open(path) {
        let mut magic = [0u8; 6];
        if let Ok(n) = file.read(&mut magic) {
            if n >= 6 && &magic[..6] == b"7z\xBC\xAF\x27\x1C" {
                return Ok(ArchiveType::SevenZip);
            }
            if n >= 4 && (&magic[..4] == b"PK\x03\x04" || &magic[..4] == b"PK\x05\x06") {
                return Ok(ArchiveType::Zip);
            }
            if n >= 2 && magic[..2] == [0x1f, 0x8b] {
                return Ok(ArchiveType::TarGz);
            }
        }
    }

    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return Ok(ArchiveType::TarGz);
    }
    match file_extension(path).as_str() {
        "7z" => Ok(ArchiveType::SevenZip),
        "zip" => Ok(ArchiveType::Zip),
        "tar" => Ok(ArchiveType::Tar),
        _ => Err(ToolError::ExtractionFailed(format!(
            "Unsupported or unrecognized archive format for: {}",
            path.display()
        ))),
    }
}

/// Detects how a release asset is placed into an install folder.
///
/// Archives are extracted; a bare executable is copied as-is. Windows ships `.exe`; a native Linux
/// port ships an AppImage, a `bin`, an architecture-suffixed name such as `game.x86_64`, or an
/// extensionless executable. An unknown extension is only accepted when the file starts with an
/// executable magic, so an arbitrary extensionless file is not mistaken for a port (design D8).
pub fn detect_asset_type(path: &Path) -> Result<AssetType, ToolError> {
    match detect_archive_type(path) {
        Ok(ArchiveType::SevenZip) => Ok(AssetType::SevenZip),
        Ok(ArchiveType::Zip) => Ok(AssetType::Zip),
        Ok(ArchiveType::Tar) => Ok(AssetType::Tar),
        Ok(ArchiveType::TarGz) => Ok(AssetType::TarGz),
        Err(error) => {
            let extension = file_extension(path);
            // `.run` is not a supported Linux release format. The list of accepted names must
            // agree with `SUPPORTED_LINUX_FORMATS` and `linux_asset::kind` (task 3.9).
            if extension == "run" {
                return Err(ToolError::ExtractionFailed(format!(
                    "`.run` is not a supported Linux release format: {}",
                    path.display()
                )));
            }
            if matches!(extension.as_str(), "exe" | "appimage" | "bin")
                || gamenode_core::ports::plan::BARE_EXECUTABLE_SUFFIXES
                    .contains(&extension.as_str())
                || looks_like_executable(path)
            {
                Ok(AssetType::BareExecutable)
            } else {
                Err(error)
            }
        }
    }
}

/// True when the file starts with an ELF, PE or Mach-O magic. Checked only for names without a
/// known extension.
fn looks_like_executable(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    if file.read(&mut magic).ok() != Some(magic.len()) {
        return false;
    }
    magic == *b"\x7fELF"
        || (magic[0] == b'M' && magic[1] == b'Z')
        || matches!(
            magic,
            [0xFE, 0xED, 0xFA, 0xCE]
                | [0xFE, 0xED, 0xFA, 0xCF]
                | [0xCE, 0xFA, 0xED, 0xFE]
                | [0xCF, 0xFA, 0xED, 0xFE]
                | [0xCA, 0xFE, 0xBA, 0xBE]
        )
}

fn file_extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Validates an entry's relative path against directory traversal attacks (`..`, absolute paths, drive prefixes).
pub fn validate_archive_entry_path(raw_name: &str) -> Result<PathBuf, ToolError> {
    let normalized = raw_name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return Err(ToolError::ExtractionFailed(format!(
            "Forbidden absolute or prefixed entry path: {raw_name}"
        )));
    }

    let trimmed = normalized.strip_prefix("./").unwrap_or(&normalized);
    if trimmed.is_empty() || trimmed == "." {
        return Ok(PathBuf::new());
    }

    let p = Path::new(trimmed);
    for comp in p.components() {
        match comp {
            Component::ParentDir => {
                return Err(ToolError::ExtractionFailed(format!(
                    "Path traversal detected ('..'): {raw_name}"
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ToolError::ExtractionFailed(format!(
                    "Forbidden root or prefix component in entry: {raw_name}"
                )));
            }
            Component::Normal(_) | Component::CurDir => {}
        }
    }

    Ok(p.to_path_buf())
}

/// Lists all entries inside a `.7z` or `.zip` archive without extracting them.
pub fn list_archive_entries(archive_path: &Path) -> Result<Vec<ArchiveEntry>, ToolError> {
    let archive_type = detect_archive_type(archive_path)?;
    match archive_type {
        ArchiveType::SevenZip => {
            let file = File::open(archive_path).map_err(|e| ToolError::Io(e.to_string()))?;
            let reader = sevenz_rust2::ArchiveReader::new(file, sevenz_rust2::Password::empty())
                .map_err(|e| ToolError::ExtractionFailed(format!("Failed to open 7z: {e}")))?;
            let mut entries = Vec::new();
            for f in reader.archive().files.iter() {
                let clean = validate_archive_entry_path(&f.name)?;
                if clean.as_os_str().is_empty() {
                    continue;
                }
                entries.push(ArchiveEntry {
                    relative_path: clean.to_string_lossy().replace('\\', "/"),
                    size_bytes: f.size(),
                    is_dir: f.is_directory(),
                });
            }
            Ok(entries)
        }
        ArchiveType::Zip => {
            let file = File::open(archive_path).map_err(|e| ToolError::Io(e.to_string()))?;
            let mut archive = zip::ZipArchive::new(file)
                .map_err(|e| ToolError::ExtractionFailed(format!("Failed to open zip: {e}")))?;
            let mut entries = Vec::new();
            for i in 0..archive.len() {
                let zip_file = archive.by_index(i).map_err(|e| {
                    ToolError::ExtractionFailed(format!("Zip read error at {i}: {e}"))
                })?;
                let clean = validate_archive_entry_path(zip_file.name())?;
                if clean.as_os_str().is_empty() {
                    continue;
                }
                entries.push(ArchiveEntry {
                    relative_path: clean.to_string_lossy().replace('\\', "/"),
                    size_bytes: zip_file.size(),
                    is_dir: zip_file.is_dir(),
                });
            }
            Ok(entries)
        }
        ArchiveType::Tar => {
            let file = File::open(archive_path).map_err(|e| ToolError::Io(e.to_string()))?;
            list_tar_entries(file)
        }
        ArchiveType::TarGz => {
            let file = File::open(archive_path).map_err(|e| ToolError::Io(e.to_string()))?;
            list_tar_entries(flate2::read::GzDecoder::new(file))
        }
    }
}

fn list_tar_entries<R: Read>(reader: R) -> Result<Vec<ArchiveEntry>, ToolError> {
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    for item in archive
        .entries()
        .map_err(|e| ToolError::ExtractionFailed(e.to_string()))?
    {
        let entry = item.map_err(|e| ToolError::ExtractionFailed(e.to_string()))?;
        let raw = entry
            .path()
            .map_err(|e| ToolError::ExtractionFailed(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let clean = validate_archive_entry_path(&raw)?;
        if clean.as_os_str().is_empty() {
            continue;
        }
        let kind = entry.header().entry_type();
        entries.push(ArchiveEntry {
            relative_path: clean.to_string_lossy().replace('\\', "/"),
            size_bytes: entry.header().size().unwrap_or(0),
            is_dir: kind.is_dir(),
        });
    }
    Ok(entries)
}

/// Safely extracts a `.7z`, `.zip`, `.tar` or `.tar.gz` archive into `dest_dir` with the default
/// 2 GiB cap.
pub fn extract_archive_to_dir(
    archive_path: &Path,
    dest_dir: &Path,
) -> Result<Vec<ArchiveEntry>, ToolError> {
    extract_archive_to_dir_with_cap(archive_path, dest_dir, MAX_TOTAL_EXTRACTED_BYTES)
}

/// Safely extracts a `.7z` or `.zip` archive into `dest_dir`, enforcing path traversal checks and
/// a caller-set uncompressed-size cap.
pub fn extract_archive_to_dir_with_cap(
    archive_path: &Path,
    dest_dir: &Path,
    max_total_bytes: u64,
) -> Result<Vec<ArchiveEntry>, ToolError> {
    extract_archive_inner(archive_path, dest_dir, max_total_bytes, false)
}

/// The shared implementation. `apply_unix_modes` is only used by the PC port installer on Linux,
/// where a zip entry's recorded Unix mode must survive extraction (design D11); everywhere else
/// the platform default stays, as before.
fn extract_archive_inner(
    archive_path: &Path,
    dest_dir: &Path,
    max_total_bytes: u64,
    apply_unix_modes: bool,
) -> Result<Vec<ArchiveEntry>, ToolError> {
    let archive_type = detect_archive_type(archive_path)?;
    std::fs::create_dir_all(dest_dir).map_err(|e| ToolError::Io(e.to_string()))?;

    let mut entries = Vec::new();
    let mut total_bytes = 0u64;
    let limit_message = extraction_limit_message(max_total_bytes);

    match archive_type {
        ArchiveType::SevenZip => {
            let file = File::open(archive_path).map_err(|e| ToolError::Io(e.to_string()))?;
            let mut reader =
                sevenz_rust2::ArchiveReader::new(file, sevenz_rust2::Password::empty())
                    .map_err(|e| ToolError::ExtractionFailed(format!("Failed to open 7z: {e}")))?;

            reader
                .for_each_entries(|entry, stream| {
                    let clean = match validate_archive_entry_path(entry.name()) {
                        Ok(p) => p,
                        Err(e) => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                e.to_string(),
                            )
                            .into());
                        }
                    };
                    if clean.as_os_str().is_empty() {
                        return Ok(true);
                    }

                    // Reject symlinks/reparse points
                    if entry.has_windows_attributes && (entry.windows_attributes & 0x0400 != 0) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Symlink/reparse point rejected: {}", entry.name()),
                        )
                        .into());
                    }

                    let target = dest_dir.join(&clean);
                    if entry.is_directory() {
                        std::fs::create_dir_all(&target)?;
                        entries.push(ArchiveEntry {
                            relative_path: clean.to_string_lossy().replace('\\', "/"),
                            size_bytes: 0,
                            is_dir: true,
                        });
                    } else {
                        if let Some(parent) = target.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        let mut out_file = File::create(&target)?;
                        let mut buf = [0u8; 64 * 1024];
                        let mut file_size = 0u64;
                        loop {
                            let n = stream.read(&mut buf)?;
                            if n == 0 {
                                break;
                            }
                            total_bytes += n as u64;
                            if total_bytes > max_total_bytes {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    limit_message.clone(),
                                )
                                .into());
                            }
                            file_size += n as u64;
                            out_file.write_all(&buf[..n])?;
                        }
                        entries.push(ArchiveEntry {
                            relative_path: clean.to_string_lossy().replace('\\', "/"),
                            size_bytes: file_size,
                            is_dir: false,
                        });
                    }
                    Ok(true)
                })
                .map_err(|e| ToolError::ExtractionFailed(e.to_string()))?;
        }
        ArchiveType::Zip => {
            let file = File::open(archive_path).map_err(|e| ToolError::Io(e.to_string()))?;
            let mut archive = zip::ZipArchive::new(file)
                .map_err(|e| ToolError::ExtractionFailed(format!("Failed to open zip: {e}")))?;

            for i in 0..archive.len() {
                let mut zip_file = archive.by_index(i).map_err(|e| {
                    ToolError::ExtractionFailed(format!("Zip read error at {i}: {e}"))
                })?;

                let clean = validate_archive_entry_path(zip_file.name())?;
                if clean.as_os_str().is_empty() {
                    continue;
                }

                // Check for Unix symlink mode
                if let Some(mode) = zip_file.unix_mode()
                    && (mode & 0o170000) == 0o120000
                {
                    return Err(ToolError::ExtractionFailed(format!(
                        "Symlink rejected in zip: {}",
                        zip_file.name()
                    )));
                }

                let target = dest_dir.join(&clean);
                if zip_file.is_dir() {
                    std::fs::create_dir_all(&target).map_err(|e| ToolError::Io(e.to_string()))?;
                    entries.push(ArchiveEntry {
                        relative_path: clean.to_string_lossy().replace('\\', "/"),
                        size_bytes: 0,
                        is_dir: true,
                    });
                } else {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| ToolError::Io(e.to_string()))?;
                    }
                    let mut out_file =
                        File::create(&target).map_err(|e| ToolError::Io(e.to_string()))?;
                    let mut buf = [0u8; 64 * 1024];
                    let mut file_size = 0u64;
                    loop {
                        let n = zip_file
                            .read(&mut buf)
                            .map_err(|e| ToolError::ExtractionFailed(e.to_string()))?;
                        if n == 0 {
                            break;
                        }
                        total_bytes += n as u64;
                        if total_bytes > max_total_bytes {
                            return Err(ToolError::ExtractionFailed(limit_message.clone()));
                        }
                        file_size += n as u64;
                        out_file
                            .write_all(&buf[..n])
                            .map_err(|e| ToolError::Io(e.to_string()))?;
                    }
                    if apply_unix_modes {
                        apply_recorded_mode(&target, zip_file.unix_mode())?;
                    }
                    entries.push(ArchiveEntry {
                        relative_path: clean.to_string_lossy().replace('\\', "/"),
                        size_bytes: file_size,
                        is_dir: false,
                    });
                }
            }
        }
        ArchiveType::Tar => {
            let file = File::open(archive_path).map_err(|e| ToolError::Io(e.to_string()))?;
            extract_tar_to_dir(
                file,
                dest_dir,
                max_total_bytes,
                apply_unix_modes,
                &mut entries,
                &mut total_bytes,
            )?;
        }
        ArchiveType::TarGz => {
            let file = File::open(archive_path).map_err(|e| ToolError::Io(e.to_string()))?;
            let decoder = flate2::read::GzDecoder::new(file);
            extract_tar_to_dir(
                decoder,
                dest_dir,
                max_total_bytes,
                apply_unix_modes,
                &mut entries,
                &mut total_bytes,
            )?;
        }
    }

    Ok(entries)
}

/// Extracts a tar stream with the same traversal, link and size rules as the archive formats.
fn extract_tar_to_dir<R: Read>(
    reader: R,
    dest_dir: &Path,
    max_total_bytes: u64,
    apply_unix_modes: bool,
    entries: &mut Vec<ArchiveEntry>,
    total_bytes: &mut u64,
) -> Result<(), ToolError> {
    let limit_message = extraction_limit_message(max_total_bytes);
    let mut archive = tar::Archive::new(reader);
    let items = archive
        .entries()
        .map_err(|e| ToolError::ExtractionFailed(e.to_string()))?;
    for item in items {
        let mut entry = item.map_err(|e| ToolError::ExtractionFailed(e.to_string()))?;
        let kind = entry.header().entry_type();
        // Links and device nodes never come from a port release; only files and directories do.
        if !kind.is_file() && !kind.is_dir() {
            return Err(ToolError::ExtractionFailed(format!(
                "Special entry rejected in tar: {}",
                entry
                    .path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default()
            )));
        }
        let raw = entry
            .path()
            .map_err(|e| ToolError::ExtractionFailed(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let clean = validate_archive_entry_path(&raw)?;
        if clean.as_os_str().is_empty() {
            continue;
        }
        let target = dest_dir.join(&clean);
        if kind.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| ToolError::Io(e.to_string()))?;
            entries.push(ArchiveEntry {
                relative_path: clean.to_string_lossy().replace('\\', "/"),
                size_bytes: 0,
                is_dir: true,
            });
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ToolError::Io(e.to_string()))?;
        }
        let mut out_file = File::create(&target).map_err(|e| ToolError::Io(e.to_string()))?;
        let mut buf = [0u8; 64 * 1024];
        let mut file_size = 0u64;
        loop {
            let n = entry
                .read(&mut buf)
                .map_err(|e| ToolError::ExtractionFailed(e.to_string()))?;
            if n == 0 {
                break;
            }
            *total_bytes += n as u64;
            if *total_bytes > max_total_bytes {
                return Err(ToolError::ExtractionFailed(limit_message.clone()));
            }
            file_size += n as u64;
            out_file
                .write_all(&buf[..n])
                .map_err(|e| ToolError::Io(e.to_string()))?;
        }
        if apply_unix_modes {
            apply_recorded_mode(&target, entry.header().mode().ok())?;
        }
        entries.push(ArchiveEntry {
            relative_path: clean.to_string_lossy().replace('\\', "/"),
            size_bytes: file_size,
            is_dir: false,
        });
    }
    Ok(())
}

/// Extracts an archive asset or copies a bare `.exe` into `dest_dir`, enforcing
/// `max_total_bytes` for both cases.
///
/// The returned entries describe what was written, relative to `dest_dir`.
pub fn extract_asset_to_dir(
    asset_path: &Path,
    dest_dir: &Path,
    max_total_bytes: u64,
) -> Result<Vec<ArchiveEntry>, ToolError> {
    match detect_asset_type(asset_path)? {
        AssetType::SevenZip | AssetType::Zip => {
            extract_archive_to_dir_with_cap(asset_path, dest_dir, max_total_bytes)
        }
        AssetType::Tar => extract_archive_inner(asset_path, dest_dir, max_total_bytes, false),
        AssetType::TarGz => extract_archive_inner(asset_path, dest_dir, max_total_bytes, false),
        AssetType::BareExecutable => copy_bare_executable(asset_path, dest_dir, max_total_bytes),
    }
}

/// Extracts a PC port release asset into `dest_dir`. When `apply_unix_modes` is true (the Linux
/// install path), a zip entry's recorded Unix mode is applied to the extracted file; 7z records no
/// mode, so archives of that type keep the platform default (design D11).
pub fn extract_port_asset_to_dir(
    asset_path: &Path,
    dest_dir: &Path,
    max_total_bytes: u64,
    apply_unix_modes: bool,
) -> Result<Vec<ArchiveEntry>, ToolError> {
    match detect_asset_type(asset_path)? {
        AssetType::SevenZip | AssetType::Zip | AssetType::Tar | AssetType::TarGz => {
            extract_archive_inner(asset_path, dest_dir, max_total_bytes, apply_unix_modes)
        }
        AssetType::BareExecutable => copy_bare_executable(asset_path, dest_dir, max_total_bytes),
    }
}

/// Applies a zip entry's recorded Unix mode, dropping the file-type bits and any setuid, setgid or
/// sticky bit. A recorded `0o000` keeps the platform default.
#[cfg(unix)]
fn apply_recorded_mode(path: &Path, mode: Option<u32>) -> Result<(), ToolError> {
    use std::os::unix::fs::PermissionsExt;
    let Some(mode) = mode else {
        return Ok(());
    };
    let mode = mode & 0o777;
    if mode == 0 {
        return Ok(());
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| ToolError::Io(e.to_string()))
}

#[cfg(not(unix))]
fn apply_recorded_mode(_: &Path, _: Option<u32>) -> Result<(), ToolError> {
    Ok(())
}

fn copy_bare_executable(
    asset_path: &Path,
    dest_dir: &Path,
    max_total_bytes: u64,
) -> Result<Vec<ArchiveEntry>, ToolError> {
    let file_name = asset_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ToolError::ExtractionFailed(format!(
                "Executable asset has no usable file name: {}",
                asset_path.display()
            ))
        })?;
    let clean = validate_archive_entry_path(file_name)?;
    if clean.as_os_str().is_empty() {
        return Err(ToolError::ExtractionFailed(format!(
            "Executable asset has no usable file name: {}",
            asset_path.display()
        )));
    }

    let size_bytes = std::fs::metadata(asset_path)
        .map_err(|e| ToolError::Io(e.to_string()))?
        .len();
    if size_bytes > max_total_bytes {
        return Err(ToolError::ExtractionFailed(extraction_limit_message(
            max_total_bytes,
        )));
    }

    std::fs::create_dir_all(dest_dir).map_err(|e| ToolError::Io(e.to_string()))?;
    let target = dest_dir.join(&clean);
    std::fs::copy(asset_path, &target).map_err(|e| ToolError::Io(e.to_string()))?;

    Ok(vec![ArchiveEntry {
        relative_path: clean.to_string_lossy().replace('\\', "/"),
        size_bytes,
        is_dir: false,
    }])
}

fn extraction_limit_message(max_total_bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    let human = if max_total_bytes.is_multiple_of(GIB) {
        format!("{} GiB", max_total_bytes / GIB)
    } else {
        format!("{} MiB", max_total_bytes / MIB)
    };
    format!("Archive exceeded maximum extracted size limit ({human})")
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_linux_binaries_are_bare_executables() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["port.AppImage", "port.bin", "game.x86_64"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"\x7fELFnot a real binary").unwrap();
            assert_eq!(
                detect_asset_type(&path).unwrap(),
                AssetType::BareExecutable,
                "{name}"
            );
        }

        // An extensionless file is accepted only when its content looks executable.
        let extensionless = dir.path().join("devilutionx");
        std::fs::write(&extensionless, b"\x7fELFnot a real binary").unwrap();
        assert_eq!(
            detect_asset_type(&extensionless).unwrap(),
            AssetType::BareExecutable
        );

        let arbitrary = dir.path().join("LICENSE");
        std::fs::write(&arbitrary, b"MIT License").unwrap();
        assert!(
            detect_asset_type(&arbitrary).is_err(),
            "an arbitrary extensionless file is not an executable"
        );

        // `.run` is not a supported Linux format, even with an executable magic.
        let run = dir.path().join("port.run");
        std::fs::write(&run, b"\x7fELFnot a real binary").unwrap();
        assert!(detect_asset_type(&run).is_err());

        let unknown = dir.path().join("port.deb");
        std::fs::write(&unknown, b"not an archive").unwrap();
        assert!(detect_asset_type(&unknown).is_err());
    }

    use super::*;
    use std::io::Write;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn rejects_parent_traversal_entries() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("traversal.zip");
        write_zip(&archive, &[("..\\..\\Windows\\evil.dll", b"evil")]);

        let dest = temp.path().join("out");
        let error = extract_archive_to_dir(&archive, &dest).unwrap_err();
        match error {
            ToolError::ExtractionFailed(message) => {
                assert!(message.contains("traversal"), "unexpected error: {message}");
            }
            other => panic!("Expected ExtractionFailed, got: {other:?}"),
        }
        assert!(!dest.join("Windows/evil.dll").exists());
    }

    #[test]
    fn rejects_absolute_entry_paths() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("absolute.zip");
        write_zip(&archive, &[("/etc/shadow", b"root:x:0:0")]);

        let dest = temp.path().join("out");
        let error = extract_archive_to_dir(&archive, &dest).unwrap_err();
        match error {
            ToolError::ExtractionFailed(message) => {
                assert!(
                    message.contains("absolute") || message.contains("prefix"),
                    "unexpected error: {message}"
                );
            }
            other => panic!("Expected ExtractionFailed, got: {other:?}"),
        }

        let drive_archive = temp.path().join("drive.zip");
        write_zip(
            &drive_archive,
            &[("C:\\Windows\\system32\\evil.dll", b"evil")],
        );
        let drive_error = extract_archive_to_dir(&drive_archive, &dest).unwrap_err();
        match drive_error {
            ToolError::ExtractionFailed(message) => {
                assert!(
                    message.contains("absolute") || message.contains("prefix"),
                    "unexpected error: {message}"
                );
            }
            other => panic!("Expected ExtractionFailed, got: {other:?}"),
        }
    }

    #[test]
    fn rejects_escaping_symlink_entries() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("symlink.zip");
        {
            let file = File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            // A Unix symlink entry whose target escapes the destination folder.
            zip.add_symlink(
                "escape",
                "../../outside.txt",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.finish().unwrap();
        }

        let dest = temp.path().join("out");
        let error = extract_archive_to_dir(&archive, &dest).unwrap_err();
        match error {
            ToolError::ExtractionFailed(message) => {
                assert!(message.contains("Symlink"), "unexpected error: {message}");
            }
            other => panic!("Expected ExtractionFailed, got: {other:?}"),
        }
        assert!(!dest.join("escape").exists());
    }

    fn tar_bytes(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, bytes, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            builder
                .append_data(&mut header, name, std::io::Cursor::new(bytes))
                .unwrap();
        }
        builder.into_inner().unwrap()
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn tar_and_tar_gz_archives_extract() {
        let temp = tempfile::tempdir().unwrap();
        let tar = tar_bytes(&[
            ("game", b"binary", 0o755),
            ("data/config.txt", b"cfg", 0o644),
        ]);

        let tar_path = temp.path().join("port.tar");
        std::fs::write(&tar_path, &tar).unwrap();
        let dest = temp.path().join("tar");
        extract_archive_to_dir(&tar_path, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("game")).unwrap(), b"binary");
        assert_eq!(std::fs::read(dest.join("data/config.txt")).unwrap(), b"cfg");

        let gz_path = temp.path().join("port.tar.gz");
        std::fs::write(&gz_path, gzip(&tar)).unwrap();
        let dest = temp.path().join("gz");
        extract_asset_to_dir(&gz_path, &dest, PORTS_MAX_TOTAL_EXTRACTED_BYTES).unwrap();
        assert_eq!(std::fs::read(dest.join("game")).unwrap(), b"binary");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dest = temp.path().join("modes");
            extract_port_asset_to_dir(&tar_path, &dest, PORTS_MAX_TOTAL_EXTRACTED_BYTES, true)
                .unwrap();
            let mode = std::fs::metadata(dest.join("game"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_port_zip_keeps_its_recorded_unix_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("port.zip");
        {
            let file = File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            zip.start_file("port", options).unwrap();
            zip.write_all(b"binary").unwrap();
            zip.finish().unwrap();
        }

        let dest = temp.path().join("out");
        extract_port_asset_to_dir(&archive, &dest, PORTS_MAX_TOTAL_EXTRACTED_BYTES, true).unwrap();
        let mode = std::fs::metadata(dest.join("port"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);

        // The shared extractor used by game tools and the extractor cache stays unchanged.
        let plain = temp.path().join("plain");
        extract_asset_to_dir(&archive, &plain, PORTS_MAX_TOTAL_EXTRACTED_BYTES).unwrap();
        let mode = std::fs::metadata(plain.join("port"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o111,
            0,
            "the default extraction does not apply the recorded mode"
        );
    }

    #[test]
    fn enforces_caller_set_extracted_size_cap() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("large.zip");
        write_zip(&archive, &[("large.bin", &[0u8; 4096])]);

        let dest = temp.path().join("out");
        let error = extract_archive_to_dir_with_cap(&archive, &dest, 1024).unwrap_err();
        match error {
            ToolError::ExtractionFailed(message) => {
                assert!(
                    message.contains("maximum extracted size"),
                    "unexpected error: {message}"
                );
            }
            other => panic!("Expected ExtractionFailed, got: {other:?}"),
        }

        // The same archive extracts under the default 2 GiB cap.
        let ok_dest = temp.path().join("out_ok");
        let entries = extract_archive_to_dir(&archive, &ok_dest).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].relative_path, "large.bin");
        assert_eq!(entries[0].size_bytes, 4096);
    }

    #[test]
    fn ports_cap_is_16_gib() {
        assert_eq!(PORTS_MAX_TOTAL_EXTRACTED_BYTES, 16 * 1024 * 1024 * 1024);
        assert_eq!(MAX_TOTAL_EXTRACTED_BYTES, 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn copies_bare_exe_asset_as_is() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("Lantern Engine.exe");
        std::fs::write(&source, b"MZ fake launcher bytes").unwrap();

        let dest = temp.path().join("install");
        let entries =
            extract_asset_to_dir(&source, &dest, PORTS_MAX_TOTAL_EXTRACTED_BYTES).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].relative_path, "Lantern Engine.exe");
        assert_eq!(entries[0].size_bytes, 22);
        assert!(!entries[0].is_dir);
        assert_eq!(
            std::fs::read(dest.join("Lantern Engine.exe")).unwrap(),
            b"MZ fake launcher bytes"
        );
    }

    #[test]
    fn rejects_bare_exe_over_the_size_cap() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("huge.exe");
        std::fs::write(&source, [0u8; 2048]).unwrap();

        let dest = temp.path().join("install");
        let error = extract_asset_to_dir(&source, &dest, 1024).unwrap_err();
        match error {
            ToolError::ExtractionFailed(message) => {
                assert!(
                    message.contains("maximum extracted size"),
                    "unexpected error: {message}"
                );
            }
            other => panic!("Expected ExtractionFailed, got: {other:?}"),
        }
        assert!(!dest.join("huge.exe").exists());
    }

    #[test]
    fn extracts_zip_assets_through_asset_entry_point() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("release.zip");
        write_zip(
            &archive,
            &[("port.exe", b"port"), ("data/readme.txt", b"hi")],
        );

        let dest = temp.path().join("install");
        let entries =
            extract_asset_to_dir(&archive, &dest, PORTS_MAX_TOTAL_EXTRACTED_BYTES).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(std::fs::read(dest.join("port.exe")).unwrap(), b"port");
        assert_eq!(
            std::fs::read_to_string(dest.join("data/readme.txt")).unwrap(),
            "hi"
        );
    }

    #[test]
    fn rejects_unknown_asset_formats() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("payload.msi");
        std::fs::write(&source, b"not supported").unwrap();

        let dest = temp.path().join("install");
        let error =
            extract_asset_to_dir(&source, &dest, PORTS_MAX_TOTAL_EXTRACTED_BYTES).unwrap_err();
        assert!(matches!(error, ToolError::ExtractionFailed(_)));
    }
}
