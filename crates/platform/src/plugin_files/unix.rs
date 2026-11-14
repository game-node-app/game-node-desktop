//! Linux: every step is `openat` from a directory handle with `O_NOFOLLOW`, so a path is never
//! resolved by the kernel behind our back and a component swapped for a link is refused when it
//! is opened, not merely when it was last looked at.

// `st_dev`, `st_ino` and the like are `u64` here but not on every Unix target; the casts keep this
// portable.
#![allow(clippy::unnecessary_cast)]

use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::Path;

use rustix::fs::{
    Access, AtFlags, Dir, FileType, Mode, OFlags, accessat, fstat, openat, statat, unlinkat,
};
use rustix::io::Errno;

use super::{
    AccessInfo, Confined, Entry, FileError, Identity, MAX_DIR_ENTRIES, Target, Tri, valid_component,
};

pub(super) fn identity_of(path: &Path) -> io::Result<Identity> {
    let metadata = std::fs::metadata(path)?;
    Ok(Identity {
        volume: metadata.dev(),
        node: metadata.ino(),
    })
}

fn errno(error: Errno) -> FileError {
    match error {
        Errno::NOENT => FileError::NotFound,
        Errno::LOOP | Errno::XDEV => FileError::Denied,
        Errno::NOTDIR => FileError::NotADirectory,
        Errno::ISDIR | Errno::NXIO => FileError::NotAFile,
        Errno::ACCESS | Errno::PERM | Errno::ROFS => FileError::Access,
        other => FileError::from(io::Error::from(other)),
    }
}

fn id_of(fd: &OwnedFd) -> Result<Identity, FileError> {
    let stat = fstat(fd).map_err(errno)?;
    Ok(Identity {
        volume: stat.st_dev as u64,
        node: stat.st_ino as u64,
    })
}

const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// The directory `components` below the root, opened one component at a time.
fn walk(c: &Confined, components: &[String]) -> Result<OwnedFd, FileError> {
    let mut dir =
        rustix::fs::open(&c.root.path, DIRECTORY, Mode::empty()).map_err(|error| match error {
            // The folder is there but the user may not open it: not the same as gone or replaced.
            Errno::ACCESS | Errno::PERM => FileError::Access,
            _ => FileError::RootChanged,
        })?;
    if id_of(&dir)? != c.root.identity {
        return Err(FileError::RootChanged);
    }
    for name in components {
        if !valid_component(name) {
            return Err(FileError::Denied);
        }
        let next = match openat(&dir, name.as_str(), DIRECTORY, Mode::empty()) {
            Ok(next) => next,
            // A link, or something that is not a directory: say which without following it.
            Err(Errno::LOOP | Errno::NOTDIR) => {
                return Err(
                    match statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
                        Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::Symlink => {
                            FileError::Denied
                        }
                        Ok(_) => FileError::NotADirectory,
                        Err(error) => errno(error),
                    },
                );
            }
            Err(error) => return Err(errno(error)),
        };
        let id = id_of(&next)?;
        if id.volume != c.root.identity.volume || c.reserved.holds_id(id) {
            return Err(FileError::Denied);
        }
        dir = next;
    }
    Ok(dir)
}

fn split(path: &[String]) -> (&[String], &str) {
    let (name, parent) = path.split_last().expect("callers pass a non-empty path");
    (parent, name.as_str())
}

/// Opens an existing regular file below `dir` without following a link.
fn open_file(c: &Confined, dir: &OwnedFd, name: &str, access: OFlags) -> Result<File, FileError> {
    let fd = openat(
        dir,
        name,
        access | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(errno)?;
    checked(c, fd)
}

fn checked(c: &Confined, fd: OwnedFd) -> Result<File, FileError> {
    let stat = fstat(&fd).map_err(errno)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(FileError::NotAFile);
    }
    if stat.st_dev as u64 != c.root.identity.volume {
        return Err(FileError::Denied);
    }
    Ok(File::from(fd))
}

