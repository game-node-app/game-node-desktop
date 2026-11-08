//! The PC ports catalog model.
//!
//! The model is GameNode's own; the upstream client maps its responses into these types before
//! anything else reads them. A [`CatalogSnapshot`] holds entries exactly as the upstream lists
//! them. The GameNode overlay is applied on top when the entries are read
//! ([`crate::ports::overlay`]), so a new overlay never requires a new catalog fetch.
//!
//! All types are pure data: nothing here performs I/O. Field names follow the upstream document in
//! camel case and every field can be absent, because the upstream is an external service that may
//! change shape at any time.
//!
//! The upstream client maps its responses into these types:
//!
//! | upstream                | here                                |
//! | ----------------------- | ----------------------------------- |
//! | `catalogId`             | [`CatalogEntry::catalog_id`]        |
//! | `supportedOS`           | [`CatalogEntry::supported_os`]      |
//! | `artwork` + `libraryArt` | [`CatalogEntry::art`]               |
//! | `projectName` (listing) | [`CatalogEntry::project`] name only |
//! | `project` (detail)      | [`CatalogEntry::project`] filled in |
//! | `launcher`              | [`CatalogEntry::launcher`]          |
//! | `checking`              | [`CatalogEntry::checking`]          |
//! | `withdrawn`             | [`CatalogEntry::withdrawn`]         |
//! | release `Release`       | [`PortRelease`]                     |
//! | release `Asset`         | [`PortAsset`]                       |

use serde::{Deserialize, Serialize};

/// One catalog entry: a community native PC port or decompilation project.
///
/// The upstream `catalogId` is the stable key that the overlay and the local database use. An entry
/// missing [`CatalogEntry::catalog_id`], [`CatalogEntry::slug`] or [`CatalogEntry::name`] cannot be
/// used and is skipped by the client (design D2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogEntry {
    /// The upstream's stable production id (`qcat_...`).
    pub catalog_id: String,
    pub slug: String,
    /// Display name. The upstream uses the original game's title here (`Bloodborne`).
    pub name: String,
    pub description: String,
    /// The port project. The listing only names it; a detail response adds author, repository and
    /// website.
    pub project: Option<CatalogProject>,
    pub developer: Option<CatalogDeveloper>,
    /// The original games the project is based on.
    pub games: Vec<CatalogGame>,
    /// Console ids such as `ps4`, from the entry's tags and its games' original systems.
    pub consoles: Vec<String>,
    /// Operating system ids the entry runs on: `windows`, `linux`, `macos`, ...
    #[serde(rename = "supportedOS", alias = "supportedOs")]
    pub supported_os: Vec<String>,
    /// Artwork URLs, also accepted under the upstream's `libraryArt` key.
    #[serde(alias = "libraryArt")]
    pub art: CatalogArt,
    /// The newest approved release that is not withdrawn; the release Install offers.
    pub verified: Option<VerifiedRelease>,
    /// A newer release the upstream is checking before it is offered. Only the detail response and
    /// the first releases page have it; never count it as an update.
    pub checking: Option<CheckingRelease>,
    /// Releases the upstream pulled as possibly unsafe. Only the detail response has them.
    pub withdrawn: Vec<WithdrawnRelease>,
    /// The recipe the upstream publishes.
    pub launcher: LauncherRecipe,
    /// The GameNode game id from the overlay (design D11).
    pub gamenode_game_id: Option<u64>,
    /// The licence from the overlay; the upstream listing has no licence field.
    pub license: Option<String>,
    /// An edition note from the overlay, such as `Requires the Director's Cut edition`.
    pub edition: Option<String>,
    /// A release version pinned by the overlay, installed instead of the verified one.
    pub pinned_release: Option<String>,
    /// The launch executable named by the overlay, relative to the install folder.
    pub launcher_executable: Option<String>,
    /// Gameplay executables named by the overlay.
    pub gameplay_executables: Vec<String>,
    /// Extraction tools pinned by the overlay for this entry's package slots.
    pub extractors: Vec<Extractor>,
}

impl CatalogEntry {
    /// True when the upstream says the entry runs on Windows. PC ports lists only these.
    pub fn supports_windows(&self) -> bool {
        self.supported_os
            .iter()
            .any(|os| os.eq_ignore_ascii_case("windows"))
    }

    /// True when every field the catalog needs is present.
    pub fn is_complete(&self) -> bool {
        !self.catalog_id.trim().is_empty()
            && !self.slug.trim().is_empty()
            && !self.name.trim().is_empty()
    }

