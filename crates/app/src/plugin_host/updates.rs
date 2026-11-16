//! Feed-driven updates for managed third-party packages.
//!
//! A selected feed release is downloaded to inert bytes, checked against the signed feed and
//! package integrity metadata, extracted with the local-import allowlist, then sent through the
//! ordinary compatibility and registry installation path. Calling `apply_approved_release`
//! represents the user's explicit approval; this module never starts plugin code.

use super::{
    managed_packages::{ManagedPackageLifecycle, PackageLifecycleError},
    package::Manifest,
};
use gamenode_platform::{
    plugin_environment::EmulationPolicy,
    plugin_network::FetchClient,
    plugin_packages::stage_package_archive,
    plugin_registry::{InstalledPluginVersion, PluginPackageRegistry},
    plugin_updates::{
        UpdateError, UpdateIndex, UpdateIndexVerifier, UpdateRelease, VerifiedUpdateIndex,
    },
};
use semver::Version;
use sha2::Digest;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug, Error)]
pub enum ManagedUpdateError {
    #[error(transparent)]
    Feed(#[from] UpdateError),
    #[error(transparent)]
    Package(#[from] PackageLifecycleError),
    #[error("plugin has no active version to update")]
    NoActiveVersion,
    #[error("update index does not match the installed plugin")]
    PluginIdentityMismatch,
    #[error("requested version is not present in the signed update index")]
    ReleaseNotFound,
    #[error("update feeds cannot silently downgrade or reinstall the active version")]
    Downgrade,
    #[error("downloaded ZIP is not a package for the indexed plugin version")]
    PackageIdentityMismatch,
    #[error("temporary update staging failed: {0}")]
    Staging(String),
    #[error("update worker failed")]
    WorkerFailed,
}

/// A static, user-reviewable comparison between the installed and candidate manifests. This is
/// produced from signed package metadata only; it does not load or start either plugin binary.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReview {
    pub plugin_id: String,
    pub installed_version: String,
    pub candidate_version: String,
    pub added_permissions: Vec<String>,
    pub removed_permissions: Vec<String>,
    pub changed_permissions: Vec<String>,
    pub permission_changes: Vec<PermissionChange>,
    pub added_native_access: Vec<String>,
    pub removed_native_access: Vec<String>,
    pub api_changed: bool,
    pub installed_api: String,
    pub candidate_api: String,
    pub license_changed: bool,
    pub installed_license: String,
    pub candidate_license: String,
    pub installed_license_files: Vec<String>,
    pub candidate_license_files: Vec<String>,
    pub installed_license_notices: Vec<String>,
    pub candidate_license_notices: Vec<String>,
    pub publisher_changed: bool,
    pub installed_publisher_key_id: String,
    pub candidate_publisher_key_id: String,
    pub requires_manual_approval: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionChange {
    pub id: String,
    pub installed_scope: String,
    pub candidate_scope: String,
    pub installed_required: bool,
    pub candidate_required: bool,
}

impl UpdateReview {
    pub fn compare(installed: &Manifest, candidate: &Manifest) -> Self {
        let permission_map = |manifest: &Manifest| -> BTreeMap<String, (String, bool)> {
            manifest
                .permissions
                .iter()
                .map(|permission| {
                    (
                        permission.id.clone(),
                        (permission.scope.clone(), permission.required),
                    )
                })
                .collect()
        };
        let before = permission_map(installed);
        let after = permission_map(candidate);
        let before_ids: BTreeSet<_> = before.keys().cloned().collect();
        let after_ids: BTreeSet<_> = after.keys().cloned().collect();
        let added_permissions: Vec<String> = after_ids.difference(&before_ids).cloned().collect();
        let removed_permissions: Vec<String> = before_ids.difference(&after_ids).cloned().collect();
        let changed_permissions: Vec<String> = before_ids
            .intersection(&after_ids)
            .filter(|id| before[*id] != after[*id])
            .cloned()
            .collect();
        let permission_changes = changed_permissions
            .iter()
            .map(|id| {
                let (installed_scope, installed_required) = &before[id];
                let (candidate_scope, candidate_required) = &after[id];
                PermissionChange {
                    id: id.clone(),
                    installed_scope: installed_scope.clone(),
                    candidate_scope: candidate_scope.clone(),
                    installed_required: *installed_required,
                    candidate_required: *candidate_required,
                }
            })
            .collect();
        let old_native: BTreeSet<_> = installed.native_access.iter().cloned().collect();
        let new_native: BTreeSet<_> = candidate.native_access.iter().cloned().collect();
        let added_native_access: Vec<String> =
            new_native.difference(&old_native).cloned().collect();
        let removed_native_access: Vec<String> =
            old_native.difference(&new_native).cloned().collect();
        let installed_api = format!("{} – {}", installed.api.min, installed.api.max);
        let candidate_api = format!("{} – {}", candidate.api.min, candidate.api.max);
        let installed_license = installed.license.expression.clone();
        let candidate_license = candidate.license.expression.clone();
        let api_changed = installed_api != candidate_api;
        let license_changed = installed.license != candidate.license;
        let publisher_changed = installed.publisher.key_id != candidate.publisher.key_id
            || installed.publisher.public_key != candidate.publisher.public_key;
        // Every version activation is a user decision. The diff is disclosure; it never mutates
        // the separate permission grant store, even when all requested permissions are unchanged.
        let requires_manual_approval = true;
        Self {
            plugin_id: candidate.id.clone(),
            installed_version: installed.version.clone(),
            candidate_version: candidate.version.clone(),
            added_permissions,
            removed_permissions,
            changed_permissions,
            permission_changes,
            added_native_access,
            removed_native_access,
            api_changed,
            installed_api,
            candidate_api,
            license_changed,
            installed_license,
            candidate_license,
            installed_license_files: installed.license.files.clone(),
            candidate_license_files: candidate.license.files.clone(),
            installed_license_notices: installed.license.notices.clone(),
            candidate_license_notices: candidate.license.notices.clone(),
            publisher_changed,
            installed_publisher_key_id: installed.publisher.key_id.clone(),
            candidate_publisher_key_id: candidate.publisher.key_id.clone(),
            requires_manual_approval,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AvailablePluginUpdate {
    pub plugin_id: String,
    pub name: String,
    pub installed_version: String,
    pub versions: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdatePreferences {
    pub periodic_checks: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginImportReview {
    pub token: String,
    pub plugin_id: String,
    pub provider_id: Option<String>,
    pub name: String,
    pub version: String,
    pub publisher_key_id: String,
    pub publisher_fingerprint: String,
    pub pinned_fingerprint: Option<String>,
    pub trust: String,
    pub license: String,
    pub license_files: Vec<String>,
    pub license_notices: Vec<String>,
    pub permission_ids: Vec<String>,
    pub requested_permissions: Vec<ImportPermissionReview>,
    pub native_access: Vec<String>,
    pub compatibility: String,
    pub compatibility_reason: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportPermissionReview {
    pub id: String,
    pub required: bool,
    pub scope: String,
    pub reason: String,
}

struct PendingCandidate {
    work: UpdateWorkDir,
    version: String,
    installed_version: String,
    publisher_key: [u8; 32],
    review: UpdateReview,
}

struct PendingImport {
    _work: UpdateWorkDir,
    package_path: PathBuf,
    publisher_key: [u8; 32],
    review: PluginImportReview,
    created_at: std::time::Instant,
}

/// User-facing updater. Periodic checks only fetch and authenticate feed metadata. Package
/// contents remain inert through staging and review; activation is a separate user command.
#[derive(Clone)]
pub struct PluginUpdateService {
    registry: PluginPackageRegistry,
    packages: ManagedPackageLifecycle,
    verifier: UpdateIndexVerifier,
    client: std::sync::Arc<FetchClient>,
    trust_dir: PathBuf,
    preferences_path: PathBuf,
    available_path: PathBuf,
    preferences: std::sync::Arc<std::sync::Mutex<PluginUpdatePreferences>>,
    available: std::sync::Arc<std::sync::Mutex<Vec<AvailablePluginUpdate>>>,
    indexes: std::sync::Arc<std::sync::Mutex<HashMap<String, VerifiedUpdateIndex>>>,
    checks: std::sync::Arc<AsyncMutex<()>>,
    pending: std::sync::Arc<AsyncMutex<HashMap<String, PendingCandidate>>>,
    imports: std::sync::Arc<AsyncMutex<HashMap<String, PendingImport>>>,
    lifecycle: std::sync::Arc<
        std::sync::RwLock<Option<std::sync::Arc<super::runtime::ThirdPartyPluginLifecycle>>>,
    >,
}

impl PluginUpdateService {
    pub fn open(paths: &gamenode_platform::paths::DataPaths) -> Result<Self, String> {
        let registry = PluginPackageRegistry::open(paths).map_err(|error| error.to_string())?;
        let verifier =
            UpdateIndexVerifier::open(paths.plugin_packages_dir().join("update-sequences"))
                .map_err(|error| error.to_string())?;
        let client = FetchClient::new().map_err(|error| error.to_string())?;
        let private_dir = paths.private_dir();
        fs::create_dir_all(&private_dir).map_err(|error| error.to_string())?;
        let trust_dir =
            super::profile::PluginStateLayout::new(paths.plugin_data_dir().join("state"))
                .trust_dir();
        let preferences_path = private_dir.join("plugin-update-preferences.json");
        let preferences = read_preferences(&preferences_path)?;
        let available_path = private_dir.join("plugin-updates-cache.json");
        let available = read_available(&available_path)?;
        Ok(Self {
            packages: ManagedPackageLifecycle::new(registry.clone()),
            registry,
            verifier,
            client: std::sync::Arc::new(client),
            trust_dir,
            preferences_path,
            available_path,
            preferences: std::sync::Arc::new(std::sync::Mutex::new(preferences)),
            available: std::sync::Arc::new(std::sync::Mutex::new(available)),
            indexes: Default::default(),
            checks: Default::default(),
            pending: Default::default(),
            imports: Default::default(),
            lifecycle: Default::default(),
        })
    }

    pub fn set_lifecycle(
        &self,
        lifecycle: std::sync::Arc<super::runtime::ThirdPartyPluginLifecycle>,
    ) {
        *self
            .lifecycle
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(lifecycle);
    }

    /// Stages and validates a user-selected local archive without executing plugin code.
    pub async fn import_review(&self, archive_path: PathBuf) -> Result<PluginImportReview, String> {
        const MAX_PENDING_IMPORTS: usize = 8;
        const IMPORT_REVIEW_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
        let mut imports = self.imports.lock().await;
        imports.retain(|_, pending| pending.created_at.elapsed() < IMPORT_REVIEW_TTL);
        if imports.len() >= MAX_PENDING_IMPORTS {
            return Err("too many plugin packages are awaiting review".into());
        }
        let work = tokio::task::spawn_blocking(UpdateWorkDir::create)
            .await
            .map_err(|_| "plugin import staging worker failed".to_owned())?
            .map_err(|error| error.to_string())?;
        let staged = work.path.join("package");
        let work_path = work.path.clone();
        let staged_for_worker = staged.clone();
        let (publisher_key, manifest) = tokio::task::spawn_blocking(move || {
            let metadata =
                fs::symlink_metadata(&archive_path).map_err(|error| error.to_string())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("selected plugin package is not a regular file".to_owned());
            }
            let key = package_archive_key(&archive_path)
                .map_err(|error| format!("selected plugin archive: {error}"))?;
            let archive_copy = work_path.join("selected.gnplugin");
            fs::copy(&archive_path, &archive_copy)
                .map_err(|error| format!("could not stage selected plugin archive: {error}"))?;
            stage_package_archive(&archive_copy, &staged_for_worker, &key)
                .map_err(|error| format!("selected plugin archive failed validation: {error}"))?;
            let manifest_path = staged_for_worker.join("plugin.json");
            let manifest_bytes = fs::read(manifest_path)
                .map_err(|error| format!("validated plugin manifest is unavailable: {error}"))?;
            let manifest = Manifest::parse_json(&manifest_bytes)
                .map_err(|error| error.to_string())?
                .manifest()
                .clone();
            Ok::<_, String>((key, manifest))
        })
        .await
        .map_err(|_| "plugin import validation worker failed".to_owned())??;
        let report = self
            .packages
            .inspect_candidate(&staged, publisher_key, EmulationPolicy::RequireNative)
            .await
            .map_err(|error| format!("staged plugin package is invalid: {error}"))?;
        super::grants::manifest_permission_requests(&report.manifest.permissions)
            .map_err(|error| format!("plugin declares unsupported permission scopes: {error}"))?;
        let (compatibility, compatibility_reason) = match report.selected_target() {
            Ok(_) => ("compatible".to_owned(), None),
            Err(error) => ("incompatible".to_owned(), Some(error.to_string())),
        };
        let private_dir = self.trust_dir.clone();
        let plugin_id = manifest.id.clone();
        let key = publisher_key;
        let (trust, publisher_fingerprint, pinned_fingerprint) =
            tokio::task::spawn_blocking(move || {
                let trust = super::publisher_trust::PublisherTrustStore::open(private_dir)
                    .map_err(|error| format!("publisher trust store is unavailable: {error}"))?;
                match trust
                    .inspect(&plugin_id, &key)
                    .map_err(|error| format!("publisher trust status is unavailable: {error}"))?
                {
                    super::publisher_trust::KeyStatus::Trusted { fingerprint } => {
                        Ok::<_, String>(("trusted".to_owned(), fingerprint, None))
                    }
                    super::publisher_trust::KeyStatus::Untrusted {
                        presented_fingerprint,
                    } => Ok(("untrusted".to_owned(), presented_fingerprint, None)),
                    super::publisher_trust::KeyStatus::Changed {
                        pinned_fingerprint,
                        presented_fingerprint,
                    } => Ok((
                        "changed".to_owned(),
                        presented_fingerprint,
                        Some(pinned_fingerprint),
                    )),
                }
            })
            .await
            .map_err(|_| "plugin trust worker failed".to_owned())??;
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|error| error.to_string())?;
        let token = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let review = PluginImportReview {
            token: token.clone(),
            plugin_id: manifest.id.clone(),
            provider_id: manifest.provider_id.clone(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            publisher_key_id: manifest.publisher.key_id.clone(),
            publisher_fingerprint,
            pinned_fingerprint,
            trust,
            license: manifest.license.expression.clone(),
            license_files: manifest.license.files.clone(),
            license_notices: manifest.license.notices.clone(),
            permission_ids: manifest
                .permissions
                .iter()
                .map(|item| item.id.clone())
                .collect(),
            requested_permissions: manifest
                .permissions
                .iter()
                .map(|item| ImportPermissionReview {
                    id: item.id.clone(),
                    required: item.required,
                    scope: item.scope.clone(),
                    reason: item.reason.clone(),
                })
                .collect(),
            native_access: manifest.native_access.clone(),
            compatibility,
            compatibility_reason,
        };
        imports.insert(
            token,
            PendingImport {
                _work: work,
                package_path: staged,
                publisher_key,
                review: review.clone(),
                created_at: std::time::Instant::now(),
            },
        );
        Ok(review)
    }

    /// Pins or explicitly re-trusts the exact key fingerprint shown by the staged review.
    pub async fn import_confirm_publisher(
        &self,
        token: &str,
        confirmed_fingerprint: &str,
    ) -> Result<PluginImportReview, String> {
        self.lifecycle()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?
            .require_active_profile()
            .map_err(|error| error.message().to_owned())?;
        let mut imports = self.imports.lock().await;
        let pending = imports
            .get_mut(token)
            .ok_or_else(|| "plugin import review expired".to_owned())?;
        if pending.created_at.elapsed() >= std::time::Duration::from_secs(15 * 60) {
            imports.remove(token);
            return Err("plugin import review expired".into());
        }
        if pending.review.publisher_fingerprint != confirmed_fingerprint {
            return Err("publisher fingerprint confirmation does not match".into());
        }
        let private_dir = self.trust_dir.clone();
        let plugin_id = pending.review.plugin_id.clone();
        let publisher_key = pending.publisher_key;
        let fingerprint = confirmed_fingerprint.to_owned();
        let trust_status = pending.review.trust.clone();
        let pinned = tokio::task::spawn_blocking(move || {
            let mut trust = super::publisher_trust::PublisherTrustStore::open(private_dir)
                .map_err(|error| error.to_string())?;
            match trust
                .inspect(&plugin_id, &publisher_key)
                .map_err(|error| error.to_string())?
            {
                super::publisher_trust::KeyStatus::Trusted {
                    fingerprint: current,
                } if current == fingerprint => Ok(current),
                super::publisher_trust::KeyStatus::Untrusted { .. }
                    if trust_status == "untrusted" =>
                {
                    trust
                        .confirm_first_key(&plugin_id, &publisher_key, &fingerprint)
                        .map_err(|error| error.to_string())
                }
                super::publisher_trust::KeyStatus::Changed { .. } if trust_status == "changed" => {
                    trust
                        .retrust_key(&plugin_id, &publisher_key, &fingerprint)
                        .map_err(|error| error.to_string())
                }
                _ => Err("publisher trust changed; review the package again".to_owned()),
            }
        })
        .await
        .map_err(|_| "publisher trust worker failed".to_owned())??;
        pending.review.trust = "trusted".into();
        pending.review.pinned_fingerprint = None;
        pending.review.publisher_fingerprint = pinned;
        Ok(pending.review.clone())
    }

    /// Imports a reviewed, compatible package as the selected version but leaves account
    /// activation disabled. No process is registered or executed by this operation.
    pub async fn import_install(&self, token: &str) -> Result<String, String> {
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        lifecycle
            .require_active_profile()
            .map_err(|error| error.message().to_owned())?;
        let mut imports = self.imports.lock().await;
        let pending = imports
            .get(token)
            .ok_or_else(|| "plugin import review expired".to_owned())?;
        if pending.created_at.elapsed() >= std::time::Duration::from_secs(15 * 60) {
            imports.remove(token);
            return Err("plugin import review expired".into());
        }
        if pending.review.compatibility != "compatible" {
            return Err("incompatible plugin packages cannot be imported".into());
        }
        if pending.review.trust != "trusted" {
            return Err("confirm the publisher fingerprint before importing".into());
        }
        let registry = self.registry.clone();
        let plugin_lookup = pending.review.plugin_id.clone();
        let already_installed =
            tokio::task::spawn_blocking(move || registry.active(&plugin_lookup))
                .await
                .map_err(|_| "plugin registry worker failed".to_owned())?
                .map_err(|error| error.to_string())?
                .is_some();
        if already_installed {
            return Err("plugin is already installed; use its update review to replace it".into());
        }
        let private_dir = self.trust_dir.clone();
        let package_id = pending.review.plugin_id.clone();
        let key = pending.publisher_key;
        let key_is_pinned = tokio::task::spawn_blocking(move || {
            super::publisher_trust::PublisherTrustStore::open(private_dir)
                .and_then(|trust| trust.is_pinned(&package_id, &key))
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "publisher trust worker failed".to_owned())??;
        if !key_is_pinned {
            return Err("publisher trust changed; review the package again".into());
        }
        let package_path = pending.package_path.clone();
        self.packages
            .install_candidate(package_path, key, EmulationPolicy::RequireNative, true)
            .await
            .map_err(|error| error.to_string())?;
        let pending = imports.remove(token).expect("pending import still exists");
        let plugin_id = pending.review.plugin_id.clone();
        drop(pending);
        Ok(plugin_id)
    }

    pub async fn permission_snapshot(
        &self,
        plugin_id: String,
    ) -> Result<super::runtime::PluginPermissionSnapshot, tonic::Status> {
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("managed plugin runtime is unavailable"))?;
        lifecycle.permission_snapshot(plugin_id).await
    }

    pub async fn configuration_state(
        &self,
        plugin_id: String,
    ) -> Result<super::runtime::PluginConfigurationState, tonic::Status> {
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("managed plugin runtime is unavailable"))?;
        lifecycle.configuration_state(plugin_id).await
    }

    pub fn preferences(&self) -> PluginUpdatePreferences {
        self.preferences
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// Read-only inventory of active signed package versions. Filesystem inspection and package
    /// validation stay on workers; this list never executes package code.
    pub async fn managed_inventory(&self) -> Result<Vec<super::PluginInfo>, String> {
        let registry = self.registry.clone();
        let active = tokio::task::spawn_blocking(move || registry.active_versions())
            .await
            .map_err(|_| "plugin registry worker failed".to_owned())?
            .map_err(|error| error.to_string())?;
        let mut inventory = Vec::new();
        for installed in active {
            let package_path = installed.package_path.clone();
            let publisher_key =
                match tokio::task::spawn_blocking(move || package_key(&package_path))
                    .await
                    .map_err(|_| "plugin manifest worker failed".to_owned())?
                {
                    Ok(key) => key,
                    Err(_) => continue,
                };
            let trust_dir = self.trust_dir.clone();
            let plugin_id = installed.plugin_id.clone();
            let key = publisher_key;
            let trusted = tokio::task::spawn_blocking(move || {
                super::publisher_trust::PublisherTrustStore::open(trust_dir)
                    .and_then(|trust| trust.is_pinned(&plugin_id, &key))
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|_| "publisher trust worker failed".to_owned())??;
            if !trusted {
                continue;
            }
            let report = match self
                .packages
                .inspect_candidate(
                    &installed.package_path,
                    publisher_key,
                    EmulationPolicy::RequireNative,
                )
                .await
            {
                Ok(report)
                    if report.manifest.id == installed.plugin_id
                        && report.manifest.version == installed.version =>
                {
                    report
                }
                _ => continue,
            };
            let compatibility = report.selected_target().is_ok();
            let manifest = report.manifest;
            let (enabled, runtime_available) = match self.lifecycle() {
                Some(lifecycle) if lifecycle.is_registered(&installed.plugin_id) => (
                    self.plugin_enabled(installed.plugin_id.clone()).await.ok(),
                    Some(true),
                ),
                _ => (None, Some(false)),
            };
            let fingerprint = self
                .publisher_fingerprint(&installed.plugin_id, &publisher_key)
                .await?;
            inventory.push(super::PluginInfo {
                provider_id: manifest
                    .provider_id
                    .clone()
                    .unwrap_or_else(|| manifest.id.clone()),
                plugin_id: manifest.id,
                name: manifest.name,
                source: "community".into(),
                status: if compatibility {
                    "installed"
                } else {
                    "incompatible"
                }
                .into(),
                capabilities: manifest
                    .permissions
                    .iter()
                    .map(|permission| permission.id.clone())
                    .collect(),
                session: "stopped".into(),
                native_access: manifest.native_access,
                version: Some(installed.version),
                enabled,
                runtime_available,
                publisher_fingerprint: Some(fingerprint),
            });
        }
        Ok(inventory)
    }

    async fn plugin_enabled(&self, plugin_id: String) -> Result<bool, String> {
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        lifecycle
            .enabled(plugin_id)
            .await
            .map_err(|error| error.message().to_owned())
    }

    fn lifecycle(&self) -> Option<std::sync::Arc<super::runtime::ThirdPartyPluginLifecycle>> {
        self.lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    async fn publisher_fingerprint(
        &self,
        plugin_id: &str,
        publisher_key: &[u8; 32],
    ) -> Result<String, String> {
        let trust_dir = self.trust_dir.clone();
        let plugin_id = plugin_id.to_owned();
        let publisher_key = *publisher_key;
        tokio::task::spawn_blocking(move || {
            let trust = super::publisher_trust::PublisherTrustStore::open(trust_dir)
                .map_err(|error| error.to_string())?;
            match trust
                .inspect(&plugin_id, &publisher_key)
                .map_err(|error| error.to_string())?
            {
                super::publisher_trust::KeyStatus::Trusted { fingerprint } => Ok(fingerprint),
                _ => Err("publisher key is no longer trusted".to_owned()),
            }
        })
        .await
        .map_err(|_| "publisher trust worker failed".to_owned())?
    }

    pub fn set_periodic_checks(&self, enabled: bool) -> Result<PluginUpdatePreferences, String> {
        let next = PluginUpdatePreferences {
            periodic_checks: enabled,
        };
        let mut preferences = self
            .preferences
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        write_preferences(&self.preferences_path, &next)?;
        *preferences = next.clone();
        Ok(next)
    }

    pub fn available_updates(&self) -> Vec<AvailablePluginUpdate> {
        self.available
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// Check every active package with an update URL. This method verifies signed metadata only;
    /// no candidate archive is downloaded and no plugin code is executed.
    pub async fn check_updates(&self) -> Result<Vec<AvailablePluginUpdate>, String> {
        let _check = self.checks.lock().await;
        let registry = self.registry.clone();
        let active = tokio::task::spawn_blocking(move || registry.active_versions())
            .await
            .map_err(|_| "plugin registry worker failed".to_owned())?
            .map_err(|error| error.to_string())?;
        let mut updates = Vec::new();
        for installed in active {
            let current = self
                .packages
                .inspect_candidate(
                    &installed.package_path,
                    package_key(&installed.package_path)?,
                    EmulationPolicy::RequireNative,
                )
                .await
                .map_err(|error| error.to_string())?;
            let Some(feed_url) = current.manifest.update_url.as_deref() else {
                continue;
            };
            let key = decode_publisher_key(&current.manifest.publisher.public_key)?;
            let trust = super::publisher_trust::PublisherTrustStore::open(&self.trust_dir)
                .map_err(|error| error.to_string())?;
            if !trust
                .is_pinned(&installed.plugin_id, &key)
                .map_err(|error| error.to_string())?
            {
                continue;
            }
            let feed_origin = url::Url::parse(feed_url)
                .map_err(|error| error.to_string())?
                .origin()
                .ascii_serialization();
            let verified = ManagedPluginUpdater::new(self.registry.clone())
                .check_feed(
                    &self.verifier,
                    &self.client,
                    &installed.plugin_id,
                    feed_url,
                    key,
                    move |origin| origin == feed_origin,
                )
                .await
                .map_err(|error| error.to_string())?;
            let current_version =
                Version::parse(&installed.version).map_err(|error| error.to_string())?;
            let mut versions: Vec<String> = verified
                .index()
                .releases
                .iter()
                .filter_map(|release| {
                    Version::parse(&release.version)
                        .ok()
                        .filter(|version| version > &current_version)
                        .map(|_| release.version.clone())
                })
                .collect();
            if versions.is_empty() {
                continue;
            }
            versions
                .sort_by(|left, right| Version::parse(right).ok().cmp(&Version::parse(left).ok()));
            self.indexes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(installed.plugin_id.clone(), verified);
            updates.push(AvailablePluginUpdate {
                plugin_id: installed.plugin_id,
                name: current.manifest.name,
                installed_version: installed.version,
                versions,
            });
        }
        write_available(&self.available_path, &updates)?;
        *self
            .available
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = updates.clone();
        Ok(updates)
    }

    pub async fn review(&self, plugin_id: &str, version: &str) -> Result<UpdateReview, String> {
        let index = self
            .indexes
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(plugin_id)
            .cloned()
            .ok_or_else(|| "check for updates first".to_owned())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs();
        if index.index().expires_at <= now {
            return Err("the signed update index expired; check for updates again".into());
        }
        let registry = self.registry.clone();
        let id = plugin_id.to_owned();
        let active = tokio::task::spawn_blocking(move || registry.active(&id))
            .await
            .map_err(|_| "plugin registry worker failed".to_owned())?
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "plugin is not installed".to_owned())?;
        let current = self
            .packages
            .inspect_candidate(
                &active.package_path,
                package_key(&active.package_path)?,
                EmulationPolicy::RequireNative,
            )
            .await
            .map_err(|error| error.to_string())?;
        let publisher_key = decode_publisher_key(&current.manifest.publisher.public_key)?;
        let feed_origin = current
            .manifest
            .update_url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok())
            .map(|url| url.origin().ascii_serialization())
            .ok_or_else(|| "plugin update URL is invalid".to_owned())?;
        let updater = ManagedPluginUpdater::new(self.registry.clone());
        let release = index
            .index()
            .releases
            .iter()
            .find(|release| release.version == version)
            .cloned()
            .ok_or_else(|| "release is no longer available".to_owned())?;
        let current_version = Version::parse(&active.version).map_err(|error| error.to_string())?;
        let candidate_version =
            Version::parse(&release.version).map_err(|error| error.to_string())?;
        if candidate_version <= current_version {
            return Err("update is no longer newer than the active version".into());
        }
        let release_origin = url::Url::parse(&release.package_url)
            .map_err(|error| error.to_string())?
            .origin()
            .ascii_serialization();
        let archive = self
            .verifier
            .download_release(&self.client, &release, move |origin| {
                origin == feed_origin || origin == release_origin
            })
            .await
            .map_err(|error| error.to_string())?;
        let candidate = updater
            .stage_review_candidate(&index, &release, archive, publisher_key)
            .await
            .map_err(|error| error.to_string())?;
        let candidate_manifest = candidate.1.manifest.clone();
        let review = UpdateReview::compare(&current.manifest, &candidate_manifest);
        let mut pending = self.pending.lock().await;
        pending.insert(
            plugin_id.to_owned(),
            PendingCandidate {
                work: candidate.0,
                version: version.to_owned(),
                installed_version: active.version,
                publisher_key,
                review: review.clone(),
            },
        );
        Ok(review)
    }

    pub async fn approve(&self, plugin_id: &str, version: &str) -> Result<String, String> {
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        let candidate = self
            .pending
            .lock()
            .await
            .remove(plugin_id)
            .ok_or_else(|| "review the update before approving it".to_owned())?;
        if candidate.version != version || candidate.review.plugin_id != plugin_id {
            return Err("the reviewed update changed; review it again".into());
        }
        let registry = self.registry.clone();
        let id = plugin_id.to_owned();
        let active = tokio::task::spawn_blocking(move || registry.active(&id))
            .await
            .map_err(|_| "plugin registry worker failed".to_owned())?
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "plugin is no longer installed".to_owned())?;
        if active.version != candidate.installed_version {
            return Err("the active plugin changed after review; review the update again".into());
        }
        // New requests are intentionally not written to the permission grants store here. Install
        // the complete candidate inactive, then require the supervisor's bounded Ready handshake
        // before the registry commits its active pointer.
        let installed = self
            .packages
            .install_candidate(
                &candidate.work.path.join("package"),
                candidate.publisher_key,
                EmulationPolicy::RequireNative,
                false,
            )
            .await
            .map_err(|error| error.to_string())?;
        let report = self
            .packages
            .inspect_candidate(
                &installed.package_path,
                candidate.publisher_key,
                EmulationPolicy::RequireNative,
            )
            .await
            .map_err(|error| error.to_string())?;
        let config =
            super::runtime::managed_process_config(&installed, &report, candidate.publisher_key)
                .map_err(|error| error.to_string())?;
        let mut config = config;
        config.blocked_capabilities = candidate
            .review
            .added_permissions
            .iter()
            .chain(&candidate.review.changed_permissions)
            .cloned()
            .collect();
        lifecycle
            .activate_update(
                plugin_id.to_owned(),
                candidate.installed_version,
                installed.clone(),
                config,
            )
            .await
            .map_err(|error| error.to_string())?;
        let mut available = self
            .available
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        available.retain(|item| item.plugin_id != plugin_id);
        let _ = write_available(&self.available_path, &available);
        Ok(installed.version)
    }

    pub async fn decline(&self, plugin_id: &str) {
        self.pending.lock().await.remove(plugin_id);
    }
}

pub(crate) fn package_key(path: &Path) -> Result<[u8; 32], String> {
    let manifest_path = path.join("plugin.json");
    let metadata = fs::symlink_metadata(&manifest_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4 * 1024 * 1024
    {
        return Err("plugin manifest is not a bounded regular file".into());
    }
    let bytes = fs::read(manifest_path).map_err(|error| error.to_string())?;
    let manifest = Manifest::parse_json(&bytes).map_err(|error| error.to_string())?;
    decode_publisher_key(&manifest.manifest().publisher.public_key)
}

fn package_archive_key(path: &Path) -> Result<[u8; 32], String> {
    use std::io::Read as _;
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > gamenode_platform::plugin_packages::MAX_COMPRESSED_BYTES
    {
        return Err("selected plugin archive is not a bounded regular file".into());
    }
    let mut archive =
        zip::ZipArchive::new(fs::File::open(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if archive.len() > gamenode_platform::plugin_packages::MAX_ENTRIES {
        return Err("plugin archive contains too many entries".into());
    }
    let manifest_file = archive
        .by_name("plugin.json")
        .map_err(|error| error.to_string())?;
    if manifest_file.size() > 4 * 1024 * 1024 {
        return Err("plugin manifest exceeds the metadata size limit".into());
    }
    let mut bytes = Vec::with_capacity(manifest_file.size() as usize);
    manifest_file
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("plugin manifest exceeds the metadata size limit".into());
    }
    let manifest = Manifest::parse_json(&bytes).map_err(|error| error.to_string())?;
    decode_publisher_key(&manifest.manifest().publisher.public_key)
}

fn decode_publisher_key(encoded: &str) -> Result<[u8; 32], String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| error.to_string())?
        .try_into()
        .map_err(|_| "publisher public key must contain 32 bytes".to_owned())
}

fn read_preferences(path: &Path) -> Result<PluginUpdatePreferences, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PluginUpdatePreferences {
            periodic_checks: false,
        }),
        Err(error) => Err(error.to_string()),
    }
}

fn read_available(path: &Path) -> Result<Vec<AvailablePluginUpdate>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}

fn write_preferences(path: &Path, preferences: &PluginUpdatePreferences) -> Result<(), String> {
    let bytes = serde_json::to_vec(preferences).map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "update preferences path is invalid".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp = parent.join("plugin-update-preferences.json.tmp");
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    fs::rename(temp, path).map_err(|error| error.to_string())
}

fn write_available(path: &Path, available: &[AvailablePluginUpdate]) -> Result<(), String> {
    let bytes = serde_json::to_vec(available).map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "update cache path is invalid".to_owned())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp = parent.join("plugin-updates-cache.json.tmp");
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    fs::rename(temp, path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::plugin_host::package::{
        ApiRange, LicenseNotice, PermissionRequest, PluginKind, Publisher, Target,
    };

    fn manifest(
        version: &str,
        scope: &str,
        native: &[&str],
        license: &str,
        api_max: &str,
    ) -> Manifest {
        Manifest {
            schema: 1,
            id: "org.example.plugin".into(),
            kind: PluginKind::Extension,
            provider_id: None,
            name: "Example".into(),
            version: version.into(),
            api: ApiRange {
                min: "1.0.0".into(),
                max: api_max.into(),
            },
            permissions: vec![PermissionRequest {
                id: "files.read".into(),
                required: false,
                scope: scope.into(),
                reason: "Read selected files".into(),
            }],
            native_access: native.iter().map(|value| (*value).to_owned()).collect(),
            targets: vec![Target {
                os: "linux".into(),
                arch: "x86_64".into(),
                abi: "gnu".into(),
                entrypoint: "plugin.bin".into(),
                requires: None,
                tested_on: vec![],
            }],
            publisher: Publisher {
                key_id: "key-1".into(),
                public_key: "key-bytes".into(),
            },
            license: LicenseNotice {
                expression: license.into(),
                files: vec![],
                notices: vec![],
            },
            source_url: None,
            update_url: None,
            configuration: Default::default(),
            resources: BTreeMap::new(),
            schemas: vec![],
        }
    }

    #[test]
    fn review_reports_scope_native_license_api_and_publisher_changes_without_granting() {
        let before = manifest("1.0.0", "game:data", &[], "MIT", "1.4.0");
        let mut after = manifest("1.1.0", "all:data", &["process"], "GPL-3.0-only", "2.0.0");
        after.permissions.push(PermissionRequest {
            id: "network.fetch".into(),
            required: true,
            scope: "https://example.org".into(),
            reason: "Fetch metadata".into(),
        });
        after.publisher.key_id = "key-2".into();
        let review = UpdateReview::compare(&before, &after);
        assert_eq!(review.added_permissions, ["network.fetch"]);
        assert_eq!(review.changed_permissions, ["files.read"]);
        assert_eq!(review.added_native_access, ["process"]);
        assert!(review.api_changed && review.license_changed && review.publisher_changed);
        assert!(review.requires_manual_approval);
    }

    #[test]
    fn even_metadata_only_updates_still_require_explicit_approval() {
        let before = manifest("1.0.0", "game:data", &[], "MIT", "1.4.0");
        let after = manifest("1.1.0", "game:data", &[], "MIT", "1.4.0");
        let review = UpdateReview::compare(&before, &after);
        assert!(review.requires_manual_approval);
        assert!(review.added_permissions.is_empty());
        assert!(review.added_native_access.is_empty());
    }

    #[test]
    fn periodic_opt_in_persists_and_defaults_off() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plugin-update-preferences.json");
        assert!(!read_preferences(&path).unwrap().periodic_checks);
        let enabled = PluginUpdatePreferences {
            periodic_checks: true,
        };
        write_preferences(&path, &enabled).unwrap();
        assert_eq!(read_preferences(&path).unwrap(), enabled);
    }

    #[test]
    fn available_update_cache_survives_service_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plugin-updates-cache.json");
        let cache = vec![AvailablePluginUpdate {
            plugin_id: "org.example.plugin".into(),
            name: "Example".into(),
            installed_version: "1.0.0".into(),
            versions: vec!["1.1.0".into()],
        }];
        write_available(&path, &cache).unwrap();
        assert_eq!(read_available(&path).unwrap(), cache);
    }
}

/// Coordinates feed checks and update installs for one managed package registry.
#[derive(Clone)]
pub struct ManagedPluginUpdater {
    registry: PluginPackageRegistry,
    packages: ManagedPackageLifecycle,
}

impl ManagedPluginUpdater {
    pub fn new(registry: PluginPackageRegistry) -> Self {
        Self {
            packages: ManagedPackageLifecycle::new(registry.clone()),
            registry,
        }
    }

