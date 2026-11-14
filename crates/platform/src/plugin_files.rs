//! File access of third-party plugins, confined beneath one user-approved root.
//!
//! The root is a directory the user chose for one plugin and one game. Everything a plugin names
//! afterwards is a *relative path* of plain components below it, and every operation re-walks
//! that path from the root and refuses anything that would leave it:
//!
//! * a component that is `..`, empty, absolute, holds a separator, a control character or a
//!   drive/stream marker is rejected before the disk is touched;
//! * a symbolic link, junction or other reparse point is never followed, at any depth, so a
//!   directory replaced by a link after a listing is refused when it is used;
//! * a directory on another volume (a mount inside the root) is refused;
//! * the host's own data folders ([`Reserved`]) are never entered, listed or reached, even when
//!   they sit under an otherwise approved parent;
//! * only regular files and directories exist for the plugin: devices, pipes and sockets are not
//!   listed and cannot be opened.
//!
//! On Linux the walk uses `openat` with `O_NOFOLLOW` from a directory handle, so there is no
//! window between checking a component and using it. On Windows each component is checked as a
//! reparse point and the opened handle's final path is verified to lie under the root.
//!
//! Nothing here returns an absolute path. The work is blocking; callers run it on a worker.

use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

/// The most entries read from one directory; a larger directory is refused rather than listed
/// in part.
pub const MAX_DIR_ENTRIES: usize = 20_000;
/// The largest file a plugin may grow through the API.
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// The deepest relative path.
pub const MAX_DEPTH: usize = 64;
/// The longest single name, in bytes.
pub const MAX_NAME_BYTES: usize = 255;

/// Why an operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FileError {
    /// The path leaves the approved area: traversal, a link, another volume, a reserved host
    /// folder or a name that is not allowed.
    #[error("the path is outside the approved access")]
    Denied,
    /// The operating system refused access to something inside the approved area.
    #[error("the operating system refused access")]
    Access,
    #[error("not found")]
    NotFound,
    #[error("not a regular file")]
    NotAFile,
    #[error("not a directory")]
    NotADirectory,
    /// A directory with too many entries, or a file that would grow too large.
    #[error("too large")]
    TooLarge,
    /// A write starting beyond the end of the file.
    #[error("the offset is beyond the end of the file")]
    OutOfRange,
    /// The approved root is gone or is no longer the folder the user approved.
    #[error("the approved folder changed")]
    RootChanged,
    #[error("the file operation failed")]
    Io,
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::PermissionDenied => Self::Access,
            _ => {
                tracing::debug!(%error, "plugin file operation failed");
                Self::Io
            }
        }
    }
}

/// Identifies a directory on disk independently of its path: the volume and the node on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    pub volume: u64,
    pub node: u64,
}

/// Host folders no plugin may reach. Built once from the host's data folders.
#[derive(Clone, Debug, Default)]
pub struct Reserved {
    paths: Vec<PathBuf>,
    ids: Vec<Identity>,
}

impl Reserved {
    /// Reserves each folder that exists (by identity) and by canonical path.
    pub fn new(folders: &[PathBuf]) -> Self {
        let mut reserved = Self::default();
        for folder in folders {
            let canonical = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.clone());
            if let Ok(id) = sys::identity_of(&canonical) {
                reserved.ids.push(id);
            }
            reserved.paths.push(canonical);
        }
        reserved
    }

    fn holds_id(&self, id: Identity) -> bool {
        self.ids.contains(&id)
    }

    /// Whether `canonical` is a reserved folder or lies inside one.
    fn covers(&self, canonical: &Path) -> bool {
        self.paths
            .iter()
            .any(|reserved| canonical.starts_with(reserved))
    }
}

/// A root the user approved: its canonical location and what it was when approved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovedRoot {
    pub path: PathBuf,
    pub identity: Identity,
}

/// Validates a folder the user chose as a root. The path may pass through links the user
/// chose; what is stored is its canonical location. Refused: not a directory, a filesystem
/// root, a path that is not valid Unicode, or one inside a reserved host folder.
pub fn approve_root(chosen: &Path, reserved: &Reserved) -> Result<ApprovedRoot, FileError> {
    let canonical = std::fs::canonicalize(chosen)?;
    if !canonical.is_dir() {
        return Err(FileError::NotADirectory);
    }
    if canonical.parent().is_none() || canonical.to_str().is_none() {
        return Err(FileError::Denied);
    }
    if reserved.covers(&canonical) {
        return Err(FileError::Denied);
    }
    let identity = sys::identity_of(&canonical)?;
    if reserved.holds_id(identity) {
        return Err(FileError::Denied);
    }
    Ok(ApprovedRoot {
        path: canonical,
        identity,
    })
}