    /// The original game titles, in upstream order, for detail and search.
    pub fn game_titles(&self) -> impl Iterator<Item = &str> + '_ {
        self.games.iter().map(|game| game.title.as_str())
    }

    /// The merged recipe asks the user for at least one file or folder.
    pub fn has_guided_setup(&self) -> bool {
        !self.launcher.setup_files.is_empty()
    }

    /// The upstream offers an approved release to install.
    pub fn has_verified_release(&self) -> bool {
        self.verified.is_some()
    }

    /// The only release the upstream verified is a pre-release.
    pub fn is_prerelease_only(&self) -> bool {
        self.verified
            .as_ref()
            .is_some_and(|release| release.prerelease)
    }

    /// Executable roles for the DRM-free registration (design D4).
    ///
    /// The overlay launcher becomes both `Registration.executable` and `Registration.launchers`.
    /// Without one, the first preferred Windows executable is the executable and the whole
    /// preferred list is kept as launchers. The installer still has to confirm that the file
    /// exists after extraction; a missing one falls back to asking the user.
    pub fn executable_roles(&self) -> ExecutableRoles {
        let preferred = &self.launcher.preferred_executables.windows;
        match &self.launcher_executable {
            Some(launcher) => ExecutableRoles {
                executable: Some(launcher.clone()),
                launchers: vec![launcher.clone()],
                gameplay: self.gameplay_executables.clone(),
            },
            None => ExecutableRoles {
                executable: preferred.first().cloned(),
                launchers: preferred.clone(),
                gameplay: self.gameplay_executables.clone(),
            },
        }
    }

    /// The extractor pinned for a package slot, when the overlay declared it.
    pub fn extractor(&self, id: &str) -> Option<&Extractor> {
        self.extractors.iter().find(|extractor| extractor.id == id)
    }
}

/// The port project the entry builds on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogProject {
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    /// `owner/repo`, or a full URL.
    pub repository: Option<String>,
    pub website: Option<String>,
    /// The hosting provider: `github`, `gitlab` or `manual`.
    pub provider: Option<String>,
}

impl CatalogProject {
    /// The project page to open in the system browser: the website when there is one, else the
    /// repository, else nothing.
    pub fn source_url(&self) -> Option<String> {
        if let Some(website) = non_empty(&self.website) {
            return Some(website.to_string());
        }
        let repository = non_empty(&self.repository)?;
        if repository.contains("://") {
            return Some(repository.to_string());
        }
        let host = match self.provider.as_deref() {
            Some(provider) if provider.eq_ignore_ascii_case("gitlab") => "gitlab.com",
            _ => "github.com",
        };
        Some(format!("https://{host}/{}", repository.trim_matches('/')))
    }
}

/// Who made the port.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogDeveloper {
    pub key: String,
    pub name: String,
}

/// One of the original games an entry is based on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogGame {
    pub id: String,
    pub slug: String,
    pub title: String,
}

/// Artwork URLs. The asset protocol serves cached copies of them (design D9).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogArt {
    /// The app's own icon, else its game's.
    pub artwork: Option<String>,
    /// A portrait cover.
    pub capsule: Option<String>,
    /// A wide cover.
    pub header: Option<String>,
    /// A text-free banner.
    pub hero: Option<String>,
    /// A transparent title for the hero.
    pub logo: Option<String>,
}

/// The release the upstream verified for the entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VerifiedRelease {
    pub version: String,
    /// Unix milliseconds.
    #[serde(deserialize_with = "deserialize_millis")]
    pub released_at: i64,
    pub prerelease: bool,
    /// Every file has a SHA-256 checksum. When false, downloads cannot be verified.
    pub pinned: bool,
    /// The project rebuilds its releases under the same tag, so the files change without a new
    /// version and are never pinned.
    pub rolling: bool,
}

/// A newer release the upstream is checking before it offers it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CheckingRelease {
    pub version: String,
    /// Unix milliseconds.
    #[serde(deserialize_with = "deserialize_millis")]
    pub released_at: i64,
    pub prerelease: bool,
    pub needs_review: bool,
    /// Why it waits, worded for players.
    pub reasons: Vec<String>,
    pub early_access: bool,
}

/// A release the upstream pulled as possibly unsafe.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WithdrawnRelease {
    pub version: String,
    pub reason: String,
    /// Unix milliseconds.
    #[serde(deserialize_with = "deserialize_millis")]
    pub at: i64,
}

