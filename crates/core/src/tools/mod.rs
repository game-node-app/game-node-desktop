//! Per-game scalable tools domain, descriptors, registry, planning, and port traits.

pub mod anticheat;
pub mod ini;
pub mod plan;
pub mod registry;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

pub use anticheat::{evaluate_anticheat, AntiCheatReport, AntiCheatSystem};
pub use ini::{apply_ini_patch, read_ini_key, revert_ini_patch};
pub use plan::{
    plan_deployment, strip_common_top_level_wrapper, target_dir_of, ArchiveEntry, FileAction,
    PlanError, PlannedDeployment, PlannedFile,
};
pub use registry::{
    builtin_tools, dlss_swapper_test_descriptor, find_tool, fsr4_rdna2_lite_test_descriptor,
    mfg_unlocker_test_descriptor, optiscaler_descriptor, AddonMode, ArchiveFormat, IniPatch,
    InstallStrategy, ProxyDllName, SwapperMode, ToolChannel, ToolChannelSpec, ToolDescriptor,
    ToolId,
};

/// Current lifecycle state of a tool installation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ToolInstallState {
    Installing,
    Installed,
    Updating,
    Uninstalling,
}

impl std::fmt::Display for ToolInstallState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Installing => write!(f, "Installing"),
            Self::Installed => write!(f, "Installed"),
            Self::Updating => write!(f, "Updating"),
            Self::Uninstalling => write!(f, "Uninstalling"),
        }
    }
}

/// Durable record of an installed game tool on a specific game entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInstallation {
    pub entry_key: String,
    pub tool_id: ToolId,
    pub channel: ToolChannel,
    pub version: String,
    pub target_exe: String,
    pub proxy_dll: Option<ProxyDllName>,
    pub state: ToolInstallState,
    pub installed_at: i64,
    pub updated_at: i64,
}

/// Manifest record of an installed file deployed or backed up for a tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFileRecord {
    pub entry_key: String,
    pub tool_id: ToolId,
    pub relative_path: String,
    pub installed_sha256: String,
    pub backup_path: Option<String>,
    pub original_sha256: Option<String>,
    pub prior_owner_tool_id: Option<ToolId>,
    pub created_dirs: Vec<String>,
}

/// Record of an applied INI patch for reversible rollback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolIniPatchRecord {
    pub entry_key: String,
    pub tool_id: ToolId,
    pub config_path: String,
    pub section: String,
    pub key: String,
    pub applied_value: String,
    pub prior_value: Option<String>,
}

/// Release metadata fetched from an authorized tool repository.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRelease {
    pub tool_id: ToolId,
    pub channel: ToolChannel,
    pub version: String,
    pub tag_name: String,
    pub name: String,
    pub published_at: String,
    pub download_url: String,
    pub asset_name: String,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub cached_path: Option<String>,
}

/// Candidate executable discovered inside a game's installation directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetCandidate {
    pub relative_exe: String,
    pub label: String,
    pub architecture: String,
    pub preferred: bool,
}

/// Target inspection result including candidate executables and anti-cheat report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetInspection {
    pub entry_key: String,
    pub install_dir: String,
    pub candidates: Vec<TargetCandidate>,
    pub selected_exe: Option<String>,
    pub anticheat: AntiCheatReport,
}

/// Request to install or update a tool on a game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRequest {
    pub entry_key: String,
    pub tool_id: ToolId,
    pub channel: ToolChannel,
    pub version: String,
    pub target_exe: String,
    pub proxy_dll: Option<ProxyDllName>,
    pub reset_config: bool,
    pub anticheat_confirmed: bool,
    pub custom_payload_path: Option<String>,
}