pub(super) fn list(c: &Confined, directory: &[String]) -> Result<Vec<Entry>, FileError> {
    let dir = walk(c, directory)?;
    let reader = Dir::read_from(&dir).map_err(errno)?;
    let mut entries = Vec::new();
    let mut seen = 0;
    for item in reader {
        let item = item.map_err(errno)?;
        let raw = item.file_name().to_bytes();
        if raw == b"." || raw == b".." {
            continue;
        }
        seen += 1;
        if seen > MAX_DIR_ENTRIES {
            return Err(FileError::TooLarge);
        }
        let Ok(name) = std::str::from_utf8(raw) else {
            continue;
        };
        if !valid_component(name) {
            continue;
        }
        let Ok(stat) = statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) else {
            continue;
        };
        let id = Identity {
            volume: stat.st_dev as u64,
            node: stat.st_ino as u64,
        };
        // Links, devices, pipes and sockets do not exist for a plugin; a mount point or a
        // reserved host folder is not part of the approved area.
        let directory = match FileType::from_raw_mode(stat.st_mode) {
            FileType::Directory => true,
            FileType::RegularFile => false,
            _ => continue,
        };
        if id.volume != c.root.identity.volume || (directory && c.reserved.holds_id(id)) {
            continue;
        }
        entries.push(entry_of(name, directory, &stat));
    }
    Ok(entries)
}

fn entry_of(name: &str, directory: bool, stat: &rustix::fs::Stat) -> Entry {
    Entry {
        name: name.to_owned(),
        directory,
        size: if directory { 0 } else { stat.st_size as u64 },
        modified: stat.st_mtime as i64,
        modified_nanos: stat.st_mtime_nsec as u32,
        read_only: stat.st_mode & 0o222 == 0,
    }
}

/// The entry at `path`, or the root itself (named "") for an empty path.
pub(super) fn stat(c: &Confined, path: &[String]) -> Result<Entry, FileError> {
    if path.is_empty() {
        let root = walk(c, &[])?;
        let stat = fstat(&root).map_err(errno)?;
        return Ok(entry_of("", true, &stat));
    }
    let (parent, name) = split(path);
    let dir = walk(c, parent)?;
    let stat = statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW).map_err(errno)?;
    let directory = match FileType::from_raw_mode(stat.st_mode) {
        FileType::Directory => true,
        FileType::RegularFile => false,
        FileType::Symlink => return Err(FileError::Denied),
        _ => return Err(FileError::NotAFile),
    };
    let id = Identity {
        volume: stat.st_dev as u64,
        node: stat.st_ino as u64,
    };
    if id.volume != c.root.identity.volume || (directory && c.reserved.holds_id(id)) {
        return Err(FileError::Denied);
    }
    Ok(entry_of(name, directory, &stat))
}

/// Asks the kernel what the effective user may do (`faccessat` with `AT_EACCESS`), which weighs
/// the mode bits, the owner and group, ACLs and a read-only mount without opening or changing
/// anything.
fn can(dir: &OwnedFd, name: &str, wanted: Access) -> Tri {
    match accessat(dir, name, wanted, AtFlags::EACCESS) {
        Ok(()) => Tri::Allowed,
        Err(Errno::ACCESS | Errno::PERM | Errno::ROFS) => Tri::Denied,
        Err(_) => Tri::Unknown,
    }
}