    /// Check an update feed without changing the active plugin version. The caller should invoke
    /// this only for an opted-in/manual check and display the returned releases for review.
    pub async fn check_feed(
        &self,
        verifier: &UpdateIndexVerifier,
        client: &FetchClient,
        plugin_id: &str,
        feed_url: &str,
        publisher_key: [u8; 32],
        allowed_origin: impl Fn(&str) -> bool + Send + Sync,
    ) -> Result<VerifiedUpdateIndex, ManagedUpdateError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?
            .as_secs();
        let verified = verifier
            .check_feed(client, feed_url, &publisher_key, now, allowed_origin)
            .await?;
        if verified.index().plugin_id != plugin_id {
            return Err(ManagedUpdateError::PluginIdentityMismatch);
        }
        Ok(verified)
    }

    async fn stage_review_candidate(
        &self,
        index: &VerifiedUpdateIndex,
        release: &UpdateRelease,
        archive: Vec<u8>,
        publisher_key: [u8; 32],
    ) -> Result<
        (
            UpdateWorkDir,
            super::managed_packages::PackageCandidateReport,
        ),
        ManagedUpdateError,
    > {
        if !index
            .index()
            .releases
            .iter()
            .any(|candidate| candidate == release)
            || archive.len() as u64 != release.package_size
            || format!("{:x}", sha2::Sha256::digest(&archive)) != release.package_sha256
        {
            return Err(ManagedUpdateError::Feed(UpdateError::InvalidPackage));
        }
        let work = UpdateWorkDir::create()
            .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
        let archive_path = work.path.join("release.zip");
        let package_path = work.path.join("package");
        let archive_for_worker = archive_path.clone();
        let package_for_worker = package_path.clone();
        tokio::task::spawn_blocking(move || -> Result<(), ManagedUpdateError> {
            fs::write(&archive_for_worker, archive)
                .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
            stage_package_archive(&archive_for_worker, &package_for_worker, &publisher_key)
                .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|_| ManagedUpdateError::WorkerFailed)??;
        let report = self
            .packages
            .inspect_candidate(&package_path, publisher_key, EmulationPolicy::RequireNative)
            .await?;
        if report.manifest.id != index.index().plugin_id
            || report.manifest.version != release.version
        {
            return Err(ManagedUpdateError::PackageIdentityMismatch);
        }
        report.selected_target()?;
        Ok((work, report))
    }

    /// Download and install one release after caller approval. The index and package are both
    /// validated before the registry can change its active pointer. Any fetch, extraction,
    /// compatibility or signature failure leaves the old pointer intact.
    pub async fn apply_approved_release(
        &self,
        verifier: &UpdateIndexVerifier,
        client: &FetchClient,
        index: &VerifiedUpdateIndex,
        version: &str,
        publisher_key: [u8; 32],
        emulation_policy: EmulationPolicy,
        allowed_origin: impl Fn(&str) -> bool + Send + Sync,
    ) -> Result<InstalledPluginVersion, ManagedUpdateError> {
        let index = index.index();
        let release = index
            .releases
            .iter()
            .find(|release| release.version == version)
            .cloned()
            .ok_or(ManagedUpdateError::ReleaseNotFound)?;
        let lookup_id = index.plugin_id.clone();
        let registry = self.registry.clone();
        let active = tokio::task::spawn_blocking(move || {
            registry
                .active(&lookup_id)
                .map_err(|error| ManagedUpdateError::Staging(error.to_string()))
        })
        .await
        .map_err(|_| ManagedUpdateError::WorkerFailed)??
        .ok_or(ManagedUpdateError::NoActiveVersion)?;
        if active.plugin_id != index.plugin_id {
            return Err(ManagedUpdateError::PluginIdentityMismatch);
        }
        let current = Version::parse(&active.version)
            .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
        let candidate = Version::parse(&release.version)
            .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
        if candidate <= current {
            return Err(ManagedUpdateError::Downgrade);
        }

        let archive = verifier
            .download_release(client, &release, allowed_origin)
            .await?;
        self.install_verified_archive(index, &release, archive, publisher_key, emulation_policy)
            .await
    }

    async fn install_verified_archive(
        &self,
        index: &UpdateIndex,
        release: &UpdateRelease,
        archive: Vec<u8>,
        publisher_key: [u8; 32],
        emulation_policy: EmulationPolicy,
    ) -> Result<InstalledPluginVersion, ManagedUpdateError> {
        if !index.releases.iter().any(|candidate| candidate == release)
            || archive.len() as u64 != release.package_size
            || format!("{:x}", sha2::Sha256::digest(&archive)) != release.package_sha256
        {
            return Err(ManagedUpdateError::Feed(UpdateError::InvalidPackage));
        }
        let lookup_id = index.plugin_id.clone();
        let registry = self.registry.clone();
        let active = tokio::task::spawn_blocking(move || {
            registry
                .active(&lookup_id)
                .map_err(|error| ManagedUpdateError::Staging(error.to_string()))
        })
        .await
        .map_err(|_| ManagedUpdateError::WorkerFailed)??
        .ok_or(ManagedUpdateError::NoActiveVersion)?;
        let current = Version::parse(&active.version)
            .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
        let candidate = Version::parse(&release.version)
            .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
        if candidate <= current {
            return Err(ManagedUpdateError::Downgrade);
        }

        let work = UpdateWorkDir::create()
            .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
        let archive_path = work.path.join("release.zip");
        let package_path = work.path.join("package");
        let archive_for_worker = archive_path.clone();
        let package_for_worker = package_path.clone();
        tokio::task::spawn_blocking(move || -> Result<(), ManagedUpdateError> {
            fs::write(&archive_for_worker, archive)
                .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
            stage_package_archive(&archive_for_worker, &package_for_worker, &publisher_key)
                .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|_| ManagedUpdateError::WorkerFailed)??;

        let report = self
            .packages
            .inspect_candidate(&package_path, publisher_key, emulation_policy)
            .await?;
        if report.manifest.id != index.plugin_id || report.manifest.version != release.version {
            return Err(ManagedUpdateError::PackageIdentityMismatch);
        }
        report.selected_target()?;
        self.packages
            .install_candidate(&package_path, publisher_key, emulation_policy, true)
            .await
            .map_err(ManagedUpdateError::from)
    }
}

struct UpdateWorkDir {
    path: PathBuf,
}

impl UpdateWorkDir {
    fn create() -> io::Result<Self> {
        for _ in 0..8 {
            let mut nonce = [0u8; 16];
            getrandom::fill(&mut nonce).map_err(io::Error::other)?;
            let name = nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let path = std::env::temp_dir().join(format!("gamenode-plugin-update-{name}"));
            match fs::create_dir(&path) {
                Ok(()) => {
                    restrict_private(&path)?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve update staging",
        ))
    }
}

impl Drop for UpdateWorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn restrict_private(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_private(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use base64::Engine as _;
    use gamenode_platform::{
        plugin_packages::SIGNATURE_DOMAIN_PREFIX,
        plugin_updates::{INDEX_SIGNATURE_DOMAIN, UpdateRelease},
    };
    use ring::signature::KeyPair;
    use std::{collections::BTreeMap, io::Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn signed_package(root: &Path, version: &str, key: &ring::signature::Ed25519KeyPair) {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("licenses")).unwrap();
        fs::write(root.join("bin/plugin"), b"fixture executable").unwrap();
        fs::write(root.join("licenses/LICENSE"), b"fixture license").unwrap();
        let manifest = serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "id": "org.example.update",
            "kind": "provider",
            "provider_id": "update-fixture",
            "name": "Update Fixture",
            "version": version,
            "api": {"min": "1.6.0", "max": "1.6.0"},
            "targets": [{"os": "linux", "arch": "x86_64", "abi": "gnu", "entrypoint": "bin/plugin"}],
            "permissions": [{"id": "storage.persist", "required": false, "scope": "own-provider", "reason": "Store plugin preferences"}],
            "publisher": {
                "key_id": "fixture-key",
                "public_key": base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref())
            },
            "license": {"expression": "MIT", "files": ["licenses/LICENSE"]}
        })).unwrap();
        fs::write(root.join("plugin.json"), &manifest).unwrap();
        let files = [
            ("plugin.json", manifest.as_slice()),
            ("bin/plugin", b"fixture executable".as_slice()),
            ("licenses/LICENSE", b"fixture license".as_slice()),
        ]
        .into_iter()
        .map(|(path, bytes)| {
            (
                path.to_owned(),
                format!("{:x}", sha2::Sha256::digest(bytes)),
            )
        })
        .collect::<BTreeMap<_, _>>();
        let integrity =
            serde_json::to_vec(&serde_json::json!({"schema": 1, "files": files})).unwrap();
        let mut signed = SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(&integrity);
        fs::write(root.join("integrity.json"), integrity).unwrap();
        fs::write(root.join("signature.ed25519"), key.sign(&signed).as_ref()).unwrap();
    }