/// The upstream recipe for installing the entry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LauncherRecipe {
    /// The subfolder name the port expects, when it cares.
    pub folder_name: Option<String>,
    /// Files the recipe adds to the install folder, such as `portable.txt`.
    pub files_to_add: Vec<String>,
    /// A case-insensitive part of the asset file name to install. `None` means any Windows asset.
    pub release_asset_filter: Option<String>,
    pub preferred_executables: PreferredExecutables,
    /// Files the player supplies, such as a legally obtained disc image.
    pub setup_files: Vec<SetupFile>,
}

/// Preferred executable names or patterns per operating system, most preferred first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PreferredExecutables {
    pub windows: Vec<String>,
    pub linux: Vec<String>,
    pub macos: Vec<String>,
}

/// A file or folder the port needs from the user's own copy of the game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SetupFile {
    pub id: String,
    pub label: String,
    pub prompt: Option<String>,
    /// Upstream omits the field for a required slot; the default is required.
    pub required: bool,
    /// Upstream omits the field for a file.
    pub kind: SetupKind,
    /// Relative path under [`SetupFile::root`] where the copy goes. Omitted when the port receives
    /// the copy by flag, positional argument or environment variable, or asks for it itself.
    pub destination: Option<String>,
    pub root: Option<CopyRoot>,
    /// Flag the launcher passes the copy's path after. A flag ending in `=` joins them
    /// (`--iso=<path>`).
    pub flag: Option<String>,
    /// Pass the copy's path as an argument of its own, before any flags.
    pub positional: bool,
    /// Environment variable the launcher puts the copy's path in.
    pub env: Option<String>,
    /// For a folder: names it must hold, such as `sce_sys/param.sfo`.
    pub contains: Vec<String>,
    /// File-picker suffixes, without a dot.
    pub extensions: Vec<String>,
    /// Any one hash satisfies the slot. Empty when the upstream publishes none.
    pub accepted: Vec<AcceptedHash>,
    /// Overlay only: which file in a folder the accepted hashes apply to (design D7).
    pub hash_file: Option<String>,
    /// Overlay only: the slot can be produced by extracting console packages (design D8).
    pub from_package: Option<FromPackage>,
}

impl Default for SetupFile {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            prompt: None,
            required: true,
            kind: SetupKind::File,
            destination: None,
            root: None,
            flag: None,
            positional: false,
            env: None,
            contains: Vec::new(),
            extensions: Vec::new(),
            accepted: Vec::new(),
            hash_file: None,
            from_package: None,
        }
    }
}

impl SetupFile {
    /// How GameNode places or passes an accepted choice (design D7).
    pub fn placement(&self) -> Result<Placement, RecipeError> {
        let destination = non_empty(&self.destination);
        let flag = non_empty(&self.flag);
        let env = non_empty(&self.env);
        let placements = usize::from(destination.is_some())
            + usize::from(flag.is_some())
            + usize::from(self.positional)
            + usize::from(env.is_some());
        if placements > 1 {
            return Err(RecipeError::ConflictingPlacement {
                slot: self.id.clone(),
            });
        }
        if let Some(flag) = flag {
            return Ok(Placement::Flag {
                flag: flag.to_string(),
                joined: flag.ends_with('='),
            });
        }
        if self.positional {
            return Ok(Placement::Positional);
        }
        if let Some(env) = env {
            return Ok(Placement::Env {
                name: env.to_string(),
            });
        }
        match destination {
            Some(destination) => Ok(Placement::Copy {
                root: self.root.unwrap_or_default(),
                destination: destination.to_string(),
            }),
            None => Ok(Placement::Unused),
        }
    }