/// One entry of a directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
    pub size: u64,
    /// Unix seconds.
    pub modified: i64,
}

/// What a write targets.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a> {
    /// An existing regular file.
    Existing(&'a [String]),
    /// The file `name` inside a directory, created if it does not exist.
    Create {
        directory: &'a [String],
        name: &'a str,
    },
}

/// File access below one approved root.
pub struct Confined {
    root: ApprovedRoot,
    reserved: Reserved,
}

/// Whether `name` is one plain path component.
pub fn valid_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}

/// Whether `name` is allowed for a file the plugin creates: a plain component that is also
/// valid on every platform, so a file made on Linux can be backed up on Windows.
pub fn valid_new_name(name: &str) -> bool {
    if !valid_component(name) || name.ends_with([' ', '.']) {
        return false;
    }
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|digit| {
                matches!(digit, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
        });
    !reserved
}

fn check_path(path: &[String]) -> Result<(), FileError> {
    if path.len() > MAX_DEPTH || !path.iter().all(|part| valid_component(part)) {
        return Err(FileError::Denied);
    }
    Ok(())
}

impl Confined {
    /// Opens the approved root again. Fails with [`FileError::RootChanged`] when the folder is
    /// gone or is no longer the one that was approved (replaced, or another volume mounted
    /// there).
    pub fn open(root: &ApprovedRoot, reserved: &Reserved) -> Result<Self, FileError> {
        let current = sys::identity_of(&root.path).map_err(|_| FileError::RootChanged)?;
        if current != root.identity {
            return Err(FileError::RootChanged);
        }
        let canonical = std::fs::canonicalize(&root.path).map_err(|_| FileError::RootChanged)?;
        if canonical != root.path || reserved.covers(&canonical) {
            return Err(FileError::RootChanged);
        }
        Ok(Self {
            root: root.clone(),
            reserved: reserved.clone(),
        })
    }

    /// The entries of the directory `directory` (relative to the root, empty for the root)
    /// whose names sort after `after`, at most `limit`, and whether more follow.
    pub fn list(
        &self,
        directory: &[String],
        after: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<Entry>, bool), FileError> {
        check_path(directory)?;
        let mut entries = sys::list(self, directory)?;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let mut rest = entries
            .into_iter()
            .filter(|entry| after.is_none_or(|after| entry.name.as_str() > after));
        let page: Vec<Entry> = rest.by_ref().take(limit).collect();
        let more = rest.next().is_some();
        Ok((page, more))
    }

    /// Up to `max` bytes of the file from `offset`, and whether the end of the file was reached.
    pub fn read(
        &self,
        file: &[String],
        offset: u64,
        max: usize,
    ) -> Result<(Vec<u8>, bool), FileError> {
        check_path(file)?;
        if file.is_empty() {
            return Err(FileError::NotAFile);
        }
        sys::read(self, file, offset, max)
    }

    /// Writes `contents` at `offset`; with `truncate` the file ends where the write does. The
    /// offset may not pass the end of the file, and the file may not grow past
    /// [`MAX_FILE_BYTES`]. Returns the new size.
    pub fn write(
        &self,
        target: Target<'_>,
        offset: u64,
        contents: &[u8],
        truncate: bool,
    ) -> Result<u64, FileError> {
        match target {
            Target::Existing(file) => {
                check_path(file)?;
                if file.is_empty() {
                    return Err(FileError::NotAFile);
                }
            }
            Target::Create { directory, name } => {
                check_path(directory)?;
                if !valid_new_name(name) {
                    return Err(FileError::Denied);
                }
            }
        }
        let end = offset
            .checked_add(contents.len() as u64)
            .ok_or(FileError::TooLarge)?;
        if end > MAX_FILE_BYTES {
            return Err(FileError::TooLarge);
        }
        sys::write(self, target, offset, contents, truncate)
    }

    /// Deletes one regular file. Directories are never deleted.
    pub fn delete(&self, file: &[String]) -> Result<(), FileError> {
        check_path(file)?;
        if file.is_empty() {
            return Err(FileError::NotAFile);
        }
        sys::delete(self, file)
    }
}

#[cfg(all(test, unix))]
mod tests;
