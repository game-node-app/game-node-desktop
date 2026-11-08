//! The PC ports projection the UI reads from `engine:snapshot` (design D5, D10).
//!
//! These types mirror `ui/src/api/ports.ts` exactly: field names are camelCase, timestamps are
//! Unix **seconds** (the catalog worker keeps milliseconds) and the job phase is the UI's flat
//! vocabulary. The app builds the view on a worker thread; the engine only clones it into a
//! snapshot, so a progress tick never rebuilds the catalog.
//!
//! The view is deliberately independent of the platform worker types: `core` cannot depend on
//! `gamenode-platform`, and the app maps the worker's status into [`PortCatalogStatus`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ports::store::PortViewMode;

/// Where the PC ports destination exists. Reported through `AppInfo.platform` as well.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortPlatform {
    Windows,
    Linux,
    Macos,
}

impl PortPlatform {
    /// The platform this build runs on. Matches the UI's fallback vocabulary.
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }

    /// True when port installs are supported (Windows first, Linux deferred).
    pub const fn supports_installs(self) -> bool {
        matches!(self, Self::Windows)
    }
}

impl std::fmt::Display for PortPlatform {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
        })
    }
}

/// Where the catalog is. Mirrors `PortCatalogState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortCatalogState {
    /// PC ports was never opened, so no fetch has run.
    #[default]
    Idle,
    /// A fetch is running; entries may still hold the previous snapshot.
    Refreshing,
    /// The last refresh completed and the snapshot is current.
    Ready,
    /// No usable snapshot exists and the last fetch failed.
    Unavailable,
    /// A snapshot exists, but the last refresh failed or it is older than a day.
    Stale,
}

/// Why the last catalog refresh failed, in the UI's vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortCatalogError {
    Unreachable,
    RateLimited,
    InvalidResponse,
    EmptyListing,
    Unknown,
}

/// The catalog status the sidebar and the catalog header show.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortCatalogStatus {
    pub state: PortCatalogState,
    /// Unix seconds of the last complete stored snapshot; `None` before the first success.
    pub fetched_at: Option<i64>,
    /// Entries in the stored snapshot, before search or filters.
    pub entry_count: usize,
    /// Set while `stale` or `unavailable`; `None` after a successful refresh.
    pub error: Option<PortCatalogError>,
}

/// Whether the artwork cache holds each size. Tiles fall back to a titled placeholder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortArtView {
    pub capsule: bool,
    pub hero: bool,
}

/// The artwork sizes the `port-art` route serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortArtKind {
    Capsule,
    Hero,
}

/// A text-only named link. Quiver logos are never used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortLink {
    pub label: String,
    pub url: String,
}

/// Why an entry that supports Windows offers no installable Windows asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortNoAssetReason {
    InstallerOnly,
    NoWindowsAsset,
    MissingAsset,
    NoExecutable,
}

/// Why Install is unavailable for an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortInstallBlockReason {
    Withdrawn,
    NoVerifiedRelease,
    InstallerOnly,
    NoWindowsAsset,
    MissingAsset,
    NoExecutable,
}

impl From<PortNoAssetReason> for PortInstallBlockReason {
    fn from(reason: PortNoAssetReason) -> Self {
        match reason {
            PortNoAssetReason::InstallerOnly => Self::InstallerOnly,
            PortNoAssetReason::NoWindowsAsset => Self::NoWindowsAsset,
            PortNoAssetReason::MissingAsset => Self::MissingAsset,
            PortNoAssetReason::NoExecutable => Self::NoExecutable,
        }
    }
}

/// What a setup slot accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortSetupKind {
    #[default]
    File,
    Folder,
}

/// Where a supplied file or folder is placed. `game` is the port's own install folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortPlacementRoot {
    Game,
    Data,
    Config,
    LocalData,
}

/// How a supplied selection reaches the port when it is not copied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortArgumentKind {
    Flag,
    Positional,
    Env,
}

/// The flag, positional or environment variable a supplied slot is passed by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortArgumentSpec {
    pub kind: PortArgumentKind,
    /// Flag name including dashes (`--iso`) or the environment variable name.
    pub name: String,
}

/// A pinned console-package extractor offered as an alternative to a folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortPackageOption {
    pub extractor_id: String,
    pub tool_name: String,
    pub tool_source_url: Option<String>,
    pub tool_license: Option<String>,
    pub title_id: Option<String>,
    pub update_version: Option<String>,
    pub requires_update: bool,
}

/// One file or folder the user must supply for guided setup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortSetupSlot {
    pub id: String,
    pub label: String,
    pub prompt: Option<String>,
    pub kind: PortSetupKind,
    pub required: bool,
    /// Lowercase extensions with the dot for `file` slots; empty accepts any file.
    pub extensions: Vec<String>,
    pub contains: Vec<String>,
    pub accepted_labels: Vec<String>,
    pub destination: Option<String>,
    pub root: Option<PortPlacementRoot>,
    pub argument: Option<PortArgumentSpec>,
    pub package_option: Option<PortPackageOption>,
}