/// Content-addressed local archive cache entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedToolArchive {
    pub tool_id: ToolId,
    pub channel: ToolChannel,
    pub version: String,
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Domain error for game tools operations.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("Game is running; stop the game before modifying tools")]
    GameRunning,
    #[error("Game installation directory not found or unreadable: {0}")]
    InstallDirNotFound(String),
    #[error("Target executable not found: {0}")]
    TargetExeNotFound(String),
    #[error("Target executable path escapes the game directory: {0}")]
    PathEscapesInstallDir(String),
    #[error("Anti-cheat warning unconfirmed: {0:?}")]
    AntiCheatWarningUnconfirmed(Vec<String>),
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    #[error("Release not found for channel {channel:?} version {version}")]
    ReleaseNotFound { channel: ToolChannel, version: String },
    #[error("Download failed: {0}")]
    DownloadFailed(String),
    #[error("Checksum mismatch for {path}: expected {expected}, actual {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("Archive extraction failed: {0}")]
    ExtractionFailed(String),
    #[error("Deployment planning failed: {0}")]
    PlanFailed(#[from] PlanError),
    #[error("Slot conflict: {0}")]
    SlotConflict(String),
    #[error("Database error: {0}")]
    Storage(#[from] crate::ports::StorageError),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("Failed: {0}")]
    Failed(String),
}

/// Port trait for SQLite tools persistence (installations, manifests, backups, releases).
pub trait ToolStore: Send + Sync {
    fn list_installations(&self) -> Result<Vec<ToolInstallation>, ToolError>;
    fn list_for_entry(&self, entry_key: &str) -> Result<Vec<ToolInstallation>, ToolError>;
    fn get_installation(
        &self,
        entry_key: &str,
        tool_id: &ToolId,
    ) -> Result<Option<ToolInstallation>, ToolError>;
    fn save_installation(&self, installation: &ToolInstallation) -> Result<(), ToolError>;
    fn delete_installation(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError>;
    fn incomplete_installations(&self) -> Result<Vec<ToolInstallation>, ToolError>;

    fn list_files(&self, entry_key: &str, tool_id: &ToolId) -> Result<Vec<ToolFileRecord>, ToolError>;
    fn save_files(&self, files: &[ToolFileRecord]) -> Result<(), ToolError>;
    fn update_file_backup_owner(
        &self,
        entry_key: &str,
        relative_path: &str,
        new_owner: Option<&ToolId>,
        backup_path: Option<&str>,
        original_sha256: Option<&str>,
    ) -> Result<(), ToolError>;
    fn delete_files(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError>;

    fn list_ini_patches(
        &self,
        entry_key: &str,
        tool_id: &ToolId,
    ) -> Result<Vec<ToolIniPatchRecord>, ToolError>;
    fn list_all_ini_patches_for_entry(
        &self,
        entry_key: &str,
    ) -> Result<Vec<ToolIniPatchRecord>, ToolError>;
    fn save_ini_patches(&self, patches: &[ToolIniPatchRecord]) -> Result<(), ToolError>;
    fn delete_ini_patches(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError>;

    fn load_releases(
        &self,
        tool_id: &ToolId,
        channel: ToolChannel,
    ) -> Result<Vec<ToolRelease>, ToolError>;
    fn save_releases(
        &self,
        tool_id: &ToolId,
        channel: ToolChannel,
        releases: &[ToolRelease],
    ) -> Result<(), ToolError>;
    fn clear_cached_archives_metadata(&self) -> Result<usize, ToolError>;
}

/// Port trait for fetching releases and caching archives from fixed HTTPS repositories.
pub trait ToolCatalogApi: Send + Sync {
    fn fetch_releases(
        &self,
        descriptor: &ToolDescriptor,
        channel: ToolChannel,
    ) -> Result<Vec<ToolRelease>, ToolError>;

    fn ensure_cached_archive(
        &self,
        descriptor: &ToolDescriptor,
        release: &ToolRelease,
    ) -> Result<CachedToolArchive, ToolError>;

    fn cache_usage(&self) -> Result<(u64, usize), ToolError>;
    fn clear_cache(&self) -> Result<u64, ToolError>;
}

/// Port trait for host inspection, stopped-game check, transactional install, and rollback.
pub trait ToolInstallerHost: Send + Sync {
    fn inspect_target(
        &self,
        entry_key: &str,
        install_dir: &str,
        title: &str,
        manual_exe: Option<&str>,
    ) -> Result<TargetInspection, ToolError>;

    fn install_or_update(
        &self,
        store: &dyn ToolStore,
        catalog: &dyn ToolCatalogApi,
        descriptor: &ToolDescriptor,
        release: &ToolRelease,
        request: &InstallRequest,
        install_dir: &str,
    ) -> Result<ToolInstallation, ToolError>;

    fn uninstall(
        &self,
        store: &dyn ToolStore,
        entry_key: &str,
        tool_id: &str,
        install_dir: &str,
    ) -> Result<(), ToolError>;

    fn recover_incomplete(
        &self,
        store: &dyn ToolStore,
        resolve_install_dir: &dyn Fn(&str) -> Option<String>,
    ) -> Result<usize, ToolError>;
}

/// Top-level view of all installed tools and active tool modal state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolsView {
    pub installed_by_entry: BTreeMap<String, Vec<GameToolView>>,
    pub modal: Option<ToolModalView>,
    pub total_cache_bytes: u64,
    pub total_cache_archives: usize,
    pub catalog: Vec<ToolCatalogItemView>,
}

impl ToolsView {
    /// Formats a concise summary of installed tools for a game entry (e.g. "OptiScaler v0.7.9 · dxgi.dll").
    pub fn summary_for_entry(&self, entry_key: &str) -> String {
        if let Some(tools) = self.installed_by_entry.get(entry_key) {
            tools
                .iter()
                .map(|t| {
                    if let Some(proxy) = &t.proxy_dll {
                        format!("{} {} · {}", t.name, t.version, proxy)
                    } else {
                        format!("{} {}", t.name, t.version)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            String::new()
        }
    }
}

/// View of an installed tool on a game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameToolView {
    pub tool_id: ToolId,
    pub name: String,
    pub channel: ToolChannel,
    pub version: String,
    pub proxy_dll: Option<String>,
    pub target_exe: String,
    pub state: ToolInstallState,
    pub summary: String,
}

/// View model for the per-game tools modal dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolModalView {
    pub entry_key: String,
    pub title: String,
    pub install_dir: String,
    pub inspection: Option<TargetInspection>,
    pub tools: Vec<GameToolView>,
    pub active_tool_id: ToolId,
    pub selected_channel: ToolChannel,
    pub available_versions: Vec<ToolRelease>,
    pub selected_version: Option<String>,
    pub selected_proxy: ProxyDllName,
    pub anticheat_confirmed: bool,
    pub operation: Option<ToolOperationStatus>,
}

/// Transient operation status for the modal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolOperationStatus {
    Installing {
        progress_percent: Option<u8>,
        message: String,
    },
    Updating {
        progress_percent: Option<u8>,
        message: String,
    },
    Uninstalling,
    Failed {
        message: String,
    },
}

/// Metadata item for a catalog tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCatalogItemView {
    pub id: ToolId,
    pub name: String,
    pub summary: String,
    pub license: String,
    pub source_url: String,
}

// ----------------------------------------------------------------------------
// Test Doubles & In-Memory Store
// ----------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryToolStore {
    pub installations: Mutex<Vec<ToolInstallation>>,
    pub files: Mutex<Vec<ToolFileRecord>>,
    pub ini_patches: Mutex<Vec<ToolIniPatchRecord>>,
    pub releases: Mutex<Vec<ToolRelease>>,
}

impl ToolStore for InMemoryToolStore {
    fn list_installations(&self) -> Result<Vec<ToolInstallation>, ToolError> {
        Ok(self.installations.lock().unwrap().clone())
    }

    fn list_for_entry(&self, entry_key: &str) -> Result<Vec<ToolInstallation>, ToolError> {
        Ok(self
            .installations
            .lock()
            .unwrap()
            .iter()
            .filter(|i| i.entry_key == entry_key)
            .cloned()
            .collect())
    }

    fn get_installation(
        &self,
        entry_key: &str,
        tool_id: &ToolId,
    ) -> Result<Option<ToolInstallation>, ToolError> {
        Ok(self
            .installations
            .lock()
            .unwrap()
            .iter()
            .find(|i| i.entry_key == entry_key && &i.tool_id == tool_id)
            .cloned())
    }

    fn save_installation(&self, installation: &ToolInstallation) -> Result<(), ToolError> {
        let mut list = self.installations.lock().unwrap();
        if let Some(idx) = list
            .iter()
            .position(|i| i.entry_key == installation.entry_key && i.tool_id == installation.tool_id)
        {
            list[idx] = installation.clone();
        } else {
            list.push(installation.clone());
        }
        Ok(())
    }

    fn delete_installation(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError> {
        let mut list = self.installations.lock().unwrap();
        list.retain(|i| !(i.entry_key == entry_key && &i.tool_id == tool_id));
        Ok(())
    }

    fn incomplete_installations(&self) -> Result<Vec<ToolInstallation>, ToolError> {
        Ok(self
            .installations
            .lock()
            .unwrap()
            .iter()
            .filter(|i| i.state != ToolInstallState::Installed)
            .cloned()
            .collect())
    }

    fn list_files(
        &self,
        entry_key: &str,
        tool_id: &ToolId,
    ) -> Result<Vec<ToolFileRecord>, ToolError> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|f| f.entry_key == entry_key && &f.tool_id == tool_id)
            .cloned()
            .collect())
    }

    fn save_files(&self, files: &[ToolFileRecord]) -> Result<(), ToolError> {
        let mut list = self.files.lock().unwrap();
        for file in files {
            if let Some(idx) = list.iter().position(|f| {
                f.entry_key == file.entry_key
                    && f.tool_id == file.tool_id
                    && f.relative_path == file.relative_path
            }) {
                list[idx] = file.clone();
            } else {
                list.push(file.clone());
            }
        }
        Ok(())
    }

    fn update_file_backup_owner(
        &self,
        entry_key: &str,
        relative_path: &str,
        new_owner: Option<&ToolId>,
        backup_path: Option<&str>,
        original_sha256: Option<&str>,
    ) -> Result<(), ToolError> {
        let mut list = self.files.lock().unwrap();
        for file in list.iter_mut() {
            if file.entry_key == entry_key && file.relative_path == relative_path {
                file.prior_owner_tool_id = new_owner.cloned();
                if let Some(bp) = backup_path {
                    file.backup_path = Some(bp.to_string());
                }
                if let Some(sha) = original_sha256 {
                    file.original_sha256 = Some(sha.to_string());
                }
            }
        }
        Ok(())
    }

    fn delete_files(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError> {
        let mut list = self.files.lock().unwrap();
        list.retain(|f| !(f.entry_key == entry_key && &f.tool_id == tool_id));
        Ok(())
    }

    fn list_ini_patches(
        &self,
        entry_key: &str,
        tool_id: &ToolId,
    ) -> Result<Vec<ToolIniPatchRecord>, ToolError> {
        Ok(self
            .ini_patches
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.entry_key == entry_key && &p.tool_id == tool_id)
            .cloned()
            .collect())
    }

    fn list_all_ini_patches_for_entry(
        &self,
        entry_key: &str,
    ) -> Result<Vec<ToolIniPatchRecord>, ToolError> {
        Ok(self
            .ini_patches
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.entry_key == entry_key)
            .cloned()
            .collect())
    }

    fn save_ini_patches(&self, patches: &[ToolIniPatchRecord]) -> Result<(), ToolError> {
        let mut list = self.ini_patches.lock().unwrap();
        for p in patches {
            if let Some(idx) = list.iter().position(|existing| {
                existing.entry_key == p.entry_key
                    && existing.tool_id == p.tool_id
                    && existing.config_path == p.config_path
                    && existing.section == p.section
                    && existing.key == p.key
            }) {
                list[idx] = p.clone();
            } else {
                list.push(p.clone());
            }
        }
        Ok(())
    }

    fn delete_ini_patches(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError> {
        let mut list = self.ini_patches.lock().unwrap();
        list.retain(|p| !(p.entry_key == entry_key && &p.tool_id == tool_id));
        Ok(())
    }

    fn load_releases(
        &self,
        tool_id: &ToolId,
        channel: ToolChannel,
    ) -> Result<Vec<ToolRelease>, ToolError> {
        Ok(self
            .releases
            .lock()
            .unwrap()
            .iter()
            .filter(|r| &r.tool_id == tool_id && r.channel == channel)
            .cloned()
            .collect())
    }

    fn save_releases(
        &self,
        tool_id: &ToolId,
        channel: ToolChannel,
        releases: &[ToolRelease],
    ) -> Result<(), ToolError> {
        let mut list = self.releases.lock().unwrap();
        list.retain(|r| !(&r.tool_id == tool_id && r.channel == channel));
        list.extend_from_slice(releases);
        Ok(())
    }

    fn clear_cached_archives_metadata(&self) -> Result<usize, ToolError> {
        let mut list = self.releases.lock().unwrap();
        let mut cleared = 0;
        for r in list.iter_mut() {
            if r.cached_path.is_some() {
                r.cached_path = None;
                cleared += 1;
            }
        }
        Ok(cleared)
    }
}