    /// Rejects a slot the installer could not honour. Used for overlay slots at parse time; the
    /// upstream client validates its entries the same way before using them.
    pub fn validate(&self) -> Result<(), RecipeError> {
        if self.id.trim().is_empty() || self.label.trim().is_empty() {
            return Err(RecipeError::Unnamed {
                slot: self.id.clone(),
            });
        }
        self.placement()?;
        if let Some(destination) = non_empty(&self.destination)
            && !is_safe_relative_path(destination)
        {
            return Err(RecipeError::UnsafeDestination {
                slot: self.id.clone(),
                destination: destination.to_string(),
            });
        }
        for marker in &self.contains {
            if !is_safe_relative_path(marker) {
                return Err(RecipeError::UnsafeMarker {
                    slot: self.id.clone(),
                    marker: marker.clone(),
                });
            }
        }
        if let Some(hash_file) = non_empty(&self.hash_file)
            && !is_safe_relative_path(hash_file)
        {
            return Err(RecipeError::UnsafeMarker {
                slot: self.id.clone(),
                marker: hash_file.to_string(),
            });
        }
        if let Some(env) = non_empty(&self.env)
            && !is_valid_env_name(env)
        {
            return Err(RecipeError::InvalidEnvironment {
                slot: self.id.clone(),
                name: env.to_string(),
            });
        }
        if self.from_package.is_some() && self.kind != SetupKind::Folder {
            return Err(RecipeError::PackageOnFile {
                slot: self.id.clone(),
            });
        }
        if let Some(package) = &self.from_package
            && package.extractor.trim().is_empty()
        {
            return Err(RecipeError::Unnamed {
                slot: self.id.clone(),
            });
        }
        for accepted in &self.accepted {
            if !accepted.is_valid() {
                return Err(RecipeError::InvalidHash {
                    slot: self.id.clone(),
                    algo: accepted.algo.label(),
                    bits: accepted.algo.digest_hex_len() * 4,
                    value: accepted.value.clone(),
                });
            }
        }
        Ok(())
    }
}

/// What a setup slot accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupKind {
    /// A single file, such as a ROM or a disc image.
    #[default]
    File,
    /// A folder holding the game's files.
    Folder,
}

/// How an accepted choice reaches the port.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Placement {
    /// Copy into the install (or user data) folder. Never moves the user's files.
    Copy {
        root: CopyRoot,
        /// Relative destination, using `/` or `\` separators.
        destination: String,
    },
    /// Pass the choice's path after a flag. `joined` means the flag ends in `=` and the path follows
    /// it directly.
    Flag { flag: String, joined: bool },
    /// Pass the choice's path as an argument of its own, before any flags.
    Positional,
    /// Put the choice's path in an environment variable, applied at launch (design D10).
    Env { name: String },
    /// The port asks for the choice itself; GameNode only validates it.
    Unused,
}

/// What a copy destination is relative to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CopyRoot {
    /// The port's own install folder.
    #[default]
    App,
    /// The user's roaming application data (`%APPDATA%`).
    Data,
    /// The user's settings folder (`%APPDATA%`).
    Config,
    /// The user's local application data (`%LOCALAPPDATA%`).
    LocalData,
}

/// One hash the upstream publishes for a supplied file or folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedHash {
    pub algo: HashAlgorithm,
    /// Lowercase hex. Deserialization lowercases whatever the source sent.
    #[serde(deserialize_with = "deserialize_lowercase")]
    pub value: String,
    /// Names the dump, such as `CUSA03173 01.09`.
    #[serde(default)]
    pub label: Option<String>,
}

impl AcceptedHash {
    /// True when `value` is a digest of the declared algorithm.
    pub fn is_valid(&self) -> bool {
        self.value.len() == self.algo.digest_hex_len()
            && self.value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    /// The digest in the case the comparison uses.
    pub fn normalized_value(&self) -> String {
        self.value.to_ascii_lowercase()
    }
}

/// The digest algorithms the upstream publishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Sha256,
    Sha1,
}

impl HashAlgorithm {
    /// Length of the digest in hex characters.
    pub fn digest_hex_len(self) -> usize {
        match self {
            Self::Sha256 => 64,
            Self::Sha1 => 40,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
            Self::Sha1 => "sha1",
        }
    }
}

/// A folder slot that console packages can produce (design D8).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FromPackage {
    /// The overlay extractor id.
    pub extractor: String,
    /// The console title id the package must be, such as `CUSA03173`.
    #[serde(alias = "title_id")]
    pub title_id: Option<String>,
    /// The update version to merge, such as `01.09`.
    #[serde(alias = "needs_update")]
    pub needs_update: Option<String>,
}

/// A pinned extraction tool from the overlay.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Extractor {
    pub id: String,
    pub name: String,
    /// `owner/repo` on GitHub.
    pub repository: String,
    pub tag: String,
    /// The release asset file name.
    pub asset: String,
    /// Lowercase hex SHA-256 of the asset.
    pub sha256: String,
    pub license: String,
    /// The executable inside the extracted tool folder.
    pub executable: String,
    /// Argument template. Placeholders such as `{package}` are replaced with validated paths.
    pub args: Vec<String>,
}

