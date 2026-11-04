//! Per-user data locations.
//!
//! | OS      | Data directory                          |
//! |---------|-----------------------------------------|
//! | Linux   | `$XDG_DATA_HOME/gamenode-desktop` (default `~/.local/share/gamenode-desktop`) |
//! | Windows | `%LOCALAPPDATA%\GameNode\Desktop`       |

use std::path::{Path, PathBuf};

/// File name of the SQLite database inside the data directory.
pub const DATABASE_FILE: &str = "gamenode.sqlite3";
/// Directory (mode `0700`) that holds the Linux fallback credential file.
pub const PRIVATE_DIR: &str = "private";
/// File name of the Linux fallback credential file inside [`PRIVATE_DIR`].
pub const CREDENTIAL_FILE: &str = "session-token";

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

    pub fn private_dir(&self) -> PathBuf {
        self.data_dir.join(PRIVATE_DIR)
    }

    pub fn credential_file(&self) -> PathBuf {
        self.private_dir().join(CREDENTIAL_FILE)
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
        assert!(paths.credential_file().starts_with(paths.data_dir()));
    }
}
