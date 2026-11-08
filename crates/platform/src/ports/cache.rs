//! Cache accounting and clearing for PC port downloads and pinned extraction tools.
//!
//! Two folders belong to the install machinery and can be deleted without touching an installed
//! port (design D6, D8):
//!
//! - `<data_dir>/ports/cache/<catalogId>/<version>/<asset>` holds verified release downloads;
//! - `<data_dir>/ports/tools/<id>/<tag>/` holds the pinned extraction tool, its archive and its
//!   extracted copy.
//!
//! An installed port lives in the folder the user chose, and its registration and provenance row
//! live in SQLite, so neither is part of these folders. Anything else under `<data_dir>/ports`
//! (catalog art, for example) is left alone.
//!
//! Everything here is blocking and must run on a worker thread, never on the UI or main thread.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::download::ports_tool_cache_dir;

/// The root of the cached release downloads: `<data_dir>/ports/cache`.
pub fn ports_downloads_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("ports").join("cache")
}

/// The root of the pinned extraction tools: `<data_dir>/ports/tools`.
pub fn ports_tools_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("ports").join("tools")
}

/// Sums the regular files under `dir`, recursively.
///
/// A temporary `.part` file from an interrupted download is ignored, like the game-tools cache
/// does, so reported usage only counts files a retry can reuse. A missing directory is zero.
pub fn directory_usage(dir: &Path) -> io::Result<u64> {
    let mut total = 0;
    accumulate_usage(dir, &mut total)?;
    Ok(total)
}

fn accumulate_usage(dir: &Path, total: &mut u64) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            accumulate_usage(&path, total)?;
        } else if file_type.is_file() && !is_partial(&path) {
            *total += entry.metadata()?.len();
        }
    }
    Ok(())
}

/// The bytes held by the cached release downloads.
pub fn ports_downloads_usage(data_dir: &Path) -> io::Result<u64> {
    directory_usage(&ports_downloads_dir(data_dir))
}

/// The bytes held by one pinned extraction tool, its download and its extracted copy.
pub fn ports_tool_usage(data_dir: &Path, tool_id: &str, tag: &str) -> io::Result<u64> {
    directory_usage(&ports_tool_cache_dir(data_dir, tool_id, tag))
}

/// Removes every downloaded release and pinned extraction tool, returning the freed bytes.
///
/// Only the contents of [`ports_downloads_dir`] and [`ports_tools_dir`] are removed; the roots
/// stay in place so the next install does not recreate them on a missing parent. Nothing outside
/// those two folders is read or written.
pub fn clear_ports_cache(data_dir: &Path) -> io::Result<u64> {
    let mut freed = 0;
    for dir in [ports_downloads_dir(data_dir), ports_tools_dir(data_dir)] {
        freed += clear_directory(&dir)?;
    }
    Ok(freed)
}

fn clear_directory(dir: &Path) -> io::Result<u64> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut freed = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // `DirEntry::metadata` does not follow a symbolic link, and `remove_dir_all` removes the
        // link itself rather than its target. A planted link therefore cannot escape the cache.
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            freed += directory_usage(&path)?;
            fs::remove_dir_all(&path)?;
        } else {
            freed += metadata.len();
            fs::remove_file(&path)?;
        }
    }
    Ok(freed)
}

/// True when a cache path is an unfinished download (`<name>.part.<pid>_<counter>`).
fn is_partial(path: &Path) -> bool {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    file_name.ends_with(".part") || file_name.contains(".part.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    fn usage(dir: &Path) -> Vec<PathBuf> {
        if !dir.exists() {
            return Vec::new();
        }
        let mut found = Vec::new();
        let mut pending = vec![dir.to_owned()];
        while let Some(current) = pending.pop() {
            for entry in fs::read_dir(&current).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    found.push(path);
                }
            }
        }
        found
    }

    #[test]
    fn usage_counts_downloads_but_ignores_unfinished_parts() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(&root.join("ports/cache/qcat/a/one.zip"), b"12345");
        write(&root.join("ports/cache/qcat/a/two.7z"), b"123");
        write(
            &root.join("ports/cache/qcat/a/three.zip.part.9_1"),
            b"unfinished",
        );
        write(&root.join("ports/tools/tool/v1/tool/tool.exe"), b"1234567");

        assert_eq!(ports_downloads_usage(root).unwrap(), 8);
        assert_eq!(ports_tool_usage(root, "tool", "v1").unwrap(), 7);
    }

    #[test]
    fn usage_of_a_missing_cache_is_zero() {
        let dir = TempDir::new().unwrap();
        assert_eq!(ports_downloads_usage(dir.path()).unwrap(), 0);
        assert_eq!(ports_tool_usage(dir.path(), "tool", "v1").unwrap(), 0);
    }

    #[test]
    fn clearing_removes_downloads_and_extractor_tools_but_nothing_else() {
        let dir = TempDir::new().unwrap();
        let data = dir.path();
        write(&data.join("ports/cache/qcat/v1/port.zip"), b"12345");
        write(
            &data.join("ports/tools/ps4-pkg/v1/download/tool.zip"),
            b"123",
        );
        write(
            &data.join("ports/tools/ps4-pkg/v1/tool/tool.exe"),
            b"1234567",
        );
        // Not part of the cache: catalog art and a sibling file under `ports/`.
        write(&data.join("ports/art/qcat/capsule.png"), b"artwork");
        write(&data.join("ports/keep.txt"), b"keep");
        // An installed port lives wherever the user chose.
        let install = data.join("installs/Bloodborne");
        write(&install.join("Bloodborne.exe"), b"installed");

        let freed = clear_ports_cache(data).unwrap();
        assert_eq!(freed, 15);
        assert!(usage(&data.join("ports/cache")).is_empty());
        assert!(usage(&data.join("ports/tools")).is_empty());
        assert_eq!(
            fs::read(data.join("ports/art/qcat/capsule.png")).unwrap(),
            b"artwork"
        );
        assert_eq!(fs::read(data.join("ports/keep.txt")).unwrap(), b"keep");
        assert_eq!(
            fs::read(install.join("Bloodborne.exe")).unwrap(),
            b"installed"
        );
    }

    #[test]
    fn clearing_an_absent_cache_frees_nothing_and_creates_no_folder() {
        let dir = TempDir::new().unwrap();
        assert_eq!(clear_ports_cache(dir.path()).unwrap(), 0);
        assert!(!ports_downloads_dir(dir.path()).exists());
        assert!(!ports_tools_dir(dir.path()).exists());
    }
}
