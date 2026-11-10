//! Per-user data locations.
//!
//! | OS      | Data directory                          |
//! |---------|-----------------------------------------|
//! | Linux   | `$XDG_DATA_HOME/gamenode-desktop` (default `~/.local/share/gamenode-desktop`) |
//! | Windows | `%LOCALAPPDATA%\GameNode\Desktop`       |

use std::path::{Path, PathBuf};

/// File name of the SQLite database inside the data directory.
pub const DATABASE_FILE: &str = "gamenode.sqlite3";
/// Directory of cached game covers, created on first use. Files are named `<game id>.jpg`.
pub const COVERS_DIR: &str = "covers";
/// Directory (mode `0700`) that holds the Linux fallback credential file.
pub const PRIVATE_DIR: &str = "private";
/// File name of the Linux fallback credential file inside [`PRIVATE_DIR`].
pub const CREDENTIAL_FILE: &str = "session-token";
/// Directory inside [`PRIVATE_DIR`] for isolated provider login profiles.
pub const LOGIN_DIR: &str = "login";
/// Directory that holds the application log files.
pub const LOGS_DIR: &str = "logs";
/// Prefix and suffix of a rotated log file: `gamenode.YYYY-MM-DD.log`.
pub const LOG_FILE_PREFIX: &str = "gamenode";
pub const LOG_FILE_SUFFIX: &str = "log";
/// Directory that holds game tools data and cached downloads.
pub const TOOLS_DIR: &str = "tools";
/// Subdirectory inside [`TOOLS_DIR`] for cached release archives.
pub const TOOLS_CACHE_DIR: &str = "cache";
/// Directory inside [`PRIVATE_DIR`] for original file backups before tool modifications.
pub const TOOL_BACKUPS_DIR: &str = "tool-backups";

/// Creates or tightens an owner-only directory (mode `0700`). A symlink or a non-directory is
/// refused instead of followed.
#[cfg(unix)]
fn owner_only_dir(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            if meta.permissions().mode() & 0o077 != 0 {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        }
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::DirBuilder::new().mode(0o700).create(path)
        }
        Err(error) => Err(error),
    }
}

fn validate_provider_component(provider: &str) -> std::io::Result<()> {
    if provider.is_empty() || provider == "." || provider == ".." || provider.contains(['/', '\\'])
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid provider id",
        ));
    }
    Ok(())
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
            std::fs::remove_dir_all(path)
        }
        Ok(_) => std::fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataPaths {
    data_dir: PathBuf,
}

impl DataPaths {
    /// The current user's data directory, or `None` if the OS reports none.
    pub fn resolve() -> Option<Self> {
        let base = dirs::data_local_dir()?;
        Some(Self::in_base(&base))
    }

    /// Paths below an explicit base directory (the OS-specific app folder is appended).
    pub fn in_base(base: &Path) -> Self {
        #[cfg(windows)]
        let data_dir = base.join("GameNode").join("Desktop");
        #[cfg(not(windows))]
        let data_dir = base.join("gamenode-desktop");
        Self { data_dir }
    }