/// Scripted / Mock catalog API for testing.
#[derive(Default)]
pub struct MockToolCatalogApi {
    pub releases: Mutex<Vec<ToolRelease>>,
    pub cached_archives: Mutex<Vec<CachedToolArchive>>,
}

impl ToolCatalogApi for MockToolCatalogApi {
    fn fetch_releases(
        &self,
        descriptor: &ToolDescriptor,
        channel: ToolChannel,
    ) -> Result<Vec<ToolRelease>, ToolError> {
        Ok(self
            .releases
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.tool_id == descriptor.id && r.channel == channel)
            .cloned()
            .collect())
    }

    fn ensure_cached_archive(
        &self,
        descriptor: &ToolDescriptor,
        release: &ToolRelease,
    ) -> Result<CachedToolArchive, ToolError> {
        let archives = self.cached_archives.lock().unwrap();
        archives
            .iter()
            .find(|a| {
                a.tool_id == descriptor.id
                    && a.channel == release.channel
                    && a.version == release.version
            })
            .cloned()
            .ok_or_else(|| {
                ToolError::ReleaseNotFound {
                    channel: release.channel,
                    version: release.version.clone(),
                }
            })
    }

    fn cache_usage(&self) -> Result<(u64, usize), ToolError> {
        let list = self.cached_archives.lock().unwrap();
        let bytes = list.iter().map(|a| a.size_bytes).sum();
        Ok((bytes, list.len()))
    }

    fn clear_cache(&self) -> Result<u64, ToolError> {
        let mut list = self.cached_archives.lock().unwrap();
        let bytes = list.iter().map(|a| a.size_bytes).sum();
        list.clear();
        Ok(bytes)
    }
}

