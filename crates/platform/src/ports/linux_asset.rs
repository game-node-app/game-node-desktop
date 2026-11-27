//! Native Linux asset handling for the PC ports installer (phase 2).
//!
//! Selection lives in `gamenode-core` ([`gamenode_core::ports::plan::select_asset_for`]); this
//! module is the platform half: which Linux formats are placed as an executable and which are
//! extracted, and making a placed executable runnable.

use std::path::Path;

/// How a native Linux asset is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinuxAssetKind {
    /// Placed as the executable itself (AppImage, bare binary).
    Executable,
    /// Extracted into the install folder.
    Archive,
}

/// The install kind for a normalized Linux asset format. An architecture suffix such as
/// `x86_64` names a bare executable, matching the selection rule (design D8).
pub fn kind(format: &str) -> Option<LinuxAssetKind> {
    match format.trim().to_ascii_lowercase().as_str() {
        "appimage" | "bin" => Some(LinuxAssetKind::Executable),
        // Tar is classified here for the platform helper, but selection refuses it until the
        // in-process tar extractor lands.
        "tar" | "tar.gz" | "tgz" | "zip" | "7z" => Some(LinuxAssetKind::Archive),
        other if gamenode_core::ports::plan::BARE_EXECUTABLE_SUFFIXES.contains(&other) => {
            Some(LinuxAssetKind::Executable)
        }
        _ => None,
    }
}

/// Makes a placed Linux executable runnable.
///
/// AppImages and bare binaries arrive from the download cache without the executable bit; the
/// installer sets it before registration so the launch validator sees the real mode.
#[cfg(unix)]
pub fn normalize_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    std::fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
pub fn normalize_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_formats_split_into_executables_and_archives() {
        assert_eq!(kind("AppImage"), Some(LinuxAssetKind::Executable));
        assert_eq!(kind(" bin "), Some(LinuxAssetKind::Executable));
        assert_eq!(kind("x86_64"), Some(LinuxAssetKind::Executable));
        assert_eq!(kind("tar.gz"), Some(LinuxAssetKind::Archive));
        assert_eq!(kind("7z"), Some(LinuxAssetKind::Archive));
        assert_eq!(kind("run"), None);
        assert_eq!(kind("deb"), None);
        assert_eq!(kind(""), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_downloaded_binary_becomes_executable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("port.AppImage");
        std::fs::write(&path, b"binary").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        normalize_executable(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}
