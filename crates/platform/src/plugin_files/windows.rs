//! Windows: there is no `openat` in the standard library, so each component is checked as a
//! reparse point (symbolic link, junction, mount point) and the handle that was actually opened
//! is then asked for its final path, which must be exactly the approved root plus the requested
//! components. A component swapped for a junction between the check and the open therefore ends
//! the operation before any byte is read, written or deleted.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ADD_FILE, FILE_ATTRIBUTE_READONLY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_DELETE_CHILD, FILE_DISPOSITION_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_DATA, FileDispositionInfo, GetFileInformationByHandle,
    GetFinalPathNameByHandleW, SetFileInformationByHandle, VOLUME_NAME_DOS,
};

use super::{
    AccessInfo, Confined, Entry, FileError, Identity, MAX_DIR_ENTRIES, Target, Tri, valid_component,
};

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
/// Seconds between 1601-01-01 (FILETIME) and the Unix epoch.
const FILETIME_UNIX_OFFSET: u64 = 11_644_473_600;

fn handle_of(file: &File) -> HANDLE {
    file.as_raw_handle() as HANDLE
}

fn information(file: &File) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    // SAFETY: `info` is plain data the call fills in, and the handle is open for the call.
    unsafe {
        let mut info = std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>();
        if GetFileInformationByHandle(handle_of(file), &mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }
}

