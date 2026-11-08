//! The TOML provider manifest, as written in `providers/*.toml`.
//!
//! These types only describe the file. Checks that need more than the shape (known scanner,
//! template syntax, duplicate ids) happen in [`super::registry`].

use serde::Deserialize;

/// Operating systems a provider can list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
}

impl Platform {
    /// The platform this build runs on, if the app supports it.
    pub const fn current() -> Option<Self> {
        if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Stable provider id, also used in stored entry keys. Lowercase letters, digits, `-`, `_`.
    pub id: String,
    /// Display name, used for filters and error messages.
    pub name: String,
    pub platforms: Vec<Platform>,
    /// Which store or source the games come from. Kept for later steps.
    pub source: String,
    pub scan: ScanSpec,
    /// A catalog the provider can read beyond what is installed (owned/family games).
    #[serde(default)]
    pub catalog: Option<CatalogSpec>,
    #[serde(default)]
    pub playtime: Option<PlaytimeSpec>,
    #[serde(default)]
    pub launch: Option<UriAction>,
    #[serde(default)]
    pub install: Option<UriAction>,
    #[serde(default, rename = "match")]
    pub matching: Option<MatchSpec>,
    /// Provider-local ids that are never shown (tools, runtimes, redistributables).
    #[serde(default)]
    pub exclude_ids: Vec<String>,
    #[serde(default)]
    pub roots: Roots,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanSpec {
    /// Name of a scanner built into the application.
    #[serde(default)]
    pub native: String,
    #[serde(default)]
    pub plugin: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogSpec {
    /// Name of a catalog backend built into the application.
    #[serde(default)]
    pub native: String,
    #[serde(default)]
    pub plugin: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum UriAction {
    Uri { uri: String },
    Executable { executable: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchSpec {
    /// The GameNode external-game category the provider's ids belong to.
    pub external_category: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roots {
    #[serde(default)]
    pub windows: Vec<RootSpec>,
    #[serde(default)]
    pub linux: Vec<RootSpec>,
}

impl Roots {
    pub fn for_platform(&self, platform: Platform) -> &[RootSpec] {
        match platform {
            Platform::Windows => &self.windows,
            Platform::Linux => &self.linux,
        }
    }
}

/// Either a `path` (a leading `~` is the home folder) or a Windows `registry` key with a `value`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootSpec {
    pub path: Option<String>,
    pub registry: Option<String>,
    pub value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaytimeSpec {
    #[serde(default)]
    pub counter: String,
    #[serde(default)]
    pub plugin: Option<String>,
}