/// Scripted / Mock installer host for testing.
#[derive(Default)]
pub struct MockToolInstallerHost {
    pub running_processes: Mutex<Vec<String>>,
}

impl ToolInstallerHost for MockToolInstallerHost {
    fn inspect_target(
        &self,
        entry_key: &str,
        install_dir: &str,
        title: &str,
        manual_exe: Option<&str>,
    ) -> Result<TargetInspection, ToolError> {
        let selected_exe = manual_exe
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Binaries/Win64/Game-Win64-Shipping.exe".to_string());

        let candidates = vec![TargetCandidate {
            relative_exe: selected_exe.clone(),
            label: "Shipping Executable".to_string(),
            architecture: "x64".to_string(),
            preferred: true,
        }];

        let anticheat = evaluate_anticheat(entry_key, title, []);

        Ok(TargetInspection {
            entry_key: entry_key.to_string(),
            install_dir: install_dir.to_string(),
            candidates,
            selected_exe: Some(selected_exe),
            anticheat,
        })
    }

    fn install_or_update(
        &self,
        store: &dyn ToolStore,
        _catalog: &dyn ToolCatalogApi,
        descriptor: &ToolDescriptor,
        release: &ToolRelease,
        request: &InstallRequest,
        _install_dir: &str,
    ) -> Result<ToolInstallation, ToolError> {
        if !self.running_processes.lock().unwrap().is_empty() {
            return Err(ToolError::GameRunning);
        }

        let installation = ToolInstallation {
            entry_key: request.entry_key.clone(),
            tool_id: descriptor.id.clone(),
            channel: request.channel,
            version: release.version.clone(),
            target_exe: request.target_exe.clone(),
            proxy_dll: request.proxy_dll,
            state: ToolInstallState::Installed,
            installed_at: 1000,
            updated_at: 1000,
        };

        store.save_installation(&installation)?;
        Ok(installation)
    }