    fn add_tree(zip: &mut ZipWriter<fs::File>, base: &Path, current: &Path) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                add_tree(zip, base, &path);
            } else {
                let name = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                zip.start_file(name, SimpleFileOptions::default()).unwrap();
                zip.write_all(&fs::read(path).unwrap()).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn interrupted_update_archive_preserves_active_version_and_valid_zip_activates() {
        let base = tempfile::tempdir().unwrap();
        let registry = PluginPackageRegistry::at(base.path().join("registry")).unwrap();
        let lifecycle = ManagedPackageLifecycle::new(registry.clone());
        let updater = ManagedPluginUpdater::new(registry.clone());
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[31; 32]).unwrap();
        let publisher_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();

        let v1 = base.path().join("v1");
        signed_package(&v1, "1.0.0", &key);
        lifecycle
            .install_candidate(&v1, publisher_key, EmulationPolicy::RequireNative, true)
            .await
            .unwrap();

        let verifier = UpdateIndexVerifier::open(base.path().join("update-sequences")).unwrap();
        let client = FetchClient::new().unwrap();
        let metadata = UpdateIndex {
            schema: 1,
            plugin_id: "org.example.update".into(),
            sequence: 3,
            expires_at: 10_000,
            releases: vec![UpdateRelease {
                version: "2.0.0".into(),
                targets: vec!["linux-x86_64-gnu".into()],
                api_range: ">=1.6.0, <2.0.0".into(),
                package_url: "https://updates.example/v2.zip".into(),
                package_size: 10,
                package_sha256: "a".repeat(64),
            }],
        };
        let signed_index = serde_json::to_vec(&metadata).unwrap();
        let mut index_message = INDEX_SIGNATURE_DOMAIN.to_vec();
        index_message.extend_from_slice(&signed_index);
        let index_signature = key.sign(&index_message);
        assert_eq!(
            verifier.verify(&signed_index, &[0; 64], &publisher_key, 100),
            Err(UpdateError::InvalidSignature)
        );
        assert_eq!(
            verifier.verify(
                &signed_index,
                index_signature.as_ref(),
                &publisher_key,
                10_000
            ),
            Err(UpdateError::Expired)
        );
        verifier
            .verify(&signed_index, index_signature.as_ref(), &publisher_key, 100)
            .unwrap();
        assert_eq!(
            verifier.verify(&signed_index, index_signature.as_ref(), &publisher_key, 100),
            Err(UpdateError::Replay)
        );
        assert_eq!(
            registry
                .active("org.example.update")
                .unwrap()
                .unwrap()
                .version,
            "1.0.0"
        );
        assert!(
            updater
                .check_feed(
                    &verifier,
                    &client,
                    "org.example.update",
                    "https://updates.invalid/offline.json",
                    publisher_key,
                    |_| true
                )
                .await
                .is_err()
        );
        assert_eq!(
            registry
                .active("org.example.update")
                .unwrap()
                .unwrap()
                .version,
            "1.0.0"
        );

        let v2 = base.path().join("v2");
        signed_package(&v2, "2.0.0", &key);
        let archive_path = base.path().join("v2.zip");
        let mut zip = ZipWriter::new(fs::File::create(&archive_path).unwrap());
        add_tree(&mut zip, &v2, &v2);
        zip.finish().unwrap();
        let archive = fs::read(&archive_path).unwrap();
        let release = UpdateRelease {
            version: "2.0.0".into(),
            targets: vec!["linux-x86_64-gnu".into()],
            api_range: ">=1.6.0, <2.0.0".into(),
            package_url: "https://updates.example/v2.zip".into(),
            package_size: archive.len() as u64,
            package_sha256: format!("{:x}", sha2::Sha256::digest(&archive)),
        };
        let index = UpdateIndex {
            schema: 1,
            plugin_id: "org.example.update".into(),
            sequence: 1,
            expires_at: u64::MAX,
            releases: vec![release.clone()],
        };

        let interrupted = archive[..archive.len() / 2].to_vec();
        assert!(
            updater
                .install_verified_archive(
                    &index,
                    &release,
                    interrupted,
                    publisher_key,
                    EmulationPolicy::RequireNative
                )
                .await
                .is_err()
        );
        assert_eq!(
            registry
                .active("org.example.update")
                .unwrap()
                .unwrap()
                .version,
            "1.0.0"
        );

        let installed = updater
            .install_verified_archive(
                &index,
                &release,
                archive,
                publisher_key,
                EmulationPolicy::RequireNative,
            )
            .await
            .unwrap();
        assert_eq!(installed.version, "2.0.0");
        assert_eq!(
            registry
                .active("org.example.update")
                .unwrap()
                .unwrap()
                .version,
            "2.0.0"
        );
    }

    #[tokio::test]
    async fn local_import_review_shows_signed_scopes_and_requires_exact_trust_without_installing() {
        let base = tempfile::tempdir().unwrap();
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[41; 32]).unwrap();
        let package = base.path().join("signed-package");
        signed_package(&package, "1.0.0", &key);
        let archive_path = base.path().join("selected.gnplugin");
        let mut archive = ZipWriter::new(fs::File::create(&archive_path).unwrap());
        add_tree(&mut archive, &package, &package);
        archive.finish().unwrap();

        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("data"));
        fs::create_dir_all(paths.plugin_data_dir().join("state")).unwrap();
        let updates = PluginUpdateService::open(&paths).unwrap();
        let api = super::super::provider::ProviderApi::default();
        api.profiles().update(Some("account-one"), true);
        let state =
            super::super::profile::PluginStateLayout::new(paths.plugin_data_dir().join("state"));
        let registry = PluginPackageRegistry::open(&paths).unwrap();
        let processes = super::super::process::ProviderProcesses::new_for_lifecycle(
            api.clone(),
            base.path().join("provider.sock"),
            vec![],
            state.clone(),
        )
        .unwrap();
        updates.set_lifecycle(std::sync::Arc::new(
            super::super::runtime::ThirdPartyPluginLifecycle::new(
                api.clone(),
                processes,
                state,
                registry.clone(),
                ManagedPackageLifecycle::new(registry),
            ),
        ));
        let review = updates.import_review(archive_path).await.unwrap();
        assert_eq!(review.plugin_id, "org.example.update");
        assert_eq!(review.trust, "untrusted");
        assert_eq!(
            review.requested_permissions,
            [ImportPermissionReview {
                id: "storage.persist".into(),
                required: false,
                scope: "own-provider".into(),
                reason: "Store plugin preferences".into(),
            }]
        );

        api.profiles().update(None, true);
        assert!(
            updates
                .import_confirm_publisher(&review.token, &review.publisher_fingerprint)
                .await
                .is_err()
        );
        api.profiles().update(Some("account-one"), true);
        assert!(
            updates
                .import_confirm_publisher(&review.token, &"0".repeat(64))
                .await
                .is_err()
        );
        let trusted = updates
            .import_confirm_publisher(&review.token, &review.publisher_fingerprint)
            .await
            .unwrap();
        assert_eq!(trusted.trust, "trusted");
        let registry = PluginPackageRegistry::open(&paths).unwrap();
        assert!(registry.active("org.example.update").unwrap().is_none());
    }
}
