//! Feed-driven updates for managed third-party packages.
//!
//! A selected feed release is downloaded to inert bytes, checked against the signed feed and
//! package integrity metadata, extracted with the local-import allowlist, then sent through the
//! ordinary compatibility and registry installation path. Calling `apply_approved_release`
//! represents the user's explicit approval; this module never starts plugin code.

use super::{
    grants::PluginIdentity,
    managed_packages::{ManagedPackageLifecycle, PackageCandidateReport, PackageLifecycleError},
    package::{Manifest, PlaytimeCapabilities},
    profile::{PluginProfiles, PluginStateLayout},
};
use gamenode_platform::{
    plugin_bundle_db::{BundleRecord, SqlitePluginBundleStore},
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
    pub installed_playtime: PlaytimeCapabilities,
    pub candidate_playtime: PlaytimeCapabilities,
    pub playtime_changed: bool,
    pub publisher_changed: bool,
    pub installed_publisher_key_id: String,
    pub candidate_publisher_key_id: String,
    pub publisher_fingerprint: String,
    /// Namespaced private-data contracts whose declared version differs. The host does not
    /// migrate or delete their stored values during package activation.
    pub changed_data_schemas: Vec<String>,
    pub preserves_prior_data: bool,
    pub is_downgrade: bool,
    pub requires_manual_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility_details: Option<super::managed_packages::PackageCompatibilityReview>,
    /// The command bridge populates choices from the active account's library.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_targets: Option<Vec<crate::engine::ExtensionReadinessChoice>>,
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
        let playtime_changed = installed.playtime != candidate.playtime;
        let installed_license = installed.license.expression.clone();
        let candidate_license = candidate.license.expression.clone();
        let api_changed = installed_api != candidate_api;
        let license_changed = installed.license != candidate.license;
        // Key ID labels are package metadata. Re-trust is tied only to the actual key bytes.
        let publisher_changed = installed.publisher.public_key != candidate.publisher.public_key;
        let schema_versions = |manifest: &Manifest| -> BTreeMap<String, BTreeSet<String>> {
            let mut versions = BTreeMap::new();
            for schema in &manifest.schemas {
                versions
                    .entry(schema.namespace.clone())
                    .or_insert_with(BTreeSet::new)
                    .insert(schema.version.clone());
            }
            versions
        };
        let old_schemas = schema_versions(installed);
        let new_schemas = schema_versions(candidate);
        let changed_data_schemas: Vec<String> = old_schemas
            .keys()
            .chain(new_schemas.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|namespace| old_schemas.get(*namespace) != new_schemas.get(*namespace))
            .cloned()
            .collect();
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
            installed_playtime: installed.playtime,
            candidate_playtime: candidate.playtime,
            playtime_changed,
            publisher_changed,
            installed_publisher_key_id: installed.publisher.key_id.clone(),
            candidate_publisher_key_id: candidate.publisher.key_id.clone(),
            publisher_fingerprint: String::new(),
            // Package activation changes no storage namespace and performs no data migration or
            // cleanup. Values remain available to a compatible version selected later.
            preserves_prior_data: true,
            is_downgrade: Version::parse(&candidate.version)
                .ok()
                .zip(Version::parse(&installed.version).ok())
                .is_some_and(|(candidate, installed)| candidate < installed),
            changed_data_schemas,
            requires_manual_approval,
            compatibility_details: None,
            extension_targets: (installed.kind == super::package::PluginKind::Extension
                && candidate.kind == super::package::PluginKind::Extension)
                .then(Vec::new),
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

/// A signed bundle release offered by a component's update feed. The release version is the
/// bundle's distribution version; component versions remain independent.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AvailableBundleUpdate {
    pub bundle_id: String,
    pub name: String,
    pub installed_version: String,
    pub versions: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AvailableBundleRollback {
    pub bundle_id: String,
    pub name: String,
    pub active_version: String,
    pub versions: Vec<String>,
}

/// One component's old/new facts in a bundle update or rollback review. Produced from signed
/// metadata only: the candidate package is staged and inspected, never executed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleComponentUpdateReview {
    pub plugin_id: String,
    pub kind: String,
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
    pub availability: super::bundles::BundleComponentAvailability,
    pub compatibility: String,
    pub compatibility_reason: Option<String>,
    pub compatibility_details: super::managed_packages::PackageCompatibilityReview,
    pub publisher_changed: bool,
    pub is_downgrade: bool,
}

/// The grouped review of one signed bundle release: the bundle hash from the update index and each
/// component's old/new version, compatibility verdict and permission diff.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleUpdateReview {
    pub bundle_id: String,
    pub name: String,
    pub installed_version: String,
    pub candidate_version: String,
    /// Lowercase SHA-256 of the exact signed `bundle.json` bytes the update index committed to.
    pub bundle_sha256: String,
    pub publisher_fingerprint: String,
    pub components: Vec<BundleComponentUpdateReview>,
    /// Distinct union over the components, for a one-line summary.
    pub added_permissions: Vec<String>,
    pub removed_permissions: Vec<String>,
    pub changed_permissions: Vec<String>,
    pub requires_manual_approval: bool,
    pub is_downgrade: bool,
    pub is_rollback: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AvailablePluginRollback {
    pub plugin_id: String,
    pub name: String,
    pub active_version: String,
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
    /// The same review a bundle component carries, produced by the same builder.
    #[serde(flatten)]
    pub package: PackageImportReview,
}

/// Everything an import review shows about one package. A standalone import adds the staging
/// token above; a bundle component embeds this unchanged beside its kind and availability.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackageImportReview {
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
    /// Signed content platforms, such as `gamecube` or `wii`. Empty for manifests that predate
    /// the declaration.
    pub supported_platforms: Vec<String>,
    /// Signed playtime behavior shown before the package is installed.
    pub playtime: PlaytimeCapabilities,
    pub compatibility: String,
    pub compatibility_reason: Option<String>,
    pub compatibility_details: super::managed_packages::PackageCompatibilityReview,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImportPermissionReview {
    pub id: String,
    pub required: bool,
    pub scope: String,
    pub reason: String,
    /// Signed disclosure of what the plugin loses when this optional permission is denied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact_if_disabled: Option<String>,
}

/// One component inside a bundle import review. The embedded review is exactly what importing the
/// component alone would show, so no manifest, license or permission detail is parsed twice.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleComponentReview {
    /// `provider` or `extension`, from the verified bundle manifest.
    pub kind: String,
    /// Platform/API compatibility verdict; an unavailable component stays in the review but is
    /// not activated by the install.
    pub availability: super::bundles::BundleComponentAvailability,
    pub review: PackageImportReview,
}

/// One component's license declarations, attributed to the plugin that ships them.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleComponentLicense {
    pub plugin_id: String,
    pub expression: String,
    pub files: Vec<String>,
    pub notices: Vec<String>,
}

/// The bundle-wide license view: distinct expressions plus every component's declared files and
/// notices, so one header can show what the whole distribution ships.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleLicenseSummary {
    /// Distinct license expressions in component order.
    pub expressions: Vec<String>,
    pub components: Vec<BundleComponentLicense>,
}

/// The aggregated review of one bundle: one identity and publisher decision, with the ordinary
/// per-package review embedded for every component.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleImportReview {
    pub token: String,
    pub bundle: super::bundles::BundleIdentity,
    /// `trusted`, `changed` or `untrusted` for the bundle publisher key. Every component shares
    /// the verified key, so this is the single publisher decision the review asks for.
    pub trust: String,
    /// The previously pinned fingerprint when the bundle publisher changed.
    pub pinned_fingerprint: Option<String>,
    pub license: BundleLicenseSummary,
    pub components: Vec<BundleComponentReview>,
}

/// One component's optional permissions the user disabled in a grouped import review. The host
/// validates the ids against that component's reviewed requests before installing anything.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleComponentDeniedPermissions {
    pub plugin_id: String,
    pub denied_permission_ids: Vec<String>,
}

/// One component's permission decisions submitted from a grouped review. The expected values are
/// the ones the review displayed; the host rechecks them before saving.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleComponentPermissionSubmission {
    pub plugin_id: String,
    pub expected_profile_id: String,
    pub expected_publisher_fingerprint: String,
    pub expected_revision: u64,
    pub decisions: Vec<super::grants::PermissionDecision>,
}

/// What happened to one component of a grouped permission submission. A rejection is reported per
/// component so a stale revision or a denied permission never hides its siblings' outcome.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleComponentPermissionOutcome {
    pub plugin_id: String,
    pub snapshot: Option<super::runtime::PluginPermissionSnapshot>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundlePermissionResult {
    pub components: Vec<BundleComponentPermissionOutcome>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginPlaytimeInfo {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub playtime: PlaytimeCapabilities,
    pub accounting_enabled: bool,
}

struct PendingCandidate {
    work: Option<UpdateWorkDir>,
    version: String,
    installed_version: String,
    publisher_key: [u8; 32],
    candidate_publisher_key: [u8; 32],
    confirmed_retrust: bool,
    is_rollback: bool,
    installed_manifest: Manifest,
    candidate_manifest: Manifest,
    review: UpdateReview,
}

struct PendingImport {
    _work: UpdateWorkDir,
    package_path: PathBuf,
    publisher_key: [u8; 32],
    review: PluginImportReview,
    created_at: std::time::Instant,
}

struct PendingBundleImport {
    /// The verified bundle publisher key; the single pin this review confirms.
    publisher_key: [u8; 32],
    review: BundleImportReview,
    /// The reviewed archive and the exact component inspection the review showed. Installation
    /// re-verifies the archive and compares it with this report, so the approved bytes are the
    /// installed bytes.
    archive_path: PathBuf,
    report: super::bundles::BundleCandidateReport,
    created_at: std::time::Instant,
}

/// The result of installing one reviewed bundle. Every component is listed with the version this
/// install published or adopted, and whether it was activated on this host.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleImportInstallOutcome {
    pub bundle_id: String,
    pub components: Vec<BundleImportInstalledComponent>,
}

/// The result of one explicitly accepted standalone import. The package stays installed even when
/// activation fails; `activation_error` then explains why it is not running.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginImportInstallOutcome {
    pub plugin_id: String,
    pub activated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleImportInstalledComponent {
    pub plugin_id: String,
    pub kind: String,
    pub version: String,
    pub active: bool,
    /// Whether the host enabled this component for the active profile right after installing it.
    pub activated: bool,
    /// Present when a compatible component stayed installed but could not be activated. Every
    /// sibling still reports its own outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation_error: Option<String>,
    /// Present when this component was installed but its reviewed permission decisions could not
    /// be saved. Every sibling still reports its own outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_error: Option<String>,
}

/// A reviewed bundle release ready to install. The work directory holds the exact downloaded
/// archive, which the update primitive re-verifies before any registry write.
struct PendingBundleUpdate {
    /// Owns the private staging directory for as long as the reviewed archive is needed.
    _work: UpdateWorkDir,
    archive_path: PathBuf,
    report: super::bundles::BundleCandidateReport,
    installed: BundleRecord,
    release_version: String,
    publisher_key: [u8; 32],
    review: BundleUpdateReview,
    created_at: std::time::Instant,
}

/// A reviewed rollback target. Approval re-reads the archived mapping and revalidates every
/// component, so this only records that the user saw the version.
struct PendingBundleRollback {
    version: String,
    created_at: std::time::Instant,
}

fn read_inert_license(package_path: &Path, file: &str) -> Result<String, String> {
    const MAX_LICENSE_BYTES: u64 = 256 * 1024;
    if Path::new(file)
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err("license file path is not package-relative".into());
    }
    let root = fs::canonicalize(package_path).map_err(|error| error.to_string())?;
    let requested = package_path.join(file);
    let metadata = fs::symlink_metadata(&requested).map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_LICENSE_BYTES
    {
        return Err("license file is not a bounded regular file".to_owned());
    }
    let path = fs::canonicalize(&requested).map_err(|error| error.to_string())?;
    if !path.starts_with(&root) {
        return Err("license file escapes the validated package".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_LICENSE_BYTES {
        return Err("license file exceeds the size limit".to_owned());
    }
    String::from_utf8(bytes).map_err(|_| "license file is not valid UTF-8".to_owned())
}

/// Builds the review fields for one staged, validated package. Standalone imports and bundle
/// components both call this, so manifest, license, permission and compatibility parsing exists
/// once and a bundle can never present different facts than the component would alone.
fn package_import_review(
    report: &PackageCandidateReport,
    trust: &super::publisher_trust::PublisherTrustStore,
    publisher_key: &[u8; 32],
) -> Result<PackageImportReview, String> {
    super::grants::manifest_permission_requests(&report.manifest.permissions)
        .map_err(|error| format!("plugin declares unsupported permission scopes: {error}"))?;
    let compatibility_details = report.compatibility_review();
    let (compatibility, compatibility_reason) = match report.selected_target() {
        Ok(_) => ("compatible".to_owned(), None),
        Err(error) => (
            if compatibility_details.state
                == super::compatibility::CompatibilityState::Indeterminate
            {
                "unknown"
            } else {
                "incompatible"
            }
            .to_owned(),
            Some(error.to_string()),
        ),
    };
    let manifest = &report.manifest;
    let (trust, publisher_fingerprint, pinned_fingerprint) = match trust
        .inspect(&manifest.id, publisher_key)
        .map_err(|error| format!("publisher trust status is unavailable: {error}"))?
    {
        super::publisher_trust::KeyStatus::Trusted { fingerprint } => {
            ("trusted".to_owned(), fingerprint, None)
        }
        super::publisher_trust::KeyStatus::Untrusted {
            presented_fingerprint,
        } => ("untrusted".to_owned(), presented_fingerprint, None),
        super::publisher_trust::KeyStatus::Changed {
            pinned_fingerprint,
            presented_fingerprint,
        } => (
            "changed".to_owned(),
            presented_fingerprint,
            Some(pinned_fingerprint),
        ),
    };
    Ok(PackageImportReview {
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
                impact_if_disabled: item.impact_if_disabled.clone(),
            })
            .collect(),
        native_access: manifest.native_access.clone(),
        supported_platforms: manifest.supported_platforms.clone(),
        playtime: manifest.playtime,
        compatibility,
        compatibility_reason,
        compatibility_details,
    })
}

/// The single publisher decision a bundle review asks for. Every component shares the verified
/// bundle key, so the bundle is trusted only when each component's pin already accepts it.
fn bundle_trust_state<'a>(
    components: impl IntoIterator<Item = &'a PackageImportReview>,
) -> (String, Option<String>) {
    let mut pinned = None;
    let mut all_trusted = true;
    for component in components {
        match component.trust.as_str() {
            "trusted" => {}
            "changed" => {
                all_trusted = false;
                if pinned.is_none() {
                    pinned = component.pinned_fingerprint.clone();
                }
            }
            _ => all_trusted = false,
        }
    }
    if all_trusted {
        ("trusted".to_owned(), None)
    } else if pinned.is_some() {
        ("changed".to_owned(), pinned)
    } else {
        ("untrusted".to_owned(), None)
    }
}

/// Build one component's old/new review from signed package metadata. Both manifests were parsed
/// from verified packages; nothing here executes package code.
fn bundle_component_update_review(
    installed: &Manifest,
    candidate: &Manifest,
    candidate_report: &PackageCandidateReport,
    installed_version: &str,
    candidate_version: &str,
) -> BundleComponentUpdateReview {
    let comparison = UpdateReview::compare(installed, candidate);
    let details = candidate_report.compatibility_review();
    let (compatibility, compatibility_reason) = match candidate_report.selected_target() {
        Ok(_) => ("compatible".to_owned(), None),
        Err(error) => (
            if details.state == super::compatibility::CompatibilityState::Indeterminate {
                "unknown"
            } else {
                "incompatible"
            }
            .to_owned(),
            Some(error.to_string()),
        ),
    };
    BundleComponentUpdateReview {
        plugin_id: candidate.id.clone(),
        kind: match candidate.kind {
            super::package::PluginKind::Provider => "provider".to_owned(),
            super::package::PluginKind::Extension => "extension".to_owned(),
        },
        installed_version: installed_version.to_owned(),
        candidate_version: candidate_version.to_owned(),
        added_permissions: comparison.added_permissions,
        removed_permissions: comparison.removed_permissions,
        changed_permissions: comparison.changed_permissions,
        permission_changes: comparison.permission_changes,
        added_native_access: comparison.added_native_access,
        removed_native_access: comparison.removed_native_access,
        api_changed: comparison.api_changed,
        installed_api: comparison.installed_api,
        candidate_api: comparison.candidate_api,
        availability: super::bundles::availability(candidate_report),
        compatibility,
        compatibility_reason,
        compatibility_details: details,
        publisher_changed: comparison.publisher_changed,
        is_downgrade: comparison.is_downgrade,
    }
}

fn union_permissions(
    components: &[BundleComponentUpdateReview],
    pick: impl Fn(&BundleComponentUpdateReview) -> &Vec<String>,
) -> Vec<String> {
    let mut union = Vec::new();
    for component in components {
        for permission in pick(component) {
            if !union.contains(permission) {
                union.push(permission.clone());
            }
        }
    }
    union
}

fn bundle_license_summary(components: &[BundleComponentReview]) -> BundleLicenseSummary {
    let mut expressions: Vec<String> = Vec::new();
    let mut licenses = Vec::with_capacity(components.len());
    for component in components {
        let review = &component.review;
        if !expressions.contains(&review.license) {
            expressions.push(review.license.clone());
        }
        licenses.push(BundleComponentLicense {
            plugin_id: review.plugin_id.clone(),
            expression: review.license.clone(),
            files: review.license_files.clone(),
            notices: review.license_notices.clone(),
        });
    }
    BundleLicenseSummary {
        expressions,
        components: licenses,
    }
}

/// Validates the optional permissions a user disabled in one package review. The ids must name
/// that package's reviewed requests; a required permission can never be denied, and a repeated id
/// is rejected rather than silently collapsed. Nothing is written here.
fn denied_permission_set(
    requested: &[ImportPermissionReview],
    denied: &[String],
) -> Result<BTreeSet<String>, String> {
    let mut denied_set = BTreeSet::new();
    for id in denied {
        let Some(permission) = requested.iter().find(|permission| &permission.id == id) else {
            return Err(format!(
                "permission `{id}` is not requested by this package and cannot be denied"
            ));
        };
        if permission.required {
            return Err(format!(
                "required permission `{id}` cannot be denied or disabled"
            ));
        }
        if !denied_set.insert(id.clone()) {
            return Err(format!("permission `{id}` is denied more than once"));
        }
    }
    Ok(denied_set)
}

