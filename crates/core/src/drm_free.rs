//! OS-neutral DRM-free installations, reviewed discovery and independent save protection.
//! Paths and identities are supplied by worker adapters; this module performs no I/O.
use crate::ports::{StorageError, is_safe_relative_path, join_relative};
pub mod identity;
pub mod scheduler;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub id: String,
    pub title: String,
    pub executable: String,
    #[serde(alias = "file_identity")]
    pub file_identity: String,
    pub root: String,
    #[serde(alias = "working_directory")]
    pub working_directory: String,
    pub arguments: Vec<String>,
    pub gameplay: Vec<String>,
    pub launchers: Vec<String>,
    pub revision: u64,
    pub removed: bool,
    #[serde(default, alias = "rediscovery_allowed")]
    pub rediscovery_allowed: bool,
    #[serde(default, alias = "transferred_from")]
    pub transferred_from: Option<String>,
    /// File name of the user's custom cover in the cover cache, over the GameNode cover.
    #[serde(default, alias = "custom_cover")]
    pub custom_cover: Option<String>,
    pub available: bool,
}
impl Registration {
    pub fn key(&self) -> String {
        format!("manual:{}", self.id)
    }

    /// The process-tracking policy for this registration.
    ///
    /// Registrations written before PC ports recorded absolute executable paths can still hold
    /// `gameplay` and `launchers` relative to the install folder (for example
    /// `BloodbornePC/engine/out/bb-probe.exe`). The tracker compares full process paths, so the
    /// policy resolves them against [`Registration::root`] here; an existing install then tracks
    /// without a reinstall. Entries that are already absolute are unchanged.
    pub fn process_policy(&self) -> ProcessPolicy {
        ProcessPolicy::Explicit {
            gameplay: self
                .gameplay
                .iter()
                .filter_map(|path| resolve_install_path(&self.root, path))
                .collect(),
            launchers: self
                .launchers
                .iter()
                .filter_map(|path| resolve_install_path(&self.root, path))
                .collect(),
        }
    }

    pub fn repair(&mut self, executable: String, identity: String) {
        self.executable = executable;
        self.file_identity = identity;
        self.revision += 1;
        self.available = true;
    }
}