/// Opens `path` itself (a directory or a file), never following a link at the end.
fn open_for_identity(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

pub(super) fn identity_of(path: &Path) -> io::Result<Identity> {
    let info = information(&open_for_identity(path)?)?;
    Ok(Identity {
        volume: u64::from(info.dwVolumeSerialNumber),
        node: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    })
}

/// The path the system reports for an open handle, with every link resolved.
fn final_path(file: &File) -> io::Result<PathBuf> {
    let mut buffer = vec![0u16; 1024];
    loop {
        // SAFETY: the buffer is valid for `buffer.len()` UTF-16 units for the call.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                handle_of(file),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length < buffer.len() {
            buffer.truncate(length);
            return Ok(PathBuf::from(String::from_utf16_lossy(&buffer)));
        }
        buffer.resize(length + 1, 0);
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (a.to_str(), b.to_str()) {
        (Some(a), Some(b)) => a.to_lowercase() == b.to_lowercase(),
        _ => false,
    }
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// `root` plus `components`, checking that no component is a link or other reparse point and
/// that the result is not a reserved host folder.
fn walk(c: &Confined, components: &[String]) -> Result<PathBuf, FileError> {
    let mut path = c.root.path.clone();
    for name in components {
        if !valid_component(name) {
            return Err(FileError::Denied);
        }
        path.push(name);
        let metadata = fs::symlink_metadata(&path).map_err(FileError::from)?;
        if is_reparse(&metadata) {
            return Err(FileError::Denied);
        }
    }
    if c.reserved.covers(&path) {
        return Err(FileError::Denied);
    }
    Ok(path)
}

/// The opened handle must be exactly where the walk said it was, and not reserved.
fn verify(c: &Confined, file: &File, expected: &Path) -> Result<(), FileError> {
    let actual = final_path(file).map_err(FileError::from)?;
    // `canonicalize` and the handle both give verbatim (`\\?\`) paths, so they compare directly.
    let expected = if actual
        .to_str()
        .is_some_and(|text| text.starts_with(r"\\?\"))
        && !expected
            .to_str()
            .is_some_and(|text| text.starts_with(r"\\?\"))
    {
        PathBuf::from(format!(r"\\?\{}", expected.display()))
    } else {
        expected.to_path_buf()
    };
    if !same_path(&actual, &expected) || c.reserved.covers(&actual) {
        return Err(FileError::Denied);
    }
    Ok(())
}

fn open_directory(c: &Confined, components: &[String]) -> Result<PathBuf, FileError> {
    let path = walk(c, components)?;
    let handle = open_for_identity(&path).map_err(FileError::from)?;
    let metadata = handle.metadata().map_err(FileError::from)?;
    if !metadata.is_dir() {
        return Err(FileError::NotADirectory);
    }
    verify(c, &handle, &path)?;
    Ok(path)
}

fn unix_seconds(metadata: &fs::Metadata) -> i64 {
    (metadata.last_write_time() / 10_000_000).saturating_sub(FILETIME_UNIX_OFFSET) as i64
}

/// The sub-second part of the write time: FILETIME counts 100-nanosecond ticks.
fn unix_nanos(metadata: &fs::Metadata) -> u32 {
    ((metadata.last_write_time() % 10_000_000) * 100) as u32
}

fn entry_of(name: String, directory: bool, metadata: &fs::Metadata) -> Entry {
    Entry {
        name,
        directory,
        size: if directory { 0 } else { metadata.file_size() },
        modified: unix_seconds(metadata),
        modified_nanos: unix_nanos(metadata),
        read_only: metadata.file_attributes() & FILE_ATTRIBUTE_READONLY != 0,
    }
}

pub(super) fn list(c: &Confined, directory: &[String]) -> Result<Vec<Entry>, FileError> {
    let path = open_directory(c, directory)?;
    let mut entries = Vec::new();
    let mut seen = 0;
    for item in fs::read_dir(&path).map_err(FileError::from)? {
        let item = item.map_err(FileError::from)?;
        seen += 1;
        if seen > MAX_DIR_ENTRIES {
            return Err(FileError::TooLarge);
        }
        let Ok(name) = item.file_name().into_string() else {
            continue;
        };
        if !valid_component(&name) {
            continue;
        }
        // Not following links: `DirEntry::metadata` describes the entry itself.
        let Ok(metadata) = item.metadata() else {
            continue;
        };
        if is_reparse(&metadata) || c.reserved.covers(&path.join(&name)) {
            continue;
        }
        let directory = metadata.is_dir();
        if !directory && !metadata.is_file() {
            continue;
        }
        entries.push(entry_of(name, directory, &metadata));
    }
    Ok(entries)
}

/// The entry at `path`, or the root itself (named "") for an empty path.
pub(super) fn stat(c: &Confined, path: &[String]) -> Result<Entry, FileError> {
    let target = if path.is_empty() {
        open_directory(c, &[])?
    } else {
        walk(c, path)?
    };
    let metadata = fs::symlink_metadata(&target).map_err(FileError::from)?;
    if is_reparse(&metadata) {
        return Err(FileError::Denied);
    }
    let directory = metadata.is_dir();
    if !directory && !metadata.is_file() {
        return Err(FileError::NotAFile);
    }
    let name = path.last().cloned().unwrap_or_default();
    Ok(entry_of(name, directory, &metadata))
}

/// Whether the current user can open `path` itself with `mask`, which is how the system weighs
/// the ACLs against the user's token. Nothing is written: the handle is closed at once and a
/// share mode of everything keeps it from getting in anyone's way. A sharing violation or any
/// other error is not an answer.
fn probe(path: &Path, mask: u32) -> Tri {
    const ACCESS_DENIED: i32 = 5;
    match OpenOptions::new()
        .access_mode(mask)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(_) => Tri::Allowed,
        Err(error) if error.raw_os_error() == Some(ACCESS_DENIED) => Tri::Denied,
        Err(_) => Tri::Unknown,
    }
}

pub(super) fn access(c: &Confined, path: &[String]) -> Result<AccessInfo, FileError> {
    let entry = stat(c, path)?;
    let directory = entry.directory;
    let target = if path.is_empty() {
        c.root.path.clone()
    } else {
        walk(c, path)?
    };
    let (read, write, create) = if directory {
        let write = probe(&target, FILE_ADD_FILE);
        (probe(&target, FILE_LIST_DIRECTORY), write, write)
    } else {
        (
            probe(&target, FILE_READ_DATA),
            probe(&target, FILE_WRITE_DATA),
            Tri::Unknown,
        )
    };
    // The right to delete comes from the entry itself or from its folder.
    let delete = match probe(&target, DELETE) {
        Tri::Allowed => Tri::Allowed,
        own => match target
            .parent()
            .filter(|_| !path.is_empty())
            .map(|parent| probe(parent, FILE_DELETE_CHILD))
        {
            Some(Tri::Allowed) => Tri::Allowed,
            Some(Tri::Denied) if own == Tri::Denied => Tri::Denied,
            _ => Tri::Unknown,
        },
    };
    Ok(AccessInfo {
        directory,
        read_only: entry.read_only,
        read,
        write,
        create,
        delete,
    })
}

fn open_regular(c: &Confined, path: &Path, options: &mut OpenOptions) -> Result<File, FileError> {
    let file = options
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(FileError::from)?;
    let metadata = file.metadata().map_err(FileError::from)?;
    if is_reparse(&metadata) {
        return Err(FileError::Denied);
    }
    if !metadata.is_file() {
        return Err(FileError::NotAFile);
    }
    verify(c, &file, path)?;
    Ok(file)
}

pub(super) fn read(
    c: &Confined,
    file: &[String],
    offset: u64,
    max: usize,
) -> Result<(Vec<u8>, bool), FileError> {
    let path = walk(c, file)?;
    let file = open_regular(c, &path, OpenOptions::new().read(true))?;
    let len = file.metadata().map_err(FileError::from)?.len();
    let want = max.min(len.saturating_sub(offset) as usize);
    let mut buffer = vec![0; want];
    let mut done = 0;
    while done < want {
        let read = file
            .seek_read(&mut buffer[done..], offset + done as u64)
            .map_err(FileError::from)?;
        if read == 0 {
            break;
        }
        done += read;
    }
    buffer.truncate(done);
    let eof = offset.saturating_add(done as u64) >= len;
    Ok((buffer, eof))
}

pub(super) fn write(
    c: &Confined,
    target: Target<'_>,
    offset: u64,
    contents: &[u8],
    truncate: bool,
) -> Result<u64, FileError> {
    let file = match target {
        Target::Existing(path) => {
            let path = walk(c, path)?;
            open_regular(c, &path, OpenOptions::new().write(true))?
        }
        Target::Create { directory, name } => {
            let parent = open_directory(c, directory)?;
            let path = parent.join(name);
            match open_regular(c, &path, OpenOptions::new().write(true)) {
                Err(FileError::NotFound) => {
                    // A new file starts empty, so a write may only begin at its start; refuse
                    // before creating anything.
                    if offset != 0 {
                        return Err(FileError::OutOfRange);
                    }
                    let created = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .share_mode(SHARE_ALL)
                        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                        .open(&path)
                        .map_err(FileError::from)?;
                    if let Err(error) = verify(c, &created, &path) {
                        drop(created);
                        let _ = fs::remove_file(&path);
                        return Err(error);
                    }
                    created
                }
                other => other?,
            }
        }
    };
    let len = file.metadata().map_err(FileError::from)?.len();
    if offset > len {
        return Err(FileError::OutOfRange);
    }
    let mut written = 0;
    while written < contents.len() {
        let wrote = file
            .seek_write(&contents[written..], offset + written as u64)
            .map_err(FileError::from)?;
        if wrote == 0 {
            return Err(FileError::Io);
        }
        written += wrote;
    }
    let end = offset + contents.len() as u64;
    if truncate {
        file.set_len(end).map_err(FileError::from)?;
        Ok(end)
    } else {
        Ok(len.max(end))
    }
}

pub(super) fn delete(c: &Confined, file: &[String]) -> Result<(), FileError> {
    let path = walk(c, file)?;
    let handle = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(SHARE_ALL)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)
        .map_err(FileError::from)?;
    let metadata = handle.metadata().map_err(FileError::from)?;
    if is_reparse(&metadata) {
        return Err(FileError::Denied);
    }
    if !metadata.is_file() {
        return Err(FileError::NotAFile);
    }
    verify(c, &handle, &path)?;
    // Mark exactly the file that was verified; no path is resolved again.
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `info` outlives the call and its size is passed; the handle is open with DELETE.
    let done = unsafe {
        SetFileInformationByHandle(
            handle_of(&handle),
            FileDispositionInfo,
            std::ptr::addr_of!(info).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if done == 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}