/// One just-installed package's reviewed decisions, expressed in the host's typed grant scopes so
/// they can be submitted through the ordinary permission-review path.
struct ImportPermissionPlan {
    profile_id: String,
    publisher_fingerprint: String,
    grant_revision: u64,
    decisions: Vec<super::grants::PermissionDecision>,
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
    grant_layout: PluginStateLayout,
    profiles: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<PluginProfiles>>>>,
    preferences_path: PathBuf,
    available_path: PathBuf,
    preferences: std::sync::Arc<std::sync::Mutex<PluginUpdatePreferences>>,
    available: std::sync::Arc<std::sync::Mutex<Vec<AvailablePluginUpdate>>>,
    indexes: std::sync::Arc<std::sync::Mutex<HashMap<String, VerifiedUpdateIndex>>>,
    bundle_indexes: std::sync::Arc<std::sync::Mutex<HashMap<String, VerifiedUpdateIndex>>>,
    bundle_available: std::sync::Arc<std::sync::Mutex<Vec<AvailableBundleUpdate>>>,
    checks: std::sync::Arc<AsyncMutex<()>>,
    activations: std::sync::Arc<AsyncMutex<()>>,
    pending: std::sync::Arc<AsyncMutex<HashMap<String, PendingCandidate>>>,
    pending_bundles: std::sync::Arc<AsyncMutex<HashMap<String, PendingBundleUpdate>>>,
    pending_rollbacks: std::sync::Arc<AsyncMutex<HashMap<String, PendingBundleRollback>>>,
    imports: std::sync::Arc<AsyncMutex<HashMap<String, PendingImport>>>,
    bundle_imports: std::sync::Arc<AsyncMutex<HashMap<String, PendingBundleImport>>>,
    /// The bundle mapping on the application database connection. It is set at startup; when it is
    /// absent (tests of standalone packages), the inventory never groups components.
    bundles: std::sync::Arc<std::sync::Mutex<Option<SqlitePluginBundleStore>>>,
    lifecycle: std::sync::Arc<
        std::sync::RwLock<Option<std::sync::Arc<super::runtime::ThirdPartyPluginLifecycle>>>,
    >,
}