/// Resolves one executable path recorded for a registration against its install folder.
///
/// Paths that are already absolute (drive letter, UNC or Unix root) are returned unchanged. A
/// relative path is joined to `root` only when [`is_safe_relative_path`] accepts it; an unsafe
/// relative path such as `..\escape.exe` is refused instead of being resolved outside the folder.
pub fn resolve_install_path(root: &str, path: &str) -> Option<String> {
    if path.starts_with(['/', '\\']) || crate::provider::path::is_windows_style(path) {
        return Some(path.to_string());
    }
    is_safe_relative_path(path).then(|| join_relative(root, path))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceKind {
    #[serde(alias = "Product")]
    Product,
    #[serde(alias = "Description")]
    Description,
    #[serde(alias = "Executable")]
    Executable,
    #[serde(alias = "Folder")]
    Folder,
    #[serde(alias = "Manifest")]
    Manifest,
    #[serde(alias = "ExternalId")]
    ExternalId,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub kind: EvidenceKind,
    pub value: String,
    pub strong: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inference {
    pub installation: String,
    pub revision: u64,
    pub title: String,
    pub evidence: Vec<Evidence>,
    #[serde(alias = "game_id")]
    pub game_id: Option<u64>,
    pub account: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub fingerprint: String,
    pub watch: String,
    pub root: String,
    pub title: String,
    pub targets: Vec<String>,
    #[serde(alias = "selected_target")]
    pub selected_target: Option<String>,
    pub evidence: Vec<Evidence>,
    pub ignored: bool,
    pub incomplete: bool,
    /// The best GameNode match the review found for this title, at 40% or more.
    #[serde(default)]
    pub suggestion: Option<crate::library::SuggestionView>,
    /// No positive game evidence and the executable or folder name looks like a maintenance tool.
    #[serde(default, alias = "likely_non_game")]
    pub likely_non_game: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchRoot {
    pub id: String,
    pub path: String,
    pub enabled: bool,
    /// Unix seconds of the last completed check of this root, if it has ever been checked.
    #[serde(default)]
    pub last_checked_at: Option<i64>,
    /// Unix seconds when the root was first found unavailable, until it is reachable again.
    #[serde(default)]
    pub unavailable_since: Option<i64>,
}

/// One watched folder as Settings shows it: the stored root plus the live state of its last check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchView {
    pub id: String,
    pub path: String,
    pub enabled: bool,
    /// Library entries whose installation root is inside this folder.
    pub entry_count: u32,
    /// Unix seconds of the last completed check of this folder.
    pub last_checked_at: Option<i64>,
    pub state: WatchState,
    /// Unix seconds when the folder became unavailable, while it is.
    pub unavailable_since: Option<i64>,
}

/// What a watched folder is doing right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchState {
    #[default]
    Idle,
    Checking,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ProcessPolicy {
    Ignored,
    Folder,
    Explicit {
        gameplay: Vec<String>,
        launchers: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveSupport {
    #[serde(alias = "Supported")]
    Supported,
    #[serde(alias = "NeedsSelection")]
    NeedsSelection,
    #[serde(alias = "Unsupported")]
    Unsupported,
    #[serde(alias = "NoSaves")]
    NoSaves,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProfile {
    pub id: String,
    pub installation: String,
    pub revision: u64,
    pub title: String,
    #[serde(alias = "manifest_hash")]
    pub manifest_hash: String,
    pub definition: String,
    pub files: Vec<String>,
    pub registry: Vec<String>,
    pub support: SaveSupport,
    pub automatic: Option<bool>,
    /// Per-installation/profile retention override; `None` follows the setting default.
    #[serde(default, alias = "retention")]
    pub retention: Option<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    #[serde(alias = "Queued")]
    Queued,
    #[serde(alias = "Running")]
    Running,
    #[serde(alias = "Unchanged")]
    Unchanged,
    #[serde(alias = "Successful")]
    Successful,
    #[serde(alias = "Partial")]
    Partial,
    #[serde(alias = "Failed")]
    Failed,
    #[serde(alias = "Cancelled")]
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupJob {
    pub id: String,
    pub installation: String,
    pub profile: String,
    pub revision: u64,
    #[serde(alias = "play_period")]
    pub play_period: Option<String>,
    pub destination: String,
    pub state: JobState,
    pub attempts: u8,
    #[serde(alias = "ready_at")]
    pub ready_at: i64,
    #[serde(alias = "backup_id")]
    pub backup_id: Option<String>,
    pub error: Option<String>,
    /// Length of the play session that queued this job, when it was an automatic backup.
    #[serde(default, alias = "play_seconds")]
    pub play_seconds: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSettings {
    pub automatic: bool,
    pub destination: String,
    /// Default full-version count for installations without a per-profile override.
    #[serde(default = "default_retention", alias = "retention")]
    pub retention: u8,
}

/// Allowed full-version retention counts (`5 | 10 | 20 | 50`).
pub const RETENTION_OPTIONS: [u8; 4] = [5, 10, 20, 50];
/// Retention used by new installations and by legacy settings.
pub const DEFAULT_RETENTION: u8 = 10;

/// Space a game's local backups use, recovery snapshots included.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUsage {
    pub total_bytes: u64,
    pub total_versions: u32,
    /// One entry per game with archives on disk, largest first.
    pub games: Vec<BackupUsageGame>,
}

/// One game's share of the backup space.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupUsageGame {
    /// The library key, `manual:<installation id>`.
    pub entry_key: String,
    /// The last known title, including for a game that was removed.
    pub title: String,
    pub cover_file: Option<String>,
    pub bytes: u64,
    pub versions: u32,
}

/// The default retention value; also used as the serde fallback for legacy settings.
pub fn default_retention() -> u8 {
    DEFAULT_RETENTION
}

/// Whether a retention value is one of the user-selectable counts.
pub fn valid_retention(value: u8) -> bool {
    RETENTION_OPTIONS.contains(&value)
}
impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            automatic: true,
            destination: String::new(),
            retention: DEFAULT_RETENTION,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LocalError {
    #[error("this operation is unavailable on this platform")]
    Unsupported,
    #[error("another local operation is already running")]
    Busy,
    #[error("invalid executable or game folder")]
    InvalidTarget,
    #[error("operation cancelled")]
    Cancelled,
    #[error("local operation failed: {0}")]
    Failed(String),
}
/// Environment variables a port's provenance adds to its launch (design D7).
///
/// Provenance lives beside the registration, never inside it (design D10), so the launch call
/// carries the map instead of `Registration`. Recipe arguments keep their order: the user's own
/// arguments follow them.
pub type LaunchEnvironment = std::collections::BTreeMap<String, String>;
/// Explicit launch-time environment changes. Unset keys override `set` validation and may not
/// overlap it; inherited variables stay untouched unless named here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchEnvironmentDelta {
    pub set: LaunchEnvironment,
    pub unset: std::collections::BTreeSet<String>,
}

impl LaunchEnvironmentDelta {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.unset.is_empty()
    }

    pub fn validate(&self) -> bool {
        let valid_key = |key: &str| {
            let upper = key.to_ascii_uppercase();
            !key.is_empty()
                && key.len() <= 256
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                && key
                    .as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
                && !key.contains('=')
                && !key.chars().any(|ch| ch == '\0' || ch.is_control())
                && ![
                    "TOKEN",
                    "COOKIE",
                    "SECRET",
                    "PASSWORD",
                    "CREDENTIAL",
                    "AUTHORIZATION",
                    "SESSION",
                    "API_KEY",
                    "ACCESS_KEY",
                    "PRIVATE_KEY",
                ]
                .iter()
                .any(|sensitive| upper.contains(sensitive))
        };
        let count = self.set.len() + self.unset.len();
        let bytes = self
            .set
            .iter()
            .map(|(key, value)| key.len().saturating_add(value.len()))
            .chain(self.unset.iter().map(String::len))
            .sum::<usize>();
        let names: std::collections::BTreeSet<_> = self
            .set
            .keys()
            .chain(self.unset.iter())
            .map(|key| key.to_ascii_lowercase())
            .collect();
        count <= 128
            && bytes <= 64 * 1024
            && names.len() == count
            && self.set.iter().all(|(key, value)| {
                valid_key(key) && !value.chars().any(|ch| ch == '\0') && !self.unset.contains(key)
            })
            && self.unset.iter().all(|key| valid_key(key))
    }
}

/// Fully resolved executable command passed by a trusted host adapter. Plugin-facing opaque
/// handles must be resolved and approved before one of these values is constructed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomLaunchCommand {
    pub executable: String,
    pub file_identity: String,
    pub arguments: Vec<String>,
    pub working_directory: String,
    pub environment: LaunchEnvironmentDelta,
}
/// Every method may block and must run on a worker.
pub trait InstallationHost: Send + Sync {
    fn inspect(&self, executable: &str) -> Result<Registration, LocalError>;
    fn launch(&self, registration: &Registration) -> Result<(), LocalError>;
    /// Launches with the environment variables a port's provenance records (design D7).
    ///
    /// `Registration` stays unchanged, so stored registrations keep their shape (design D10). A
    /// host that cannot set an environment launches without it, which keeps existing adapters
    /// working.
    fn launch_with_environment(
        &self,
        registration: &Registration,
        environment: &LaunchEnvironment,
    ) -> Result<(), LocalError> {
        let _ = environment;
        self.launch(registration)
    }
    /// Executes a host-resolved custom command. Adapters that cannot apply every field must
    /// reject it explicitly instead of silently dropping arguments, cwd or environment changes.
    fn launch_custom(&self, _command: &CustomLaunchCommand) -> Result<(), LocalError> {
        Err(LocalError::Unsupported)
    }
    fn validate_registration(
        &self,
        registration: &Registration,
    ) -> Result<Registration, LocalError> {
        Ok(registration.clone())
    }
    fn evidence(&self, registration: &Registration) -> Result<Vec<Evidence>, LocalError>;
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryReport {
    pub candidates: Vec<Candidate>,
    pub incomplete: bool,
}
pub trait DiscoveryHost: Send + Sync {
    fn discover(
        &self,
        watch: &WatchRoot,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<DiscoveryReport, LocalError>;
}
/// Global Windows-user records; catalog inferences alone are account scoped.
pub trait LocalGameStore: Send + Sync {
    fn registrations(&self) -> Result<Vec<Registration>, StorageError>;
    fn save_registration(&self, value: &Registration) -> Result<(), StorageError>;
    fn save_registration_if_revision(
        &self,
        value: &Registration,
        revision: u64,
    ) -> Result<bool, StorageError> {
        if self
            .registrations()?
            .iter()
            .any(|r| r.id == value.id && r.revision == revision && !r.removed)
        {
            self.save_registration(value)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn watches(&self) -> Result<Vec<WatchRoot>, StorageError>;
    fn save_watch(&self, value: &WatchRoot) -> Result<(), StorageError>;
    fn candidates(&self) -> Result<Vec<Candidate>, StorageError>;
    fn save_candidate(&self, value: &Candidate) -> Result<(), StorageError>;
    /// Records a review suggestion on an existing candidate without touching its other fields.
    fn save_candidate_suggestion(
        &self,
        fingerprint: &str,
        suggestion: Option<&crate::library::SuggestionView>,
    ) -> Result<(), StorageError>;
    /// Forgets a discovery so a later scan can propose it again.
    fn delete_candidate(&self, fingerprint: &str) -> Result<(), StorageError>;
    fn inferences(&self, account: &str) -> Result<Vec<Inference>, StorageError>;
    fn save_inference(&self, value: &Inference) -> Result<(), StorageError>;
    fn profiles(&self) -> Result<Vec<SaveProfile>, StorageError>;
    fn save_profile(&self, value: &SaveProfile) -> Result<(), StorageError>;
    fn jobs(&self) -> Result<Vec<BackupJob>, StorageError>;
    fn save_job(&self, value: &BackupJob) -> Result<(), StorageError>;
    fn backup_settings(&self) -> Result<BackupSettings, StorageError>;
    fn save_backup_settings(&self, value: &BackupSettings) -> Result<(), StorageError>;
    /// Recorded origins for a game's listed versions.
    fn backup_version_meta(
        &self,
        installation: &str,
    ) -> Result<Vec<BackupVersionMeta>, StorageError>;
    fn save_backup_version_meta(&self, value: &BackupVersionMeta) -> Result<(), StorageError>;
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BackupOperation {
    Preview,
    Backup,
    History,
    RestorePreview { backup: String },
    Restore { backup: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRequest {
    pub job_id: Option<String>,
    pub profile: SaveProfile,
    pub installation: Registration,
    pub destination: String,
    pub operation: BackupOperation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupReport {
    pub state: JobState,
    pub json: String,
}

/// Host-resolved backup context. It describes a real installation of any provider without
/// registering it as a manual game or changing its library identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupTarget {
    pub installation_id: String,
    pub root: String,
    pub gameplay: Vec<String>,
    /// Store installations use folder containment; manual games keep explicit gameplay paths.
    pub track_folder: bool,
}

/// A plugin backup uses a host-derived namespace, an approved save profile and a frozen target.
/// Callers of this internal port must never take paths or namespaces from the plugin request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopedBackupRequest {
    pub owner: String,
    pub target: BackupTarget,
    pub job_id: Option<String>,
    pub profile: SaveProfile,
    pub destination: String,
    pub operation: BackupOperation,
}

/// Worker-side checks, including after previews and while the helper runs. Restore targets must
/// be checked against current approved roots/rules before recovery or replacement writes.
pub trait BackupAuthority: Send + Sync {
    fn check(&self) -> Result<(), LocalError>;
    fn validate_targets(&self, files: &[String], registry: &[String]) -> Result<(), LocalError>;
}

pub trait BackupEngine: Send + Sync {
    fn execute(
        &self,
        request: &BackupRequest,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<BackupReport, LocalError>;

    fn scoped_available(&self) -> bool {
        false
    }

    fn execute_scoped(
        &self,
        _request: &ScopedBackupRequest,
        _cancelled: &std::sync::atomic::AtomicBool,
        _authority: &dyn BackupAuthority,
    ) -> Result<BackupReport, LocalError> {
        Err(LocalError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, atomic::AtomicBool};
    struct ScriptedBackup(Mutex<Vec<Result<BackupReport, LocalError>>>);
    impl BackupEngine for ScriptedBackup {
        fn execute(
            &self,
            _: &BackupRequest,
            cancelled: &AtomicBool,
        ) -> Result<BackupReport, LocalError> {
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(LocalError::Cancelled);
            }
            self.0.lock().unwrap().remove(0)
        }
    }
    #[test]
    fn backup_settings_default_and_legacy_payload_use_ten_versions() {
        assert_eq!(BackupSettings::default().retention, DEFAULT_RETENTION);
        assert_eq!(
            serde_json::from_str::<BackupSettings>(r#"{"automatic":false,"destination":"D:/b"}"#)
                .unwrap()
                .retention,
            10
        );
        for value in RETENTION_OPTIONS {
            assert!(valid_retention(value));
        }
        for value in [0, 1, 6, 9, 11, 25, 100, 255] {
            assert!(!valid_retention(value), "{value} is not selectable");
        }
    }
    #[test]
    fn backup_version_serializes_origin_and_play_length() {
        let version = BackupVersion {
            installation: "game".into(),
            profile: "profile".into(),
            destination: "D:/b".into(),
            id: "b1".into(),
            label: "2026-11-15 14:20:00".into(),
            bytes: 7_100_000,
            created_at: Some(1_763_216_400),
            reason: Some(BackupReason::AfterSession),
            play_seconds: Some(42 * 60),
        };
        let json = serde_json::to_value(&version).unwrap();
        assert_eq!(json["reason"], "afterSession");
        assert_eq!(json["playSeconds"], 2520);
        assert_eq!(json["createdAt"], 1_763_216_400_i64);
        let legacy: BackupVersion = serde_json::from_str(
            r#"{"installation":"g","profile":"p","destination":"D:/b","id":"b","label":"when","bytes":1}"#,
        )
        .unwrap();
        assert_eq!(legacy.created_at, None);
        assert_eq!(legacy.reason, None);
        assert_eq!(legacy.play_seconds, None);
    }
    #[test]
    fn local_models_and_scripted_engine_need_no_os() {
        let mut registration = Registration {
            id: "stable".into(),
            title: "Game".into(),
            executable: "old.exe".into(),
            file_identity: "one".into(),
            root: "game".into(),
            working_directory: "game".into(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            custom_cover: None,
            available: false,
        };
        registration.repair("new.exe".into(), "two".into());
        assert_eq!(registration.key(), "manual:stable");
        assert_eq!(registration.revision, 2);
        let profile = SaveProfile {
            id: "p".into(),
            installation: "stable".into(),
            revision: 1,
            title: "Game".into(),
            manifest_hash: "snapshot".into(),
            definition: String::new(),
            files: vec![],
            registry: vec![],
            support: SaveSupport::NoSaves,
            automatic: None,
            retention: None,
        };
        let request = BackupRequest {
            job_id: None,
            profile,
            installation: registration,
            destination: "local".into(),
            operation: BackupOperation::Backup,
        };
        let engine = ScriptedBackup(Mutex::new(vec![Ok(BackupReport {
            state: JobState::Unchanged,
            json: "{}".into(),
        })]));
        assert_eq!(
            engine.execute(&request, &AtomicBool::new(true)),
            Err(LocalError::Cancelled)
        );
        assert_eq!(
            engine
                .execute(&request, &AtomicBool::new(false))
                .unwrap()
                .state,
            JobState::Unchanged
        );
    }

    #[test]
    fn a_host_without_environment_support_still_launches() {
        struct CountingHost(Mutex<usize>);
        impl InstallationHost for CountingHost {
            fn inspect(&self, _: &str) -> Result<Registration, LocalError> {
                unreachable!()
            }
            fn launch(&self, _: &Registration) -> Result<(), LocalError> {
                *self.0.lock().unwrap() += 1;
                Ok(())
            }
            fn evidence(&self, _: &Registration) -> Result<Vec<Evidence>, LocalError> {
                unreachable!()
            }
        }
        let registration = Registration {
            id: "stable".into(),
            title: "Game".into(),
            executable: "game.exe".into(),
            file_identity: "one".into(),
            root: "game".into(),
            working_directory: "game".into(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            custom_cover: None,
            available: true,
        };
        let host = CountingHost(Mutex::new(0));
        assert!(
            host.launch_with_environment(
                &registration,
                &LaunchEnvironment::from([("PORT_DATA".into(), "C:/data".into())]),
            )
            .is_ok()
        );
        assert_eq!(*host.0.lock().unwrap(), 1);
    }
}

/// Project approved records only; candidates never enter this interface.
pub fn records(registrations: &[Registration]) -> Vec<crate::provider::Record> {
    registrations
        .iter()
        .filter(|r| !r.removed)
        .map(|r| crate::provider::Record {
            id: r.id.clone(),
            name: Some(r.title.clone()),
            install_dir: r.root.clone(),
            fields: [
                ("available".into(), r.available.to_string()),
                ("executable".into(), r.executable.clone()),
            ]
            .into(),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupReason {
    AfterSession,
    Manual,
    Recovery,
}
impl BackupReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AfterSession => "afterSession",
            Self::Manual => "manual",
            Self::Recovery => "recovery",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "afterSession" => Some(Self::AfterSession),
            "manual" => Some(Self::Manual),
            "recovery" => Some(Self::Recovery),
            _ => None,
        }
    }
}
/// Persisted origin of one listed backup version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupVersionMeta {
    pub installation: String,
    pub profile: String,
    #[serde(alias = "backup_id")]
    pub backup_id: String,
    pub reason: BackupReason,
    #[serde(default, alias = "play_seconds")]
    pub play_seconds: Option<u32>,
    #[serde(alias = "created_at")]
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupVersion {
    pub installation: String,
    pub profile: String,
    pub destination: String,
    pub id: String,
    pub label: String,
    pub bytes: u64,
    /// Unix seconds from the archive time (or the recorded meta time as fallback).
    #[serde(default, alias = "created_at")]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub reason: Option<BackupReason>,
    #[serde(default, alias = "play_seconds")]
    pub play_seconds: Option<u32>,
}
impl BackupVersion {
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.profile.len(),
            self.profile,
            self.id.len(),
            self.id,
            self.destination
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePlan {
    pub request: BackupRequest,
    pub targets: Vec<String>,
}

/// The store entry a tracking conflict was found with, as the conflict dialog shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimStoreEntry {
    pub title: String,
    pub provider: String,
    #[serde(default, alias = "cover_file")]
    pub cover_file: Option<String>,
    #[serde(default, alias = "playtime_seconds")]
    pub playtime_seconds: Option<i64>,
}

/// The DRM-free registration a tracking conflict would create, as the dialog shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRegistration {
    pub title: String,
    pub root: String,
    #[serde(default, alias = "cover_file")]
    pub cover_file: Option<String>,
}

/// A DRM-free registration that would track a game a store entry already tracks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingClaim {
    /// The library key the conflict is with.
    pub store_key: String,
    pub store_entry: ClaimStoreEntry,
    pub registration: ClaimRegistration,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalView {
    pub pending_claim: Option<PendingClaim>,
    pub inferences: Vec<Inference>,
    pub recently_added: Vec<String>,
    pub discovery_incomplete: Vec<String>,
    pub discovery_scanning: bool,
    pub discovery_completed: bool,
    pub discovery_error: Option<String>,
    /// Unix seconds of the last completed watched-folder check, kept across restarts.
    pub discovery_last_checked_at: Option<i64>,
    pub watches: Vec<WatchView>,
    pub review_selected: Vec<String>,
    pub versions: Vec<BackupVersion>,
    /// Sum of `versions` bytes, recovery snapshots included.
    #[serde(default, alias = "history_bytes")]
    pub history_bytes: u64,
    /// Number of listed versions, recovery snapshots included.
    #[serde(default, alias = "history_count")]
    pub history_count: u32,
    pub restore: Option<RestorePlan>,
    pub busy: bool,
    pub error: Option<String>,
    pub registrations: Vec<Registration>,
    pub candidates: Vec<Candidate>,
    pub profiles: Vec<SaveProfile>,
    pub jobs: Vec<BackupJob>,
    pub settings: BackupSettings,
}

pub type LocalGameSummary = Registration;
pub type LocalEditorView = Registration;
pub type CandidateExeView = Candidate;
pub type SaveProtectionView = SaveProfile;
pub type SavePathStatusView = String;
pub type SaveRegistryStatusView = String;
pub type BackupHistoryEntryView = BackupVersion;
pub type RestorePreviewView = RestorePlan;
pub type BackupJobStateView = JobState;
pub type DiscoveryModalView = DiscoveryReport;
pub type DiscoveredCandidateView = Candidate;
pub type ClaimConflictView = PendingClaim;
pub type RemoveLocalModalView = Registration;
pub type LocalBackupSettings = BackupSettings;

/// Windows CRT argument rules, without a program-name prefix. Unbalanced quotes are rejected.
pub fn parse_arguments(text: &str) -> Result<Vec<String>, LocalError> {
    if text.contains('\0') {
        return Err(LocalError::InvalidTarget);
    }
    let mut chars = text.chars().peekable();
    let mut args = Vec::new();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }
        let mut arg = String::new();
        let mut quoted = false;
        while let Some(c) = chars.peek().copied() {
            if c.is_whitespace() && !quoted {
                break;
            }
            if c == '\\' {
                let mut count = 0;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    count += 1;
                }
                if chars.peek() == Some(&'"') {
                    arg.extend(std::iter::repeat_n('\\', count / 2));
                    chars.next();
                    if count % 2 == 1 {
                        arg.push('"');
                    } else {
                        quoted = !quoted;
                    }
                } else {
                    arg.extend(std::iter::repeat_n('\\', count));
                }
            } else {
                chars.next();
                if c == '"' {
                    quoted = !quoted;
                } else {
                    arg.push(c);
                }
            }
        }
        if quoted {
            return Err(LocalError::Failed("unclosed argument quote".into()));
        }
        args.push(arg);
    }
    Ok(args)
}
pub fn format_arguments(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|a| {
            let mut result = String::from("\"");
            let mut slashes = 0;
            for c in a.chars() {
                if c == '\\' {
                    slashes += 1;
                    continue;
                }
                result.extend(std::iter::repeat_n(
                    '\\',
                    if c == '"' { slashes * 2 + 1 } else { slashes },
                ));
                slashes = 0;
                result.push(c);
            }
            result.extend(std::iter::repeat_n('\\', slashes * 2));
            result.push('"');
            result
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn approved_registrations_keep_identity_when_unavailable_and_exclude_tombstones() {
        let registration = Registration {
            id: "installation-uuid".into(),
            title: "Celeste".into(),
            executable: r"F:\Games\Celeste\Celeste.exe".into(),
            file_identity: "volume:file".into(),
            root: r"F:\Games\Celeste".into(),
            working_directory: r"F:\Games\Celeste".into(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            custom_cover: None,
            available: true,
        };
        let first = records(std::slice::from_ref(&registration));
        let mut missing = registration.clone();
        missing.available = false;
        missing.title = "Corrected title".into();
        missing.executable = r"G:\Moved\Celeste.exe".into();
        missing.revision += 1;
        let mut removed = registration.clone();
        removed.id = "removed".into();
        removed.removed = true;
        let projected = records(&[missing, removed]);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].id, first[0].id);
        assert_eq!(
            crate::library::entry_key("manual", &projected[0].id),
            registration.key()
        );
        assert_eq!(projected[0].field("available"), Some("false"));
        assert_eq!(projected[0].name.as_deref(), Some("Corrected title"));
        // Discovery candidates are a separate model and cannot be passed to records.
        assert!(records(&[]).is_empty());
    }

    #[test]
    fn a_pending_claim_serializes_both_entries_in_camel_case() {
        let claim = PendingClaim {
            store_key: "steam:620".into(),
            store_entry: ClaimStoreEntry {
                title: "Portal 2".into(),
                provider: "steam".into(),
                cover_file: Some("7.jpg".into()),
                playtime_seconds: Some(54_000),
            },
            registration: ClaimRegistration {
                title: "Portal 2".into(),
                root: r"D:\Jogos\Portal 2".into(),
                cover_file: Some("local-claim.webp".into()),
            },
        };
        let value = serde_json::to_value(&claim).unwrap();
        assert_eq!(value["storeKey"], "steam:620");
        assert_eq!(value["storeEntry"]["title"], "Portal 2");
        assert_eq!(value["storeEntry"]["provider"], "steam");
        assert_eq!(value["storeEntry"]["coverFile"], "7.jpg");
        assert_eq!(value["storeEntry"]["playtimeSeconds"], 54_000);
        assert_eq!(value["registration"]["root"], r"D:\Jogos\Portal 2");
        assert_eq!(value["registration"]["coverFile"], "local-claim.webp");
        assert!(
            value.get("storeTitle").is_none(),
            "the view no longer carries the old store title field"
        );
    }
}