pub(super) fn access(c: &Confined, path: &[String]) -> Result<AccessInfo, FileError> {
    let entry = stat(c, path)?;
    let directory = entry.directory;
    // The entry's own directory entry is checked from its parent, the root from itself.
    let (holder, name, parent_path): (OwnedFd, String, Option<&[String]>) = if path.is_empty() {
        (walk(c, &[])?, ".".to_owned(), None)
    } else {
        let (parent, name) = split(path);
        (walk(c, parent)?, name.to_owned(), Some(parent))
    };
    let (read, write, create) = if directory {
        let entered = can(&holder, &name, Access::EXEC_OK);
        let all = |wanted: Access| match (entered, can(&holder, &name, wanted)) {
            (Tri::Allowed, Tri::Allowed) => Tri::Allowed,
            (Tri::Denied, _) | (_, Tri::Denied) => Tri::Denied,
            _ => Tri::Unknown,
        };
        let write = all(Access::WRITE_OK);
        (all(Access::READ_OK), write, write)
    } else {
        (
            can(&holder, &name, Access::READ_OK),
            can(&holder, &name, Access::WRITE_OK),
            Tri::Unknown,
        )
    };
    // Removing an entry needs write and search permission on its folder. The root's folder is
    // outside the approval, and a sticky folder also looks at who owns what: neither is guessed.
    let delete = match parent_path {
        None => Tri::Unknown,
        Some(parent) => {
            let folder = walk(c, parent)?;
            let sticky = fstat(&folder).map_err(errno)?.st_mode & 0o1000 != 0;
            if sticky {
                Tri::Unknown
            } else {
                match (
                    can(&folder, ".", Access::EXEC_OK),
                    can(&folder, ".", Access::WRITE_OK),
                ) {
                    (Tri::Allowed, Tri::Allowed) => Tri::Allowed,
                    (Tri::Denied, _) | (_, Tri::Denied) => Tri::Denied,
                    _ => Tri::Unknown,
                }
            }
        }
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

pub(super) fn read(
    c: &Confined,
    file: &[String],
    offset: u64,
    max: usize,
) -> Result<(Vec<u8>, bool), FileError> {
    let (parent, name) = split(file);
    let dir = walk(c, parent)?;
    let file = open_file(c, &dir, name, OFlags::RDONLY)?;
    let len = file.metadata().map_err(FileError::from)?.len();
    let want = max.min(len.saturating_sub(offset) as usize);
    let mut buffer = vec![0; want];
    let mut done = 0;
    while done < want {
        let read = file
            .read_at(&mut buffer[done..], offset + done as u64)
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
            let (parent, name) = split(path);
            let dir = walk(c, parent)?;
            open_file(c, &dir, name, OFlags::WRONLY)?
        }
        Target::Create { directory, name } => {
            let dir = walk(c, directory)?;
            match open_file(c, &dir, name, OFlags::WRONLY) {
                Err(FileError::NotFound) => {
                    // A new file starts empty, so a write may only begin at its start; refuse
                    // before creating anything.
                    if offset != 0 {
                        return Err(FileError::OutOfRange);
                    }
                    let fd = openat(
                        &dir,
                        name,
                        OFlags::WRONLY
                            | OFlags::CREATE
                            | OFlags::EXCL
                            | OFlags::NOFOLLOW
                            | OFlags::CLOEXEC
                            | OFlags::NONBLOCK,
                        Mode::from_bits_truncate(0o666),
                    )
                    .map_err(errno)?;
                    checked(c, fd)?
                }
                other => other?,
            }
        }
    };
    let len = file.metadata().map_err(FileError::from)?.len();
    if offset > len {
        return Err(FileError::OutOfRange);
    }
    file.write_all_at(contents, offset)
        .map_err(FileError::from)?;
    let end = offset + contents.len() as u64;
    if truncate {
        file.set_len(end).map_err(FileError::from)?;
        Ok(end)
    } else {
        Ok(len.max(end))
    }
}

pub(super) fn delete(c: &Confined, file: &[String]) -> Result<(), FileError> {
    let (parent, name) = split(file);
    let dir = walk(c, parent)?;
    let stat = statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW).map_err(errno)?;
    match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => {}
        FileType::Symlink => return Err(FileError::Denied),
        _ => return Err(FileError::NotAFile),
    }
    if stat.st_dev as u64 != c.root.identity.volume {
        return Err(FileError::Denied);
    }
    unlinkat(&dir, name, AtFlags::empty()).map_err(errno)
}
