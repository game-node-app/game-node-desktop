//! Folder suggestions declared by third-party plugins.
//!
//! A plugin may say where the folder for a game's saves or prefix usually is
//! (`FilesRequestAccess.declared_locations`). The text is untrusted: it is only ever a
//! *suggestion* shown to the user, who confirms or chooses another folder, so everything here is
//! pure parsing and expansion. Nothing is read, listed or opened; the caller checks existence.
//!
//! A location is either an absolute path or starts with one of a closed set of placeholders that
//! the host expands from the system's own folders:
//!
//! | Placeholder | Where | Meaning |
//! | --- | --- | --- |
//! | `{home}` | both | the user's home folder |
//! | `{documents}` | both | the user's Documents folder (Linux: only if XDG defines one) |
//! | `{appdata}` | Windows | the roaming application data folder |
//! | `{localappdata}` | Windows | the local application data folder |
//! | `{xdg_data}` | Linux | `XDG_DATA_HOME` |
//! | `{xdg_config}` | Linux | `XDG_CONFIG_HOME` |
//!
//! A suggestion that cannot apply on this system (a placeholder of the other system, a drive
//! path on Linux, a system folder that does not exist) expands to nothing and is dropped without
//! an error, so one list serves every platform. A location that is not well formed (an unknown
//! placeholder, a relative path, `..`, controls, too long) is an error the plugin is told about.

use std::path::PathBuf;

/// The longest location text, in bytes.
pub const MAX_LOCATION_BYTES: usize = 1024;
/// The most suggestions a plugin may declare for one kind.
pub const MAX_PER_KIND: usize = 8;

/// The system a location is expanded for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Windows,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Why a location is refused. The messages are safe to show the plugin.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LocationError {
    #[error("a location is empty")]
    Empty,
    #[error("a location is longer than {MAX_LOCATION_BYTES} bytes")]
    TooLong,
    #[error("a location contains a control character")]
    Control,
    #[error("a location uses a placeholder that does not exist")]
    UnknownPlaceholder,
    #[error("a location must be an absolute path or start with a placeholder")]
    Relative,
    #[error(
        "a location may not contain `..`, a lone `{{`/`}}` or other than plain names after a placeholder"
    )]
    Malformed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placeholder {
    Home,
    Documents,
    AppData,
    LocalAppData,
    XdgData,
    XdgConfig,
}

impl Placeholder {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "home" => Self::Home,
            "documents" => Self::Documents,
            "appdata" => Self::AppData,
            "localappdata" => Self::LocalAppData,
            "xdg_data" => Self::XdgData,
            "xdg_config" => Self::XdgConfig,
            _ => return None,
        })
    }

    fn exists_on(self, os: Os) -> bool {
        match self {
            Self::Home | Self::Documents => true,
            Self::AppData | Self::LocalAppData => os == Os::Windows,
            Self::XdgData | Self::XdgConfig => os == Os::Linux,
        }
    }
}

/// The system folders placeholders expand to. `None` means the system has no such folder.
pub trait Folders {
    fn home(&self) -> Option<PathBuf>;
    fn documents(&self) -> Option<PathBuf>;
    /// Windows: the roaming application data folder. Linux: `XDG_DATA_HOME`.
    fn data(&self) -> Option<PathBuf>;
    /// Windows: the local application data folder.
    fn local_data(&self) -> Option<PathBuf>;
    /// Linux: `XDG_CONFIG_HOME`.
    fn config(&self) -> Option<PathBuf>;
}

/// The folders of the running system.
pub struct SystemFolders;

impl Folders for SystemFolders {
    fn home(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }
    fn documents(&self) -> Option<PathBuf> {
        dirs::document_dir()
    }
    fn data(&self) -> Option<PathBuf> {
        dirs::data_dir()
    }
    fn local_data(&self) -> Option<PathBuf> {
        dirs::data_local_dir()
    }
    fn config(&self) -> Option<PathBuf> {
        dirs::config_dir()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Root {
    Placeholder(Placeholder),
    /// An absolute path written the way Unix writes it.
    Unix,
    /// An absolute path written the way Windows writes it (a drive or a UNC share).
    Windows(String),
}

/// A location that is well formed. Expanding it is a separate, pure step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    root: Root,
    /// Plain names below the root, separators collapsed, no `.` and no `..`.
    parts: Vec<String>,
}

fn plain_parts(rest: &str) -> Result<Vec<String>, LocationError> {
    let mut parts = Vec::new();
    for part in rest.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(LocationError::Malformed),
            name if name.contains(['{', '}']) => return Err(LocationError::Malformed),
            name => parts.push(name.to_owned()),
        }
    }
    Ok(parts)
}

/// Checks a location's text. Both `/` and `\` separate names.
pub fn parse(location: &str) -> Result<Location, LocationError> {
    if location.is_empty() {
        return Err(LocationError::Empty);
    }
    if location.len() > MAX_LOCATION_BYTES {
        return Err(LocationError::TooLong);
    }
    if location.chars().any(char::is_control) {
        return Err(LocationError::Control);
    }
    if let Some(after) = location.strip_prefix('{') {
        let (name, rest) = after.split_once('}').ok_or(LocationError::Malformed)?;
        let placeholder = Placeholder::parse(name).ok_or(LocationError::UnknownPlaceholder)?;
        if !(rest.is_empty() || rest.starts_with(['/', '\\'])) {
            return Err(LocationError::Malformed);
        }
        return Ok(Location {
            root: Root::Placeholder(placeholder),
            parts: plain_parts(rest)?,
        });
    }
    let bytes = location.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let (root, rest) = if drive {
        (
            Root::Windows(location[..2].to_ascii_uppercase()),
            &location[3..],
        )
    } else if let Some(share) = location.strip_prefix("\\\\") {
        // `\\server\share\...`: the server and share are the first two names.
        (Root::Windows("\\\\".to_owned()), share)
    } else if let Some(rest) = location.strip_prefix('/') {
        (Root::Unix, rest)
    } else {
        return Err(LocationError::Relative);
    };
    if location.contains('{') || location.contains('}') {
        return Err(LocationError::Malformed);
    }
    Ok(Location {
        root,
        parts: plain_parts(rest)?,
    })
}

impl Location {
    /// The absolute path this location means on `os`, or `None` when it does not apply there: a
    /// placeholder or a path of the other system, or a system folder that does not exist. The
    /// result is valid Unicode and is not checked against the disk.
    pub fn expand(&self, os: Os, folders: &dyn Folders) -> Option<PathBuf> {
        let mut path = match &self.root {
            Root::Placeholder(placeholder) => {
                if !placeholder.exists_on(os) {
                    return None;
                }
                match placeholder {
                    Placeholder::Home => folders.home(),
                    Placeholder::Documents => folders.documents(),
                    Placeholder::AppData | Placeholder::XdgData => folders.data(),
                    Placeholder::LocalAppData => folders.local_data(),
                    Placeholder::XdgConfig => folders.config(),
                }?
            }
            Root::Unix if os == Os::Linux => PathBuf::from("/"),
            Root::Windows(root) if os == Os::Windows => {
                if root == "\\\\" {
                    // The first two names are the server and the share.
                    let mut names = self.parts.iter();
                    let (server, share) = (names.next()?, names.next()?);
                    let mut path = PathBuf::from(format!("\\\\{server}\\{share}\\"));
                    path.extend(names);
                    return path.to_str().is_some().then_some(path);
                }
                PathBuf::from(format!("{root}\\"))
            }
            _ => return None,
        };
        path.extend(&self.parts);
        path.to_str().is_some().then_some(path)
    }
}

#[cfg(test)]
mod tests;