impl Extractor {
    /// True when every field the download and run steps rely on is present and well formed.
    pub fn is_valid(&self) -> bool {
        !self.id.trim().is_empty()
            && !self.name.trim().is_empty()
            && !self.repository.trim().is_empty()
            && !self.tag.trim().is_empty()
            && !self.asset.trim().is_empty()
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !self.executable.trim().is_empty()
            && is_safe_relative_path(&self.executable)
    }
}

/// One release of a catalog entry, as fetched for a detail pane or an install.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PortRelease {
    pub release_id: String,
    pub version: String,
    /// Unix milliseconds.
    #[serde(deserialize_with = "deserialize_millis")]
    pub released_at: i64,
    pub notes: Option<String>,
    pub prerelease: bool,
    pub assets: Vec<PortAsset>,
}

/// One downloadable file of a release.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PortAsset {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub os: Option<String>,
    pub architecture: Option<String>,
    pub format: Option<String>,
    #[serde(deserialize_with = "deserialize_bytes")]
    pub size: u64,
    /// `sha256:<hex>` of a verified file.
    pub checksum: Option<String>,
    /// The checksum of a verified file that changed upstream since; confirm before using it.
    pub unverified_checksum: Option<String>,
    pub scan: Option<ReleaseScan>,
}

impl PortAsset {
    /// True when the file is built for Windows. An asset without an operating system is not
    /// assumed to be one.
    pub fn is_windows(&self) -> bool {
        self.os
            .as_deref()
            .is_some_and(|os| os.eq_ignore_ascii_case("windows"))
    }

    /// The verified SHA-256 as lowercase hex, when the checksum is one.
    pub fn verified_sha256(&self) -> Option<String> {
        parse_sha256(self.checksum.as_deref()?)
    }

    /// The unverified SHA-256 as lowercase hex, when the checksum is one.
    pub fn unverified_sha256(&self) -> Option<String> {
        parse_sha256(self.unverified_checksum.as_deref()?)
    }

    /// True when a malware scan flagged the file.
    pub fn is_scan_flagged(&self) -> bool {
        self.scan
            .as_ref()
            .is_some_and(|scan| scan.verdict == ScanVerdict::Flagged)
    }

    /// Ranking of the asset's architecture for the 64-bit Windows build, lowest first. Unknown
    /// architectures rank between 64-bit and the rest so a named build always wins.
    pub fn architecture_rank(&self) -> u8 {
        match self.architecture.as_deref().map(str::to_ascii_lowercase) {
            Some(architecture) if matches!(architecture.as_str(), "x86_64" | "x64" | "amd64") => 0,
            Some(architecture) if matches!(architecture.as_str(), "unknown" | "") => 1,
            None => 1,
            Some(_) => 2,
        }
    }

    /// The file format in lowercase, when the upstream names one.
    pub fn normalized_format(&self) -> Option<String> {
        self.format
            .as_deref()
            .map(|format| format.trim().to_ascii_lowercase())
            .filter(|format| !format.is_empty())
    }
}

/// VirusTotal's verdict on a release file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReleaseScan {
    pub verdict: ScanVerdict,
    /// How many engines call it malicious, as `1 of 70 engines`.
    pub engines: Option<String>,
    pub url: Option<String>,
}

/// The scan verdict the upstream reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanVerdict {
    Clean,
    Warning,
    Flagged,
    Pending,
    #[default]
    Missing,
}

/// The complete catalog listing: upstream entries and when they were fetched.
///
/// The snapshot holds entries before the overlay is merged, so an overlay update is applied without
/// another fetch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogSnapshot {
    /// Unix milliseconds of the last complete, validated fetch.
    pub fetched_at: i64,
    pub entries: Vec<CatalogEntry>,
}

impl CatalogSnapshot {
    pub fn new(fetched_at: i64, entries: Vec<CatalogEntry>) -> Self {
        Self {
            fetched_at,
            entries,
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

/// The executable roles a merged entry gives the DRM-free registration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutableRoles {
    /// `Registration.executable`, relative to the install folder.
    pub executable: Option<String>,
    /// `Registration.launchers`.
    pub launchers: Vec<String>,
    /// `Registration.gameplay`.
    pub gameplay: Vec<String>,
}

/// Why a recipe cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RecipeError {
    #[error("setup file {slot:?} has more than one of destination, flag, positional and env")]
    ConflictingPlacement { slot: String },
    #[error(
        "setup file {slot:?} has a destination that is not a safe relative path: {destination:?}"
    )]
    UnsafeDestination { slot: String, destination: String },
    #[error("setup file {slot:?} has a marker that is not a safe relative path: {marker:?}")]
    UnsafeMarker { slot: String, marker: String },
    #[error("setup file {slot:?} has an invalid environment variable name: {name:?}")]
    InvalidEnvironment { slot: String, name: String },
    #[error(
        "setup file {slot:?} accepts {algo} hash {value:?}, which is not a {bits}-bit hex digest"
    )]
    InvalidHash {
        slot: String,
        algo: &'static str,
        bits: usize,
        value: String,
    },
    #[error("setup file {slot:?} has no id or label")]
    Unnamed { slot: String },
    #[error("setup file {slot:?} is a file slot and cannot be produced from packages")]
    PackageOnFile { slot: String },
}

