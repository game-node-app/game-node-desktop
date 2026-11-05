//! OS-neutral DRM-free installations, reviewed discovery and independent save protection.
//! Paths and identities are supplied by worker adapters; this module performs no I/O.
use crate::ports::StorageError;
pub mod identity;
pub mod scheduler;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    pub id: String,
    pub title: String,
    pub executable: String,
    pub file_identity: String,
    pub root: String,
    pub working_directory: String,
    pub arguments: Vec<String>,
    pub gameplay: Vec<String>,
    pub launchers: Vec<String>,
    pub revision: u64,
    pub removed: bool,
    pub available: bool,
}
impl Registration {
    pub fn key(&self) -> String {
        format!("manual:{}", self.id)
    }
    pub fn repair(&mut self, executable: String, identity: String) {
        self.executable = executable;
        self.file_identity = identity;
        self.revision += 1;
        self.available = true;
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceKind {
    Product,
    Description,
    Executable,
    Folder,
    Manifest,
    ExternalId,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    pub value: String,
    pub strong: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inference {
    pub installation: String,
    pub revision: u64,
    pub title: String,
    pub evidence: Vec<Evidence>,
    pub game_id: Option<u64>,
    pub account: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub fingerprint: String,
    pub watch: String,
    pub root: String,
    pub title: String,
    pub targets: Vec<String>,
    pub selected_target: Option<String>,
    pub evidence: Vec<Evidence>,
    pub ignored: bool,
    pub incomplete: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchRoot {
    pub id: String,
    pub path: String,
    pub enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessPolicy {
    Folder,
    Explicit {
        gameplay: Vec<String>,
        launchers: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveSupport {
    Supported,
    NeedsSelection,
    Unsupported,
    NoSaves,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveProfile {
    pub id: String,
    pub installation: String,
    pub revision: u64,
    pub title: String,
    pub manifest_hash: String,
    pub definition: String,
    pub files: Vec<String>,
    pub registry: Vec<String>,
    pub support: SaveSupport,
    pub automatic: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Queued,
    Running,
    Unchanged,
    Successful,
    Partial,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupJob {
    pub id: String,
    pub installation: String,
    pub profile: String,
    pub revision: u64,
    pub play_period: Option<String>,
    pub destination: String,
    pub state: JobState,
    pub attempts: u8,
    pub ready_at: i64,
    pub backup_id: Option<String>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSettings {
    pub automatic: bool,
    pub destination: String,
}
impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            automatic: true,
            destination: String::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LocalError {
    #[error("this operation is unavailable on this platform")]
    Unsupported,
    #[error("invalid executable or game folder")]
    InvalidTarget,
    #[error("operation cancelled")]
    Cancelled,
    #[error("local operation failed: {0}")]
    Failed(String),
}
/// Every method may block and must run on a worker.
pub trait InstallationHost: Send + Sync {
    fn inspect(&self, executable: &str) -> Result<Registration, LocalError>;
    fn launch(&self, registration: &Registration) -> Result<(), LocalError>;
    fn evidence(&self, registration: &Registration) -> Result<Vec<Evidence>, LocalError>;
}
pub trait DiscoveryHost: Send + Sync {
    fn discover(
        &self,
        watch: &WatchRoot,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<Candidate>, LocalError>;
}
/// Global Windows-user records; catalog inferences alone are account scoped.
pub trait LocalGameStore: Send + Sync {
    fn registrations(&self) -> Result<Vec<Registration>, StorageError>;
    fn save_registration(&self, value: &Registration) -> Result<(), StorageError>;
    fn watches(&self) -> Result<Vec<WatchRoot>, StorageError>;
    fn save_watch(&self, value: &WatchRoot) -> Result<(), StorageError>;
    fn candidates(&self) -> Result<Vec<Candidate>, StorageError>;
    fn save_candidate(&self, value: &Candidate) -> Result<(), StorageError>;
    fn inferences(&self, account: &str) -> Result<Vec<Inference>, StorageError>;
    fn save_inference(&self, value: &Inference) -> Result<(), StorageError>;
    fn profiles(&self) -> Result<Vec<SaveProfile>, StorageError>;
    fn save_profile(&self, value: &SaveProfile) -> Result<(), StorageError>;
    fn jobs(&self) -> Result<Vec<BackupJob>, StorageError>;
    fn save_job(&self, value: &BackupJob) -> Result<(), StorageError>;
    fn backup_settings(&self) -> Result<BackupSettings, StorageError>;
    fn save_backup_settings(&self, value: &BackupSettings) -> Result<(), StorageError>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupOperation {
    Preview,
    Backup,
    History,
    RestorePreview { backup: String },
    Restore { backup: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupRequest {
    pub profile: SaveProfile,
    pub installation: Registration,
    pub destination: String,
    pub operation: BackupOperation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupReport {
    pub state: JobState,
    pub json: String,
}
pub trait BackupEngine: Send + Sync {
    fn execute(
        &self,
        request: &BackupRequest,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<BackupReport, LocalError>;
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
        };
        let request = BackupRequest {
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalView {
    pub busy: bool,
    pub error: Option<String>,
    pub registrations: Vec<Registration>,
    pub candidates: Vec<Candidate>,
    pub profiles: Vec<SaveProfile>,
    pub jobs: Vec<BackupJob>,
    pub settings: BackupSettings,
}

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
            id: "installation-uuid".into(), title: "Celeste".into(),
            executable: r"F:\Games\Celeste\Celeste.exe".into(),
            file_identity: "volume:file".into(), root: r"F:\Games\Celeste".into(),
            working_directory: r"F:\Games\Celeste".into(), arguments: vec![],
            gameplay: vec![], launchers: vec![], revision: 1,
            removed: false, available: true,
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
        assert_eq!(crate::library::entry_key("manual", &projected[0].id), registration.key());
        assert_eq!(projected[0].field("available"), Some("false"));
        assert_eq!(projected[0].name.as_deref(), Some("Corrected title"));
        // Discovery candidates are a separate model and cannot be passed to records.
        assert!(records(&[]).is_empty());
    }
}