    /// Paths rooted exactly at `data_dir` (tests).
    pub fn at(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn database_file(&self) -> PathBuf {
        self.data_dir.join(DATABASE_FILE)
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.data_dir.join(COVERS_DIR)
    }

    pub fn private_dir(&self) -> PathBuf {
        self.data_dir.join(PRIVATE_DIR)
    }

    pub fn credential_file(&self) -> PathBuf {
        self.private_dir().join(CREDENTIAL_FILE)
    }

    /// The root of the isolated provider login profiles.
    pub fn login_dir(&self) -> PathBuf {
        self.private_dir().join(LOGIN_DIR)
    }

    /// The ephemeral sign-in profile directory of one provider, below [`LOGIN_DIR`].
    pub fn login_profile_dir(&self, provider: &str) -> PathBuf {
        self.login_dir().join(provider)
    }

    /// Creates the provider login profile directory with owner-only permissions (mode `0700` on
    /// Unix), tightening an existing directory when needed.
    pub fn ensure_login_profile_dir(&self, provider: &str) -> std::io::Result<PathBuf> {
        validate_provider_component(provider)?;
        let path = self.login_profile_dir(provider);
        #[cfg(unix)]
        {
            owner_only_dir(&self.private_dir())?;
            owner_only_dir(&self.login_dir())?;
            owner_only_dir(&path)?;
        }
        #[cfg(not(unix))]
        {
            std::fs::create_dir_all(&path)?;
        }
        Ok(path)
    }

    /// Removes one provider's login profile directory and its contents. A missing directory is
    /// success, and a symlink is removed itself, never followed.
    pub fn remove_login_profile_dir(&self, provider: &str) -> std::io::Result<()> {
        validate_provider_component(provider)?;
        remove_path(&self.login_profile_dir(provider))
    }

    /// The directory of rotating application logs.
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join(LOGS_DIR)
    }

    /// The log file name pattern: `gamenode.<date>.log` with one file per UTC day.
    pub fn log_file_name(&self, date: &str) -> String {
        format!("{LOG_FILE_PREFIX}.{date}.{LOG_FILE_SUFFIX}")
    }

    pub fn tools_cache_dir(&self) -> PathBuf {
        self.data_dir.join(TOOLS_DIR).join(TOOLS_CACHE_DIR)
    }

    pub fn tool_backups_dir(&self) -> PathBuf {
        self.private_dir().join(TOOL_BACKUPS_DIR)
    }

    /// Creates the data directory (and parents) if needed.
    pub fn ensure_data_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_follow_the_documented_layout() {
        let paths = DataPaths::in_base(Path::new("/base"));
        #[cfg(not(windows))]
        assert_eq!(paths.data_dir(), Path::new("/base/gamenode-desktop"));
        #[cfg(windows)]
        assert_eq!(
            paths.data_dir(),
            Path::new("/base").join("GameNode").join("Desktop")
        );
        assert_eq!(
            paths.database_file().file_name().unwrap(),
            "gamenode.sqlite3"
        );
        assert_eq!(
            paths.credential_file(),
            paths.private_dir().join("session-token")
        );
        assert_eq!(
            paths.login_profile_dir("steam"),
            paths.private_dir().join("login/steam")
        );
        assert!(paths.credential_file().starts_with(paths.data_dir()));
        assert!(
            paths
                .login_profile_dir("steam")
                .starts_with(paths.data_dir())
        );
        assert_eq!(paths.covers_dir(), paths.data_dir().join("covers"));
        assert_eq!(
            paths.tools_cache_dir(),
            paths.data_dir().join("tools").join("cache")
        );
        assert_eq!(
            paths.tool_backups_dir(),
            paths.private_dir().join("tool-backups")
        );
        assert!(paths.tools_cache_dir().starts_with(paths.data_dir()));
        assert!(paths.tool_backups_dir().starts_with(paths.data_dir()));
    }

    #[test]
    fn a_provider_login_profile_is_owner_only_and_removable() {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = DataPaths::at(dir.path().join("data"));
        let profile = paths.ensure_login_profile_dir("fixture").unwrap();
        assert_eq!(profile, paths.login_profile_dir("fixture"));
        assert!(profile.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&profile).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
            let private = std::fs::metadata(paths.private_dir())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(private, 0o700, "the containing private directory is 0700");
            let login = std::fs::metadata(paths.login_dir())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(login, 0o700, "the login root is 0700");
        }
        std::fs::write(profile.join("Cookies"), "profile data").unwrap();

        paths.remove_login_profile_dir("fixture").unwrap();
        assert!(!paths.login_profile_dir("fixture").exists());
        paths.remove_login_profile_dir("fixture").unwrap(); // already gone
        assert!(paths.ensure_login_profile_dir("../escape").is_err());
        assert!(paths.remove_login_profile_dir("other/provider").is_err());
    }
}