/// True when `path` is a relative path that cannot escape its base folder.
///
/// Both `/` and `\` count as separators because recipes are authored for Windows. Absolute paths,
/// drive prefixes and any `..` or `.` component are refused.
pub fn is_safe_relative_path(path: &str) -> bool {
    if path.trim().is_empty() || path.chars().any(char::is_control) {
        return false;
    }
    let unified = path.replace('\\', "/");
    if unified.starts_with('/') {
        return false;
    }
    if unified.len() >= 2
        && unified.as_bytes()[1] == b':'
        && unified.as_bytes()[0].is_ascii_alphabetic()
    {
        return false;
    }
    let trimmed = unified.trim_end_matches('/');
    if trimmed.is_empty() {
        return false;
    }
    trimmed
        .split('/')
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// An environment variable name: a letter or underscore, then letters, digits and underscores.
pub fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Parses a `sha256:<hex>` checksum, or a bare 64-character hex digest, into lowercase hex.
fn parse_sha256(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let digest = match trimmed.split_once(':') {
        Some((algo, digest)) if algo.eq_ignore_ascii_case("sha256") => digest,
        Some(_) => return None,
        None => trimmed,
    };
    (digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| digest.to_ascii_lowercase())
}

fn deserialize_lowercase<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(String::deserialize(deserializer)?.to_ascii_lowercase())
}

/// Reads a JSON number that should be an integer, accepting a fractional part.
fn deserialize_millis<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        Integer(i64),
        Float(f64),
    }
    Ok(match Number::deserialize(deserializer)? {
        Number::Integer(value) => value,
        Number::Float(value) => value as i64,
    })
}