/// One merged catalog entry. Only entries supporting Windows are present.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortEntryView {
    pub catalog_id: String,
    pub slug: String,
    pub name: String,
    pub games: Vec<String>,
    pub consoles: Vec<String>,
    #[serde(rename = "supportedOS")]
    pub supported_os: Vec<String>,
    pub verified: bool,
    pub prerelease: bool,
    pub version: Option<String>,
    pub guided: bool,
    pub withdrawn: bool,
    pub has_windows_asset: bool,
    pub no_asset_reason: Option<PortNoAssetReason>,
    pub installable: bool,
    pub install_block_reason: Option<PortInstallBlockReason>,
    pub art: PortArtView,
    pub project: Option<PortLink>,
    pub source: Option<PortLink>,
    pub license: Option<String>,
    pub requirements: Vec<PortSetupSlot>,
    /// Registration key of the library entry this install produced, when installed.
    pub installed_registration_key: Option<String>,
}

/// Provenance for a completed install, keyed by registration key in [`PortsView`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInstallationView {
    pub registration_key: String,
    pub catalog_id: String,
    pub slug: String,
    pub port_name: String,
    pub installed_version: String,
    pub install_folder: String,
    pub job_id: Option<String>,
    /// Unix seconds; the job's finish time, or `None` when it is unknown.
    pub installed_at: Option<i64>,
}

/// The UI's flat job phase vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortJobPhaseView {
    Queued,
    Resolving,
    Downloading,
    Verifying,
    Extracting,
    SuppliedFiles,
    Finalizing,
    Registering,
    Completed,
    Cancelling,
    Cancelled,
    Failed,
    Interrupted,
}

/// A terminal job outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortJobOutcome {
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

/// The description of a failed job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortJobFailure {
    pub message: String,
    /// Capped stdout/stderr of the pinned extractor, for the failure details view.
    pub tool_output: Option<String>,
}

/// One install job, newest first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortJobView {
    pub job_id: String,
    pub catalog_id: String,
    pub slug: String,
    pub name: String,
    pub phase: PortJobPhaseView,
    /// The supplied slot being processed while `phase` is `suppliedFiles`.
    pub slot_id: Option<String>,
    pub outcome: Option<PortJobOutcome>,
    pub failure: Option<PortJobFailure>,
    pub interruptible: bool,
    pub retryable: bool,
    /// True on a completed job whose preferred executable was missing.
    pub needs_executable: bool,
    pub seen: bool,
    /// Unix seconds.
    pub started_at: Option<i64>,
    /// Unix seconds.
    pub finished_at: Option<i64>,
}

/// Catalog filters; they combine with AND.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortFiltersView {
    /// Console names from [`PortEntryView::consoles`]; empty means every console.
    pub consoles: Vec<String>,
    pub guided: bool,
    pub verified: bool,
    pub installed: bool,
}

/// Remembered view state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortPreferencesView {
    pub view: PortViewMode,
    pub filters: PortFiltersView,
    /// Offered first by the parent-folder picker; `None` before the first install.
    pub last_parent_folder: Option<String>,
}

/// `Snapshot.ports`: one payload for the sidebar badge, the catalog and the detail pane.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortsView {
    pub catalog: PortCatalogStatus,
    pub entries: Vec<PortEntryView>,
    /// Provenance by registration key.
    pub provenance: BTreeMap<String, PortInstallationView>,
    /// Newest jobs first.
    pub jobs: Vec<PortJobView>,
    /// Running jobs plus completed, failed or interrupted jobs the user has not opened yet.
    pub unseen_count: usize,
    pub preferences: PortPreferencesView,
}

impl PortsView {
    /// The empty view the snapshot carries before the host delivers a payload.
    pub fn empty() -> Self {
        Self::default()
    }
}

/// One pinned extraction tool as the Settings Tools panel shows it (task 8.1).
///
/// The fields come from the overlay's `[[extractor]]` block; nothing is invented when the overlay
/// has not pinned an extractor yet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortHelperToolView {
    pub id: String,
    pub name: String,
    pub license: String,
    /// The GitHub repository URL derived from the overlay's `owner/repo`, when it names one.
    pub source_url: Option<String>,
    /// The pinned release tag from the overlay.
    pub version: String,
    /// Bytes held by the tool, its download and its extracted copy.
    pub cache_bytes: u64,
}

/// The Tools panel's PC ports cache block, returned by the `ports_tools_cache` command.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortToolsCacheView {
    /// Bytes held by cached release downloads under `<data_dir>/ports/cache`.
    pub downloads_bytes: u64,
    /// The pinned extraction tools, one per overlay entry. Empty while none is authored.
    pub helper_tools: Vec<PortHelperToolView>,
    /// True while a queued, running or executable-awaiting install would use the cache.
    pub install_active: bool,
}