    fn uninstall(
        &self,
        store: &dyn ToolStore,
        entry_key: &str,
        tool_id: &str,
        _install_dir: &str,
    ) -> Result<(), ToolError> {
        if !self.running_processes.lock().unwrap().is_empty() {
            return Err(ToolError::GameRunning);
        }

        let id = ToolId::new(tool_id);
        store.delete_installation(entry_key, &id)?;
        store.delete_files(entry_key, &id)?;
        store.delete_ini_patches(entry_key, &id)?;
        Ok(())
    }

    fn recover_incomplete(
        &self,
        store: &dyn ToolStore,
        _resolve_install_dir: &dyn Fn(&str) -> Option<String>,
    ) -> Result<usize, ToolError> {
        let incomplete = store.incomplete_installations()?;
        let count = incomplete.len();
        for item in incomplete {
            store.delete_installation(&item.entry_key, &item.tool_id)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_in_memory_store_and_host_flow() {
        let store = InMemoryToolStore::default();
        let catalog = MockToolCatalogApi::default();
        let host = MockToolInstallerHost::default();

        let opti = optiscaler_descriptor();
        let release = ToolRelease {
            tool_id: opti.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler v0.7.9".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://github.com/optiscaler/OptiScaler/releases/download/v0.7.9/OptiScaler-v0.7.9.7z".to_string(),
            asset_name: "OptiScaler-v0.7.9.7z".to_string(),
            size_bytes: 5000000,
            sha256: Some("sha_test".to_string()),
            cached_path: Some("/cache/optiscaler/v0.7.9.7z".to_string()),
        };

        let request = InstallRequest {
            entry_key: "steam:1091500".to_string(),
            tool_id: opti.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "bin/x64/Cyberpunk2077.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        // Target inspection
        let inspection = host
            .inspect_target("steam:1091500", "C:/Games/Cyberpunk 2077", "Cyberpunk 2077", None)
            .expect("inspection succeeds");
        assert_eq!(inspection.entry_key, "steam:1091500");
        assert!(!inspection.anticheat.has_warning());

        // Install
        let inst = host
            .install_or_update(&store, &catalog, &opti, &release, &request, "C:/Games/Cyberpunk 2077")
            .expect("install succeeds");
        assert_eq!(inst.state, ToolInstallState::Installed);
        assert_eq!(inst.proxy_dll, Some(ProxyDllName::Dxgi));

        // Check store
        let retrieved = store
            .get_installation("steam:1091500", &opti.id)
            .expect("store get succeeds")
            .expect("installation exists");
        assert_eq!(retrieved.version, "0.7.9");

        // Uninstall
        host.uninstall(&store, "steam:1091500", "optiscaler", "C:/Games/Cyberpunk 2077")
            .expect("uninstall succeeds");
        let empty = store
            .get_installation("steam:1091500", &opti.id)
            .expect("store get succeeds");
        assert!(empty.is_none());
    }

    #[test]
    fn test_tools_view_summary() {
        let mut view = ToolsView::default();
        view.installed_by_entry.insert(
            "steam:1091500".to_string(),
            vec![GameToolView {
                tool_id: ToolId::new("optiscaler"),
                name: "OptiScaler".to_string(),
                channel: ToolChannel::Stable,
                version: "v0.7.9".to_string(),
                proxy_dll: Some("dxgi.dll".to_string()),
                target_exe: "bin/x64/Cyberpunk2077.exe".to_string(),
                state: ToolInstallState::Installed,
                summary: "Universal upscaler".to_string(),
            }],
        );

        assert_eq!(
            view.summary_for_entry("steam:1091500"),
            "OptiScaler v0.7.9 · dxgi.dll"
        );
        assert_eq!(view.summary_for_entry("steam:999"), "");
    }

    #[test]
    fn test_stopped_game_guard_refuses_operations() {
        let store = InMemoryToolStore::default();
        let catalog = MockToolCatalogApi::default();
        let host = MockToolInstallerHost::default();

        *host.running_processes.lock().unwrap() = vec!["Game.exe".to_string()];

        let opti = optiscaler_descriptor();
        let release = ToolRelease {
            tool_id: opti.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler v0.7.9".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/optiscaler.7z".to_string(),
            asset_name: "optiscaler.7z".to_string(),
            size_bytes: 1024,
            sha256: None,
            cached_path: None,
        };

        let request = InstallRequest {
            entry_key: "steam:123".to_string(),
            tool_id: opti.id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "Game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            reset_config: false,
            anticheat_confirmed: true,
            custom_payload_path: None,
        };

        let err = host
            .install_or_update(&store, &catalog, &opti, &release, &request, "C:/Games/Test")
            .unwrap_err();
        assert_eq!(err, ToolError::GameRunning);

        let err = host
            .uninstall(&store, "steam:123", "optiscaler", "C:/Games/Test")
            .unwrap_err();
        assert_eq!(err, ToolError::GameRunning);
    }

    #[test]
    fn test_recover_incomplete_installations() {
        let store = InMemoryToolStore::default();
        let host = MockToolInstallerHost::default();

        store
            .save_installation(&ToolInstallation {
                entry_key: "steam:123".to_string(),
                tool_id: ToolId::new("optiscaler"),
                channel: ToolChannel::Stable,
                version: "0.7.9".to_string(),
                target_exe: "Game.exe".to_string(),
                proxy_dll: Some(ProxyDllName::Dxgi),
                state: ToolInstallState::Installing,
                installed_at: 100,
                updated_at: 100,
            })
            .unwrap();

        assert_eq!(store.incomplete_installations().unwrap().len(), 1);

        let recovered = host
            .recover_incomplete(&store, &|_| Some("C:/Games/Test".to_string()))
            .unwrap();
        assert_eq!(recovered, 1);
        assert_eq!(store.incomplete_installations().unwrap().len(), 0);
    }

    #[test]
    fn test_file_records_and_backup_owner_transfer() {
        let store = InMemoryToolStore::default();

        let files = vec![ToolFileRecord {
            entry_key: "steam:123".to_string(),
            tool_id: ToolId::new("optiscaler"),
            relative_path: "amd_fidelityfx_dx12.dll".to_string(),
            installed_sha256: "sha_opti".to_string(),
            backup_path: Some("/backups/amd_fidelityfx_dx12.dll".to_string()),
            original_sha256: Some("sha_orig".to_string()),
            prior_owner_tool_id: None,
            created_dirs: vec![],
        }];

        store.save_files(&files).unwrap();
        assert_eq!(store.list_files("steam:123", &ToolId::new("optiscaler")).unwrap().len(), 1);

        // Transfer backup owner to higher tool layer (e.g. fsr4-rdna2-lite)
        let new_owner = ToolId::new("fsr4-rdna2-lite");
        store
            .update_file_backup_owner(
                "steam:123",
                "amd_fidelityfx_dx12.dll",
                Some(&new_owner),
                Some("/backups/new_backup.dll"),
                Some("sha_new"),
            )
            .unwrap();

        let updated = store.list_files("steam:123", &ToolId::new("optiscaler")).unwrap();
        assert_eq!(updated[0].prior_owner_tool_id, Some(new_owner));
        assert_eq!(updated[0].backup_path.as_deref(), Some("/backups/new_backup.dll"));
        assert_eq!(updated[0].original_sha256.as_deref(), Some("sha_new"));
    }

    #[test]
    fn test_catalog_and_release_metadata_lifecycle() {
        let store = InMemoryToolStore::default();
        let catalog = MockToolCatalogApi::default();

        let opti_id = ToolId::new("optiscaler");
        let release = ToolRelease {
            tool_id: opti_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler v0.7.9".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/opti.7z".to_string(),
            asset_name: "opti.7z".to_string(),
            size_bytes: 2048,
            sha256: Some("sha256abc".to_string()),
            cached_path: Some("/cache/opti.7z".to_string()),
        };

        store.save_releases(&opti_id, ToolChannel::Stable, std::slice::from_ref(&release)).unwrap();
        let loaded = store.load_releases(&opti_id, ToolChannel::Stable).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].version, "0.7.9");

        let cleared_meta = store.clear_cached_archives_metadata().unwrap();
        assert_eq!(cleared_meta, 1);
        let after_clear = store.load_releases(&opti_id, ToolChannel::Stable).unwrap();
        assert!(after_clear[0].cached_path.is_none());

        // Test catalog mock cache usage
        *catalog.cached_archives.lock().unwrap() = vec![CachedToolArchive {
            tool_id: opti_id,
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            path: "/cache/opti.7z".to_string(),
            size_bytes: 2048,
            sha256: "sha256abc".to_string(),
        }];

        let (usage_bytes, count) = catalog.cache_usage().unwrap();
        assert_eq!(usage_bytes, 2048);
        assert_eq!(count, 1);

        let freed = catalog.clear_cache().unwrap();
        assert_eq!(freed, 2048);
        let (usage_after, count_after) = catalog.cache_usage().unwrap();
        assert_eq!(usage_after, 0);
        assert_eq!(count_after, 0);
    }
}