/// Reads a JSON number of bytes, accepting a fractional part from the upstream.
fn deserialize_bytes<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        Integer(u64),
        Float(f64),
    }
    Ok(match Number::deserialize(deserializer)? {
        Number::Integer(value) => value,
        Number::Float(value) => value.max(0.0) as u64,
    })
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_upstream_listing_entry_maps_into_the_catalog_model() {
        let entry: CatalogEntry = serde_json::from_str(
            r#"{
                "catalogId": "qcat_x",
                "slug": "bloodborne-bloodborne-pc-windows-port",
                "name": "Bloodborne",
                "description": "A native Windows port.",
                "consoles": ["ps4"],
                "supportedOS": ["windows"],
                "games": [{"id": "g", "slug": "bloodborne", "title": "Bloodborne"}],
                "libraryArt": {
                    "capsule": "https://example.com/capsule.png",
                    "hero": "https://example.com/hero.png"
                },
                "launcher": {
                    "folderName": "Bloodborne-BloodbornePcWindowsPort",
                    "filesToAdd": ["portable.txt"],
                    "releaseAssetFilter": "windows",
                    "preferredExecutables": {"windows": ["Bloodborne.exe"]},
                    "setupFiles": [{
                        "id": "game",
                        "label": "Bloodborne CUSA03173",
                        "kind": "folder",
                        "contains": ["eboot.bin"],
                        "destination": "CUSA03173",
                        "accepted": []
                    }]
                },
                "verified": {
                    "version": "v0.9.38",
                    "releasedAt": 1793220348000,
                    "prerelease": false,
                    "pinned": true
                }
            }"#,
        )
        .expect("upstream listing shape");
        assert_eq!(entry.catalog_id, "qcat_x");
        assert!(entry.supports_windows());
        assert_eq!(entry.game_titles().collect::<Vec<_>>(), vec!["Bloodborne"]);
        assert_eq!(
            entry.art.hero.as_deref(),
            Some("https://example.com/hero.png")
        );
        assert_eq!(
            entry.launcher.release_asset_filter.as_deref(),
            Some("windows")
        );
        assert_eq!(entry.launcher.setup_files.len(), 1);
        assert!(entry.launcher.setup_files[0].required);
        assert_eq!(entry.launcher.setup_files[0].kind, SetupKind::Folder);
        assert_eq!(
            entry.launcher.setup_files[0].placement(),
            Ok(Placement::Copy {
                root: CopyRoot::App,
                destination: "CUSA03173".into()
            })
        );
        assert_eq!(
            entry
                .verified
                .as_ref()
                .map(|release| release.version.as_str()),
            Some("v0.9.38")
        );
    }

    #[test]
    fn windows_support_is_case_insensitive_and_exclusive() {
        let entry = CatalogEntry {
            supported_os: vec!["Linux".into(), "WINDOWS".into()],
            ..CatalogEntry::default()
        };
        assert!(entry.supports_windows());
        let linux_only = CatalogEntry {
            supported_os: vec!["linux".into()],
            ..CatalogEntry::default()
        };
        assert!(!linux_only.supports_windows());
        assert!(!CatalogEntry::default().supports_windows());
    }

    #[test]
    fn complete_entries_need_catalog_id_slug_and_name() {
        let entry = CatalogEntry {
            catalog_id: "qcat_1".into(),
            slug: "slug".into(),
            name: "Name".into(),
            ..CatalogEntry::default()
        };
        assert!(entry.is_complete());
        let mut incomplete = entry;
        incomplete.slug.clear();
        assert!(!incomplete.is_complete());
    }

    #[test]
    fn checksums_are_parsed_only_when_they_are_sha256_digests() {
        let digest = "a".repeat(64);
        let asset = PortAsset {
            checksum: Some(format!("sha256:{}", digest.to_uppercase())),
            unverified_checksum: Some("sha1:abc".into()),
            ..PortAsset::default()
        };
        assert_eq!(asset.verified_sha256(), Some(digest.clone()));
        assert_eq!(asset.unverified_sha256(), None);
        assert_eq!(
            PortAsset {
                checksum: Some(digest.clone()),
                ..PortAsset::default()
            }
            .verified_sha256(),
            Some(digest)
        );
        assert_eq!(
            PortAsset {
                checksum: Some("sha256:nothex".into()),
                ..PortAsset::default()
            }
            .verified_sha256(),
            None
        );
    }

    #[test]
    fn placements_follow_the_upstream_rules() {
        let folder = SetupFile {
            id: "game".into(),
            label: "Game".into(),
            kind: SetupKind::Folder,
            destination: Some("CUSA03173".into()),
            contains: vec!["eboot.bin".into()],
            ..SetupFile::default()
        };
        assert_eq!(
            folder.placement(),
            Ok(Placement::Copy {
                root: CopyRoot::App,
                destination: "CUSA03173".into()
            })
        );
        let flag = SetupFile {
            id: "iso".into(),
            label: "Disc".into(),
            flag: Some("--iso=".into()),
            ..SetupFile::default()
        };
        assert_eq!(
            flag.placement(),
            Ok(Placement::Flag {
                flag: "--iso=".into(),
                joined: true
            })
        );
        let positional = SetupFile {
            id: "rom".into(),
            label: "ROM".into(),
            positional: true,
            ..SetupFile::default()
        };
        assert_eq!(positional.placement(), Ok(Placement::Positional));
        let env = SetupFile {
            id: "env".into(),
            label: "Env".into(),
            env: Some("GAME_ISO".into()),
            ..SetupFile::default()
        };
        assert_eq!(
            env.placement(),
            Ok(Placement::Env {
                name: "GAME_ISO".into()
            })
        );
        let unused = SetupFile {
            id: "pick".into(),
            label: "Picked by the port".into(),
            ..SetupFile::default()
        };
        assert_eq!(unused.placement(), Ok(Placement::Unused));
        let conflicting = SetupFile {
            destination: Some("game".into()),
            flag: Some("--rom".into()),
            ..SetupFile::default()
        };
        assert!(matches!(
            conflicting.placement(),
            Err(RecipeError::ConflictingPlacement { .. })
        ));
    }

    #[test]
    fn slot_validation_rejects_escaping_paths_and_bad_hashes() {
        let slot = SetupFile {
            id: "game".into(),
            label: "Game".into(),
            destination: Some("../Windows".into()),
            ..SetupFile::default()
        };
        assert!(matches!(
            slot.validate(),
            Err(RecipeError::UnsafeDestination { .. })
        ));
        let slot = SetupFile {
            id: "game".into(),
            label: "Game".into(),
            accepted: vec![AcceptedHash {
                algo: HashAlgorithm::Sha256,
                value: "abcd".into(),
                label: None,
            }],
            ..SetupFile::default()
        };
        assert!(matches!(
            slot.validate(),
            Err(RecipeError::InvalidHash { .. })
        ));
        let slot = SetupFile {
            id: "game".into(),
            label: "Game".into(),
            env: Some("1BAD".into()),
            ..SetupFile::default()
        };
        assert!(matches!(
            slot.validate(),
            Err(RecipeError::InvalidEnvironment { .. })
        ));
        let slot = SetupFile {
            id: "game".into(),
            label: "Game".into(),
            contains: vec!["/etc/passwd".into()],
            ..SetupFile::default()
        };
        assert!(matches!(
            slot.validate(),
            Err(RecipeError::UnsafeMarker { .. })
        ));
        let slot = SetupFile {
            id: "game".into(),
            label: "Game".into(),
            kind: SetupKind::Folder,
            destination: Some("CUSA03173".into()),
            contains: vec!["sce_sys/param.sfo".into()],
            accepted: vec![AcceptedHash {
                algo: HashAlgorithm::Sha256,
                value: "A".repeat(64),
                label: Some("v1.09".into()),
            }],
            ..SetupFile::default()
        };
        assert_eq!(slot.validate(), Ok(()));
    }

    #[test]
    fn relative_paths_refuse_escaping_shapes() {
        for safe in [
            "CUSA03173",
            "sce_sys/param.sfo",
            "data\\config.ini",
            "a/b/c.txt",
        ] {
            assert!(is_safe_relative_path(safe), "{safe}");
        }
        for unsafe_path in [
            "",
            "..",
            "../x",
            "a/../x",
            "/etc/passwd",
            "\\Windows\\system32",
            "C:\\Windows",
            "a//b",
            "./a",
        ] {
            assert!(!is_safe_relative_path(unsafe_path), "{unsafe_path}");
        }
    }

    #[test]
    fn extractor_validity_requires_a_pinned_sha256() {
        let mut extractor = Extractor {
            id: "ps4-pkg".into(),
            name: "Tool".into(),
            repository: "owner/repo".into(),
            tag: "v1.0.0".into(),
            asset: "tool-win64.zip".into(),
            sha256: "a".repeat(64),
            license: "GPL-3.0".into(),
            executable: "tool.exe".into(),
            args: vec!["{package}".into()],
        };
        assert!(extractor.is_valid());
        extractor.sha256 = "abc".into();
        assert!(!extractor.is_valid());
    }

    #[test]
    fn numeric_fields_accept_fractional_and_missing_upstream_values() {
        let release: PortRelease = serde_json::from_str(
            r#"{
                "releaseId": "rel",
                "version": "v1.0.0",
                "releasedAt": 1794015373000.0,
                "assets": [{
                    "id": "asset",
                    "url": "https://example.com/a.zip",
                    "filename": "a.zip",
                    "os": "windows",
                    "architecture": "unknown",
                    "format": "zip",
                    "size": 1234.5
                }]
            }"#,
        )
        .expect("tolerant numbers");
        assert_eq!(release.released_at, 1_794_015_373_000);
        assert_eq!(release.assets[0].size, 1234);

        let empty: PortAsset = serde_json::from_str("{}").expect("every field is optional");
        assert!(empty.filename.is_empty());
        assert_eq!(empty.size, 0);
    }

    #[test]
    fn project_source_url_prefers_the_website_then_the_repository() {
        let project = CatalogProject {
            repository: Some("Mrsuss60/bloodborne_pc_windows_port".into()),
            ..CatalogProject::default()
        };
        assert_eq!(
            project.source_url().as_deref(),
            Some("https://github.com/Mrsuss60/bloodborne_pc_windows_port")
        );
        let gitlab = CatalogProject {
            repository: Some("owner/repo".into()),
            provider: Some("gitlab".into()),
            ..CatalogProject::default()
        };
        assert_eq!(
            gitlab.source_url().as_deref(),
            Some("https://gitlab.com/owner/repo")
        );
        let website = CatalogProject {
            website: Some("https://example.com/port".into()),
            repository: Some("owner/repo".into()),
            ..CatalogProject::default()
        };
        assert_eq!(
            website.source_url().as_deref(),
            Some("https://example.com/port")
        );
        assert_eq!(CatalogProject::default().source_url(), None);
    }
}