impl PortToolsCacheView {
    /// Total bytes `Clear cache` frees: release downloads plus every pinned extraction tool.
    pub fn total_bytes(&self) -> u64 {
        self.downloads_bytes
            + self
                .helper_tools
                .iter()
                .map(|tool| tool.cache_bytes)
                .sum::<u64>()
    }
}

/// One release with the Windows asset selection and its safety flags.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortReleaseView {
    pub version: String,
    /// ISO-8601 date; `None` when unknown.
    pub released_at: Option<String>,
    pub verified: bool,
    pub withdrawn: bool,
    pub asset_name: Option<String>,
    pub asset_format: Option<String>,
    pub size_bytes: Option<u64>,
    pub checksum_verified: bool,
    pub requires_checksum_confirmation: bool,
    /// Malware-scan warning text; Install asks for a confirmation naming the risk.
    pub malware_flag: Option<String>,
}

/// Returned by `ports_detail`. The host caches it for 15 minutes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortDetailView {
    pub catalog_id: String,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub license: Option<String>,
    pub project: Option<PortLink>,
    pub source: Option<PortLink>,
    pub art: PortArtView,
    pub requirements: Vec<PortSetupSlot>,
    /// Newest releases first.
    pub releases: Vec<PortReleaseView>,
    pub selected_release: Option<PortReleaseView>,
    pub withdrawal_reason: Option<String>,
    /// Newest verified release newer than the installed version, for the update notice.
    pub update_available: Option<PortReleaseView>,
    pub no_asset_reason: Option<PortNoAssetReason>,
}

/// The phases reported on `ports:progress`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PortProgressPhase {
    Downloading,
    Verifying,
    Extracting,
    Hashing,
    ExtractingPackage,
    Copying,
    Finalizing,
    Registering,
}

/// One coalesced progress tick. `job_id` is the install job or the guided-setup session id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortProgress {
    pub job_id: String,
    pub phase: PortProgressPhase,
    pub slot_id: Option<String>,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_view_matches_the_ui_default() {
        let view = PortsView::empty();
        assert_eq!(view.catalog.state, PortCatalogState::Idle);
        assert_eq!(view.preferences.view, PortViewMode::Grid);
        assert!(view.entries.is_empty());
        assert_eq!(view.unseen_count, 0);
        let json = serde_json::to_value(&view).expect("serializable");
        assert_eq!(json["catalog"]["fetchedAt"], serde_json::Value::Null);
        assert_eq!(
            json["preferences"]["lastParentFolder"],
            serde_json::Value::Null
        );
        assert_eq!(json["provenance"], serde_json::json!({}));
    }

    #[test]
    fn view_keys_match_the_ui_contract() {
        let entry = PortEntryView {
            catalog_id: "qcat_x".into(),
            slug: "slug".into(),
            name: "Name".into(),
            games: vec!["Game".into()],
            consoles: vec!["PlayStation 4".into()],
            supported_os: vec!["windows".into()],
            verified: true,
            prerelease: false,
            version: Some("v1".into()),
            guided: true,
            withdrawn: false,
            has_windows_asset: true,
            no_asset_reason: None,
            installable: true,
            install_block_reason: None,
            art: PortArtView {
                capsule: true,
                hero: false,
            },
            project: None,
            source: None,
            license: None,
            requirements: Vec::new(),
            installed_registration_key: None,
        };
        let json = serde_json::to_value(&entry).expect("serializable");
        assert_eq!(json["catalogId"], "qcat_x");
        assert_eq!(json["supportedOS"][0], "windows");
        assert_eq!(json["installBlockReason"], serde_json::Value::Null);

        let job = PortJobView {
            job_id: "job".into(),
            catalog_id: "qcat_x".into(),
            slug: "slug".into(),
            name: "Name".into(),
            phase: PortJobPhaseView::SuppliedFiles,
            slot_id: Some("game".into()),
            outcome: None,
            failure: None,
            interruptible: true,
            retryable: false,
            needs_executable: false,
            seen: false,
            started_at: Some(1),
            finished_at: None,
        };
        let json = serde_json::to_value(&job).expect("serializable");
        assert_eq!(json["jobId"], "job");
        assert_eq!(json["slotId"], "game");
        assert_eq!(json["phase"], "suppliedFiles");
        assert_eq!(json["needsExecutable"], false);
        assert_eq!(json["finishedAt"], serde_json::Value::Null);
    }

    #[test]
    fn platform_capability_is_windows_only() {
        assert!(PortPlatform::Windows.supports_installs());
        assert!(!PortPlatform::Linux.supports_installs());
        assert!(!PortPlatform::Macos.supports_installs());
        assert_eq!(PortPlatform::Linux.to_string(), "linux");
    }
}