impl PluginUpdateService {
    pub fn open(paths: &gamenode_platform::paths::DataPaths) -> Result<Self, String> {
        PluginStateLayout::refuse_legacy_layouts(paths).map_err(|error| error.to_string())?;
        let registry = PluginPackageRegistry::open(paths).map_err(|error| error.to_string())?;
        let verifier =
            UpdateIndexVerifier::open(paths.plugin_packages_dir().join("update-sequences"))
                .map_err(|error| error.to_string())?;
        let client = FetchClient::new().map_err(|error| error.to_string())?;
        let private_dir = paths.private_dir();
        let grant_layout =
            PluginStateLayout::prepare_managed(paths).map_err(|error| error.to_string())?;
        fs::create_dir_all(&private_dir).map_err(|error| error.to_string())?;
        // Import and runtime share this store. The private directory only holds update metadata.
        let trust_dir = grant_layout.trust_dir();
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
            grant_layout,
            profiles: Default::default(),
            preferences_path,
            available_path,
            preferences: std::sync::Arc::new(std::sync::Mutex::new(preferences)),
            available: std::sync::Arc::new(std::sync::Mutex::new(available)),
            indexes: Default::default(),
            bundle_indexes: Default::default(),
            bundle_available: Default::default(),
            checks: Default::default(),
            activations: Default::default(),
            pending: Default::default(),
            pending_bundles: Default::default(),
            pending_rollbacks: Default::default(),
            imports: Default::default(),
            bundle_imports: Default::default(),
            bundles: Default::default(),
            lifecycle: Default::default(),
        })
    }

    /// Shares the application's bundle mapping with the inventory. Startup calls this once the
    /// settings store is open, so the inventory and uninstall observe the same connection.
    pub fn set_bundle_store(&self, bundles: SqlitePluginBundleStore) {
        *self
            .bundles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(bundles);
    }

    fn bundle_store(&self) -> Option<SqlitePluginBundleStore> {
        self.bundles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
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

    /// The connected managed runtime, if the host composed one.
    #[cfg(test)]
    pub(crate) fn lifecycle(
        &self,
    ) -> Option<std::sync::Arc<super::runtime::ThirdPartyPluginLifecycle>> {
        self.lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Re-reads one installed package through the same registry and publisher pin the startup
    /// composition uses. A command that accepts an import calls this afterwards so the new
    /// provider joins the running library and becomes scannable without a restart.
    pub async fn library_provider(
        &self,
        plugin_id: &str,
    ) -> Result<Option<gamenode_core::provider::Provider>, String> {
        let platform = gamenode_core::provider::Platform::current()
            .ok_or_else(|| "GameNode Desktop supports Windows and Linux only".to_owned())?;
        super::startup::installed_provider_from_registry(
            platform,
            self.registry.clone(),
            self.trust_dir.clone(),
            plugin_id,
        )
        .await
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
        let (publisher_key, _manifest) = tokio::task::spawn_blocking(move || {
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
        let private_dir = self.trust_dir.clone();
        let key = publisher_key;
        let package = tokio::task::spawn_blocking(move || {
            let trust = super::publisher_trust::PublisherTrustStore::open(private_dir)
                .map_err(|error| format!("publisher trust store is unavailable: {error}"))?;
            package_import_review(&report, &trust, &key)
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
            package,
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
        self.lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
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
        if pending.review.package.publisher_fingerprint != confirmed_fingerprint {
            return Err("publisher fingerprint confirmation does not match".into());
        }
        let private_dir = self.trust_dir.clone();
        let plugin_id = pending.review.package.plugin_id.clone();
        let publisher_key = pending.publisher_key;
        let fingerprint = confirmed_fingerprint.to_owned();
        let trust_status = pending.review.package.trust.clone();
        let pinned = tokio::task::spawn_blocking(move || {
            let _publisher = super::managed_packages::lock_publisher_identity();
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
        pending.review.package.trust = "trusted".into();
        pending.review.package.pinned_fingerprint = None;
        pending.review.package.publisher_fingerprint = pinned;
        Ok(pending.review.clone())
    }

    /// Reads a declared license or notice from the inert, validated import staging directory.
    /// The frontend supplies only the exact manifest path; arbitrary filesystem paths are denied.
    pub async fn import_license_text(&self, token: &str, file: &str) -> Result<String, String> {
        let imports = self.imports.lock().await;
        let pending = imports
            .get(token)
            .ok_or_else(|| "plugin import review expired".to_owned())?;
        if pending.created_at.elapsed() >= std::time::Duration::from_secs(15 * 60) {
            return Err("plugin import review expired".into());
        }
        if !pending
            .review
            .package
            .license_files
            .iter()
            .any(|path| path == file)
            && !pending
                .review
                .package
                .license_notices
                .iter()
                .any(|path| path == file)
        {
            return Err("file is not a declared license or notice".into());
        }
        let package_path = pending.package_path.clone();
        let file = file.to_owned();
        drop(imports);
        tokio::task::spawn_blocking(move || read_inert_license(&package_path, &file))
            .await
            .map_err(|_| "license viewer worker failed".to_owned())?
    }

    /// Imports a reviewed, compatible package as the selected version and applies the review's
    /// optional-permission decisions to the active profile. Every optional permission is granted
    /// unless the user denied it and every required permission is granted. No process is registered
    /// or executed by this operation.
    pub async fn import_install(
        &self,
        token: &str,
        native_code_consent: bool,
        denied_permission_ids: &[String],
    ) -> Result<PluginImportInstallOutcome, String> {
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
        if pending.review.package.compatibility != "compatible" {
            return Err("incompatible plugin packages cannot be imported".into());
        }
        if pending.review.package.trust != "trusted" {
            return Err("confirm the publisher fingerprint before importing".into());
        }
        if !pending.review.package.native_access.is_empty() && !native_code_consent {
            return Err("explicit native-code consent is required for this package".into());
        }
        // Unknown or required denials are rejected before any registry write.
        let denied = denied_permission_set(
            &pending.review.package.requested_permissions,
            denied_permission_ids,
        )?;
        let registry = self.registry.clone();
        let plugin_lookup = pending.review.package.plugin_id.clone();
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
        let package_id = pending.review.package.plugin_id.clone();
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
        let plugin_id = pending.review.package.plugin_id.clone();
        let publisher_fingerprint = pending.review.package.publisher_fingerprint.clone();
        let reviewed_permissions = !pending.review.package.requested_permissions.is_empty();
        self.packages
            .install_candidate(package_path, key, EmulationPolicy::RequireNative, true)
            .await
            .map_err(|error| error.to_string())?;
        imports.remove(token);
        drop(imports);
        if reviewed_permissions {
            self.apply_install_permission_review(&plugin_id, &publisher_fingerprint, &denied)
                .await?;
        }
        // Registration and activation happen now, not on the next composition. A failure leaves
        // the installed package and its reviewed grants in place and is reported to the caller.
        let activation =
            Self::register_and_activate_installed(&lifecycle, std::slice::from_ref(&plugin_id))
                .await;
        let outcome = activation.get(&plugin_id).cloned().unwrap_or(Ok(()));
        Ok(PluginImportInstallOutcome {
            plugin_id,
            activated: outcome.is_ok(),
            activation_error: outcome.err(),
        })
    }

    /// Resolves reviewed optional-permission decisions into the host's typed grant scopes for one
    /// just-installed package in the active profile. Required permissions are always granted and a
    /// denied optional permission is never silently re-granted.
    async fn import_permission_plan(
        &self,
        plugin_id: &str,
        publisher_fingerprint: &str,
        denied: &BTreeSet<String>,
    ) -> Result<ImportPermissionPlan, String> {
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        let snapshot = lifecycle
            .installed_permission_snapshot(plugin_id.to_owned(), publisher_fingerprint.to_owned())
            .await
            .map_err(|error| error.message().to_owned())?;
        let mut decisions = Vec::new();
        for request in &snapshot.requested_permissions {
            // A host-unknown optional permission has no enforceable scope, so it stays
            // unsupported rather than being presented as granted.
            if request.allowed_scopes.is_empty() {
                continue;
            }
            let granted = request.required || !denied.contains(&request.permission_id);
            for scope in &request.allowed_scopes {
                decisions.push(super::grants::PermissionDecision {
                    permission_id: request.permission_id.clone(),
                    scope: scope.clone(),
                    granted,
                });
            }
        }
        Ok(ImportPermissionPlan {
            profile_id: snapshot.profile_id,
            publisher_fingerprint: snapshot.publisher_fingerprint,
            grant_revision: snapshot.grant_revision,
            decisions,
        })
    }

    /// Submits one package's reviewed decisions through the same permission-review path the
    /// activation review uses. A failure is reported instead of leaving a denied permission
    /// granted.
    async fn apply_install_permission_review(
        &self,
        plugin_id: &str,
        publisher_fingerprint: &str,
        denied: &BTreeSet<String>,
    ) -> Result<(), String> {
        let plan = self
            .import_permission_plan(plugin_id, publisher_fingerprint, denied)
            .await
            .map_err(|error| {
                format!(
                    "plugin {plugin_id} was installed but its permission review failed: {error}"
                )
            })?;
        if plan.decisions.is_empty() {
            return Ok(());
        }
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        lifecycle
            .replace_installed_permissions(
                &self.activations,
                plugin_id.to_owned(),
                plan.profile_id,
                plan.publisher_fingerprint,
                plan.grant_revision,
                plan.decisions,
            )
            .await
            .map(|_| ())
            .map_err(|error| {
                format!(
                    "plugin {plugin_id} was installed but its permission decisions were not saved: {}",
                    error.message()
                )
            })
    }

    /// Registers just-installed packages with the live host and enables each one for the active
    /// profile. Every failure is reported per plugin; the installed package and its grants remain.
    async fn register_and_activate_installed(
        lifecycle: &std::sync::Arc<super::runtime::ThirdPartyPluginLifecycle>,
        plugin_ids: &[String],
    ) -> BTreeMap<String, Result<(), String>> {
        let registration = match lifecycle.register_installed_packages().await {
            Ok(outcomes) => outcomes,
            Err(error) => {
                let message = format!(
                    "the installed package runtime could not be registered: {}",
                    error.message()
                );
                return plugin_ids
                    .iter()
                    .map(|plugin_id| (plugin_id.clone(), Err(message.clone())))
                    .collect();
            }
        };
        let mut results = BTreeMap::new();
        for plugin_id in plugin_ids {
            if let Some(error) = registration
                .iter()
                .find(|outcome| outcome.plugin_id == *plugin_id)
                .and_then(|outcome| outcome.error.as_deref())
            {
                results.insert(
                    plugin_id.clone(),
                    Err(format!(
                        "the installed package runtime could not be registered: {error}"
                    )),
                );
                continue;
            }
            if !lifecycle.is_registered(plugin_id) {
                results.insert(
                    plugin_id.clone(),
                    Err("the installed package is not registered for this host".to_owned()),
                );
                continue;
            }
            results.insert(
                plugin_id.clone(),
                lifecycle
                    .enable(plugin_id.clone())
                    .await
                    .map_err(|error| format!("activation failed: {}", error.message())),
            );
        }
        results
    }

    /// Stages, verifies and reviews a user-selected bundle without executing component code.
    ///
    /// The bundle envelope and every component are validated by the bundle verifier and the
    /// ordinary package inspection, then each component's review is built by the same
    /// [`package_import_review`] a standalone import uses. No permission is granted here; the
    /// single publisher decision is [`Self::bundle_import_confirm_publisher`].
    pub async fn bundle_import_review(
        &self,
        archive_path: PathBuf,
    ) -> Result<BundleImportReview, String> {
        const MAX_PENDING_IMPORTS: usize = 8;
        const IMPORT_REVIEW_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
        {
            let mut imports = self.bundle_imports.lock().await;
            imports.retain(|_, pending| pending.created_at.elapsed() < IMPORT_REVIEW_TTL);
            if imports.len() >= MAX_PENDING_IMPORTS {
                return Err("too many plugin bundles are awaiting review".into());
            }
        }
        let selected = archive_path.clone();
        tokio::task::spawn_blocking(move || {
            let metadata = fs::symlink_metadata(&selected).map_err(|error| error.to_string())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("selected plugin bundle is not a regular file".to_owned());
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|_| "plugin bundle import staging worker failed".to_owned())??;
        let candidate =
            super::bundles::inspect_bundle_review(&archive_path, EmulationPolicy::RequireNative)
                .await
                .map_err(|error| format!("selected plugin bundle failed validation: {error}"))?;
        let super::bundles::BundleReviewCandidate {
            bundle,
            publisher_key,
            components: staged,
        } = candidate;
        let report = super::bundles::BundleCandidateReport {
            bundle: bundle.clone(),
            components: staged
                .iter()
                .map(|component| component.inspection.clone())
                .collect(),
        };
        let private_dir = self.trust_dir.clone();
        let key = publisher_key;
        let components = tokio::task::spawn_blocking(move || {
            let trust = super::publisher_trust::PublisherTrustStore::open(private_dir)
                .map_err(|error| format!("publisher trust store is unavailable: {error}"))?;
            staged
                .into_iter()
                .map(|component| {
                    let review = package_import_review(&component.report, &trust, &key)?;
                    Ok(BundleComponentReview {
                        kind: component.inspection.kind,
                        availability: component.inspection.availability,
                        review,
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .await
        .map_err(|_| "plugin bundle trust worker failed".to_owned())??;
        let (trust, pinned_fingerprint) =
            bundle_trust_state(components.iter().map(|component| &component.review));
        let license = bundle_license_summary(&components);
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|error| error.to_string())?;
        let token = nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let review = BundleImportReview {
            token: token.clone(),
            bundle,
            trust,
            pinned_fingerprint,
            license,
            components,
        };
        self.bundle_imports.lock().await.insert(
            token,
            PendingBundleImport {
                publisher_key,
                review: review.clone(),
                archive_path,
                report,
                created_at: std::time::Instant::now(),
            },
        );
        Ok(review)
    }

    /// Pins the bundle publisher key for every component in one write after the user confirms the
    /// exact fingerprint the review displayed. This is one publisher decision: either every
    /// component accepts the key or none does, and no permission is granted.
    pub async fn bundle_import_confirm_publisher(
        &self,
        token: &str,
        confirmed_fingerprint: &str,
    ) -> Result<BundleImportReview, String> {
        self.lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?
            .require_active_profile()
            .map_err(|error| error.message().to_owned())?;
        let mut imports = self.bundle_imports.lock().await;
        let pending = imports
            .get_mut(token)
            .ok_or_else(|| "plugin bundle import review expired".to_owned())?;
        if pending.created_at.elapsed() >= std::time::Duration::from_secs(15 * 60) {
            imports.remove(token);
            return Err("plugin bundle import review expired".into());
        }
        if pending.review.bundle.publisher_fingerprint != confirmed_fingerprint {
            return Err("publisher fingerprint confirmation does not match".into());
        }
        let private_dir = self.trust_dir.clone();
        let publisher_key = pending.publisher_key;
        let plugin_ids = pending
            .review
            .components
            .iter()
            .map(|component| component.review.plugin_id.clone())
            .collect::<Vec<_>>();
        let fingerprint = confirmed_fingerprint.to_owned();
        let pinned = tokio::task::spawn_blocking(move || {
            let _publisher = super::managed_packages::lock_publisher_identity();
            let mut trust = super::publisher_trust::PublisherTrustStore::open(private_dir)
                .map_err(|error| error.to_string())?;
            trust
                .confirm_publishers(&plugin_ids, &publisher_key, &fingerprint)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "publisher trust worker failed".to_owned())??;
        pending.review.trust = "trusted".into();
        pending.review.pinned_fingerprint = None;
        for component in &mut pending.review.components {
            component.review.trust = "trusted".into();
            component.review.pinned_fingerprint = None;
            component.review.publisher_fingerprint.clone_from(&pinned);
        }
        Ok(pending.review.clone())
    }

    /// Installs one reviewed bundle through the ordinary staged registry path. This is the
    /// approved step behind the single review: the publisher must be pinned for every component,
    /// native-code consent must cover a component that declares native access, and the archive on
    /// disk is re-verified against the reviewed component report before any registry write. The
    /// safety rules match [`Self::import_install`]; the difference is only that one bundle approval
    /// installs all components atomically and then applies each component's optional-permission
    /// decisions to the active profile.
    pub async fn bundle_import_install(
        &self,
        token: &str,
        native_code_consent: bool,
        component_denied_permissions: &[BundleComponentDeniedPermissions],
    ) -> Result<BundleImportInstallOutcome, String> {
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        lifecycle
            .require_active_profile()
            .map_err(|error| error.message().to_owned())?;
        let bundles = self
            .bundle_store()
            .ok_or_else(|| "plugin bundle mapping is unavailable".to_owned())?;
        let mut imports = self.bundle_imports.lock().await;
        let pending = imports
            .get(token)
            .ok_or_else(|| "plugin bundle import review expired".to_owned())?;
        if pending.created_at.elapsed() >= std::time::Duration::from_secs(15 * 60) {
            imports.remove(token);
            return Err("plugin bundle import review expired".into());
        }
        if pending.review.trust != "trusted" {
            return Err("confirm the bundle publisher fingerprint before importing".into());
        }
        if pending
            .review
            .components
            .iter()
            .any(|component| !component.review.native_access.is_empty())
            && !native_code_consent
        {
            return Err("explicit native-code consent is required for this bundle".into());
        }
        // Every component denial names a reviewed component, and every denied id is one of that
        // component's optional requests. This runs before any registry write.
        let mut denials: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for submission in component_denied_permissions {
            let Some(component) = pending
                .review
                .components
                .iter()
                .find(|component| component.review.plugin_id == submission.plugin_id)
            else {
                return Err(format!(
                    "plugin `{}` is not part of this bundle review",
                    submission.plugin_id
                ));
            };
            if denials.contains_key(&submission.plugin_id) {
                return Err(format!(
                    "component {} appears more than once in one grouped import review",
                    submission.plugin_id
                ));
            }
            let denied = denied_permission_set(
                &component.review.requested_permissions,
                &submission.denied_permission_ids,
            )?;
            denials.insert(submission.plugin_id.clone(), denied);
        }
        let private_dir = self.trust_dir.clone();
        let plugin_ids = pending
            .review
            .components
            .iter()
            .map(|component| component.review.plugin_id.clone())
            .collect::<Vec<_>>();
        let key = pending.publisher_key;
        let all_pinned = tokio::task::spawn_blocking(move || {
            let trust = super::publisher_trust::PublisherTrustStore::open(private_dir)
                .map_err(|error| error.to_string())?;
            for plugin_id in plugin_ids {
                if !trust
                    .is_pinned(&plugin_id, &key)
                    .map_err(|error| error.to_string())?
                {
                    return Ok::<_, String>(false);
                }
            }
            Ok(true)
        })
        .await
        .map_err(|_| "publisher trust worker failed".to_owned())??;
        if !all_pinned {
            return Err("publisher trust changed; review the bundle again".into());
        }
        let installed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs() as i64;
        let installed = super::bundles::install_bundle_candidate(
            &pending.archive_path,
            pending.report.clone(),
            self.registry.clone(),
            bundles,
            EmulationPolicy::RequireNative,
            installed_at,
        )
        .await
        .map_err(|error| error.to_string())?;
        let reviewed_components = pending.review.components.clone();
        imports.remove(token);
        drop(imports);
        let mut permission_errors = BTreeMap::new();
        let mut submissions = Vec::new();
        for component in &installed.components {
            let Some(reviewed) = reviewed_components
                .iter()
                .find(|reviewed| reviewed.review.plugin_id == component.plugin_id)
            else {
                continue;
            };
            if reviewed.review.requested_permissions.is_empty() {
                continue;
            }
            let denied = denials
                .get(&component.plugin_id)
                .cloned()
                .unwrap_or_default();
            if !component.active {
                if !denied.is_empty() {
                    permission_errors.insert(
                        component.plugin_id.clone(),
                        "the component is installed but not active on this host; its permission decisions cannot be saved yet"
                            .to_owned(),
                    );
                }
                continue;
            }
            match self
                .import_permission_plan(
                    &component.plugin_id,
                    &reviewed.review.publisher_fingerprint,
                    &denied,
                )
                .await
            {
                Ok(plan) if plan.decisions.is_empty() => {}
                Ok(plan) => submissions.push(BundleComponentPermissionSubmission {
                    plugin_id: component.plugin_id.clone(),
                    expected_profile_id: plan.profile_id,
                    expected_publisher_fingerprint: plan.publisher_fingerprint,
                    expected_revision: plan.grant_revision,
                    decisions: plan.decisions,
                }),
                Err(error) => {
                    permission_errors.insert(
                        component.plugin_id.clone(),
                        format!("permission review failed: {error}"),
                    );
                }
            }
        }
        if !submissions.is_empty() {
            let result = self.apply_installed_bundle_permissions(submissions).await?;
            for outcome in result.components {
                if let Some(error) = outcome.error {
                    permission_errors.insert(
                        outcome.plugin_id,
                        format!("permission decisions were not saved: {error}"),
                    );
                }
            }
        }
        // Enable every compatible component for the active profile now. Unavailable components
        // stay installed without a runtime; an activation failure keeps the component installed
        // and is reported instead of pretending it runs.
        let compatible_ids = installed
            .components
            .iter()
            .filter(|component| component.active)
            .map(|component| component.plugin_id.clone())
            .collect::<Vec<_>>();
        let mut activation =
            Self::register_and_activate_installed(&lifecycle, &compatible_ids).await;
        Ok(BundleImportInstallOutcome {
            bundle_id: installed.bundle.bundle_id,
            components: installed
                .components
                .into_iter()
                .map(|component| {
                    let outcome = component
                        .active
                        .then(|| activation.remove(&component.plugin_id))
                        .flatten();
                    let (activated, activation_error) = match outcome {
                        Some(Ok(())) => (true, None),
                        Some(Err(error)) => (false, Some(error)),
                        None => (false, None),
                    };
                    BundleImportInstalledComponent {
                        permission_error: permission_errors.remove(&component.plugin_id),
                        plugin_id: component.plugin_id,
                        kind: component.kind,
                        version: component.version,
                        active: component.active,
                        activated,
                        activation_error,
                    }
                })
                .collect(),
        })
    }

    /// Applies per-component permission decisions from one grouped review through the ordinary
    /// per-plugin grant path. Each component is independent: a rejected decision (including a
    /// stale grant revision) is reported for that component only and its siblings are still
    /// saved. This command itself grants nothing; every grant is an explicit component decision.
    pub async fn apply_bundle_permissions(
        &self,
        submissions: Vec<BundleComponentPermissionSubmission>,
    ) -> Result<BundlePermissionResult, String> {
        self.apply_bundle_permissions_with(submissions, true).await
    }

    /// The same per-component application for components a reviewed bundle import just installed
    /// and pinned. Their providers are registered only when the host next composes the runtime,
    /// so the installed-and-trusted package verification stands in for the registration check.
    async fn apply_installed_bundle_permissions(
        &self,
        submissions: Vec<BundleComponentPermissionSubmission>,
    ) -> Result<BundlePermissionResult, String> {
        self.apply_bundle_permissions_with(submissions, false).await
    }

    async fn apply_bundle_permissions_with(
        &self,
        submissions: Vec<BundleComponentPermissionSubmission>,
        require_registration: bool,
    ) -> Result<BundlePermissionResult, String> {
        const MAX_COMPONENTS: usize = 8;
        if submissions.is_empty() {
            return Err("a grouped permission review requires at least one component".into());
        }
        if submissions.len() > MAX_COMPONENTS {
            return Err(
                "a grouped permission review cannot exceed the bundle component limit".into(),
            );
        }
        let mut seen = BTreeSet::new();
        for submission in &submissions {
            if !seen.insert(submission.plugin_id.as_str()) {
                return Err(format!(
                    "component {} appears more than once in one grouped review",
                    submission.plugin_id
                ));
            }
        }
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        let mut components = Vec::with_capacity(submissions.len());
        for submission in submissions {
            let result = if require_registration {
                self.replace_permissions(
                    submission.plugin_id.clone(),
                    submission.expected_profile_id,
                    submission.expected_publisher_fingerprint,
                    submission.expected_revision,
                    submission.decisions,
                )
                .await
            } else {
                lifecycle
                    .replace_installed_permissions(
                        &self.activations,
                        submission.plugin_id.clone(),
                        submission.expected_profile_id,
                        submission.expected_publisher_fingerprint,
                        submission.expected_revision,
                        submission.decisions,
                    )
                    .await
            };
            let outcome = match result {
                Ok(snapshot) => BundleComponentPermissionOutcome {
                    plugin_id: submission.plugin_id,
                    snapshot: Some(snapshot),
                    error: None,
                },
                Err(status) => BundleComponentPermissionOutcome {
                    plugin_id: submission.plugin_id,
                    snapshot: None,
                    error: Some(status.message().to_owned()),
                },
            };
            components.push(outcome);
        }
        Ok(BundlePermissionResult { components })
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

    pub async fn replace_permissions(
        &self,
        plugin_id: String,
        expected_profile_id: String,
        expected_publisher_fingerprint: String,
        expected_revision: u64,
        decisions: Vec<super::grants::PermissionDecision>,
    ) -> Result<super::runtime::PluginPermissionSnapshot, tonic::Status> {
        // Package activation and permission review must agree on the active signed manifest.
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| tonic::Status::unavailable("managed plugin runtime is unavailable"))?;
        lifecycle
            .replace_permissions(
                &self.activations,
                plugin_id,
                expected_profile_id,
                expected_publisher_fingerprint,
                expected_revision,
                decisions,
            )
            .await
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

    /// Connects profile-scoped grants once the authenticated plugin profile source is created.
    pub fn set_profiles(&self, profiles: std::sync::Arc<PluginProfiles>) {
        *self
            .profiles
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(profiles);
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

    /// Returns inert metadata for installed, publisher-pinned packages. Signature and compatibility
    /// inspection use the package lifecycle worker; this inventory never starts plugin code.
    ///
    /// Installed bundles are aggregated: their components become one `bundle` entry that carries
    /// each component's own status, while standalone packages keep one entry each. Grouping comes
    /// from the registry mapping, never from matching names or publishers.
    pub async fn managed_inventory(&self) -> Result<Vec<super::PluginInfo>, String> {
        let registry = self.registry.clone();
        let trust_dir = self.trust_dir.clone();
        let installed = tokio::task::spawn_blocking(move || {
            registry
                .active_versions()
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "plugin registry worker failed".to_owned())??;
        let bundles: Vec<BundleRecord> = match self.bundle_store() {
            Some(store) => tokio::task::spawn_blocking(move || {
                store.bundles().map_err(|error| error.to_string())
            })
            .await
            .map_err(|_| "plugin bundle mapping worker failed".to_owned())??,
            None => Vec::new(),
        };
        let trust = tokio::task::spawn_blocking(move || {
            super::publisher_trust::PublisherTrustStore::open(trust_dir)
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "publisher trust worker failed".to_owned())??;
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let bundled: BTreeSet<&str> = bundles
            .iter()
            .flat_map(|bundle| bundle.components.iter())
            .map(|component| component.plugin_id.as_str())
            .collect();
        let mut inventory = Vec::new();
        for package in installed {
            if bundled.contains(package.plugin_id.as_str()) {
                continue;
            }
            let key = match package_key(&package.package_path) {
                Ok(key) => key,
                Err(_) => continue,
            };
            if !trust
                .is_pinned(&package.plugin_id, &key)
                .map_err(|error| error.to_string())?
            {
                continue;
            }
            let inspection = self
                .packages
                .inspect_candidate(&package.package_path, key, EmulationPolicy::RequireNative)
                .await;
            let Ok(report) = inspection else { continue };
            if report.manifest.id != package.plugin_id || report.manifest.version != package.version
            {
                continue;
            }
            let plugin_kind = match report.manifest.kind {
                super::package::PluginKind::Provider => "provider",
                super::package::PluginKind::Extension => "extension",
            };
            let is_extension = plugin_kind == "extension";
            let compatible = report.selected_target().is_ok();
            let runtime_available = compatible
                && (!is_extension
                    || lifecycle.as_ref().is_some_and(|lifecycle| {
                        lifecycle.extension_runtime_available(&package.plugin_id)
                    }));
            let enabled = match &lifecycle {
                Some(lifecycle) if is_extension => lifecycle
                    .enabled(package.plugin_id.clone())
                    .await
                    .unwrap_or(false),
                _ => false,
            };
            inventory.push(super::PluginInfo {
                provider_id: report.manifest.provider_id.clone().unwrap_or_default(),
                plugin_id: report.manifest.id.clone(),
                plugin_kind: plugin_kind.to_owned(),
                name: report.manifest.name.clone(),
                source: "community".to_owned(),
                status: "installed".to_owned(),
                capabilities: Vec::new(),
                session: "stopped".to_owned(),
                native_access: report.manifest.native_access.clone(),
                version: package.version,
                enabled: Some(enabled),
                runtime_available: Some(runtime_available),
                compatibility_details: Some(report.compatibility_review()),
                publisher_fingerprint: Some(super::publisher_trust::key_fingerprint(&key)),
                bundle_id: None,
                bundle_name: None,
                bundle_version: None,
                bundle_component_kind: None,
                bundle_publisher_fingerprint: None,
                bundle_components: None,
            });
        }
        for bundle in bundles {
            inventory.push(self.bundle_entry(&bundle, &trust, lifecycle.as_ref()).await);
        }
        Ok(inventory)
    }

    /// Builds the single inventory entry for one installed bundle from its mapping. Each component
    /// is inspected at the exact version the mapping recorded, so an installed-but-inactive
    /// component (for example an unavailable extension) is reported with its real status instead
    /// of disappearing from the inventory.
    async fn bundle_entry(
        &self,
        bundle: &BundleRecord,
        trust: &super::publisher_trust::PublisherTrustStore,
        lifecycle: Option<&std::sync::Arc<super::runtime::ThirdPartyPluginLifecycle>>,
    ) -> super::PluginInfo {
        let mut components = Vec::with_capacity(bundle.components.len());
        for component in &bundle.components {
            components.push(
                self.bundle_component_entry(component, trust, lifecycle)
                    .await,
            );
        }
        let status = if components
            .iter()
            .any(|component| component.status == "installed")
        {
            "installed"
        } else if components
            .iter()
            .any(|component| component.status == "failed")
        {
            "failed"
        } else if components
            .iter()
            .any(|component| component.status == "unavailable")
        {
            "unavailable"
        } else if components
            .iter()
            .any(|component| component.status == "incompatible")
        {
            "incompatible"
        } else {
            "missing"
        };
        let session = if components
            .iter()
            .any(|component| component.session == "active")
        {
            "active"
        } else {
            "stopped"
        };
        let mut native_access = Vec::new();
        for component in &components {
            for access in &component.native_access {
                if !native_access.contains(access) {
                    native_access.push(access.clone());
                }
            }
        }
        super::PluginInfo {
            provider_id: String::new(),
            plugin_id: bundle.bundle_id.clone(),
            plugin_kind: "bundle".to_owned(),
            name: bundle.name.clone(),
            version: bundle.version.clone(),
            source: "community".to_owned(),
            status: status.to_owned(),
            capabilities: Vec::new(),
            session: session.to_owned(),
            native_access,
            enabled: None,
            runtime_available: None,
            publisher_fingerprint: None,
            compatibility_details: None,
            bundle_id: Some(bundle.bundle_id.clone()),
            bundle_name: Some(bundle.name.clone()),
            bundle_version: Some(bundle.version.clone()),
            bundle_component_kind: None,
            bundle_publisher_fingerprint: Some(bundle.publisher_fingerprint.clone()),
            bundle_components: Some(components),
        }
    }

    async fn bundle_component_entry(
        &self,
        component: &gamenode_platform::plugin_bundle_db::BundleComponentRecord,
        trust: &super::publisher_trust::PublisherTrustStore,
        lifecycle: Option<&std::sync::Arc<super::runtime::ThirdPartyPluginLifecycle>>,
    ) -> super::PluginBundleComponent {
        let missing = |status: &str| super::PluginBundleComponent {
            plugin_id: component.plugin_id.clone(),
            plugin_kind: component.kind.clone(),
            name: component.plugin_id.clone(),
            version: component.package_version.clone(),
            status: status.to_owned(),
            session: "stopped".to_owned(),
            enabled: None,
            runtime_available: None,
            publisher_fingerprint: None,
            compatibility_details: None,
            native_access: Vec::new(),
        };
        let registry = self.registry.clone();
        let lookup_id = component.plugin_id.clone();
        let lookup_version = component.package_version.clone();
        let installed = tokio::task::spawn_blocking(move || registry.versions(&lookup_id))
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(|versions| {
                versions
                    .into_iter()
                    .find(|installed| installed.version == lookup_version)
            });
        let Some(installed) = installed else {
            return missing("missing");
        };
        let Ok(key) = package_key(&installed.package_path) else {
            return missing("failed");
        };
        if !trust.is_pinned(&installed.plugin_id, &key).unwrap_or(false) {
            return missing("failed");
        }
        let Ok(report) = self
            .packages
            .inspect_candidate(&installed.package_path, key, EmulationPolicy::RequireNative)
            .await
        else {
            return missing("failed");
        };
        if report.manifest.id != installed.plugin_id
            || report.manifest.version != installed.version
            || !matches!(
                (report.manifest.kind, component.kind.as_str()),
                (super::package::PluginKind::Provider, "provider")
                    | (super::package::PluginKind::Extension, "extension")
            )
        {
            return missing("failed");
        }
        let plugin_kind = match report.manifest.kind {
            super::package::PluginKind::Provider => "provider",
            super::package::PluginKind::Extension => "extension",
        };
        let compatible = report.selected_target().is_ok();
        let runtime_available = compatible
            && (plugin_kind == "provider"
                || lifecycle.is_some_and(|lifecycle| {
                    lifecycle.extension_runtime_available(&installed.plugin_id)
                }));
        let status = if !compatible {
            "incompatible"
        } else if plugin_kind == "extension" && !runtime_available {
            "unavailable"
        } else {
            "installed"
        };
        let enabled = match lifecycle {
            Some(lifecycle) if plugin_kind == "extension" => lifecycle
                .enabled(installed.plugin_id.clone())
                .await
                .unwrap_or(false),
            _ => false,
        };
        super::PluginBundleComponent {
            plugin_id: installed.plugin_id.clone(),
            plugin_kind: plugin_kind.to_owned(),
            name: report.manifest.name.clone(),
            version: installed.version,
            status: status.to_owned(),
            session: "stopped".to_owned(),
            enabled: Some(enabled),
            runtime_available: Some(runtime_available),
            publisher_fingerprint: Some(super::publisher_trust::key_fingerprint(&key)),
            compatibility_details: Some(report.compatibility_review()),
            native_access: report.manifest.native_access.clone(),
        }
    }

    /// Returns signed declarations for currently active, publisher-pinned third-party packages.
    /// Package reads and signature checks run on a blocking worker.
    pub async fn playtime_inventory(&self) -> Result<Vec<PluginPlaytimeInfo>, String> {
        let registry = self.registry.clone();
        let trust_dir = self.trust_dir.clone();
        tokio::task::spawn_blocking(move || {
            let (active, trust) = {
                let _publisher = super::managed_packages::lock_publisher_identity();
                let active = registry
                    .active_versions()
                    .map_err(|error| error.to_string())?;
                let trust = super::publisher_trust::PublisherTrustStore::open(trust_dir)
                    .map_err(|error| error.to_string())?;
                (active, trust)
            };
            let mut inventory = Vec::new();
            for installed in active {
                let key = match package_key(&installed.package_path) {
                    Ok(key) => key,
                    Err(_) => continue,
                };
                if !trust
                    .is_pinned(&installed.plugin_id, &key)
                    .map_err(|error| error.to_string())?
                {
                    continue;
                }
                let _integrity = match gamenode_platform::plugin_packages::verify_package_directory(
                    &installed.package_path,
                    &key,
                ) {
                    Ok(integrity) => integrity,
                    Err(_) => continue,
                };
                let bytes = match fs::read(installed.package_path.join("plugin.json")) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                };
                let manifest = match Manifest::parse_json(&bytes) {
                    Ok(manifest) => manifest.manifest().clone(),
                    Err(_) => continue,
                };
                if manifest.id != installed.plugin_id || manifest.version != installed.version {
                    continue;
                }
                // Only a package that declares a playtime capability belongs in this inventory.
                // Every other trusted package stays installed without a playtime row.
                if !manifest.playtime.declares_support() {
                    continue;
                }
                inventory.push(PluginPlaytimeInfo {
                    plugin_id: manifest.id,
                    name: manifest.name,
                    version: manifest.version,
                    playtime: manifest.playtime,
                    accounting_enabled: true,
                });
            }
            Ok::<_, String>(inventory)
        })
        .await
        .map_err(|_| "plugin inventory worker failed".to_owned())?
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
        let bundled_ids: BTreeSet<String> = match self.bundle_store() {
            Some(store) => tokio::task::spawn_blocking(move || store.bundles())
                .await
                .map_err(|_| "plugin bundle mapping worker failed".to_owned())?
                .map_err(|error| error.to_string())?
                .into_iter()
                .flat_map(|bundle| bundle.components)
                .map(|component| component.plugin_id)
                .collect(),
            None => BTreeSet::new(),
        };
        let mut updates = Vec::new();
        for installed in active {
            // A bundle component updates only as part of its bundle; offering a standalone update
            // here would mix the two installation models.
            if bundled_ids.contains(&installed.plugin_id) {
                continue;
            }
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
                    if release.bundle_sha256.is_some() {
                        return None;
                    }
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
        if let Some(store) = self.bundle_store() {
            let lookup = plugin_id.to_owned();
            let mapped = tokio::task::spawn_blocking(move || store.bundle_for_plugin(&lookup))
                .await
                .map_err(|_| "plugin bundle mapping worker failed".to_owned())?
                .map_err(|error| error.to_string())?;
            if mapped.is_some() {
                return Err(
                    "this plugin belongs to a bundle; review the bundle update instead".into(),
                );
            }
        }
        let current = self
            .packages
            .inspect_candidate(
                &active.package_path,
                package_key(&active.package_path)?,
                EmulationPolicy::RequireNative,
            )
            .await
            .map_err(|error| error.to_string())?;
        let feed_origin = current
            .manifest
            .update_url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok())
            .map(|url| url.origin().ascii_serialization())
            .ok_or_else(|| "plugin update URL is invalid".to_owned())?;
        let release = index
            .index()
            .releases
            .iter()
            .find(|release| release.version == version)
            .cloned()
            .ok_or_else(|| "release is no longer available".to_owned())?;
        if release.bundle_sha256.is_some() {
            return Err(
                "this index release is a signed bundle; use the bundle update review".into(),
            );
        }
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
        self.review_downloaded_release(active, current.manifest, &index, &release, archive)
            .await
    }

    async fn review_downloaded_release(
        &self,
        active: InstalledPluginVersion,
        current: Manifest,
        index: &VerifiedUpdateIndex,
        release: &UpdateRelease,
        archive: Vec<u8>,
    ) -> Result<UpdateReview, String> {
        let publisher_key = decode_publisher_key(&current.publisher.public_key)?;
        let (work, report, candidate_key) = ManagedPluginUpdater::new(self.registry.clone())
            .stage_review_candidate(index, release, archive, publisher_key)
            .await
            .map_err(|error| error.to_string())?;
        let compatibility_details = report.compatibility_review();
        let candidate_manifest = report.manifest;
        // An update can widen or malform `network.fetch`; reject unsupported scopes at review
        // time, as import does, rather than after activation breaks the permission screen.
        super::grants::manifest_permission_requests(&candidate_manifest.permissions)
            .map_err(|error| format!("plugin declares unsupported permission scopes: {error}"))?;
        let mut review = UpdateReview::compare(&current, &candidate_manifest);
        review.compatibility_details = Some(compatibility_details);
        review.publisher_fingerprint = super::publisher_trust::key_fingerprint(&candidate_key);
        self.pending.lock().await.insert(
            active.plugin_id,
            PendingCandidate {
                work: Some(work),
                version: release.version.clone(),
                installed_version: active.version,
                publisher_key,
                candidate_publisher_key: candidate_key,
                confirmed_retrust: false,
                is_rollback: false,
                installed_manifest: current,
                candidate_manifest,
                review: review.clone(),
            },
        );
        Ok(review)
    }

    /// Installed versions older than the active version. Feed results never include these.
    pub async fn rollback_versions(&self, plugin_id: &str) -> Result<Vec<String>, String> {
        let registry = self.registry.clone();
        let id = plugin_id.to_owned();
        let (active, versions) = tokio::task::spawn_blocking(move || {
            let active = registry
                .active(&id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "plugin is not installed".to_owned())?;
            let versions = registry.versions(&id).map_err(|error| error.to_string())?;
            Ok::<_, String>((active, versions))
        })
        .await
        .map_err(|_| "plugin registry worker failed".to_owned())??;
        let active_version = Version::parse(&active.version).map_err(|error| error.to_string())?;
        let mut versions: Vec<_> = versions
            .into_iter()
            .filter_map(|installed| {
                Version::parse(&installed.version)
                    .ok()
                    .filter(|version| version < &active_version)
                    .map(|_| installed.version)
            })
            .collect();
        versions.sort_by(|left, right| Version::parse(right).ok().cmp(&Version::parse(left).ok()));
        Ok(versions)
    }

    pub async fn available_rollbacks(&self) -> Result<Vec<AvailablePluginRollback>, String> {
        let registry = self.registry.clone();
        let active = tokio::task::spawn_blocking(move || registry.active_versions())
            .await
            .map_err(|_| "plugin registry worker failed".to_owned())?
            .map_err(|error| error.to_string())?;
        let mut available = Vec::new();
        for installed in active {
            let manifest = self
                .packages
                .inspect_candidate(
                    &installed.package_path,
                    package_key(&installed.package_path)?,
                    EmulationPolicy::RequireNative,
                )
                .await
                .map_err(|error| error.to_string())?
                .manifest;
            let versions = self.rollback_versions(&installed.plugin_id).await?;
            if !versions.is_empty() {
                available.push(AvailablePluginRollback {
                    plugin_id: installed.plugin_id,
                    name: manifest.name,
                    active_version: installed.version,
                    versions,
                });
            }
        }
        Ok(available)
    }

    /// Bundle releases cached by the last check. Unlike standalone updates these are not persisted:
    /// the index verifier's replay state is the durable part, and a review always re-downloads.
    pub fn bundle_update_available(&self) -> Vec<AvailableBundleUpdate> {
        self.bundle_available
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// Fetch each installed bundle's component feed and list releases that carry a bundle hash.
    /// Components mapped to a bundle are never listed as standalone updates, and a standalone
    /// plugin's feed releases without a bundle hash remain ordinary package updates.
    pub async fn check_bundle_updates(&self) -> Result<Vec<AvailableBundleUpdate>, String> {
        let _check = self.checks.lock().await;
        let Some(store) = self.bundle_store() else {
            return Ok(Vec::new());
        };
        let bundles = tokio::task::spawn_blocking(move || store.bundles())
            .await
            .map_err(|_| "plugin bundle mapping worker failed".to_owned())?
            .map_err(|error| error.to_string())?;
        let trust = super::publisher_trust::PublisherTrustStore::open(&self.trust_dir)
            .map_err(|error| error.to_string())?;
        let mut available = Vec::new();
        for bundle in bundles {
            let Some((installed, current)) = self.bundle_feed_manifest(&bundle).await? else {
                continue;
            };
            let Some(feed_url) = current.manifest.update_url.as_deref() else {
                continue;
            };
            let key = decode_publisher_key(&current.manifest.publisher.public_key)?;
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
            let installed_version =
                Version::parse(&bundle.version).map_err(|error| error.to_string())?;
            let mut versions: Vec<String> = verified
                .index()
                .releases
                .iter()
                .filter(|release| release.bundle_sha256.is_some())
                .filter_map(|release| {
                    Version::parse(&release.version)
                        .ok()
                        .filter(|version| version > &installed_version)
                        .map(|_| release.version.clone())
                })
                .collect();
            if versions.is_empty() {
                continue;
            }
            versions
                .sort_by(|left, right| Version::parse(right).ok().cmp(&Version::parse(left).ok()));
            self.bundle_indexes
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(bundle.bundle_id.clone(), verified);
            available.push(AvailableBundleUpdate {
                bundle_id: bundle.bundle_id,
                name: bundle.name,
                installed_version: bundle.version,
                versions,
            });
        }
        *self
            .bundle_available
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = available.clone();
        Ok(available)
    }

    /// The first bundle component whose active feed can carry a bundle release. Components are
    /// inspected at the exact version the mapping records, never at an unrelated standalone version.
    async fn bundle_feed_manifest(
        &self,
        bundle: &BundleRecord,
    ) -> Result<Option<(InstalledPluginVersion, PackageCandidateReport)>, String> {
        for component in &bundle.components {
            let registry = self.registry.clone();
            let plugin_id = component.plugin_id.clone();
            let version = component.package_version.clone();
            let installed = tokio::task::spawn_blocking(move || {
                registry.versions(&plugin_id).map(|versions| {
                    versions
                        .into_iter()
                        .find(|installed| installed.version == version)
                })
            })
            .await
            .map_err(|_| "plugin registry worker failed".to_owned())?
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                format!(
                    "bundle component {} version {} is no longer installed",
                    component.plugin_id, component.package_version
                )
            })?;
            let key = package_key(&installed.package_path)?;
            let report = self
                .packages
                .inspect_candidate(&installed.package_path, key, EmulationPolicy::RequireNative)
                .await
                .map_err(|error| error.to_string())?;
            if report.manifest.update_url.is_some() {
                return Ok(Some((installed, report)));
            }
        }
        Ok(None)
    }

    /// Reviews one signed bundle release from the feed. The downloaded archive must hash to the
    /// bundle hash the index committed to and to the same bundle identity, publisher and component
    /// set as the installed mapping; every component is staged and inspected without executing it.
    pub async fn review_bundle_update(
        &self,
        bundle_id: &str,
        version: &str,
    ) -> Result<BundleUpdateReview, String> {
        let Some(store) = self.bundle_store() else {
            return Err("plugin bundle mapping is unavailable".into());
        };
        let lookup = bundle_id.to_owned();
        let installed = tokio::task::spawn_blocking(move || store.bundle(&lookup))
            .await
            .map_err(|_| "plugin bundle mapping worker failed".to_owned())?
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "plugin bundle is not installed".to_owned())?;
        let index = self
            .bundle_indexes
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(bundle_id)
            .cloned()
            .ok_or_else(|| "check for bundle updates first".to_owned())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs();
        if index.index().expires_at <= now {
            return Err("the signed update index expired; check for updates again".into());
        }
        let release = index
            .index()
            .releases
            .iter()
            .find(|release| release.version == version)
            .cloned()
            .ok_or_else(|| "release is no longer available".to_owned())?;
        if release.bundle_sha256.is_none() {
            return Err(
                "this index release is a single package, not a bundle; use the package update review"
                    .into(),
            );
        }
        let Some((_feed_installed, current)) = self.bundle_feed_manifest(&installed).await? else {
            return Err("the bundle has no component with an update feed".into());
        };
        let feed_origin = current
            .manifest
            .update_url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok())
            .map(|url| url.origin().ascii_serialization())
            .ok_or_else(|| "bundle update URL is invalid".to_owned())?;
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
        let work = tokio::task::spawn_blocking(UpdateWorkDir::create)
            .await
            .map_err(|_| "bundle update staging worker failed".to_owned())?
            .map_err(|error| error.to_string())?;
        let archive_path = work.path.join("bundle.gnbundle");
        let write_path = archive_path.clone();
        tokio::task::spawn_blocking(move || fs::write(&write_path, archive))
            .await
            .map_err(|_| "bundle update staging worker failed".to_owned())?
            .map_err(|error| error.to_string())?;
        let (review, report) = self
            .bundle_update_review_from_archive(&installed, &release, &archive_path)
            .await?;
        if review.components.is_empty() {
            return Err("the bundle release has no components".into());
        }
        let first = installed
            .components
            .first()
            .ok_or_else(|| "the bundle mapping has no components".to_owned())?;
        let installed_report = self
            .inspect_installed_version(&first.plugin_id, &first.package_version)
            .await?;
        let publisher_key = decode_publisher_key(&installed_report.manifest.publisher.public_key)?;
        self.pending_bundles.lock().await.insert(
            bundle_id.to_owned(),
            PendingBundleUpdate {
                _work: work,
                archive_path,
                report,
                installed,
                release_version: version.to_owned(),
                publisher_key,
                review: review.clone(),
                created_at: std::time::Instant::now(),
            },
        );
        Ok(review)
    }

    /// Build the grouped bundle review from one downloaded archive. Factored from the feed path so
    /// the review can be exercised without a network and is the single place that decides which
    /// bundle releases are compatible with an installed mapping.
    pub(super) async fn bundle_update_review_from_archive(
        &self,
        installed: &BundleRecord,
        release: &UpdateRelease,
        archive_path: &Path,
    ) -> Result<(BundleUpdateReview, super::bundles::BundleCandidateReport), String> {
        let bundle_sha256 = release
            .bundle_sha256
            .as_deref()
            .ok_or_else(|| "the index release is not a signed bundle".to_owned())?;
        let archive_for_worker = archive_path.to_owned();
        let verified = tokio::task::spawn_blocking(move || {
            gamenode_platform::plugin_bundles::verify_bundle_archive(&archive_for_worker)
        })
        .await
        .map_err(|_| "bundle verification worker failed".to_owned())?
        .map_err(|error| error.to_string())?;
        if verified.manifest_sha256 != bundle_sha256 {
            return Err("the downloaded bundle does not match the index bundle hash".into());
        }
        if verified.manifest.id != installed.bundle_id {
            return Err("the bundle release belongs to another bundle".into());
        }
        let publisher_key = verified
            .manifest
            .publisher_key()
            .map_err(|e| e.to_string())?;
        if super::publisher_trust::key_fingerprint(&publisher_key)
            != installed.publisher_fingerprint
        {
            return Err("a bundle update cannot change the bundle publisher".into());
        }
        if release.version != verified.manifest.version {
            return Err("the index release version does not match the bundle version".into());
        }
        let installed_version =
            Version::parse(&installed.version).map_err(|error| error.to_string())?;
        let candidate_version =
            Version::parse(&verified.manifest.version).map_err(|error| error.to_string())?;
        if candidate_version <= installed_version {
            return Err("update is no longer newer than the installed bundle version".into());
        }
        let candidate =
            super::bundles::inspect_bundle_review(archive_path, EmulationPolicy::RequireNative)
                .await
                .map_err(|error| format!("selected bundle update failed validation: {error}"))?;
        let mapped = installed
            .components
            .iter()
            .map(|component| (component.plugin_id.as_str(), component.kind.as_str()))
            .collect::<BTreeSet<_>>();
        let candidate_set = candidate
            .components
            .iter()
            .map(|component| {
                (
                    component.inspection.plugin_id.as_str(),
                    component.inspection.kind.as_str(),
                )
            })
            .collect::<BTreeSet<_>>();
        if mapped != candidate_set || candidate.components.len() != installed.components.len() {
            return Err("a bundle update cannot add, remove or re-kind a component".into());
        }
        let mut components = Vec::with_capacity(candidate.components.len());
        for staged in &candidate.components {
            let previous = installed
                .components
                .iter()
                .find(|component| component.plugin_id == staged.inspection.plugin_id)
                .ok_or_else(|| "a bundle update cannot add a component".to_owned())?;
            let installed_report = self
                .inspect_installed_version(&previous.plugin_id, &previous.package_version)
                .await?;
            components.push(bundle_component_update_review(
                &installed_report.manifest,
                &staged.report.manifest,
                &staged.report,
                &previous.package_version,
                &staged.inspection.version,
            ));
        }
        let added_permissions =
            union_permissions(&components, |component| &component.added_permissions);
        let removed_permissions =
            union_permissions(&components, |component| &component.removed_permissions);
        let changed_permissions =
            union_permissions(&components, |component| &component.changed_permissions);
        let review = BundleUpdateReview {
            bundle_id: candidate.bundle.id.clone(),
            name: candidate.bundle.name.clone(),
            installed_version: installed.version.clone(),
            candidate_version: candidate.bundle.version.clone(),
            bundle_sha256: bundle_sha256.to_owned(),
            publisher_fingerprint: candidate.bundle.publisher_fingerprint.clone(),
            components,
            added_permissions,
            removed_permissions,
            changed_permissions,
            requires_manual_approval: true,
            is_downgrade: false,
            is_rollback: false,
        };
        let report = super::bundles::BundleCandidateReport {
            bundle: candidate.bundle,
            components: candidate
                .components
                .iter()
                .map(|component| component.inspection.clone())
                .collect(),
        };
        Ok((review, report))
    }

    async fn inspect_installed_version(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<PackageCandidateReport, String> {
        let registry = self.registry.clone();
        let lookup_id = plugin_id.to_owned();
        let wanted = version.to_owned();
        let installed = tokio::task::spawn_blocking(move || registry.versions(&lookup_id))
            .await
            .map_err(|_| "plugin registry worker failed".to_owned())?
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|installed| installed.version == wanted)
            .ok_or_else(|| {
                format!("plugin {plugin_id} version {version} is no longer installed")
            })?;
        let key = package_key(&installed.package_path)?;
        self.packages
            .inspect_candidate(&installed.package_path, key, EmulationPolicy::RequireNative)
            .await
            .map_err(|error| error.to_string())
    }

    /// Reviews an archived bundle version. The archived mapping is read from the bundle history,
    /// every component's installed package is inspected, and the review shows the current→target
    /// version and permission differences without executing anything.
    pub async fn review_bundle_rollback(
        &self,
        bundle_id: &str,
        version: &str,
    ) -> Result<BundleUpdateReview, String> {
        let Some(store) = self.bundle_store() else {
            return Err("plugin bundle mapping is unavailable".into());
        };
        let lookup = bundle_id.to_owned();
        let wanted = version.to_owned();
        let (current, target) = tokio::task::spawn_blocking(move || {
            let current = store
                .bundle(&lookup)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "plugin bundle is not installed".to_owned())?;
            let target = store
                .previous_bundle(&lookup, &wanted)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "that bundle version is not available for rollback".to_owned())?;
            Ok::<_, String>((current, target))
        })
        .await
        .map_err(|_| "plugin bundle mapping worker failed".to_owned())??;
        if target.components.len() != current.components.len() {
            return Err("the archived bundle version has a different component set".into());
        }
        let mut components = Vec::with_capacity(target.components.len());
        for archived in &target.components {
            let current_component = current
                .components
                .iter()
                .find(|component| component.plugin_id == archived.plugin_id)
                .ok_or_else(|| {
                    "the archived bundle version has a different component set".to_owned()
                })?;
            if current_component.kind != archived.kind {
                return Err("the archived bundle version has a different component set".into());
            }
            let current_report = self
                .inspect_installed_version(
                    &current_component.plugin_id,
                    &current_component.package_version,
                )
                .await?;
            let target_report = self
                .inspect_installed_version(&archived.plugin_id, &archived.package_version)
                .await?;
            let mut review = bundle_component_update_review(
                &current_report.manifest,
                &target_report.manifest,
                &target_report,
                &current_component.package_version,
                &archived.package_version,
            );
            review.is_downgrade = true;
            components.push(review);
        }
        let added_permissions =
            union_permissions(&components, |component| &component.added_permissions);
        let removed_permissions =
            union_permissions(&components, |component| &component.removed_permissions);
        let changed_permissions =
            union_permissions(&components, |component| &component.changed_permissions);
        let review = BundleUpdateReview {
            bundle_id: target.bundle_id.clone(),
            name: target.name.clone(),
            installed_version: current.version.clone(),
            candidate_version: target.version.clone(),
            bundle_sha256: target.manifest_sha256.clone(),
            publisher_fingerprint: target.publisher_fingerprint.clone(),
            components,
            added_permissions,
            removed_permissions,
            changed_permissions,
            requires_manual_approval: true,
            is_downgrade: true,
            is_rollback: true,
        };
        self.pending_rollbacks.lock().await.insert(
            bundle_id.to_owned(),
            PendingBundleRollback {
                version: version.to_owned(),
                created_at: std::time::Instant::now(),
            },
        );
        Ok(review)
    }

    pub async fn available_bundle_rollbacks(&self) -> Result<Vec<AvailableBundleRollback>, String> {
        let Some(store) = self.bundle_store() else {
            return Ok(Vec::new());
        };
        let bundles = tokio::task::spawn_blocking(move || store.bundles())
            .await
            .map_err(|_| "plugin bundle mapping worker failed".to_owned())?
            .map_err(|error| error.to_string())?;
        let store = self.bundle_store().expect("bundle store was checked");
        let mut available = Vec::new();
        for bundle in bundles {
            let id = bundle.bundle_id.clone();
            let lookup_store = store.clone();
            let versions = tokio::task::spawn_blocking(move || lookup_store.previous_versions(&id))
                .await
                .map_err(|_| "plugin bundle mapping worker failed".to_owned())?
                .map_err(|error| error.to_string())?;
            if !versions.is_empty() {
                available.push(AvailableBundleRollback {
                    bundle_id: bundle.bundle_id,
                    name: bundle.name,
                    active_version: bundle.version,
                    versions,
                });
            }
        }
        Ok(available)
    }

    /// Approves the reviewed bundle update. Every component is staged, installed and activated
    /// together; the archive is re-verified against the reviewed report first, so a changed file
    /// installs nothing. A failure keeps the previous set active.
    pub async fn approve_bundle_update(
        &self,
        bundle_id: &str,
        version: &str,
    ) -> Result<super::bundles::BundleUpdateOutcome, String> {
        let _activation = self.activations.lock().await;
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        lifecycle
            .require_active_profile()
            .map_err(|error| error.message().to_owned())?;
        let bundles = self
            .bundle_store()
            .ok_or_else(|| "plugin bundle mapping is unavailable".to_owned())?;
        let profiles = self
            .profiles
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
            .ok_or_else(|| "plugin profiles are unavailable".to_owned())?;
        let mut pending = self.pending_bundles.lock().await;
        let reviewed = pending
            .remove(bundle_id)
            .ok_or_else(|| "review the bundle update before approving it".to_owned())?;
        drop(pending);
        if reviewed.release_version != version || reviewed.review.candidate_version != version {
            return Err("the reviewed bundle update changed; review it again".into());
        }
        if reviewed.installed.version != self.current_bundle_version(bundle_id).await? {
            return Err(
                "the installed bundle changed after review; review the update again".into(),
            );
        }
        if reviewed.created_at.elapsed() >= std::time::Duration::from_secs(15 * 60) {
            return Err("the bundle update review expired; review it again".into());
        }
        let private_dir = self.trust_dir.clone();
        let plugin_ids = reviewed
            .installed
            .components
            .iter()
            .map(|component| component.plugin_id.clone())
            .collect::<Vec<_>>();
        let key = reviewed.publisher_key;
        let all_pinned = tokio::task::spawn_blocking(move || {
            let trust = super::publisher_trust::PublisherTrustStore::open(private_dir)
                .map_err(|error| error.to_string())?;
            for plugin_id in plugin_ids {
                if !trust
                    .is_pinned(&plugin_id, &key)
                    .map_err(|error| error.to_string())?
                {
                    return Ok::<_, String>(false);
                }
            }
            Ok(true)
        })
        .await
        .map_err(|_| "publisher trust worker failed".to_owned())??;
        if !all_pinned {
            return Err("publisher trust changed; review the bundle update again".into());
        }
        let installed_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs() as i64;
        let grants = super::bundles::BundleGrantContext {
            layout: self.grant_layout.clone(),
            profiles,
        };
        let result = super::bundles::apply_bundle_update(
            &reviewed.archive_path,
            reviewed.report,
            reviewed.installed,
            self.registry.clone(),
            bundles,
            lifecycle.as_ref(),
            grants,
            EmulationPolicy::RequireNative,
            installed_at,
        )
        .await
        .map_err(|error| error.to_string());
        if result.is_ok() {
            self.bundle_available
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .retain(|update| update.bundle_id != bundle_id);
        }
        result
    }

    async fn current_bundle_version(&self, bundle_id: &str) -> Result<String, String> {
        let store = self
            .bundle_store()
            .ok_or_else(|| "plugin bundle mapping is unavailable".to_owned())?;
        let lookup = bundle_id.to_owned();
        tokio::task::spawn_blocking(move || store.bundle(&lookup))
            .await
            .map_err(|_| "plugin bundle mapping worker failed".to_owned())?
            .map_err(|error| error.to_string())?
            .map(|bundle| bundle.version)
            .ok_or_else(|| "plugin bundle is not installed".to_owned())
    }

    /// Approves a reviewed rollback only after explicit confirmation. The archived versions and
    /// mapping are restored together and grants are intersected with the archived manifests.
    pub async fn approve_bundle_rollback(
        &self,
        bundle_id: &str,
        version: &str,
        confirmed: bool,
    ) -> Result<super::bundles::BundleRollbackOutcome, String> {
        if !confirmed {
            return Err("rollback requires explicit user confirmation".into());
        }
        let _activation = self.activations.lock().await;
        let lifecycle = self
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| "managed plugin runtime is unavailable".to_owned())?;
        lifecycle
            .require_active_profile()
            .map_err(|error| error.message().to_owned())?;
        let bundles = self
            .bundle_store()
            .ok_or_else(|| "plugin bundle mapping is unavailable".to_owned())?;
        let profiles = self
            .profiles
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
            .ok_or_else(|| "plugin profiles are unavailable".to_owned())?;
        let mut pending = self.pending_rollbacks.lock().await;
        let reviewed = pending
            .remove(bundle_id)
            .ok_or_else(|| "review the bundle rollback before approving it".to_owned())?;
        drop(pending);
        if reviewed.version != version
            || reviewed.created_at.elapsed() >= std::time::Duration::from_secs(15 * 60)
        {
            return Err("the reviewed bundle rollback changed; review it again".into());
        }
        let archived_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs() as i64;
        let grants = super::bundles::BundleGrantContext {
            layout: self.grant_layout.clone(),
            profiles,
        };
        super::bundles::rollback_bundle_version(
            bundle_id,
            version,
            self.registry.clone(),
            bundles,
            lifecycle.as_ref(),
            grants,
            EmulationPolicy::RequireNative,
            archived_at,
        )
        .await
        .map_err(|error| error.to_string())
    }

    pub async fn decline_bundle_update(&self, bundle_id: &str) {
        let candidate = self.pending_bundles.lock().await.remove(bundle_id);
        if let Some(candidate) = candidate {
            let _ = tokio::task::spawn_blocking(move || drop(candidate)).await;
        }
        self.pending_rollbacks.lock().await.remove(bundle_id);
    }

    /// Reviews an already installed, verified package as an explicit rollback candidate.
    pub async fn review_rollback(
        &self,
        plugin_id: &str,
        version: &str,
    ) -> Result<UpdateReview, String> {
        let registry = self.registry.clone();
        let id = plugin_id.to_owned();
        let wanted = version.to_owned();
        let (active, selected) = tokio::task::spawn_blocking(move || {
            let active = registry
                .active(&id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "plugin is not installed".to_owned())?;
            let selected = registry
                .versions(&id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|item| item.version == wanted)
                .ok_or_else(|| "rollback version is not installed".to_owned())?;
            Ok::<_, String>((active, selected))
        })
        .await
        .map_err(|_| "plugin registry worker failed".to_owned())??;
        if Version::parse(version).map_err(|error| error.to_string())?
            >= Version::parse(&active.version).map_err(|error| error.to_string())?
        {
            return Err("rollback must select a version older than the active version".into());
        }
        let current = self
            .packages
            .inspect_candidate(
                &active.package_path,
                package_key(&active.package_path)?,
                EmulationPolicy::RequireNative,
            )
            .await
            .map_err(|error| error.to_string())?;
        let current_key = decode_publisher_key(&current.manifest.publisher.public_key)?;
        let candidate_key = package_key(&selected.package_path)?;
        let candidate = self
            .packages
            .inspect_candidate(
                &selected.package_path,
                candidate_key,
                EmulationPolicy::RequireNative,
            )
            .await
            .map_err(|error| error.to_string())?;
        if candidate.manifest.id != plugin_id || candidate.manifest.version != version {
            return Err("rollback package identity does not match the selected version".into());
        }
        // An incompatible target is reviewable (the verdict is shown) but cannot be approved.
        super::grants::manifest_permission_requests(&candidate.manifest.permissions)
            .map_err(|error| format!("plugin declares unsupported permission scopes: {error}"))?;
        let mut review = UpdateReview::compare(&current.manifest, &candidate.manifest);
        review.compatibility_details = Some(candidate.compatibility_review());
        review.publisher_fingerprint = super::publisher_trust::key_fingerprint(&candidate_key);
        if !review.is_downgrade {
            return Err("selected package is not older than the active version".into());
        }
        self.pending.lock().await.insert(
            plugin_id.to_owned(),
            PendingCandidate {
                work: None,
                version: version.to_owned(),
                installed_version: active.version,
                publisher_key: current_key,
                candidate_publisher_key: candidate_key,
                confirmed_retrust: false,
                is_rollback: true,
                installed_manifest: current.manifest,
                candidate_manifest: candidate.manifest,
                review: review.clone(),
            },
        );
        Ok(review)
    }

    pub async fn approve(
        &self,
        plugin_id: &str,
        version: &str,
        target: Option<super::process::ExtensionTarget>,
    ) -> Result<String, String> {
        self.approve_reviewed(plugin_id, version, false, false, target)
            .await
    }

    /// Activates a rollback only after a separate explicit user confirmation.
    pub async fn approve_rollback(
        &self,
        plugin_id: &str,
        version: &str,
        confirmed: bool,
        target: Option<super::process::ExtensionTarget>,
    ) -> Result<String, String> {
        if !confirmed {
            return Err("rollback requires explicit user confirmation".into());
        }
        self.approve_reviewed(plugin_id, version, true, true, target)
            .await
    }

    async fn approve_reviewed(
        &self,
        plugin_id: &str,
        version: &str,
        is_rollback: bool,
        explicit_downgrade: bool,
        target: Option<super::process::ExtensionTarget>,
    ) -> Result<String, String> {
        let _activation = self.activations.lock().await;
        let candidate = self
            .pending
            .lock()
            .await
            .remove(plugin_id)
            .ok_or_else(|| "review the update before approving it".to_owned())?;
        if candidate.version != version || candidate.review.plugin_id != plugin_id {
            return Err("the reviewed update changed; review it again".into());
        }
        if candidate.is_rollback != is_rollback {
            return Err("reviewed package does not match the requested action".into());
        }
        if candidate
            .review
            .compatibility_details
            .as_ref()
            .is_some_and(|details| {
                details.state != super::compatibility::CompatibilityState::Compatible
            })
        {
            return Err("the reviewed package has no compatible target".into());
        }
        if candidate.review.extension_targets.is_some() != target.is_some() {
            self.pending
                .lock()
                .await
                .insert(plugin_id.to_owned(), candidate);
            return Err("extension approval requires a selected game".into());
        }
        // Refuse a stale review before installing anything; the activation worker re-checks
        // under the publisher lock.
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
        // New and rescaled requests never receive a grant from package activation.
        if candidate.review.publisher_changed && !candidate.confirmed_retrust {
            self.pending
                .lock()
                .await
                .insert(plugin_id.to_owned(), candidate);
            return Err("the changed publisher key must be re-trusted separately".into());
        }

        if candidate.is_rollback && (!candidate.review.is_downgrade || !explicit_downgrade) {
            return Err("the reviewed package is not a downgrade".into());
        }
        let version = self
            .activate_selected_candidate(plugin_id, candidate, target)
            .await?;
        let mut available = self
            .available
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        available.retain(|item| item.plugin_id != plugin_id);
        let _ = write_available(&self.available_path, &available);
        Ok(version)
    }

    /// Confirms only the exact publisher key fingerprint shown by the current review. This
    /// records the separate user decision in the pending review. The new key is persisted only
    /// after the reviewed package passes signature and compatibility checks at activation.
    pub async fn confirm_publisher_retrust(
        &self,
        plugin_id: &str,
        version: &str,
        confirmed_fingerprint: &str,
    ) -> Result<(), String> {
        let mut pending = self.pending.lock().await;
        let candidate = pending
            .get_mut(plugin_id)
            .ok_or_else(|| "review the package before re-trusting its publisher".to_owned())?;
        if candidate.version != version || !candidate.review.publisher_changed {
            return Err("the reviewed package has no publisher-key change".into());
        }
        if candidate.review.publisher_fingerprint != confirmed_fingerprint {
            return Err("publisher fingerprint does not match the reviewed key".into());
        }
        candidate.confirmed_retrust = true;
        Ok(())
    }

    async fn activate_selected_candidate(
        &self,
        plugin_id: &str,
        candidate: PendingCandidate,
        target: Option<super::process::ExtensionTarget>,
    ) -> Result<String, String> {
        use super::package::PluginKind;
        let extension = candidate.installed_manifest.kind == PluginKind::Extension
            || candidate.candidate_manifest.kind == PluginKind::Extension;
        if extension && candidate.installed_manifest.kind != candidate.candidate_manifest.kind {
            return Err("an update cannot change the extension's registration kind".into());
        }
        if extension && candidate.review.publisher_changed {
            return Err("the extension must be installed and granted access under its new publisher identity".into());
        }
        let key = candidate.candidate_publisher_key;
        let version = candidate.version.clone();
        let retained = if extension || candidate.work.is_none() {
            let registry = self.registry.clone();
            let id = plugin_id.to_owned();
            let wanted = version.clone();
            tokio::task::spawn_blocking(move || {
                registry
                    .versions(&id)
                    .map(|versions| versions.into_iter().find(|item| item.version == wanted))
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|_| "plugin registry worker failed".to_owned())??
        } else {
            None
        };
        let installed = match (candidate.work, retained) {
            (Some(work), Some(installed)) => {
                // A failed readiness check retains the inactive package. Reuse it on retry
                // only if it contains the same authenticated payload the user just reviewed.
                let root = installed.package_path.clone();
                tokio::task::spawn_blocking(move || {
                    use gamenode_platform::plugin_packages::verify_package_directory;
                    let staged = verify_package_directory(&work.path.join("package"), &key)
                        .map_err(|error| error.to_string())?;
                    let retained =
                        verify_package_directory(&root, &key).map_err(|error| error.to_string())?;
                    if staged.files.0 != retained.files.0 {
                        return Err(
                            "an installed version differs from the reviewed package".to_owned()
                        );
                    }
                    Ok(())
                })
                .await
                .map_err(|_| "plugin package worker failed".to_owned())??;
                installed
            }
            (Some(work), None) => self
                .packages
                .install_candidate(
                    work.path.join("package"),
                    key,
                    EmulationPolicy::RequireNative,
                    false,
                )
                .await
                .map_err(|error| error.to_string())?,
            (None, Some(installed)) => installed,
            (None, None) => return Err("rollback package was removed after review".into()),
        };
        let verified = self
            .packages
            .inspect_candidate(&installed.package_path, key, EmulationPolicy::RequireNative)
            .await
            .map_err(|error| error.to_string())?;
        if verified.manifest.id != plugin_id || verified.manifest.version != version {
            return Err("installed package no longer matches the reviewed version".into());
        }
        // Network revocation and grant intersection below use the reviewed manifest, so the
        // package being activated must declare exactly the permissions the user reviewed.
        if verified.manifest.permissions != candidate.candidate_manifest.permissions {
            return Err(
                "installed package permissions differ from the reviewed package; review again"
                    .into(),
            );
        }
        verified
            .selected_target()
            .map_err(|error| error.to_string())?;
        if extension {
            let lifecycle = self
                .lifecycle
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
                .ok_or_else(|| "extension runtime is unavailable".to_owned())?;
            if verified.manifest != candidate.candidate_manifest {
                return Err("the extension package changed after review".into());
            }
            let mut config = super::runtime::managed_process_config(&installed, &verified, key)
                .map_err(|error| error.message().to_owned())?;
            config.blocked_capabilities = candidate.review.removed_permissions;
            config.blocked_capabilities.extend(
                candidate
                    .review
                    .permission_changes
                    .into_iter()
                    .filter(|change| change.installed_scope != change.candidate_scope)
                    .map(|change| change.id),
            );
            let plugin_id = plugin_id.to_owned();
            let active_version = candidate.installed_version;
            tokio::spawn(async move {
                lifecycle
                    .activate_update(plugin_id, active_version, installed, config, target)
                    .await
            })
            .await
            .map_err(|_| "extension update task failed".to_owned())?
            .map_err(|error| error.message().to_owned())?;
            return Ok(version);
        }

        let registry = self.registry.clone();
        let trust_dir = self.trust_dir.clone();
        let changed_key = candidate.review.publisher_changed;
        let previous_version = candidate.installed_version;
        let previous_key = candidate.publisher_key;
        let fingerprint = candidate.review.publisher_fingerprint;
        let profiles = self
            .profiles
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let profile = profiles.as_ref().and_then(|profiles| profiles.current());
        let layout = self.grant_layout.clone();
        let installed_id = plugin_id.to_owned();
        let publisher = super::publisher_trust::key_fingerprint(&candidate.publisher_key);
        let selected_publisher = super::publisher_trust::key_fingerprint(&key);
        let previous = candidate.installed_manifest;
        let selected = candidate.candidate_manifest;
        fn network_scope(manifest: &Manifest) -> Option<&str> {
            manifest
                .permissions
                .iter()
                .find(|permission| {
                    permission.id == gamenode_plugin_protocol::capability::NETWORK_FETCH
                })
                .map(|permission| permission.scope.as_str())
        }
        tokio::task::spawn_blocking(move || {
            let _publisher = super::managed_packages::lock_publisher_identity();
            let active = registry
                .active(&installed_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "plugin is no longer installed".to_owned())?;
            if active.version != previous_version || package_key(&active.package_path)? != previous_key {
                return Err("the active plugin changed during approval; review again".into());
            }
            let mut trust = super::publisher_trust::PublisherTrustStore::open(trust_dir)
                .map_err(|error| error.to_string())?;
            if !trust.is_pinned(&installed_id, &previous_key).map_err(|error| error.to_string())? {
                return Err("the installed package publisher key is no longer trusted; review again".into());
            }

            let changed_network = network_scope(&previous) != network_scope(&selected);
            if changed_network || changed_key {
                layout
                    .revoke_network_for_package(&installed_id, &publisher)
                    .map_err(|error| error.to_string())?;
            }
            if changed_key {
                layout
                    .revoke_network_for_package(&installed_id, &selected_publisher)
                    .map_err(|error| error.to_string())?;
            }
            if (changed_network || changed_key)
                && let Some(profiles) = &profiles
            {
                profiles.notify_permissions_changed();
            }
            if let (Some(profiles), Some(profile)) = (profiles, profile) {
                let identity = PluginIdentity {
                    plugin_id: installed_id.clone(),
                    publisher_fingerprint: publisher,
                    profile_id: profile.profile_id().to_owned(),
                };
                let mut grants = layout
                    .open_grants(&profile)
                    .map_err(|error| error.to_string())?;
                let before = grants.revision(&identity);
                let after = grants
                    .intersect_with_selected_manifest(&identity, &previous, &selected)
                    .map_err(|error| error.to_string())?;
                if after != before {
                    profiles.notify_permissions_changed();
                }
            }
            registry
                .set_active(&installed_id, &version)
                .map_err(|error| error.to_string())?;
            if changed_key
                && let Err(trust_error) = trust.retrust_key(&installed_id, &key, &fingerprint)
            {
                registry
                    .set_active(&installed_id, &previous_version)
                    .map_err(|rollback_error| {
                        format!(
                            "publisher re-trust failed ({trust_error}); restoring the prior package also failed ({rollback_error})"
                        )
                    })?;
                return Err(trust_error.to_string());
            }
            Ok(version)
        })
        .await
        .map_err(|_| "plugin activation worker failed".to_owned())?
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) async fn stage_extension_update_for_test(
        &self,
        previous: &InstalledPluginVersion,
        candidate: &Path,
    ) {
        let work = UpdateWorkDir::create().unwrap();
        let package = work.path.join("package");
        fs::create_dir(&package).unwrap();
        for entry in fs::read_dir(candidate).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), package.join(entry.file_name())).unwrap();
        }
        let read = |root: &Path| {
            Manifest::parse_json(&fs::read(root.join("plugin.json")).unwrap())
                .unwrap()
                .manifest()
                .clone()
        };
        let installed_manifest = read(&previous.package_path);
        let candidate_manifest = read(&package);
        let review = UpdateReview::compare(&installed_manifest, &candidate_manifest);
        self.pending.lock().await.insert(
            previous.plugin_id.clone(),
            PendingCandidate {
                work: Some(work),
                version: candidate_manifest.version.clone(),
                installed_version: previous.version.clone(),
                publisher_key: package_key(&previous.package_path).unwrap(),
                candidate_publisher_key: package_key(&package).unwrap(),
                confirmed_retrust: false,
                is_rollback: false,
                installed_manifest,
                candidate_manifest,
                review,
            },
        );
    }

    pub async fn decline(&self, plugin_id: &str) {
        let candidate = self.pending.lock().await.remove(plugin_id);
        if let Some(candidate) = candidate {
            let _ = tokio::task::spawn_blocking(move || drop(candidate)).await;
        }
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
        ApiRange, LicenseNotice, Manifest, PermissionRequest, PlaytimeCapabilities, PluginKind,
        Publisher, Target,
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
                impact_if_disabled: None,
            }],
            playtime: PlaytimeCapabilities::default(),
            native_access: native.iter().map(|value| (*value).to_owned()).collect(),
            supported_platforms: vec![],
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
            impact_if_disabled: None,
        });
        after.publisher.key_id = "key-2".into();
        after.publisher.public_key = "different-key".into();
        let review = UpdateReview::compare(&before, &after);
        assert_eq!(review.added_permissions, ["network.fetch"]);
        assert_eq!(review.changed_permissions, ["files.read"]);
        assert_eq!(review.added_native_access, ["process"]);
        assert!(review.api_changed && review.license_changed && review.publisher_changed);
        assert!(review.requires_manual_approval);
    }

    #[test]
    fn review_discloses_signed_playtime_policy_changes() {
        let before = manifest("1.0.0", "game:data", &[], "MIT", "1.4.0");
        let mut after = manifest("1.1.0", "game:data", &[], "MIT", "1.4.0");
        after.playtime = PlaytimeCapabilities {
            identifies_gameplay: true,
            identifies_pauses: false,
            without_pause_detection: super::super::package::PlaytimeAccountingMode::ApplicationOpen,
        };

        let review = UpdateReview::compare(&before, &after);
        assert!(review.playtime_changed);
        assert_eq!(review.installed_playtime, before.playtime);
        assert_eq!(review.candidate_playtime, after.playtime);
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

    #[test]
    fn import_decision_inputs_use_camel_case_fields_and_omit_absent_impact() {
        let permission = ImportPermissionReview {
            id: "network.fetch".into(),
            required: false,
            scope: "https://api.example.com".into(),
            reason: "Fetch metadata".into(),
            impact_if_disabled: Some("Metadata stays unavailable".into()),
        };
        let value = serde_json::to_value(&permission).unwrap();
        assert_eq!(value["impactIfDisabled"], "Metadata stays unavailable");
        let absent = ImportPermissionReview {
            impact_if_disabled: None,
            ..permission
        };
        assert!(
            serde_json::to_value(&absent)
                .unwrap()
                .get("impactIfDisabled")
                .is_none()
        );

        let denial = BundleComponentDeniedPermissions {
            plugin_id: "org.example.plugin".into(),
            denied_permission_ids: vec!["network.fetch".into()],
        };
        let value = serde_json::to_value(&denial).unwrap();
        assert_eq!(value["pluginId"], "org.example.plugin");
        assert_eq!(value["deniedPermissionIds"][0], "network.fetch");
        let parsed: BundleComponentDeniedPermissions = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, denial);
    }
}

/// One host-approved release and the verified sources needed to install it.
pub struct ApprovedRelease<'a, F> {
    pub verifier: &'a UpdateIndexVerifier,
    pub client: &'a FetchClient,
    pub index: &'a VerifiedUpdateIndex,
    pub version: &'a str,
    pub publisher_key: [u8; 32],
    pub emulation_policy: EmulationPolicy,
    pub allowed_origin: F,
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
        _publisher_key: [u8; 32],
    ) -> Result<
        (
            UpdateWorkDir,
            super::managed_packages::PackageCandidateReport,
            [u8; 32],
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
            gamenode_platform::plugin_packages::stage_package_archive_from_verified_feed(
                &archive_for_worker,
                &package_for_worker,
            )
            .map_err(|error| ManagedUpdateError::Staging(error.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|_| ManagedUpdateError::WorkerFailed)??;
        let candidate_key = package_key(&package_path).map_err(ManagedUpdateError::Staging)?;
        let report = self
            .packages
            .inspect_candidate(&package_path, candidate_key, EmulationPolicy::RequireNative)
            .await?;
        if report.manifest.id != index.index().plugin_id
            || report.manifest.version != release.version
        {
            return Err(ManagedUpdateError::PackageIdentityMismatch);
        }
        Ok((work, report, candidate_key))
    }

    /// Download and install one release after caller approval. The index and package are both
    /// validated before the registry can change its active pointer. Any fetch, extraction,
    /// compatibility or signature failure leaves the old pointer intact.
    pub async fn apply_approved_release<F>(
        &self,
        release: ApprovedRelease<'_, F>,
    ) -> Result<InstalledPluginVersion, ManagedUpdateError>
    where
        F: Fn(&str) -> bool + Send + Sync,
    {
        let ApprovedRelease {
            verifier,
            client,
            index,
            version,
            publisher_key,
            emulation_policy,
            allowed_origin,
        } = release;
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
pub(super) mod tests {
    use super::*;
    use base64::Engine as _;
    use gamenode_platform::{
        plugin_packages::SIGNATURE_DOMAIN_PREFIX,
        plugin_updates::{INDEX_SIGNATURE_DOMAIN, UpdateRelease},
    };
    use ring::signature::KeyPair;
    use std::{
        collections::{BTreeMap, BTreeSet},
        io::Write,
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn offline_license_reader_is_bounded_utf8_and_package_relative() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("licenses")).unwrap();
        fs::write(temp.path().join("licenses/LICENSE"), "MIT notice").unwrap();
        assert_eq!(
            read_inert_license(temp.path(), "licenses/LICENSE").unwrap(),
            "MIT notice"
        );
        assert!(read_inert_license(temp.path(), "../outside").is_err());
        assert!(read_inert_license(temp.path(), "licenses/missing").is_err());
        fs::write(temp.path().join("licenses/binary"), [0xff, 0xfe]).unwrap();
        assert!(read_inert_license(temp.path(), "licenses/binary").is_err());
        fs::write(
            temp.path().join("licenses/large"),
            vec![b'x'; 256 * 1024 + 1],
        )
        .unwrap();
        assert!(read_inert_license(temp.path(), "licenses/large").is_err());
    }

    #[tokio::test]
    async fn import_review_returns_playtime_capabilities_from_the_signed_manifest() {
        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        let service = import_service(&paths);
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[43; 32]).unwrap();
        let package = base.path().join("playtime-disclosure");
        signed_package_with_playtime(
            &package,
            "1.0.0",
            &key,
            serde_json::json!({
                "identifies_gameplay": true,
                "identifies_pauses": false,
                "without_pause_detection": "application_open"
            }),
        );

        let review = service
            .import_review(archive_package(&package))
            .await
            .unwrap();
        assert_eq!(
            review.package.playtime,
            PlaytimeCapabilities {
                identifies_gameplay: true,
                identifies_pauses: false,
                without_pause_detection:
                    super::super::package::PlaytimeAccountingMode::ApplicationOpen,
            }
        );
        let serialized = serde_json::to_value(&review.package).unwrap();
        assert_eq!(
            serialized["playtime"],
            serde_json::json!({
                "identifiesGameplay": true,
                "identifiesPauses": false,
                "withoutPauseDetection": "application_open"
            })
        );
    }

    fn signed_package(root: &Path, version: &str, key: &ring::signature::Ed25519KeyPair) {
        signed_package_with_permissions(
            root,
            version,
            key,
            vec![serde_json::json!({
                "id": "storage.persist", "required": false, "scope": "own-provider",
                "reason": "Store plugin preferences"
            })],
        );
    }

    /// A package that discloses content platforms and what each permission loses when disabled.
    fn disclosure_permissions() -> Vec<serde_json::Value> {
        vec![
            serde_json::json!({
                "id": "storage.persist", "required": true, "scope": "own-provider",
                "reason": "Store plugin preferences",
                "impact_if_disabled": "Preferences are lost between sessions"
            }),
            serde_json::json!({
                "id": "network.fetch", "required": false, "scope": "https://api.example.com",
                "reason": "Fetch remote metadata",
                "impact_if_disabled": "Remote artwork and metadata stay unavailable"
            }),
        ]
    }

    #[tokio::test]
    async fn import_review_exposes_content_platforms_and_permission_impact() {
        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        let service = import_service(&paths);
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[44; 32]).unwrap();
        let package = base.path().join("disclosures");
        let mut manifest = fixture_manifest("1.0.0", disclosure_permissions());
        manifest["supported_platforms"] = serde_json::json!(["gamecube", "wii"]);
        signed_package_from_manifest(&package, &key, &manifest);

        let review = service
            .import_review(archive_package(&package))
            .await
            .unwrap();
        assert_eq!(review.package.supported_platforms, ["gamecube", "wii"]);
        assert_eq!(
            review.package.requested_permissions[0]
                .impact_if_disabled
                .as_deref(),
            Some("Preferences are lost between sessions")
        );
        assert_eq!(
            review.package.requested_permissions[1]
                .impact_if_disabled
                .as_deref(),
            Some("Remote artwork and metadata stay unavailable")
        );
        // The command surface keeps camelCase field names for the Tauri IPC boundary.
        let serialized = serde_json::to_value(&review.package).unwrap();
        assert_eq!(
            serialized["supportedPlatforms"],
            serde_json::json!(["gamecube", "wii"])
        );
        assert_eq!(
            serialized["requestedPermissions"][0]["impactIfDisabled"],
            "Preferences are lost between sessions"
        );
    }

    #[tokio::test]
    async fn import_install_grants_reviewed_permissions_except_denied_optionals() {
        use crate::plugin_host::grants::PermissionState;

        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        let service = import_service(&paths);
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[45; 32]).unwrap();
        let package = base.path().join("reviewed-decisions");
        signed_package_from_manifest(
            &package,
            &key,
            &fixture_manifest("1.0.0", disclosure_permissions()),
        );
        let imported = import_review_and_pin(&service, &package).await;

        let plugin_id = service
            .import_install(&imported.token, false, &["network.fetch".to_owned()])
            .await
            .unwrap();
        assert_eq!(plugin_id.plugin_id, "org.example.update");

        let lifecycle = service
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap();
        let snapshot = lifecycle
            .installed_permission_snapshot(
                plugin_id.plugin_id.clone(),
                imported.package.publisher_fingerprint.clone(),
            )
            .await
            .unwrap();
        assert!(snapshot.grant_revision > 0);
        let state = |permission_id: &str| {
            snapshot
                .effective_grants
                .iter()
                .find(|grant| grant.permission_id == permission_id)
                .map(|grant| grant.state.clone())
        };
        assert_eq!(state("storage.persist"), Some(PermissionState::Granted));
        assert_eq!(state("network.fetch"), Some(PermissionState::Denied));
    }

    #[tokio::test]
    async fn import_install_rejects_required_and_unknown_denials_before_installing() {
        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        let service = import_service(&paths);
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[46; 32]).unwrap();
        let package = base.path().join("rejected-decisions");
        signed_package_from_manifest(
            &package,
            &key,
            &fixture_manifest("1.0.0", disclosure_permissions()),
        );
        let imported = import_review_and_pin(&service, &package).await;
        for (denied, message) in [
            (vec!["storage.persist".to_owned()], "required permission"),
            (vec!["files.read".to_owned()], "not requested"),
            (
                vec!["network.fetch".to_owned(), "network.fetch".to_owned()],
                "more than once",
            ),
        ] {
            let error = service
                .import_install(&imported.token, false, &denied)
                .await
                .unwrap_err();
            assert!(error.contains(message), "{error}");
            assert!(
                service
                    .registry
                    .active("org.example.update")
                    .unwrap()
                    .is_none(),
                "a rejected denial must not install anything"
            );
        }

        // The same reviewed token still installs when the decisions are valid.
        assert_eq!(
            service
                .import_install(&imported.token, false, &[])
                .await
                .unwrap()
                .plugin_id,
            "org.example.update"
        );
        assert!(
            service
                .registry
                .active("org.example.update")
                .unwrap()
                .is_some()
        );
    }

    pub(in crate::plugin_host) fn signed_package_with_permissions(
        root: &Path,
        version: &str,
        key: &ring::signature::Ed25519KeyPair,
        permissions: Vec<serde_json::Value>,
    ) {
        signed_package_with_permissions_and_playtime(root, version, key, permissions, None);
    }

    fn signed_package_with_playtime(
        root: &Path,
        version: &str,
        key: &ring::signature::Ed25519KeyPair,
        playtime: serde_json::Value,
    ) {
        signed_package_with_permissions_and_playtime(
            root,
            version,
            key,
            vec![serde_json::json!({
                "id": "storage.persist", "required": false, "scope": "own-provider",
                "reason": "Store plugin preferences"
            })],
            Some(playtime),
        );
    }

    fn signed_package_with_permissions_and_playtime(
        root: &Path,
        version: &str,
        key: &ring::signature::Ed25519KeyPair,
        permissions: Vec<serde_json::Value>,
        playtime: Option<serde_json::Value>,
    ) {
        let mut manifest_value = fixture_manifest(version, permissions);
        if let Some(playtime) = playtime {
            manifest_value["playtime"] = playtime;
        }
        signed_package_from_manifest(root, key, &manifest_value);
    }

    fn fixture_manifest(version: &str, permissions: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "id": "org.example.update",
            "kind": "provider",
            "provider_id": "update-fixture",
            "name": "Update Fixture",
            "version": version,
            "api": {"min": "1.6.0", "max": "1.6.0"},
            "permissions": permissions,
            "targets": [{"os": "linux", "arch": "x86_64", "abi": "gnu", "entrypoint": "bin/plugin"}],
            "publisher": {
                "key_id": "fixture-key",
                "public_key": ""
            },
            "license": {"expression": "MIT", "files": ["licenses/LICENSE"]}
        })
    }

    fn extension_fixture_manifest(version: &str) -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "id": "org.example.launch",
            "kind": "extension",
            "name": "Launch Fixture",
            "version": version,
            "api": {"min": "1.6.0", "max": "1.6.0"},
            "permissions": [{
                "id": "game.launch", "required": true, "scope": "selected game",
                "reason": "Start the selected game"
            }],
            "targets": [{"os": "linux", "arch": "x86_64", "abi": "gnu", "entrypoint": "bin/plugin"}],
            "publisher": {
                "key_id": "fixture-key",
                "public_key": ""
            },
            "license": {"expression": "MIT", "files": ["licenses/LICENSE"]}
        })
    }

    fn signed_package_from_manifest(
        root: &Path,
        key: &ring::signature::Ed25519KeyPair,
        manifest_value: &serde_json::Value,
    ) {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("licenses")).unwrap();
        fs::write(root.join("bin/plugin"), b"fixture executable").unwrap();
        fs::write(root.join("licenses/LICENSE"), b"fixture license").unwrap();
        let mut manifest_value = manifest_value.clone();
        manifest_value["publisher"]["public_key"] = serde_json::Value::String(
            base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref()),
        );
        let manifest = serde_json::to_vec(&manifest_value).unwrap();
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

    /// Stages the real example `package/` tree as a signed fixture. The source tree contains no
    /// built executable, so every target entrypoint the manifest declares is filled with inert
    /// fixture bytes. Everything else — the manifest, license, notices and payload helpers — is
    /// copied exactly as the pack tool would ship it.
    fn signed_example_package(root: &Path, example: &str, key: &ring::signature::Ed25519KeyPair) {
        fn collect(base: &Path, current: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
            for entry in fs::read_dir(current).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    collect(base, &path, files);
                    continue;
                }
                let relative = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if relative == "plugin.json" {
                    continue;
                }
                files.insert(relative, fs::read(&path).unwrap());
            }
        }

        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = workspace
            .join("examples/plugins")
            .join(example)
            .join("package");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(source.join("plugin.json")).unwrap()).unwrap();
        manifest["publisher"]["public_key"] = serde_json::Value::String(
            base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref()),
        );
        let manifest = serde_json::to_vec_pretty(&manifest).unwrap();
        let mut files = BTreeMap::new();
        collect(&source, &source, &mut files);
        for target in manifest_targets(&manifest) {
            files.insert(
                target,
                b"fixture executable; never run in import test".to_vec(),
            );
        }
        files.insert("plugin.json".to_owned(), manifest);

        for (relative, bytes) in &files {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let integrity = serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "files": files
                .iter()
                .map(|(path, bytes)| (path.clone(), format!("{:x}", sha2::Sha256::digest(bytes))))
                .collect::<BTreeMap<_, _>>()
        }))
        .unwrap();
        let mut signed = SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(&integrity);
        fs::write(root.join("integrity.json"), integrity).unwrap();
        fs::write(root.join("signature.ed25519"), key.sign(&signed).as_ref()).unwrap();
    }

    /// The entrypoints a manifest declares, read from the JSON before the typed parser runs so a
    /// malformed manifest fails in the importer rather than in this fixture.
    fn manifest_targets(manifest: &[u8]) -> Vec<String> {
        let value: serde_json::Value = serde_json::from_slice(manifest).unwrap();
        value["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|target| target["entrypoint"].as_str().map(str::to_owned))
            .collect()
    }

    pub(in crate::plugin_host) fn add_tree(
        zip: &mut ZipWriter<fs::File>,
        base: &Path,
        current: &Path,
    ) {
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
        signed_package_with_playtime(
            &v1,
            "1.0.0",
            &key,
            serde_json::json!({
                "identifies_gameplay": true,
                "identifies_pauses": true,
                "without_pause_detection": "disabled"
            }),
        );
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
                bundle_sha256: None,
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
            bundle_sha256: None,
        };
        let index = UpdateIndex {
            schema: 1,
            plugin_id: "org.example.update".into(),
            sequence: 1,
            expires_at: u64::MAX,
            releases: vec![release.clone()],
        };

        let feed_downgrade = UpdateRelease {
            version: "0.9.0".into(),
            ..release.clone()
        };
        let downgrade_index = UpdateIndex {
            releases: vec![feed_downgrade.clone()],
            ..index.clone()
        };
        assert!(matches!(
            updater
                .install_verified_archive(
                    &downgrade_index,
                    &feed_downgrade,
                    archive.clone(),
                    publisher_key,
                    EmulationPolicy::RequireNative,
                )
                .await,
            Err(ManagedUpdateError::Downgrade)
        ));

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

    fn archive_package(package: &Path) -> PathBuf {
        let archive = package.with_extension("gnplugin");
        let mut zip = ZipWriter::new(fs::File::create(&archive).unwrap());
        add_tree(&mut zip, package, package);
        zip.finish().unwrap();
        archive
    }

    fn import_service(paths: &gamenode_platform::paths::DataPaths) -> PluginUpdateService {
        import_service_with_profiles(paths).0
    }

    fn import_service_with_profiles(
        paths: &gamenode_platform::paths::DataPaths,
    ) -> (PluginUpdateService, std::sync::Arc<PluginProfiles>) {
        use crate::plugin_host::{
            PluginApi, process::ProviderProcesses, runtime::ThirdPartyPluginLifecycle,
        };

        let service = PluginUpdateService::open(paths).unwrap();
        let api = PluginApi::default();
        api.set_state_layout(service.grant_layout.clone());
        let profiles = api.providers.profiles();
        profiles.update(Some("account-a"), true);
        let processes = ProviderProcesses::new_for_lifecycle(
            api.providers.clone(),
            paths.data_dir().join("provider.sock"),
            vec![],
            service.grant_layout.clone(),
        )
        .unwrap();
        service.set_lifecycle(std::sync::Arc::new(ThirdPartyPluginLifecycle::new(
            api.providers,
            processes,
            service.grant_layout.clone(),
            service.registry.clone(),
            service.packages.clone(),
        )));
        service.set_profiles(profiles.clone());
        (service, profiles)
    }

    /// Stages the pending review the update/downgrade flow would build for a version that is
    /// already installed. It reuses the production comparison and activation path so focused
    /// tests can exercise approval without a live update feed.
    async fn stage_installed_update(
        service: &PluginUpdateService,
        plugin_id: &str,
        installed_version: &str,
        candidate_version: &str,
    ) -> UpdateReview {
        let active = service.registry.active(plugin_id).unwrap().unwrap();
        assert_eq!(active.version, installed_version);
        let candidate = service
            .registry
            .versions(plugin_id)
            .unwrap()
            .into_iter()
            .find(|version| version.version == candidate_version)
            .expect("candidate version is installed");
        let publisher_key = package_key(&active.package_path).unwrap();
        let candidate_publisher_key = package_key(&candidate.package_path).unwrap();
        let installed = service
            .packages
            .inspect_candidate(
                &active.package_path,
                publisher_key,
                EmulationPolicy::RequireNative,
            )
            .await
            .unwrap();
        let selected = service
            .packages
            .inspect_candidate(
                &candidate.package_path,
                candidate_publisher_key,
                EmulationPolicy::RequireNative,
            )
            .await
            .unwrap();
        let mut review = UpdateReview::compare(&installed.manifest, &selected.manifest);
        review.compatibility_details = Some(selected.compatibility_review());
        review.publisher_fingerprint =
            super::super::publisher_trust::key_fingerprint(&candidate_publisher_key);
        service.pending.lock().await.insert(
            plugin_id.to_owned(),
            PendingCandidate {
                work: None,
                version: candidate_version.to_owned(),
                installed_version: installed_version.to_owned(),
                publisher_key,
                candidate_publisher_key,
                confirmed_retrust: false,
                is_rollback: false,
                installed_manifest: installed.manifest,
                candidate_manifest: selected.manifest,
                review: review.clone(),
            },
        );
        review
    }

    async fn import_review_and_pin(
        service: &PluginUpdateService,
        package: &Path,
    ) -> PluginImportReview {
        let review = service
            .import_review(archive_package(package))
            .await
            .unwrap();
        service
            .import_confirm_publisher(&review.token, &review.package.publisher_fingerprint)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn mvp_example_packages_use_production_import_and_consent_validators() {
        use crate::plugin_host::{
            PluginApi, grants::PermissionDecision, grants::PermissionState,
            managed_packages::ManagedPackageLifecycle, process::ProviderProcesses,
            profile::PluginStateLayout, runtime::HostedProviders,
            runtime::ThirdPartyPluginLifecycle,
        };
        use gamenode_core::library::LibraryStore;
        use gamenode_platform::{
            db::{self, SqliteSettingsStore},
            paths::DataPaths,
            plugin_registry::PluginPackageRegistry,
        };

        for (example, plugin_id, seed, requested) in [
            ("go-grpc", "org.example.gamenode.go-grpc", 71_u8, 8),
            ("rust-sdk", "org.example.gamenode.rust-mvp", 72_u8, 4),
        ] {
            let base = tempfile::tempdir().unwrap();
            let paths = DataPaths::at(base.path().join("app"));
            let endpoint = base.path().join("plugins.sock");
            let api = PluginApi::default();
            let state = PluginStateLayout::managed(&paths);
            api.set_state_layout(state.clone());
            api.providers.profiles().update(Some("account-one"), true);
            let registry = PluginPackageRegistry::open(&paths).unwrap();
            let preliminary_processes = ProviderProcesses::new_for_lifecycle(
                api.providers.clone(),
                endpoint.clone(),
                vec![],
                state.clone(),
            )
            .unwrap();
            let prelimary = std::sync::Arc::new(ThirdPartyPluginLifecycle::new(
                api.providers.clone(),
                preliminary_processes,
                state,
                registry.clone(),
                ManagedPackageLifecycle::new(registry.clone()),
            ));
            let service = PluginUpdateService::open(&paths).unwrap();
            service.set_lifecycle(prelimary);

            let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).unwrap();
            let package = base.path().join(format!("{example}-package"));
            signed_example_package(&package, example, &key);
            let review = service
                .import_review(archive_package(&package))
                .await
                .unwrap();
            assert_eq!(review.package.plugin_id, plugin_id);
            assert_eq!(review.package.trust, "untrusted");
            assert_eq!(review.package.compatibility, "compatible");
            assert_eq!(
                review.package.license_files,
                vec!["licenses/LICENSE".to_owned()]
            );
            assert_eq!(
                review.package.license_notices,
                vec!["licenses/THIRD-PARTY-NOTICES.txt".to_owned()]
            );
            assert!(
                service
                    .import_license_text(&review.token, "licenses/LICENSE")
                    .await
                    .unwrap()
                    .contains("MIT License")
            );
            assert!(
                !service
                    .import_license_text(&review.token, "outside.txt")
                    .await
                    .is_ok()
            );
            assert!(
                service
                    .import_install(&review.token, false, &[])
                    .await
                    .unwrap_err()
                    .contains("confirm the publisher")
            );

            let pinned = service
                .import_confirm_publisher(&review.token, &review.package.publisher_fingerprint)
                .await
                .unwrap();
            assert_eq!(pinned.package.trust, "trusted");
            assert_eq!(
                service
                    .import_install(&review.token, false, &[])
                    .await
                    .unwrap()
                    .plugin_id,
                plugin_id
            );

            // Startup composes the registry into the runtime; that composition is what registers
            // the installed provider for consent review. Rebuild it here as an application
            // launch would, then review permissions through the production validator.
            let library: std::sync::Arc<dyn LibraryStore> = std::sync::Arc::new(
                SqliteSettingsStore::new(db::open(Path::new(":memory:")).unwrap()).library_store(),
            );
            let host = HostedProviders::bundled(
                api.providers.clone(),
                std::sync::Arc::new(Default::default()),
                Some(endpoint),
                paths,
                library,
                None,
            )
            .await
            .unwrap();

            let snapshot = host
                .lifecycle()
                .permission_snapshot(plugin_id.into())
                .await
                .unwrap();
            assert_eq!(snapshot.requested_permissions.len(), requested);
            let decisions = snapshot
                .requested_permissions
                .iter()
                .map(|request| PermissionDecision {
                    permission_id: request.permission_id.clone(),
                    scope: request
                        .allowed_scopes
                        .iter()
                        .next()
                        .cloned()
                        .expect("manifest permission exposes a reviewable scope"),
                    granted: request.permission_id != "library.discovery",
                })
                .collect();
            let reviewed = host
                .lifecycle()
                .replace_permissions(
                    &tokio::sync::Mutex::new(()),
                    plugin_id.into(),
                    snapshot.profile_id.clone(),
                    snapshot.publisher_fingerprint.clone(),
                    snapshot.grant_revision,
                    decisions,
                )
                .await
                .unwrap();
            assert!(
                reviewed
                    .effective_grants
                    .iter()
                    .any(|grant| grant.permission_id == "library.discovery"
                        && grant.state == PermissionState::Denied)
            );
            assert!(
                reviewed
                    .effective_grants
                    .iter()
                    .any(|grant| grant.permission_id == "library.catalog"
                        && grant.state == PermissionState::Granted)
            );
        }
    }

    #[tokio::test]
    async fn import_install_registers_and_enables_a_signed_extension_without_a_restart() {
        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        let (service, _profiles) = import_service_with_profiles(&paths);
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[47; 32]).unwrap();
        let package = base.path().join("launch-extension");
        signed_package_from_manifest(&package, &key, &extension_fixture_manifest("1.0.0"));
        let imported = import_review_and_pin(&service, &package).await;

        let outcome = service
            .import_install(&imported.token, false, &[])
            .await
            .unwrap();
        assert!(outcome.activated, "{:?}", outcome.activation_error);
        assert_eq!(outcome.plugin_id, "org.example.launch");

        // No second lifecycle and no new HostedProviders: the same host is usable right away.
        let lifecycle = service
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap();
        assert!(lifecycle.is_registered("org.example.launch"));
        assert!(lifecycle.has_process_slot("org.example.launch"));
        assert!(lifecycle.extension_runtime_available("org.example.launch"));
        assert!(
            lifecycle
                .enabled("org.example.launch".into())
                .await
                .unwrap()
        );

        let inventory = service.managed_inventory().await.unwrap();
        let row = inventory
            .iter()
            .find(|plugin| plugin.plugin_id == "org.example.launch")
            .expect("the imported extension is listed");
        assert_eq!(row.status, "installed");
        assert_eq!(row.runtime_available, Some(true));
        assert_eq!(row.enabled, Some(true));
        let serialized = serde_json::to_value(row).unwrap();
        assert_eq!(serialized["runtimeAvailable"], serde_json::json!(true));
        assert_eq!(serialized["enabled"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn import_activation_failure_keeps_the_package_installed_and_reports_the_error() {
        use crate::plugin_host::grants::PermissionState;

        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        let service = import_service(&paths);
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[48; 32]).unwrap();
        let package = base.path().join("unstartable-provider");
        // The inert fixture entrypoint cannot start, so activation must fail after installation.
        signed_package(&package, "1.0.0", &key);
        let imported = import_review_and_pin(&service, &package).await;

        let outcome = service
            .import_install(&imported.token, true, &[])
            .await
            .unwrap();
        assert!(!outcome.activated);
        assert_eq!(outcome.plugin_id, "org.example.update");
        assert!(
            outcome
                .activation_error
                .as_deref()
                .unwrap_or_default()
                .contains("activation failed"),
            "{:?}",
            outcome.activation_error
        );

        // The package and the reviewed grants are not rolled back.
        assert!(
            service
                .registry
                .active("org.example.update")
                .unwrap()
                .is_some()
        );
        let lifecycle = service
            .lifecycle
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap();
        assert!(lifecycle.is_registered("org.example.update"));
        assert!(lifecycle.has_process_slot("org.example.update"));
        assert!(
            !lifecycle
                .enabled("org.example.update".into())
                .await
                .unwrap()
        );
        let snapshot = lifecycle
            .installed_permission_snapshot(
                "org.example.update".into(),
                imported.package.publisher_fingerprint.clone(),
            )
            .await
            .unwrap();
        assert!(snapshot.effective_grants.iter().any(|grant| {
            grant.permission_id == "storage.persist" && grant.state == PermissionState::Granted
        }));

        // The registration is complete, not half-applied: a later attempt skips it by identity.
        let outcomes = lifecycle.register_installed_packages().await.unwrap();
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.plugin_id != "org.example.update")
        );
        assert!(lifecycle.is_registered("org.example.update"));
    }

    #[test]
    fn playtime_capabilities_serialize_camel_case_and_accept_manifest_snake_case() {
        let parsed: PlaytimeCapabilities = serde_json::from_value(serde_json::json!({
            "identifies_gameplay": true,
            "identifies_pauses": false,
            "without_pause_detection": "application_open"
        }))
        .unwrap();
        assert!(parsed.identifies_gameplay);
        assert!(!parsed.identifies_pauses);
        assert_eq!(
            parsed.without_pause_detection,
            super::super::package::PlaytimeAccountingMode::ApplicationOpen
        );
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            serde_json::json!({
                "identifiesGameplay": true,
                "identifiesPauses": false,
                "withoutPauseDetection": "application_open"
            })
        );
    }

    #[tokio::test]
    async fn playtime_inventory_lists_only_packages_that_declare_playtime() {
        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        let service = import_service(&paths);
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[49; 32]).unwrap();

        let declaring = base.path().join("declaring-playtime");
        signed_package_with_playtime(
            &declaring,
            "1.0.0",
            &key,
            serde_json::json!({
                "identifies_gameplay": true,
                "identifies_pauses": false,
                "without_pause_detection": "application_open"
            }),
        );
        let imported = import_review_and_pin(&service, &declaring).await;
        service
            .import_install(&imported.token, true, &[])
            .await
            .unwrap();

        let plain = base.path().join("plain-package");
        let mut manifest = fixture_manifest("1.0.0", vec![]);
        manifest["id"] = serde_json::json!("org.example.plain");
        manifest["provider_id"] = serde_json::json!("plain-fixture");
        manifest["name"] = serde_json::json!("Plain Fixture");
        signed_package_from_manifest(&plain, &key, &manifest);
        let imported = import_review_and_pin(&service, &plain).await;
        service
            .import_install(&imported.token, true, &[])
            .await
            .unwrap();

        let inventory = service.playtime_inventory().await.unwrap();
        assert_eq!(inventory.len(), 1, "{inventory:#?}");
        assert_eq!(inventory[0].plugin_id, "org.example.update");
        let serialized = serde_json::to_value(&inventory[0]).unwrap();
        assert_eq!(
            serialized["playtime"],
            serde_json::json!({
                "identifiesGameplay": true,
                "identifiesPauses": false,
                "withoutPauseDetection": "application_open"
            })
        );
    }

    #[tokio::test]
    async fn imported_publisher_pin_authorizes_update_rollback_and_playtime() {
        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        let service = import_service(&paths);
        let plugin_id = "org.example.update";
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[39; 32]).unwrap();
        let v1 = base.path().join("v1");
        signed_package_with_playtime(
            &v1,
            "1.0.0",
            &key,
            serde_json::json!({
                "identifies_gameplay": true,
                "identifies_pauses": true,
                "without_pause_detection": "disabled"
            }),
        );
        let imported = import_review_and_pin(&service, &v1).await;
        assert_eq!(
            service
                .import_install(&imported.token, true, &[])
                .await
                .unwrap()
                .plugin_id,
            plugin_id
        );
        assert_eq!(
            service.playtime_inventory().await.unwrap()[0].version,
            "1.0.0"
        );

        // Stage the update where the reviewed feed flow would; the pin must come from the
        // import flow's trust store with no artificial entry in the legacy private directory.
        let work = UpdateWorkDir::create().unwrap();
        let staged = work.path.join("package");
        signed_package_with_playtime(
            &staged,
            "2.0.0",
            &key,
            serde_json::json!({
                "identifies_gameplay": true,
                "identifies_pauses": true,
                "without_pause_detection": "disabled"
            }),
        );
        let public_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
        let active = service.registry.active(plugin_id).unwrap().unwrap();
        let current = service
            .packages
            .inspect_candidate(
                &active.package_path,
                public_key,
                EmulationPolicy::RequireNative,
            )
            .await
            .unwrap();
        let candidate = service
            .packages
            .inspect_candidate(&staged, public_key, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let mut review = UpdateReview::compare(&current.manifest, &candidate.manifest);
        review.publisher_fingerprint = super::super::publisher_trust::key_fingerprint(&public_key);
        service.pending.lock().await.insert(
            plugin_id.to_owned(),
            PendingCandidate {
                work: Some(work),
                version: "2.0.0".into(),
                installed_version: active.version,
                publisher_key: public_key,
                candidate_publisher_key: public_key,
                confirmed_retrust: false,
                is_rollback: false,
                installed_manifest: current.manifest,
                candidate_manifest: candidate.manifest,
                review: review.clone(),
            },
        );
        assert!(!review.publisher_changed);
        assert_eq!(
            service.approve(plugin_id, "2.0.0", None).await.unwrap(),
            "2.0.0"
        );
        assert_eq!(
            service.playtime_inventory().await.unwrap()[0].version,
            "2.0.0"
        );

        let review = service.review_rollback(plugin_id, "1.0.0").await.unwrap();
        assert!(!review.publisher_changed);
        assert_eq!(
            service
                .approve_rollback(plugin_id, "1.0.0", true, None)
                .await
                .unwrap(),
            "1.0.0"
        );
        assert_eq!(
            service.playtime_inventory().await.unwrap()[0].version,
            "1.0.0"
        );
        assert!(!paths.private_dir().join("publisher-keys.json").exists());
    }

    /// Installs `old_permissions` as 1.0.0 and imports a pinned 2.0.0 signed by the same key.
    async fn service_with_older_version(
        base: &Path,
        old_permissions: Vec<serde_json::Value>,
    ) -> PluginUpdateService {
        let paths = gamenode_platform::paths::DataPaths::at(base.join("app"));
        paths.ensure_data_dir().unwrap();
        let service = import_service(&paths);
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[43; 32]).unwrap();
        let public: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
        let old = base.join("old");
        signed_package_with_permissions(&old, "1.0.0", &key, old_permissions);
        service
            .packages
            .install_candidate(&old, public, EmulationPolicy::RequireNative, false)
            .await
            .unwrap();
        let current = base.join("current");
        signed_package(&current, "2.0.0", &key);
        let imported = import_review_and_pin(&service, &current).await;
        service
            .import_install(&imported.token, true, &[])
            .await
            .unwrap();
        service
    }

    #[tokio::test]
    async fn rollback_review_rejects_a_network_scope_that_is_not_an_exact_origin() {
        let base = tempfile::tempdir().unwrap();
        let service = service_with_older_version(
            base.path(),
            vec![serde_json::json!({
                "id": "network.fetch", "required": false, "scope": "api.example.com",
                "reason": "Fetch metadata"
            })],
        )
        .await;
        let error = service
            .review_rollback("org.example.update", "1.0.0")
            .await
            .unwrap_err();
        assert!(error.contains("unsupported permission scopes"), "{error}");
        assert_eq!(
            service
                .registry
                .active("org.example.update")
                .unwrap()
                .unwrap()
                .version,
            "2.0.0"
        );
    }

    #[tokio::test]
    async fn approval_refuses_a_package_whose_permissions_differ_from_the_review() {
        let base = tempfile::tempdir().unwrap();
        let service = service_with_older_version(
            base.path(),
            vec![serde_json::json!({
                "id": "storage.persist", "required": false, "scope": "own-provider",
                "reason": "Store plugin preferences"
            })],
        )
        .await;
        service
            .review_rollback("org.example.update", "1.0.0")
            .await
            .unwrap();
        // The reviewed manifest no longer matches the verified package on disk.
        service
            .pending
            .lock()
            .await
            .get_mut("org.example.update")
            .unwrap()
            .candidate_manifest
            .permissions
            .push(crate::plugin_host::package::PermissionRequest {
                id: "network.fetch".into(),
                required: false,
                scope: "https://api.example.com".into(),
                reason: "Fetch metadata".into(),
                impact_if_disabled: None,
            });
        let error = service
            .approve_rollback("org.example.update", "1.0.0", true, None)
            .await
            .unwrap_err();
        assert!(
            error.contains("permissions differ from the reviewed package"),
            "{error}"
        );
        assert_eq!(
            service
                .registry
                .active("org.example.update")
                .unwrap()
                .unwrap()
                .version,
            "2.0.0"
        );
    }

    #[tokio::test]
    async fn rollback_requires_confirmation_and_exact_retrust_for_a_changed_key() {
        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        paths.ensure_data_dir().unwrap();
        let service = import_service(&paths);
        let old_key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[41; 32]).unwrap();
        let current_key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
        let old_public: [u8; 32] = old_key.public_key().as_ref().try_into().unwrap();

        let old_package = base.path().join("old");
        signed_package(&old_package, "1.0.0", &old_key);
        service
            .packages
            .install_candidate(
                &old_package,
                old_public,
                EmulationPolicy::RequireNative,
                false,
            )
            .await
            .unwrap();
        let current_package = base.path().join("current");
        signed_package(&current_package, "2.0.0", &current_key);
        let imported = import_review_and_pin(&service, &current_package).await;
        service
            .import_install(&imported.token, true, &[])
            .await
            .unwrap();
        assert_eq!(
            service
                .rollback_versions("org.example.update")
                .await
                .unwrap(),
            ["1.0.0"]
        );
        let review = service
            .review_rollback("org.example.update", "1.0.0")
            .await
            .unwrap();
        assert!(review.is_downgrade && review.publisher_changed);
        assert!(
            service
                .approve_rollback("org.example.update", "1.0.0", false, None)
                .await
                .is_err()
        );
        assert_eq!(
            service
                .approve_rollback("org.example.update", "1.0.0", true, None)
                .await
                .unwrap_err(),
            "the changed publisher key must be re-trusted separately"
        );
        assert!(
            service
                .confirm_publisher_retrust("org.example.update", "1.0.0", "SHA256:wrong")
                .await
                .is_err()
        );
        service
            .confirm_publisher_retrust(
                "org.example.update",
                "1.0.0",
                &super::super::publisher_trust::key_fingerprint(&old_public),
            )
            .await
            .unwrap();
        assert_eq!(
            service
                .approve_rollback("org.example.update", "1.0.0", true, None)
                .await
                .unwrap(),
            "1.0.0"
        );
        assert!(
            super::super::publisher_trust::PublisherTrustStore::open(&service.trust_dir)
                .unwrap()
                .is_pinned("org.example.update", &old_public)
                .unwrap()
        );
    }

    /// Re-trusting a changed publisher key establishes a distinct identity. Both keys' approved
    /// origins are revoked, so a stale approval recorded under either fingerprint cannot resurface,
    /// and the new identity still needs its own origin review before it can fetch.
    #[tokio::test]
    async fn changed_publisher_key_revokes_previous_grants_and_origin_approvals() {
        use crate::plugin_host::{
            grants::{
                GrantScope, PermissionDecision, PermissionState, PluginIdentity,
                manifest_permission_requests,
            },
            network::{OriginScopes as _, PersistedOriginScopes},
        };

        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        paths.ensure_data_dir().unwrap();
        let (service, profiles) = import_service_with_profiles(&paths);
        let plugin_id = "org.example.update";
        let key_a = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[61; 32]).unwrap();
        let key_b = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[62; 32]).unwrap();
        let public_a: [u8; 32] = key_a.public_key().as_ref().try_into().unwrap();
        let public_b: [u8; 32] = key_b.public_key().as_ref().try_into().unwrap();
        let first_origin = "https://first.example";
        let second_origin = "https://second.example";
        let permissions = || {
            vec![
                serde_json::json!({
                    "id": "network.fetch", "required": false,
                    "scope": format!("{first_origin}, {second_origin}"),
                    "reason": "Fetch the plugin's service data"
                }),
                serde_json::json!({
                    "id": "storage.persist", "required": false, "scope": "own-provider",
                    "reason": "Store plugin preferences"
                }),
            ]
        };

        let v1 = base.path().join("v1");
        signed_package_with_permissions(&v1, "1.0.0", &key_a, permissions());
        service
            .packages
            .install_candidate(&v1, public_a, EmulationPolicy::RequireNative, true)
            .await
            .unwrap();
        let fingerprint_a = super::super::publisher_trust::key_fingerprint(&public_a);
        super::super::publisher_trust::PublisherTrustStore::open(&service.trust_dir)
            .unwrap()
            .confirm_first_key(plugin_id, &public_a, &fingerprint_a)
            .unwrap();

        let profile = profiles.current().unwrap();
        let identity_a = PluginIdentity {
            plugin_id: plugin_id.into(),
            publisher_fingerprint: fingerprint_a,
            profile_id: profile.profile_id().to_owned(),
        };
        let installed_manifest = service
            .packages
            .inspect_candidate(&v1, public_a, EmulationPolicy::RequireNative)
            .await
            .unwrap()
            .manifest;
        let requests = manifest_permission_requests(&installed_manifest.permissions).unwrap();
        service
            .grant_layout
            .open_grants(&profile)
            .unwrap()
            .replace_grants(
                identity_a.clone(),
                &requests,
                &[
                    PermissionDecision {
                        permission_id: "network.fetch".into(),
                        scope: GrantScope::Declared {
                            value: first_origin.into(),
                        },
                        granted: true,
                    },
                    PermissionDecision {
                        permission_id: "network.fetch".into(),
                        scope: GrantScope::Declared {
                            value: second_origin.into(),
                        },
                        granted: true,
                    },
                    PermissionDecision {
                        permission_id: "storage.persist".into(),
                        scope: GrantScope::OwnPrivateNamespace,
                        granted: true,
                    },
                ],
            )
            .unwrap();
        let fingerprint_b = super::super::publisher_trust::key_fingerprint(&public_b);
        let identity_b = PluginIdentity {
            plugin_id: plugin_id.into(),
            publisher_fingerprint: fingerprint_b.clone(),
            profile_id: profile.profile_id().to_owned(),
        };
        // A stale approval recorded under the incoming key (for example from an install of that
        // publisher before the current one) must not resurface when the key is re-trusted.
        service
            .grant_layout
            .open_grants(&profile)
            .unwrap()
            .replace_grants(
                identity_b.clone(),
                &requests,
                &[PermissionDecision {
                    permission_id: "network.fetch".into(),
                    scope: GrantScope::Declared {
                        value: first_origin.into(),
                    },
                    granted: true,
                }],
            )
            .unwrap();
        let scopes = PersistedOriginScopes {
            profiles: profiles.clone(),
            layout: service.grant_layout.clone(),
        };
        assert_eq!(
            scopes
                .approved(&identity_a, profile.epoch())
                .await
                .unwrap()
                .origins,
            BTreeSet::from([first_origin.to_owned(), second_origin.to_owned()])
        );
        assert_eq!(
            scopes
                .approved(&identity_b, profile.epoch())
                .await
                .unwrap()
                .origins,
            BTreeSet::from([first_origin.to_owned()])
        );

        // The update flow presents a package signed by the new key. Activation waits for the
        // separate re-trust decision bound to that exact fingerprint.
        let v2 = base.path().join("v2");
        signed_package_with_permissions(&v2, "2.0.0", &key_b, permissions());
        service
            .packages
            .install_candidate(&v2, public_b, EmulationPolicy::RequireNative, false)
            .await
            .unwrap();
        let review = stage_installed_update(&service, plugin_id, "1.0.0", "2.0.0").await;
        assert!(review.publisher_changed);
        assert!(
            service
                .approve(plugin_id, "2.0.0", None)
                .await
                .unwrap_err()
                .contains("re-trusted separately")
        );
        service
            .confirm_publisher_retrust(plugin_id, "2.0.0", &fingerprint_b)
            .await
            .unwrap();
        assert_eq!(
            service.approve(plugin_id, "2.0.0", None).await.unwrap(),
            "2.0.0"
        );

        // Neither identity keeps a granted origin, nobody holds a granted permission under the
        // new key yet (the user must review it), and only the new key remains pinned.
        let grants = service.grant_layout.open_grants(&profile).unwrap();
        let previous = grants.effective_grants(&identity_a);
        assert!(!previous.is_empty());
        for grant in previous.iter().filter(|grant| {
            grant.permission_id == gamenode_plugin_protocol::capability::NETWORK_FETCH
        }) {
            assert_eq!(grant.state, PermissionState::Denied);
        }
        let current = grants.effective_grants(&identity_b);
        assert!(!current.is_empty());
        assert!(
            current
                .iter()
                .all(|grant| grant.state != PermissionState::Granted)
        );
        assert!(
            scopes
                .approved(&identity_a, profile.epoch())
                .await
                .unwrap()
                .origins
                .is_empty()
        );
        let new_identity = scopes.approved(&identity_b, profile.epoch()).await.unwrap();
        assert!(new_identity.origins.is_empty());
        assert!(new_identity.revision > 0);
        let trust =
            super::super::publisher_trust::PublisherTrustStore::open(&service.trust_dir).unwrap();
        assert!(trust.is_pinned(plugin_id, &public_b).unwrap());
        assert!(!trust.is_pinned(plugin_id, &public_a).unwrap());
    }

    /// Removes a read-only mode on drop so a failing assertion cannot leave the test's temporary
    /// registry unwritable during cleanup.
    #[cfg(unix)]
    struct RestoreDirectoryMode {
        path: PathBuf,
        restore: u32,
    }

    #[cfg(unix)]
    impl Drop for RestoreDirectoryMode {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(self.restore));
        }
    }

    /// A failed active-pointer write must leave the previous version in place and usable. The
    /// candidate stays installed but inactive, nothing reports it active, and a later review of
    /// the same payload can activate it once the registry is writable again.
    #[cfg(unix)]
    #[tokio::test]
    async fn failed_active_pointer_write_keeps_the_previous_version_active_and_usable() {
        use std::os::unix::fs::PermissionsExt;

        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        paths.ensure_data_dir().unwrap();
        let (service, _profiles) = import_service_with_profiles(&paths);
        let plugin_id = "org.example.update";
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[71; 32]).unwrap();
        let public: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
        let fingerprint = super::super::publisher_trust::key_fingerprint(&public);

        let v1 = base.path().join("v1");
        signed_package_with_playtime(
            &v1,
            "1.0.0",
            &key,
            serde_json::json!({
                "identifies_gameplay": true,
                "identifies_pauses": true,
                "without_pause_detection": "disabled"
            }),
        );
        service
            .packages
            .install_candidate(&v1, public, EmulationPolicy::RequireNative, true)
            .await
            .unwrap();
        super::super::publisher_trust::PublisherTrustStore::open(&service.trust_dir)
            .unwrap()
            .confirm_first_key(plugin_id, &public, &fingerprint)
            .unwrap();
        let v2 = base.path().join("v2");
        signed_package(&v2, "2.0.0", &key);
        service
            .packages
            .install_candidate(&v2, public, EmulationPolicy::RequireNative, false)
            .await
            .unwrap();

        // The reviewed update is approved while the active-pointer directory rejects writes.
        let active_dir = paths.plugin_packages_dir().join("active");
        let locked = RestoreDirectoryMode {
            path: active_dir.clone(),
            restore: 0o700,
        };
        fs::set_permissions(&active_dir, fs::Permissions::from_mode(0o500)).unwrap();
        stage_installed_update(&service, plugin_id, "1.0.0", "2.0.0").await;
        let error = service.approve(plugin_id, "2.0.0", None).await.unwrap_err();
        assert!(!error.is_empty());

        // The write failed before the pointer rename: no package reports itself active except
        // the previous version, which is still intact.
        let reported: Vec<_> = service
            .registry
            .active_versions()
            .unwrap()
            .into_iter()
            .filter(|version| version.plugin_id == plugin_id)
            .collect();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].version, "1.0.0");
        assert_eq!(
            service
                .registry
                .verify_active_package(plugin_id, &public)
                .unwrap()
                .expect("the previous version is still usable")
                .version,
            "1.0.0"
        );
        assert!(
            service
                .registry
                .versions(plugin_id)
                .unwrap()
                .iter()
                .any(|version| version.version == "2.0.0"),
            "the failed candidate stays installed but inactive"
        );

        // A fresh review can replace the failed candidate once the registry is writable.
        drop(locked);
        stage_installed_update(&service, plugin_id, "1.0.0", "2.0.0").await;
        assert_eq!(
            service.approve(plugin_id, "2.0.0", None).await.unwrap(),
            "2.0.0"
        );
        assert_eq!(
            service.registry.active(plugin_id).unwrap().unwrap().version,
            "2.0.0"
        );
    }

    #[tokio::test]
    async fn rollback_preserves_profile_data_and_does_not_restore_rescoped_grants() {
        use crate::plugin_host::{
            grants::{
                GrantScope, PermissionDecision, PermissionRequest as GrantRequest, PluginIdentity,
            },
            profile::{PluginProfiles, PluginStateLayout},
        };
        use gamenode_platform::plugin_storage::{
            DurableStore, PutResult, QUOTA_BYTES, namespace_dir,
        };

        let base = tempfile::tempdir().unwrap();
        let paths = gamenode_platform::paths::DataPaths::at(base.path().join("app"));
        paths.ensure_data_dir().unwrap();
        let registry = PluginPackageRegistry::open(&paths).unwrap();
        let lifecycle = ManagedPackageLifecycle::new(registry.clone());
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[51; 32]).unwrap();
        let public_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
        let current_origin = "https://current.example";
        let previous_origin = "https://previous.example";

        let previous_package = base.path().join("previous");
        signed_package_with_permissions(
            &previous_package,
            "1.0.0",
            &key,
            vec![serde_json::json!({
                "id": "network.fetch",
                "required": false,
                "scope": format!("{current_origin}, {previous_origin}"),
                "reason": "Fetch the plugin's service data"
            })],
        );
        lifecycle
            .install_candidate(
                &previous_package,
                public_key,
                EmulationPolicy::RequireNative,
                true,
            )
            .await
            .unwrap();

        let current_package = base.path().join("current");
        signed_package_with_permissions(
            &current_package,
            "2.0.0",
            &key,
            vec![serde_json::json!({
                "id": "network.fetch",
                "required": false,
                "scope": current_origin,
                "reason": "Fetch the plugin's service data"
            })],
        );
        lifecycle
            .install_candidate(
                &current_package,
                public_key,
                EmulationPolicy::RequireNative,
                true,
            )
            .await
            .unwrap();
        let fingerprint = super::super::publisher_trust::key_fingerprint(&public_key);
        super::super::publisher_trust::PublisherTrustStore::open(
            PluginStateLayout::prepare_managed(&paths)
                .unwrap()
                .trust_dir(),
        )
        .unwrap()
        .confirm_first_key("org.example.update", &public_key, &fingerprint)
        .unwrap();

        let profiles = std::sync::Arc::new(PluginProfiles::default());
        profiles.update(Some("inactive-account"), true);
        let inactive = profiles.current().unwrap();
        profiles.update(Some("account-a"), true);
        let profile = profiles.current().unwrap();
        let identity = PluginIdentity {
            plugin_id: "org.example.update".into(),
            publisher_fingerprint: fingerprint.clone(),
            profile_id: profile.profile_id().to_owned(),
        };
        let current_scope = GrantScope::Declared {
            value: current_origin.into(),
        };
        let layout = PluginStateLayout::prepare_managed(&paths).unwrap();
        let inactive_identity = PluginIdentity {
            profile_id: inactive.profile_id().to_owned(),
            ..identity.clone()
        };
        for (account, identity) in [(&profile, &identity), (&inactive, &inactive_identity)] {
            layout
                .open_grants(account)
                .unwrap()
                .replace_grants(
                    identity.clone(),
                    &[GrantRequest {
                        permission_id: "network.fetch".into(),
                        required: false,
                        allowed_scopes: [current_scope.clone()].into(),
                    }],
                    &[PermissionDecision {
                        permission_id: "network.fetch".into(),
                        scope: current_scope.clone(),
                        granted: true,
                    }],
                )
                .unwrap();
        }

        let data_namespace = namespace_dir(
            &paths.plugin_data_dir(),
            &fingerprint,
            "org.example.update",
            profile.profile_id(),
        );
        let mut data = DurableStore::open(&data_namespace).unwrap();
        assert!(matches!(
            data.put(
                "settings.v1",
                b"shared profile value",
                1,
                None,
                1,
                QUOTA_BYTES,
            )
            .unwrap(),
            PutResult::Stored { .. }
        ));
        drop(data);

        let service = PluginUpdateService::open(&paths).unwrap();
        service.set_profiles(profiles);
        let review = service
            .review_rollback("org.example.update", "1.0.0")
            .await
            .unwrap();
        assert!(review.is_downgrade && review.preserves_prior_data);
        service
            .approve_rollback("org.example.update", "1.0.0", true, None)
            .await
            .unwrap();

        assert_eq!(
            registry
                .active("org.example.update")
                .unwrap()
                .unwrap()
                .version,
            "1.0.0"
        );
        let retained_data = DurableStore::open_existing(&data_namespace)
            .unwrap()
            .expect("rollback must leave the plugin/profile namespace in place");
        assert_eq!(
            retained_data.get("settings.v1", 1).unwrap().unwrap().value,
            b"shared profile value"
        );
        let rollback_grants = layout.open_grants(&profile).unwrap();
        assert!(rollback_grants.effective_grants(&identity).is_empty());
        let inactive_grants = layout
            .open_grants(&inactive)
            .unwrap()
            .effective_grants(&inactive_identity);
        assert_eq!(inactive_grants.len(), 1);
        assert_eq!(
            inactive_grants[0].state,
            super::super::grants::PermissionState::Denied
        );
    }
}
