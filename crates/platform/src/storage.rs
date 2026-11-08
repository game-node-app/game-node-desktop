//! Disk-space queries for installers.
//!
//! The port planner needs the free bytes of the volume a new install would land on. Blocking;
//! call it from a worker thread.

use std::path::Path;

/// Free bytes available to the current user on the volume that holds `path`.
///
/// `path` may not exist yet: the nearest existing ancestor decides the volume. Returns `None`
/// when the operating system cannot answer; the planner then refuses the install rather than
/// pretending there is room.
pub fn free_space(path: &Path) -> Option<u64> {
    let mut candidate = path;
    while !candidate.exists() {
        candidate = candidate.parent()?;
    }
    free_space_at(candidate)
}

#[cfg(windows)]
fn free_space_at(path: &Path) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut available = 0u64;
    // SAFETY: the path is NUL-terminated and the output pointer is writable for the call.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(available)
}

#[cfg(unix)]
fn free_space_at(path: &Path) -> Option<u64> {
    let stats = rustix::fs::statvfs(path).ok()?;
    Some(stats.f_bavail.saturating_mul(stats.f_frsize))
}

#[cfg(not(any(windows, unix)))]
fn free_space_at(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_directory_reports_some_free_space() {
        let dir = std::env::temp_dir();
        assert!(free_space(&dir).is_some_and(|bytes| bytes > 0));
    }

    #[test]
    fn a_missing_path_walks_up_to_an_existing_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not-there").join("deeper");
        assert!(free_space(&missing).is_some());
    }
}
