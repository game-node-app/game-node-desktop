//! Per-component inspection and staged installation of a verified bundle.
//!
//! A bundle is only a distribution envelope. Every component is staged through the ordinary
//! bounded package extractor and evaluated with the same signature, manifest and compatibility
//! code a standalone import uses, so a component is never trusted merely because the bundle
//! verified. Installation re-stages the reviewed archive, verifies it again and compares every
//! component hash with the review before the registry changes, so the reviewed bytes are the
//! installed bytes.
//!
//! The two failure modes stay separate:
//!
//! - A structural failure (bad envelope, corrupt component archive, signature failure, invalid
//!   manifest) rejects the whole bundle: [`inspect_bundle_candidate`] returns an error and no
//!   component report, and [`install_bundle_candidate`] leaves no component and no mapping.
//! - A component that is only incompatible with the current platform or API is reported as
//!   [`BundleComponentAvailability::Unavailable`] while compatible siblings remain available.
//!   Installed, that component is disabled; the bundle is not rejected.
//!
//! Duplicate identities are publisher-checked. An active component already installed at the
//! reviewed version under the bundle publisher's key is adopted into the mapping without being
//! reinstalled, and rollback never removes it. An active package that does not verify under that
//! key, or a plugin already owned by another bundle, aborts the whole bundle as a conflict before
//! any registry write; an existing installation is never silently overwritten.
//!
//! Removal is the exact inverse of the mapping: every component with an active package is stopped
//! with the ordinary bounded grace, then the recorded `package_version` of each component is
//! deleted, and finally the mapping row of every component whose package is actually gone. Other
//! installed versions of the same plugin id, grants and library history are never touched. When one
//! component cannot be deleted, its mapping row stays so the uninstall can be retried, while the
//! rows of the components that were removed disappear: a partial removal never leaves a mapping
//! that names a package which no longer exists.

use std::{
    fs,
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
};

use gamenode_platform::{
    plugin_bundle_db::{BundleComponentRecord, BundleRecord, SqlitePluginBundleStore},
    plugin_bundles::{BundleError, VerifiedBundle, VerifiedBundleComponent, stage_bundle_archive},
    plugin_environment::{EmulationPolicy, PluginEnvironment},
    plugin_packages::stage_package_archive,
    plugin_registry::{InstallSignedError, InstalledPluginVersion, PluginPackageRegistry},
};
use thiserror::Error;

use super::{
    managed_packages::{
        PackageCandidateReport, PackageCompatibilityReview, PackageLifecycleError,
        inspect_staged_package, inspect_staged_package_blocking, lock_publisher_identity,
    },
    package::PluginKind,
    process::ProviderProcesses,
    publisher_trust::key_fingerprint,
    retention::{DataRetention, DurableRetentionOutcome, DurableRetentionSink},
};

/// Bundle-level identity shown once for the whole review.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleIdentity {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher_key_id: String,
    pub publisher_fingerprint: String,
}

/// Whether one component can be selected for this execution environment.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum BundleComponentAvailability {
    Available,
    /// Platform or API incompatible. The component is reported, not installed as usable, and
    /// does not block its siblings.
    Unavailable {
        reason: BundleUnavailabilityReason,
    },
}

/// The compatibility evaluator's cause for an unavailable component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BundleUnavailabilityReason {
    /// No declared target matched the detected OS, architecture, ABI, libc or emulation state.
    NoCompatibleTarget,
    /// More than one declared target matched, so no target is selected implicitly.
    AmbiguousTarget,
}

/// One component's verified identity, compatibility verdict and existing review details.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleComponentInspection {
    pub plugin_id: String,
    pub kind: String,
    pub version: String,
    /// Package-relative path inside the bundle, e.g. `components/provider.gnplugin`.
    pub archive_path: String,
    /// Lowercase SHA-256 of the exact component archive bytes declared by the bundle.
    pub sha256: String,
    pub availability: BundleComponentAvailability,
    pub compatibility: PackageCompatibilityReview,
}

/// The complete per-component inspection of one bundle.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleCandidateReport {
    pub bundle: BundleIdentity,
    pub components: Vec<BundleComponentInspection>,
}

#[derive(Debug, Error)]
pub enum BundleInspectionError {
    #[error("bundle verification failed: {0}")]
    Bundle(#[from] BundleError),
    #[error("bundle inspection staging failed: {0}")]
    Staging(String),
    /// Structural: the component cannot be installed at all, so the bundle is rejected.
    #[error("bundle component {plugin_id} failed package validation: {error}")]
    Component { plugin_id: String, error: String },
    #[error("bundle component {plugin_id} does not match the verified bundle manifest")]
    ComponentIdentityMismatch { plugin_id: String },
    #[error("bundle inspection worker failed")]
    WorkerFailed,
}

/// The verified per-component data that only the import review needs: the public inspection plus
/// the ordinary staged-package report the shared review builder reads its manifest from.
#[derive(Clone, Debug)]
pub(crate) struct BundleReviewComponent {
    pub inspection: BundleComponentInspection,
    pub report: PackageCandidateReport,
}

/// A bundle staged for import review. The publisher key is the authority every component
/// verified under, so the review pins one fingerprint for the whole bundle.
#[derive(Clone, Debug)]
pub(crate) struct BundleReviewCandidate {
    pub bundle: BundleIdentity,
    pub publisher_key: [u8; 32],
    pub components: Vec<BundleReviewComponent>,
}

/// Verify a bundle archive, stage every component through the bounded package extractor and run
/// the ordinary candidate inspection on each one. No package code is executed and all file I/O
/// runs on blocking workers; the private staging directory is removed before this returns.
pub async fn inspect_bundle_candidate(
    archive_path: impl AsRef<Path>,
    emulation_policy: EmulationPolicy,
) -> Result<BundleCandidateReport, BundleInspectionError> {
    let candidate = inspect_bundle_review(archive_path, emulation_policy).await?;
    Ok(BundleCandidateReport {
        bundle: candidate.bundle,
        components: candidate
            .components
            .into_iter()
            .map(|component| component.inspection)
            .collect(),
    })
}

/// The same staged, verified inspection as [`inspect_bundle_candidate`], keeping each component's
/// package report so the import review can reuse the standalone per-package review builder.
pub(crate) async fn inspect_bundle_review(
    archive_path: impl AsRef<Path>,
    emulation_policy: EmulationPolicy,
) -> Result<BundleReviewCandidate, BundleInspectionError> {
    let archive_path = archive_path.as_ref().to_owned();
    let work = tokio::task::spawn_blocking(BundleWorkDir::create)
        .await
        .map_err(|_| BundleInspectionError::WorkerFailed)?
        .map_err(|error| BundleInspectionError::Staging(error.to_string()))?;
    let staging_root = work.path.clone();
    let verified =
        tokio::task::spawn_blocking(move || stage_bundle_archive(&archive_path, &staging_root))
            .await
            .map_err(|_| BundleInspectionError::WorkerFailed)??;
    let publisher_key = verified.manifest.publisher_key()?;
    let bundle = BundleIdentity {
        id: verified.manifest.id.clone(),
        name: verified.manifest.name.clone(),
        version: verified.manifest.version.clone(),
        publisher_key_id: verified.manifest.publisher.key_id.clone(),
        publisher_fingerprint: key_fingerprint(&publisher_key),
    };

    let mut components = Vec::with_capacity(verified.components.len());
    for (index, component) in verified.components.iter().enumerate() {
        let archive = work.path.join(&component.path);
        let package_root = work.path.join("packages").join(index.to_string());
        let plugin_id = component.plugin_id.clone();
        let package_for_worker = package_root.clone();
        tokio::task::spawn_blocking(move || {
            stage_package_archive(&archive, &package_for_worker, &publisher_key)
        })
        .await
        .map_err(|_| BundleInspectionError::WorkerFailed)?
        .map_err(|error| BundleInspectionError::Component {
            plugin_id: plugin_id.clone(),
            error: error_chain(&error),
        })?;

        let report = inspect_staged_package(&package_root, publisher_key, emulation_policy)
            .await
            .map_err(|error| BundleInspectionError::Component {
                plugin_id: component.plugin_id.clone(),
                error: error.to_string(),
            })?;
        if report.manifest.id != component.plugin_id
            || report.manifest.version != component.version
            || !kind_matches(report.manifest.kind, &component.kind)
        {
            return Err(BundleInspectionError::ComponentIdentityMismatch {
                plugin_id: component.plugin_id.clone(),
            });
        }
        components.push(BundleReviewComponent {
            inspection: BundleComponentInspection {
                plugin_id: report.manifest.id.clone(),
                kind: component.kind.clone(),
                version: report.manifest.version.clone(),
                archive_path: component.path.clone(),
                sha256: component.sha256.clone(),
                availability: availability(&report),
                compatibility: report.compatibility_review(),
            },
            report,
        });
    }

    Ok(BundleReviewCandidate {
        bundle,
        publisher_key,
        components,
    })
}

/// One component whose package this bundle install wrote into the registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledBundleComponent {
    pub plugin_id: String,
    pub kind: String,
    pub version: String,
    pub package_path: PathBuf,
    /// False when the component was installed for its identity but cannot run on this host.
    pub active: bool,
}

/// The recorded bundle mapping plus what the install did to each component package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledBundle {
    pub bundle: BundleRecord,
    pub components: Vec<InstalledBundleComponent>,
}

#[derive(Debug, Error)]
pub enum BundleInstallError {
    #[error("bundle verification failed: {0}")]
    Bundle(#[from] BundleError),
    #[error("bundle install staging failed: {0}")]
    Staging(String),
    /// The archive on disk is not the bundle the user reviewed; nothing was installed.
    #[error("the bundle archive no longer matches the reviewed bundle: {0}")]
    ChangedSinceReview(String),
    /// A component's active installation exists but does not verify under the bundle publisher's
    /// key. Adoption is publisher-checked, so the bundle is rejected before any registry write
    /// instead of replacing the existing package.
    #[error(
        "plugin {plugin_id} is already installed by a different or unverifiable publisher; remove it before importing this bundle"
    )]
    PublisherConflict { plugin_id: String },
    /// A component is already owned by a bundle mapping, so claiming it would silently move it
    /// between bundles.
    #[error(
        "plugin {plugin_id} already belongs to bundle {bundle_id}; remove that bundle before importing this one"
    )]
    BundleConflict {
        plugin_id: String,
        bundle_id: String,
    },
    /// Structural: the component could not be installed, so every component of this bundle was
    /// rolled back.
    #[error("bundle component {plugin_id} failed to install: {error}")]
    Component { plugin_id: String, error: String },
    #[error("the bundle mapping could not be recorded: {0}")]
    Mapping(String),
    /// A failure was reported and undoing the partial install did not fully succeed.
    #[error("{cause}; rollback was incomplete: {problems}")]
    Rollback { cause: String, problems: String },
    #[error("execution environment detection failed: {0}")]
    Environment(String),
    #[error("bundle install worker failed")]
    WorkerFailed,
}

/// Install every component of a reviewed bundle, then record the bundle mapping.
///
/// `reviewed` must be the report [`inspect_bundle_candidate`] returned for `archive_path`. The
/// archive is verified and every component hash is compared with that report before the registry
/// changes, so the reviewed bytes are the installed bytes; a changed archive fails without
/// touching the registry. Each component is staged through the bounded package extractor and
/// installed through the ordinary signed registry path. No package code is executed.
///
/// `installed_at` is the caller's clock (Unix seconds); the store never substitutes its own.
/// Before anything is written, each component's active installation is verified with the bundle
/// publisher's key: a component already active under that key and version is adopted into the
/// mapping without being reinstalled, while an active package under another publisher — or a
/// plugin already mapped to a bundle — is an explicit conflict and leaves the registry untouched.
/// Every step runs on one blocking worker, and a failure removes the component versions this call
/// created (restoring any previous active version) without touching adopted components, and leaves
/// no bundle mapping.
pub async fn install_bundle_candidate(
    archive_path: impl AsRef<Path>,
    reviewed: BundleCandidateReport,
    registry: PluginPackageRegistry,
    bundles: SqlitePluginBundleStore,
    emulation_policy: EmulationPolicy,
    installed_at: i64,
) -> Result<InstalledBundle, BundleInstallError> {
    let archive_path = archive_path.as_ref().to_owned();
    let environment = gamenode_platform::plugin_environment::detect()
        .await
        .map_err(BundleInstallError::Environment)?;
    tokio::task::spawn_blocking(move || {
        install_bundle_blocking(
            archive_path,
            &reviewed,
            &registry,
            &bundles,
            emulation_policy,
            &environment,
            installed_at,
        )
    })
    .await
    .map_err(|_| BundleInstallError::WorkerFailed)?
}

/// A component this call published to the registry, or an existing installation it adopted, with
/// the state needed to undo only what this call created.
struct PendingComponent {
    installed: InstalledPluginVersion,
    kind: String,
    /// Whether the component can be activated on this host.
    available: bool,
    /// The version that was active before this install, if any.
    previous_active: Option<String>,
    /// True when the bundle claimed an existing active installation under the same publisher key.
    /// An adopted component was not published by this call and must never be removed by rollback.
    adopted: bool,
}

fn install_bundle_blocking(
    archive_path: PathBuf,
    reviewed: &BundleCandidateReport,
    registry: &PluginPackageRegistry,
    bundles: &SqlitePluginBundleStore,
    emulation_policy: EmulationPolicy,
    environment: &PluginEnvironment,
    installed_at: i64,
) -> Result<InstalledBundle, BundleInstallError> {
    let work =
        BundleWorkDir::create().map_err(|error| BundleInstallError::Staging(error.to_string()))?;
    let verified = stage_bundle_archive(&archive_path, &work.path)?;
    confirm_reviewed_bundle(reviewed, &verified)?;
    let publisher_key = verified.manifest.publisher_key()?;

    // Registry writes for one install are serialized against other imports and updates.
    let _publisher = lock_publisher_identity();
    // Publisher and ownership checks happen before any component is published, so a conflict
    // cannot leave an earlier component of the same bundle behind.
    let adoptable = preflight_components(&verified, &publisher_key, registry, bundles)?;
    let record = BundleRecord {
        bundle_id: verified.manifest.id.clone(),
        name: verified.manifest.name.clone(),
        version: verified.manifest.version.clone(),
        publisher_fingerprint: key_fingerprint(&publisher_key),
        manifest_sha256: verified.manifest_sha256.clone(),
        installed_at,
        components: verified
            .components
            .iter()
            .zip(&adoptable)
            .map(|(component, adopted)| BundleComponentRecord {
                plugin_id: component.plugin_id.clone(),
                kind: component.kind.clone(),
                component_sha256: component.sha256.clone(),
                package_version: adopted.as_ref().map_or_else(
                    || component.version.clone(),
                    |installed| installed.version.clone(),
                ),
            })
            .collect(),
    };

    let mut pending = Vec::with_capacity(verified.components.len());
    let mut failure = None;
    for (index, component) in verified.components.iter().enumerate() {
        // An adopted component is already installed and active; claiming it must not reinstall,
        // replace or (on a later failure) remove it.
        if let Some(installed) = adoptable[index].clone() {
            pending.push(PendingComponent {
                installed,
                kind: component.kind.clone(),
                available: true,
                previous_active: None,
                adopted: true,
            });
            continue;
        }
        match install_component(
            &work.path,
            index,
            component,
            &publisher_key,
            registry,
            emulation_policy,
            environment,
        ) {
            Ok(installed) => pending.push(installed),
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    // Activate only after every package is published, so a failure leaves the previous pointer
    // untouched and the whole install can be undone without a half-active bundle.
    if failure.is_none() {
        for component in &pending {
            if component.adopted || !component.available {
                continue;
            }
            if let Err(error) =
                registry.set_active(&component.installed.plugin_id, &component.installed.version)
            {
                failure = Some(BundleInstallError::Component {
                    plugin_id: component.installed.plugin_id.clone(),
                    error: error.to_string(),
                });
                break;
            }
        }
    }
    if failure.is_none()
        && let Err(error) = bundles.record_bundle(&record)
    {
        failure = Some(mapping_failure(bundles, &verified, error.to_string()));
    }
    if let Some(error) = failure {
        let problems = rollback_install(registry, &pending);
        if !problems.is_empty() {
            return Err(BundleInstallError::Rollback {
                cause: error.to_string(),
                problems: problems.join("; "),
            });
        }
        return Err(error);
    }
    Ok(InstalledBundle {
        bundle: record,
        components: pending
            .into_iter()
            .map(|component| InstalledBundleComponent {
                plugin_id: component.installed.plugin_id,
                kind: component.kind,
                version: component.installed.version,
                package_path: component.installed.package_path,
                active: component.available,
            })
            .collect(),
    })
}

/// Decide, before any registry write, which components this install may adopt and which are
/// conflicts.
///
/// A component whose active version is already installed and verifies with the bundle publisher's
/// key is adoptable only for the exact reviewed version; the bundle claims it into its mapping
/// instead of publishing an identical immutable version again. Any other active package that does
/// not verify with that key is a publisher conflict, and a plugin already owned by a bundle
/// mapping is an ownership conflict. Both abort the whole bundle with a typed error, leaving the
/// existing installation untouched.
fn preflight_components(
    verified: &VerifiedBundle,
    publisher_key: &[u8; 32],
    registry: &PluginPackageRegistry,
    bundles: &SqlitePluginBundleStore,
) -> Result<Vec<Option<InstalledPluginVersion>>, BundleInstallError> {
    let mut adoptable = Vec::with_capacity(verified.components.len());
    for component in &verified.components {
        let plugin_id = component.plugin_id.clone();
        if let Some(mapped) = bundles
            .bundle_for_plugin(&plugin_id)
            .map_err(|error| BundleInstallError::Mapping(error.to_string()))?
        {
            return Err(BundleInstallError::BundleConflict {
                plugin_id,
                bundle_id: mapped.bundle_id,
            });
        }
        match registry.verify_active_package(&plugin_id, publisher_key) {
            Ok(Some(installed)) if installed.version == component.version => {
                adoptable.push(Some(installed));
            }
            Ok(_) => adoptable.push(None),
            // Integrity covers the publisher signature check: an active package that does not
            // verify under this bundle's key is never replaced silently.
            Err(InstallSignedError::Integrity(_)) => {
                return Err(BundleInstallError::PublisherConflict { plugin_id });
            }
            Err(error) => {
                return Err(BundleInstallError::Component {
                    plugin_id,
                    error: error.to_string(),
                });
            }
        }
    }
    Ok(adoptable)
}

/// A rejected mapping write is normally a duplicate bundle id; when a component was mapped to
/// another bundle after the preflight, report that ownership conflict instead of a generic store
/// failure.
fn mapping_failure(
    bundles: &SqlitePluginBundleStore,
    verified: &VerifiedBundle,
    error: String,
) -> BundleInstallError {
    for component in &verified.components {
        if let Ok(Some(mapped)) = bundles.bundle_for_plugin(&component.plugin_id) {
            return BundleInstallError::BundleConflict {
                plugin_id: component.plugin_id.clone(),
                bundle_id: mapped.bundle_id,
            };
        }
    }
    BundleInstallError::Mapping(error)
}

/// Stage, validate and publish one component without activating it. Activation is a separate
/// step so a component install can never leave a newly active package behind on failure.
fn install_component(
    work_root: &Path,
    index: usize,
    component: &VerifiedBundleComponent,
    publisher_key: &[u8; 32],
    registry: &PluginPackageRegistry,
    emulation_policy: EmulationPolicy,
    environment: &PluginEnvironment,
) -> Result<PendingComponent, BundleInstallError> {
    let plugin_id = component.plugin_id.clone();
    let failed = |error: String| BundleInstallError::Component {
        plugin_id: plugin_id.clone(),
        error,
    };
    let archive = work_root.join(&component.path);
    let package_root = work_root.join("packages").join(index.to_string());
    stage_package_archive(&archive, &package_root, publisher_key)
        .map_err(|error| failed(error_chain(&error)))?;
    let report = inspect_staged_package_blocking(
        &package_root,
        *publisher_key,
        emulation_policy,
        environment,
    )
    .map_err(|error| failed(error.to_string()))?;
    if report.manifest.id != component.plugin_id
        || report.manifest.version != component.version
        || !kind_matches(report.manifest.kind, &component.kind)
    {
        return Err(failed(
            "the staged component identity does not match the bundle manifest".into(),
        ));
    }
    let previous_active = registry
        .active(&plugin_id)
        .map_err(|error| failed(error.to_string()))?
        .map(|installed| installed.version);
    let already_published = registry
        .versions(&plugin_id)
        .map_err(|error| failed(error.to_string()))?
        .iter()
        .any(|installed| installed.version == component.version);
    let entrypoint = report
        .selected_target()
        .ok()
        .map(|target| target.target.entrypoint.clone());
    let installed = match &entrypoint {
        Some(entrypoint) => registry.install_signed_with_entrypoint(
            &plugin_id,
            &component.version,
            &package_root,
            publisher_key,
            entrypoint,
            false,
        ),
        None => registry.install_signed(
            &plugin_id,
            &component.version,
            &package_root,
            publisher_key,
            false,
        ),
    }
    .map_err(|error| {
        // A failure after the immutable version was published (for example during entrypoint
        // preparation) must not leak a package this call created, and a cleanup failure must be
        // reported rather than left for a later install to discover.
        let mut text = error.to_string();
        if !already_published
            && let Err(cleanup) = registry.remove_version(&plugin_id, &component.version)
        {
            text.push_str(&format!(
                "; cleanup of the published version failed: {cleanup}"
            ));
        }
        failed(text)
    })?;
    Ok(PendingComponent {
        installed,
        kind: component.kind.clone(),
        available: entrypoint.is_some(),
        previous_active,
        adopted: false,
    })
}

/// Undo the components this call published, newest first: removing a version also clears the
/// active pointer that names it, and any version that was active before is restored afterwards.
/// Adopted components were never published by this call, so they and their active pointer survive
/// any failure.
fn rollback_install(registry: &PluginPackageRegistry, pending: &[PendingComponent]) -> Vec<String> {
    let mut problems = Vec::new();
    for component in pending.iter().rev() {
        if component.adopted {
            continue;
        }
        let plugin_id = &component.installed.plugin_id;
        let version = &component.installed.version;
        if let Err(error) = registry.remove_version(plugin_id, version) {
            problems.push(format!("remove {plugin_id} {version}: {error}"));
            continue;
        }
        if let Some(previous) = &component.previous_active
            && let Err(error) = registry.set_active(plugin_id, previous)
        {
            problems.push(format!("reactivate {plugin_id} {previous}: {error}"));
        }
    }
    problems
}

/// Stops the supervised processes of one bundle component before its packages are deleted.
///
/// Production uses [`ProviderProcesses`], whose termination path waits for the child with the
/// existing bounded grace. A component that is installed but inactive or unavailable has no active
/// package and therefore no process, so [`remove_installed_bundle`] never asks it to stop and an
/// implementation must also treat an idle component as a successful no-op.
pub trait BundleComponentStopper: Send + Sync {
    fn stop_component<'a>(
        &'a self,
        plugin_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
}

impl BundleComponentStopper for ProviderProcesses {
    fn stop_component<'a>(
        &'a self,
        plugin_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(self.stop_plugin(plugin_id))
    }
}

/// One component whose recorded package version this removal deleted.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedBundleComponent {
    pub plugin_id: String,
    pub version: String,
    /// True when the component had an active package and its supervised process was stopped first.
    pub stopped: bool,
}

/// One component whose package could not be deleted. Its mapping row is retained, so the bundle
/// uninstall can be retried instead of forgetting a package that is still installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleRemovalFailure {
    pub plugin_id: String,
    pub version: String,
    pub error: String,
}

/// A fully removed bundle: the mapping that was read and every component package it deleted.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedBundle {
    pub bundle: BundleRecord,
    pub components: Vec<RemovedBundleComponent>,
}

#[derive(Debug, Error)]
pub enum BundleRemovalError {
    #[error("bundle {bundle_id} is not installed")]
    UnknownBundle { bundle_id: String },
    #[error("the bundle mapping could not be read: {0}")]
    Mapping(String),
    /// The component packages were removed but the mapping could not be updated. The whole
    /// mapping was deleted as a fallback so no row keeps naming a deleted package.
    #[error("the bundle mapping could not be updated after removal: {0}")]
    MappingUpdate(String),
    #[error("plugin package state is unavailable: {0}")]
    Registry(String),
    /// Some packages could not be deleted. The components that were removed are no longer mapped;
    /// the failures keep their mapping row so the uninstall can be retried.
    #[error(
        "bundle {bundle_id} was only partly removed; the remaining components stay mapped: {problems}"
    )]
    PartialRemoval {
        bundle_id: String,
        removed: Vec<RemovedBundleComponent>,
        failures: Vec<BundleRemovalFailure>,
        problems: String,
    },
    #[error("bundle removal worker failed")]
    WorkerFailed,
}

/// Remove one installed bundle: stop every component that is active at its recorded version, delete
/// exactly the recorded package version of each component, then delete the removed components from
/// the bundle mapping.
///
/// The removal is deliberately narrower than an uninstall. It never deletes another installed
/// version of the same plugin id (a standalone version, or a newer one adopted by another bundle),
/// and it never touches grants, activation, private data or library history; the caller owns the
/// retention and session choices. Components are stopped before any package is deleted, so no
/// process keeps running against a package that is already gone.
///
/// A component that cannot be removed keeps its mapping row and is reported, while the components
/// that were removed lose theirs. A mapping therefore never names a package that does not exist; a
/// retry removes what is left. File and database work runs on blocking workers.
pub async fn remove_installed_bundle(
    bundle_id: impl Into<String>,
    registry: PluginPackageRegistry,
    bundles: SqlitePluginBundleStore,
    stopper: &dyn BundleComponentStopper,
) -> Result<RemovedBundle, BundleRemovalError> {
    let bundle_id = bundle_id.into();
    let bundle = {
        let store = bundles.clone();
        let lookup = bundle_id.clone();
        tokio::task::spawn_blocking(move || store.bundle(&lookup))
            .await
            .map_err(|_| BundleRemovalError::WorkerFailed)?
            .map_err(|error| BundleRemovalError::Mapping(error.to_string()))?
            .ok_or_else(|| BundleRemovalError::UnknownBundle {
                bundle_id: bundle_id.clone(),
            })?
    };

    // Which components are active at the recorded version, and therefore have a supervised process
    // to stop. An installed-but-inactive (for example unavailable) component has nothing to stop,
    // and neither does one whose plugin id is only active at another standalone version.
    let mut active = Vec::with_capacity(bundle.components.len());
    for component in &bundle.components {
        let registry = registry.clone();
        let plugin_id = component.plugin_id.clone();
        let version = component.package_version.clone();
        let installed = tokio::task::spawn_blocking(move || registry.active(&plugin_id))
            .await
            .map_err(|_| BundleRemovalError::WorkerFailed)?
            .map_err(|error| BundleRemovalError::Registry(error.to_string()))?;
        active.push(installed.is_some_and(|active| active.version == version));
    }
    // Stop before any package removal, so a failure below cannot leave a process running against
    // half-removed files.
    for (component, active) in bundle.components.iter().zip(&active) {
        if *active {
            stopper.stop_component(&component.plugin_id).await;
        }
    }

    let components = bundle.components.clone();
    let (removed, failures) = tokio::task::spawn_blocking(move || {
        let mut removed = Vec::with_capacity(components.len());
        let mut failures = Vec::new();
        for (component, active) in components.into_iter().zip(active) {
            match registry.remove_version(&component.plugin_id, &component.package_version) {
                Ok(()) => removed.push(RemovedBundleComponent {
                    plugin_id: component.plugin_id,
                    version: component.package_version,
                    stopped: active,
                }),
                Err(error) => failures.push(BundleRemovalFailure {
                    plugin_id: component.plugin_id,
                    version: component.package_version,
                    error: error.to_string(),
                }),
            }
        }
        (removed, failures)
    })
    .await
    .map_err(|_| BundleRemovalError::WorkerFailed)?;

    if !removed.is_empty() {
        let plugin_ids: Vec<String> = removed
            .iter()
            .map(|component| component.plugin_id.clone())
            .collect();
        let store = bundles.clone();
        let lookup = bundle_id.clone();
        let updated = tokio::task::spawn_blocking(move || {
            store.remove_bundle_components(&lookup, &plugin_ids)
        })
        .await
        .map_err(|_| BundleRemovalError::WorkerFailed)?;
        if let Err(error) = updated {
            // The packages are gone, so the mapping must not keep naming them. Deleting the whole
            // mapping is the safe fallback: the failed components become ordinary standalone
            // packages instead of a mapping that points at deleted versions.
            let store = bundles;
            let lookup = bundle_id.clone();
            let fallback = tokio::task::spawn_blocking(move || store.remove_bundle(&lookup))
                .await
                .map_err(|_| BundleRemovalError::WorkerFailed)?;
            return Err(BundleRemovalError::MappingUpdate(match fallback {
                Ok(_) => format!("{error}; the whole bundle mapping was deleted instead"),
                Err(fallback) => {
                    format!("{error}; deleting the whole mapping also failed: {fallback}")
                }
            }));
        }
    }

    if failures.is_empty() {
        return Ok(RemovedBundle {
            bundle,
            components: removed,
        });
    }
    let problems = failures
        .iter()
        .map(|failure| {
            format!(
                "{} {}: {}",
                failure.plugin_id, failure.version, failure.error
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    Err(BundleRemovalError::PartialRemoval {
        bundle_id,
        removed,
        failures,
        problems,
    })
}

/// One component whose durable shared records could not be retained or deleted.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleDurableRetentionFailure {
    pub plugin_id: String,
    pub version: String,
    pub error: String,
}

/// One retention choice applied to every component a package-only removal removed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleDurableRetentionResult {
    /// `kept` or `deleted`, matching the choice applied to every removed component.
    pub data_retention: &'static str,
    pub retained: Vec<DurableRetentionOutcome>,
    pub failures: Vec<BundleDurableRetentionFailure>,
}

/// A package-only removal plus the durable-retention result for its removed components.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedBundleWithRetention {
    pub removed: RemovedBundle,
    pub durable: BundleDurableRetentionResult,
}

/// Applies one keep-or-delete choice to every component a bundle removal actually deleted.
///
/// The bundle mapping is package-only, so retention is a separate host decision. Keeping archives
/// each component's rows under its own owner; deleting removes only that owner's rows. A failure
/// for one component is reported by plugin id and does not change another component's data.
pub async fn apply_bundle_durable_retention(
    removed: &[RemovedBundleComponent],
    retention: DataRetention,
    sink: &dyn DurableRetentionSink,
) -> BundleDurableRetentionResult {
    let mut retained = Vec::with_capacity(removed.len());
    let mut failures = Vec::new();
    for component in removed {
        match sink.apply(&component.plugin_id, retention).await {
            Ok(outcome) => retained.push(outcome),
            Err(error) => failures.push(BundleDurableRetentionFailure {
                plugin_id: component.plugin_id.clone(),
                version: component.version.clone(),
                error,
            }),
        }
    }
    BundleDurableRetentionResult {
        data_retention: retention.as_str(),
        retained,
        failures,
    }
}

/// Remove one installed bundle and apply one durable-retention choice to the components whose
/// packages were removed. The mapping, grants, activation and private stores are still left to
/// the caller, exactly as in [`remove_installed_bundle`]. A partial package removal returns its
/// error before any retention is applied; retry the removal to finish it.
pub async fn remove_installed_bundle_with_retention(
    bundle_id: impl Into<String>,
    registry: PluginPackageRegistry,
    bundles: SqlitePluginBundleStore,
    stopper: &dyn BundleComponentStopper,
    retention: DataRetention,
    sink: &dyn DurableRetentionSink,
) -> Result<RemovedBundleWithRetention, BundleRemovalError> {
    let removed = remove_installed_bundle(bundle_id, registry, bundles, stopper).await?;
    let durable = apply_bundle_durable_retention(&removed.components, retention, sink).await;
    Ok(RemovedBundleWithRetention { removed, durable })
}

/// The archive must still be the exact bundle the user reviewed. Byte-level comparison is the
/// per-component SHA-256 the review carried; the bundle fields are compared as well so a
/// re-signed manifest cannot slip a different identity past the review.
fn confirm_reviewed_bundle(
    reviewed: &BundleCandidateReport,
    verified: &VerifiedBundle,
) -> Result<(), BundleInstallError> {
    let publisher_key = verified.manifest.publisher_key()?;
    let identity_matches = reviewed.bundle.id == verified.manifest.id
        && reviewed.bundle.name == verified.manifest.name
        && reviewed.bundle.version == verified.manifest.version
        && reviewed.bundle.publisher_key_id == verified.manifest.publisher.key_id
        && reviewed.bundle.publisher_fingerprint == key_fingerprint(&publisher_key);
    if !identity_matches {
        return Err(BundleInstallError::ChangedSinceReview(
            "the bundle identity differs from the review".into(),
        ));
    }
    if reviewed.components.len() != verified.components.len() {
        return Err(BundleInstallError::ChangedSinceReview(
            "the bundle component count differs from the review".into(),
        ));
    }
    for (reviewed_component, component) in reviewed.components.iter().zip(&verified.components) {
        if reviewed_component.archive_path != component.path
            || reviewed_component.plugin_id != component.plugin_id
            || reviewed_component.kind != component.kind
            || reviewed_component.version != component.version
            || reviewed_component.sha256 != component.sha256
        {
            return Err(BundleInstallError::ChangedSinceReview(format!(
                "component {} differs from the review",
                component.plugin_id
            )));
        }
    }
    Ok(())
}

/// Flatten an error and its causes so a staging failure says why it failed.
fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    text
}

fn kind_matches(kind: PluginKind, declared: &str) -> bool {
    matches!(
        (kind, declared),
        (PluginKind::Provider, "provider") | (PluginKind::Extension, "extension")
    )
}

/// Only the compatibility evaluator's selection decides availability; package validation
/// failures are already structural errors and never reach this function.
fn availability(report: &PackageCandidateReport) -> BundleComponentAvailability {
    match report.selected_target() {
        Ok(_) => BundleComponentAvailability::Available,
        // `selected_target` currently reports exactly these two causes; anything else also fails
        // target selection, so it must not be shown as available.
        Err(PackageLifecycleError::AmbiguousTarget) => BundleComponentAvailability::Unavailable {
            reason: BundleUnavailabilityReason::AmbiguousTarget,
        },
        Err(_) => BundleComponentAvailability::Unavailable {
            reason: BundleUnavailabilityReason::NoCompatibleTarget,
        },
    }
}

/// Private staging root for one bundle inspection or install. It is created with owner-only
/// permissions and removed on drop, including when a component fails validation or installation.
struct BundleWorkDir {
    path: PathBuf,
}

impl BundleWorkDir {
    fn create() -> io::Result<Self> {
        for _ in 0..8 {
            let mut nonce = [0u8; 16];
            getrandom::fill(&mut nonce).map_err(io::Error::other)?;
            let name = nonce
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let path = std::env::temp_dir().join(format!("gamenode-plugin-bundle-{name}"));
            match fs::create_dir(&path) {
                Ok(()) => {
                    restrict_private(&path)?;
                    if let Err(error) = fs::create_dir(path.join("packages")) {
                        let _ = fs::remove_dir_all(&path);
                        return Err(error);
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve bundle staging",
        ))
    }
}

impl Drop for BundleWorkDir {
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
    use crate::plugin_host::{
        PluginApi, PluginApiServer,
        compatibility::CompatibilityState,
        grants::{
            GrantScope, PermissionDecision, PermissionGrantStore, PermissionRequest,
            PermissionState, PluginIdentity,
        },
        managed_packages::ManagedPackageLifecycle,
        process::ProviderProcesses,
        profile::PluginStateLayout,
        publisher_trust,
        runtime::{HostedProviders, PluginPermissionSnapshot, ThirdPartyPluginLifecycle},
        updates::{BundleComponentPermissionSubmission, PluginUpdateService},
    };
    use base64::Engine as _;
    use gamenode_core::{library::LibraryStore as _, provider::Record};
    use gamenode_platform::{
        db::{self, SqliteSettingsStore},
        paths::DataPaths,
        plugin_registry::PluginPackageRegistry,
    };
    use std::{
        collections::{BTreeMap, BTreeSet},
        io::{Read, Write},
        path::PathBuf,
        sync::{Arc, Mutex},
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    /// Runs the repository pack tool; the fixture key and every archive are real tool output.
    fn package_tool(work: &Path, args: &[&str]) -> String {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/plugin-package.py");
        assert!(script.is_file(), "missing plugin package tool: {script:?}");
        let output = std::process::Command::new("python3")
            .arg(&script)
            .args(args)
            .current_dir(work)
            .output()
            .expect("python3 with the cryptography package must run the bundle fixture tooling");
        assert!(
            output.status.success(),
            "plugin-package.py {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    struct Fixture {
        dir: tempfile::TempDir,
        key: PathBuf,
        public_key: String,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let key = dir.path().join("fixture-dev.pem");
            let output = package_tool(
                dir.path(),
                &[
                    "keygen",
                    "--development",
                    "--private-key",
                    key.to_str().unwrap(),
                ],
            );
            let public_key = output
                .lines()
                .find_map(|line| line.strip_prefix("publisher.public_key (base64): "))
                .expect("keygen prints the fixture publisher key")
                .to_owned();
            Self {
                dir,
                key,
                public_key,
            }
        }

        fn base64_key(&self) -> [u8; 32] {
            base64::engine::general_purpose::STANDARD
                .decode(&self.public_key)
                .unwrap()
                .try_into()
                .unwrap()
        }

        #[allow(clippy::too_many_arguments)]
        fn component(
            &self,
            file: &str,
            kind: &str,
            id: &str,
            version: &str,
            os: &str,
            abi: &str,
            api_min: &str,
        ) -> PathBuf {
            self.component_with_permissions(file, kind, id, version, os, abi, api_min, vec![])
        }

        /// A component that declares the given signed permission requests, so the import review
        /// and the grouped consent flow can be exercised with real manifests.
        #[allow(clippy::too_many_arguments)]
        fn component_with_permissions(
            &self,
            file: &str,
            kind: &str,
            id: &str,
            version: &str,
            os: &str,
            abi: &str,
            api_min: &str,
            permissions: Vec<serde_json::Value>,
        ) -> PathBuf {
            let source = self.dir.path().join(format!("{file}-source"));
            fs::create_dir_all(source.join("bin")).unwrap();
            fs::create_dir_all(source.join("licenses")).unwrap();
            fs::write(source.join("bin/plugin"), b"fixture executable").unwrap();
            fs::write(source.join("licenses/LICENSE"), b"fixture license").unwrap();
            let mut manifest = serde_json::json!({
                "schema": 1,
                "id": id,
                "kind": kind,
                "name": format!("{id} fixture"),
                "version": version,
                "api": { "min": api_min, "max": "1.6.0" },
                "targets": [{
                    "os": os,
                    "arch": "x86_64",
                    "abi": abi,
                    "entrypoint": "bin/plugin"
                }],
                "publisher": {
                    "key_id": "dolphin-dev",
                    "public_key": self.public_key
                },
                "license": { "expression": "MIT", "files": ["licenses/LICENSE"] }
            });
            if kind == "provider" {
                manifest["provider_id"] = serde_json::json!("dolphin-fixture");
            }
            manifest["permissions"] = permissions.into();
            fs::write(
                source.join("plugin.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            let output = self.dir.path().join(format!("{file}.gnplugin"));
            package_tool(
                self.dir.path(),
                &[
                    "pack",
                    "--source",
                    source.to_str().unwrap(),
                    "--output",
                    output.to_str().unwrap(),
                    "--private-key",
                    self.key.to_str().unwrap(),
                ],
            );
            output
        }

        fn bundle(&self, components: &[&Path]) -> PathBuf {
            let output = self.dir.path().join("fixture.gnbundle");
            let mut args = vec!["bundle"];
            for component in components {
                args.push("--component");
                args.push(component.to_str().unwrap());
            }
            args.extend([
                "--id",
                "org.example.dolphin",
                "--name",
                "Dolphin GameCube and Wii",
                "--version",
                "0.1.0",
                "--output",
                output.to_str().unwrap(),
                "--private-key",
                self.key.to_str().unwrap(),
            ]);
            package_tool(self.dir.path(), &args);
            output
        }
    }

    fn component<'a>(
        report: &'a BundleCandidateReport,
        plugin_id: &str,
    ) -> &'a BundleComponentInspection {
        report
            .components
            .iter()
            .find(|component| component.plugin_id == plugin_id)
            .unwrap_or_else(|| panic!("missing component report for {plugin_id}"))
    }

    #[tokio::test]
    async fn a_valid_bundle_reports_every_component_with_its_own_review() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);

        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        assert_eq!(report.bundle.id, "org.example.dolphin");
        assert_eq!(report.bundle.name, "Dolphin GameCube and Wii");
        assert_eq!(report.bundle.version, "0.1.0");
        assert_eq!(report.bundle.publisher_key_id, "dolphin-dev");
        assert_eq!(
            report.bundle.publisher_fingerprint,
            publisher_trust::key_fingerprint(&fixture.base64_key())
        );
        assert_eq!(report.components.len(), 2);

        let provider = component(&report, "org.example.dolphin");
        assert_eq!(provider.kind, "provider");
        assert_eq!(provider.version, "0.1.0");
        assert_eq!(provider.archive_path, "components/provider.gnplugin");
        assert_eq!(
            provider.availability,
            BundleComponentAvailability::Available
        );
        assert_eq!(provider.compatibility.state, CompatibilityState::Compatible);
        assert!(!provider.compatibility.targets.is_empty());

        let launch = component(&report, "org.example.dolphin.launch");
        assert_eq!(launch.kind, "extension");
        assert_eq!(launch.version, "0.2.0");
        assert_eq!(launch.archive_path, "components/launch.gnplugin");
        assert_eq!(launch.availability, BundleComponentAvailability::Available);
        assert_eq!(launch.compatibility.state, CompatibilityState::Compatible);
    }

    #[tokio::test]
    async fn an_incompatible_component_is_unavailable_and_its_sibling_stays_usable() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        // No Windows target can match the Linux test host; the package itself is valid.
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "windows",
            "msvc",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);

        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        assert_eq!(report.components.len(), 2);
        assert_eq!(
            component(&report, "org.example.dolphin").availability,
            BundleComponentAvailability::Available
        );
        let launch = component(&report, "org.example.dolphin.launch");
        assert_eq!(
            launch.availability,
            BundleComponentAvailability::Unavailable {
                reason: BundleUnavailabilityReason::NoCompatibleTarget
            }
        );
        assert_eq!(launch.compatibility.state, CompatibilityState::Incompatible);
    }

    #[tokio::test]
    async fn a_corrupt_component_fails_the_whole_bundle_structurally() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);

        // Rewrite the outer envelope with one extra payload byte in the provider archive. The
        // bundle still parses, but the declared component hash no longer matches.
        let corrupt = fixture.dir.path().join("corrupt.gnbundle");
        let mut source = zip::ZipArchive::new(fs::File::open(&bundle).unwrap()).unwrap();
        let mut writer = ZipWriter::new(fs::File::create(&corrupt).unwrap());
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            let name = entry.name().to_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            if name == "components/provider.gnplugin" {
                data.push(0);
            }
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&data).unwrap();
        }
        writer.finish().unwrap();

        let result = inspect_bundle_candidate(&corrupt, EmulationPolicy::RequireNative).await;
        assert!(
            matches!(
                result,
                Err(BundleInspectionError::Bundle(BundleError::InvalidComponent))
            ),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn a_component_that_fails_package_validation_rejects_the_bundle() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        // The pack tool does not evaluate the API range, so this archive verifies as a bundle
        // component but the host's full manifest validator rejects it.
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "not-semver",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);

        let result = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative).await;
        assert!(
            matches!(
                result,
                Err(BundleInspectionError::Component { ref plugin_id, .. })
                    if plugin_id == "org.example.dolphin.launch"
            ),
            "{result:?}"
        );
    }

    /// A fresh database and registry for one bundle install test.
    fn install_target(fixture: &Fixture) -> (PluginPackageRegistry, SqliteSettingsStore) {
        let registry = PluginPackageRegistry::at(fixture.dir.path().join("registry")).unwrap();
        let settings =
            SqliteSettingsStore::open(&fixture.dir.path().join("state.sqlite3")).unwrap();
        (registry, settings)
    }

    #[tokio::test]
    async fn a_valid_bundle_installs_every_component_and_records_the_mapping() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let (registry, settings) = install_target(&fixture);

        let installed_at = 1_700_000_000;
        let installed = install_bundle_candidate(
            &bundle,
            report.clone(),
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            installed_at,
        )
        .await
        .unwrap();

        assert_eq!(installed.bundle.bundle_id, "org.example.dolphin");
        assert_eq!(installed.bundle.name, "Dolphin GameCube and Wii");
        assert_eq!(installed.bundle.version, "0.1.0");
        assert_eq!(
            installed.bundle.publisher_fingerprint,
            report.bundle.publisher_fingerprint
        );
        assert_eq!(installed.bundle.installed_at, installed_at);
        assert_eq!(installed.bundle.manifest_sha256.len(), 64);
        assert_eq!(installed.bundle.components.len(), 2);
        assert_eq!(installed.components.len(), 2);
        assert!(
            installed
                .components
                .iter()
                .all(|component| component.active),
            "{:?}",
            installed.components
        );

        let key = fixture.base64_key();
        let stored = settings
            .bundle_store()
            .bundle("org.example.dolphin")
            .unwrap()
            .expect("the mapping is stored");
        assert_eq!(stored, installed.bundle);
        assert_eq!(
            settings
                .bundle_store()
                .bundle_for_plugin("org.example.dolphin.launch")
                .unwrap(),
            Some(installed.bundle.clone())
        );
        for inspected in &report.components {
            let recorded = installed
                .bundle
                .components
                .iter()
                .find(|component| component.plugin_id == inspected.plugin_id)
                .expect("every inspected component is recorded");
            assert_eq!(recorded.component_sha256, inspected.sha256);
            assert_eq!(recorded.package_version, inspected.version);
        }

        let provider_active = registry.active("org.example.dolphin").unwrap().unwrap();
        assert_eq!(provider_active.version, "0.1.0");
        assert_eq!(
            registry
                .verify_active_package("org.example.dolphin", &key)
                .unwrap(),
            Some(provider_active)
        );
        let launch_active = registry
            .active("org.example.dolphin.launch")
            .unwrap()
            .unwrap();
        assert_eq!(launch_active.version, "0.2.0");
        assert_eq!(
            registry
                .verify_active_package("org.example.dolphin.launch", &key)
                .unwrap(),
            Some(launch_active)
        );
    }

    #[tokio::test]
    async fn a_component_failure_rolls_back_and_keeps_the_previous_installation() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        // The bundle's extension version is installed standalone but inactive, while an older
        // standalone version is active. Adoption only claims the active version, so publishing the
        // bundle's version collides with the installed immutable version path.
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch_previous = fixture.component(
            "launch-previous",
            "extension",
            "org.example.dolphin.launch",
            "0.1.9",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let key = fixture.base64_key();
        let (registry, settings) = install_target(&fixture);

        let staged_previous = fixture.dir.path().join("standalone-launch-previous");
        stage_package_archive(&launch_previous, &staged_previous, &key).unwrap();
        let previous = registry
            .install_signed(
                "org.example.dolphin.launch",
                "0.1.9",
                &staged_previous,
                &key,
                true,
            )
            .unwrap();
        let staged = fixture.dir.path().join("standalone-launch");
        stage_package_archive(&launch, &staged, &key).unwrap();
        registry
            .install_signed("org.example.dolphin.launch", "0.2.0", &staged, &key, false)
            .unwrap();
        assert_eq!(
            registry.active("org.example.dolphin.launch").unwrap(),
            Some(previous.clone())
        );

        let result = install_bundle_candidate(
            &bundle,
            report,
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await;
        assert!(
            matches!(
                result,
                Err(BundleInstallError::Component { ref plugin_id, .. })
                    if plugin_id == "org.example.dolphin.launch"
            ),
            "{result:?}"
        );

        // The provider was published before the failure and is rolled back completely.
        assert!(registry.versions("org.example.dolphin").unwrap().is_empty());
        assert!(registry.active("org.example.dolphin").unwrap().is_none());
        // Both standalone versions are untouched and the previous one is still active.
        assert_eq!(
            registry
                .versions("org.example.dolphin.launch")
                .unwrap()
                .iter()
                .map(|installed| installed.version.as_str())
                .collect::<Vec<_>>(),
            vec!["0.1.9", "0.2.0"]
        );
        assert_eq!(
            registry.active("org.example.dolphin.launch").unwrap(),
            Some(previous.clone())
        );
        assert_eq!(
            registry
                .verify_active_package("org.example.dolphin.launch", &key)
                .unwrap(),
            Some(previous)
        );
        // No bundle mapping survives the failed install.
        let store = settings.bundle_store();
        assert_eq!(store.bundle("org.example.dolphin").unwrap(), None);
        assert_eq!(
            store.bundle_for_plugin("org.example.dolphin").unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_standalone_component_with_the_same_publisher_is_adopted() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let key = fixture.base64_key();
        let (registry, settings) = install_target(&fixture);

        // The extension was imported standalone first and is the active installation.
        let staged = fixture.dir.path().join("standalone-launch");
        stage_package_archive(&launch, &staged, &key).unwrap();
        let previous = registry
            .install_signed("org.example.dolphin.launch", "0.2.0", &staged, &key, true)
            .unwrap();
        assert_eq!(
            registry.active("org.example.dolphin.launch").unwrap(),
            Some(previous.clone())
        );

        let installed = install_bundle_candidate(
            &bundle,
            report.clone(),
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await
        .unwrap();
        assert_eq!(installed.components.len(), 2);

        // Adoption must not publish a second version or move the active pointer.
        assert_eq!(
            registry.versions("org.example.dolphin.launch").unwrap(),
            vec![previous.clone()]
        );
        assert_eq!(
            registry.active("org.example.dolphin.launch").unwrap(),
            Some(previous.clone())
        );
        assert_eq!(
            registry
                .verify_active_package("org.example.dolphin.launch", &key)
                .unwrap(),
            Some(previous)
        );

        // The provider was newly installed and activated.
        assert_eq!(
            registry
                .active("org.example.dolphin")
                .unwrap()
                .unwrap()
                .version,
            "0.1.0"
        );

        // The mapping records the existing version and the bundle's declared component hash.
        let recorded = settings
            .bundle_store()
            .bundle("org.example.dolphin")
            .unwrap()
            .expect("the mapping is stored");
        let launch_record = recorded
            .components
            .iter()
            .find(|component| component.plugin_id == "org.example.dolphin.launch")
            .expect("the adopted component is mapped");
        assert_eq!(launch_record.package_version, "0.2.0");
        assert_eq!(
            launch_record.component_sha256,
            component(&report, "org.example.dolphin.launch").sha256
        );
    }

    #[tokio::test]
    async fn a_component_from_another_publisher_aborts_before_any_write() {
        // The standalone installation is signed by the first publisher.
        let installed_fixture = Fixture::new();
        let standalone_launch = installed_fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );

        // The bundle and both of its components are signed by a different publisher.
        let bundle_fixture = Fixture::new();
        let provider = bundle_fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = bundle_fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = bundle_fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let key = installed_fixture.base64_key();
        let (registry, settings) = install_target(&installed_fixture);

        let staged = installed_fixture.dir.path().join("standalone-launch");
        stage_package_archive(&standalone_launch, &staged, &key).unwrap();
        let previous = registry
            .install_signed("org.example.dolphin.launch", "0.2.0", &staged, &key, true)
            .unwrap();

        let result = install_bundle_candidate(
            &bundle,
            report,
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await;
        assert!(
            matches!(
                result,
                Err(BundleInstallError::PublisherConflict { ref plugin_id })
                    if plugin_id == "org.example.dolphin.launch"
            ),
            "{result:?}"
        );

        // The conflict is detected before the provider is published, and the existing
        // installation is untouched.
        assert!(registry.versions("org.example.dolphin").unwrap().is_empty());
        assert!(registry.active("org.example.dolphin").unwrap().is_none());
        assert_eq!(
            registry.versions("org.example.dolphin.launch").unwrap(),
            vec![previous.clone()]
        );
        assert_eq!(
            registry.active("org.example.dolphin.launch").unwrap(),
            Some(previous.clone())
        );
        assert_eq!(
            registry
                .verify_active_package("org.example.dolphin.launch", &key)
                .unwrap(),
            Some(previous)
        );
        let store = settings.bundle_store();
        assert_eq!(store.bundle("org.example.dolphin").unwrap(), None);
        assert_eq!(
            store
                .bundle_for_plugin("org.example.dolphin.launch")
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_failure_after_adoption_rolls_back_only_the_created_component() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let key = fixture.base64_key();
        let (registry, settings) = install_target(&fixture);

        let staged = fixture.dir.path().join("standalone-launch");
        stage_package_archive(&launch, &staged, &key).unwrap();
        let previous = registry
            .install_signed("org.example.dolphin.launch", "0.2.0", &staged, &key, true)
            .unwrap();

        // Occupy the bundle id with an unrelated mapping so the mapping write fails only after the
        // provider was installed and the extension was adopted.
        let store = settings.bundle_store();
        let existing = BundleRecord {
            bundle_id: "org.example.dolphin".to_owned(),
            name: "Existing mapping".to_owned(),
            version: "9.9.9".to_owned(),
            publisher_fingerprint: "sha256:existing".to_owned(),
            manifest_sha256: "b".repeat(64),
            installed_at: 1,
            components: vec![BundleComponentRecord {
                plugin_id: "org.example.other".to_owned(),
                kind: "provider".to_owned(),
                component_sha256: "c".repeat(64),
                package_version: "1.0.0".to_owned(),
            }],
        };
        store.record_bundle(&existing).unwrap();

        let result = install_bundle_candidate(
            &bundle,
            report,
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await;
        assert!(
            matches!(result, Err(BundleInstallError::Mapping(_))),
            "{result:?}"
        );

        // The provider this call created is rolled back.
        assert!(registry.versions("org.example.dolphin").unwrap().is_empty());
        assert!(registry.active("org.example.dolphin").unwrap().is_none());
        // The adopted extension keeps its version, active pointer and verifiable payload.
        assert_eq!(
            registry.versions("org.example.dolphin.launch").unwrap(),
            vec![previous.clone()]
        );
        assert_eq!(
            registry.active("org.example.dolphin.launch").unwrap(),
            Some(previous.clone())
        );
        assert_eq!(
            registry
                .verify_active_package("org.example.dolphin.launch", &key)
                .unwrap(),
            Some(previous)
        );
        // The pre-existing mapping is unchanged and the bundle did not claim any component.
        assert_eq!(store.bundle("org.example.dolphin").unwrap(), Some(existing));
        assert_eq!(
            store
                .bundle_for_plugin("org.example.dolphin.launch")
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_component_already_mapped_to_another_bundle_is_an_explicit_conflict() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let (registry, settings) = install_target(&fixture);
        let store = settings.bundle_store();
        let other = BundleRecord {
            bundle_id: "org.example.other-bundle".to_owned(),
            name: "Other bundle".to_owned(),
            version: "1.0.0".to_owned(),
            publisher_fingerprint: "sha256:other".to_owned(),
            manifest_sha256: "d".repeat(64),
            installed_at: 2,
            components: vec![BundleComponentRecord {
                plugin_id: "org.example.dolphin.launch".to_owned(),
                kind: "extension".to_owned(),
                component_sha256: "e".repeat(64),
                package_version: "0.1.0".to_owned(),
            }],
        };
        store.record_bundle(&other).unwrap();

        let result = install_bundle_candidate(
            &bundle,
            report,
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await;
        assert!(
            matches!(
                result,
                Err(BundleInstallError::BundleConflict {
                    ref plugin_id,
                    ref bundle_id,
                }) if plugin_id == "org.example.dolphin.launch"
                    && bundle_id == "org.example.other-bundle"
            ),
            "{result:?}"
        );

        // Nothing is installed and the other mapping still owns its component.
        assert!(registry.versions("org.example.dolphin").unwrap().is_empty());
        assert!(registry.active("org.example.dolphin").unwrap().is_none());
        assert_eq!(store.bundle("org.example.dolphin").unwrap(), None);
        assert_eq!(
            store.bundle("org.example.other-bundle").unwrap(),
            Some(other)
        );
    }

    #[tokio::test]
    async fn an_unavailable_component_is_installed_but_left_inactive() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "windows",
            "msvc",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let (registry, settings) = install_target(&fixture);

        let installed = install_bundle_candidate(
            &bundle,
            report,
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await
        .unwrap();

        assert!(installed.components[0].active);
        assert!(!installed.components[1].active);
        assert_eq!(
            registry
                .active("org.example.dolphin")
                .unwrap()
                .unwrap()
                .version,
            "0.1.0"
        );
        assert!(
            registry
                .active("org.example.dolphin.launch")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            registry
                .versions("org.example.dolphin.launch")
                .unwrap()
                .iter()
                .map(|installed| installed.version.as_str())
                .collect::<Vec<_>>(),
            vec!["0.2.0"]
        );
        assert_eq!(
            settings
                .bundle_store()
                .bundle("org.example.dolphin")
                .unwrap()
                .expect("the mapping is stored")
                .components
                .len(),
            2
        );
    }

    fn permission(id: &str, scope: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "required": false,
            "scope": scope,
            "reason": "Signed fixture permission"
        })
    }

    /// A service with an authenticated profile and the same registry and state the fixtures
    /// install into, matching application startup before the hosted providers are composed.
    fn review_service(
        api: &PluginApi,
        paths: &DataPaths,
        state: &PluginStateLayout,
        registry: &PluginPackageRegistry,
    ) -> PluginUpdateService {
        let service = PluginUpdateService::open(paths).unwrap();
        let processes = ProviderProcesses::new_for_lifecycle(
            api.providers.clone(),
            paths.data_dir().join("plugins.sock"),
            vec![],
            state.clone(),
        )
        .unwrap();
        service.set_lifecycle(Arc::new(ThirdPartyPluginLifecycle::new(
            api.providers.clone(),
            processes,
            state.clone(),
            registry.clone(),
            ManagedPackageLifecycle::new(registry.clone()),
        )));
        service.set_profiles(api.providers.profiles());
        service
    }

    fn decision(snapshot: &PluginPermissionSnapshot, granted: bool) -> PermissionDecision {
        let request = &snapshot.requested_permissions[0];
        PermissionDecision {
            permission_id: request.permission_id.clone(),
            scope: request.allowed_scopes.iter().next().unwrap().clone(),
            granted,
        }
    }

    fn submission(
        snapshot: &PluginPermissionSnapshot,
        revision: u64,
        granted: bool,
    ) -> BundleComponentPermissionSubmission {
        BundleComponentPermissionSubmission {
            plugin_id: snapshot.plugin_id.clone(),
            expected_profile_id: snapshot.profile_id.clone(),
            expected_publisher_fingerprint: snapshot.publisher_fingerprint.clone(),
            expected_revision: revision,
            decisions: vec![decision(snapshot, granted)],
        }
    }

    #[tokio::test]
    async fn bundle_review_groups_both_components_and_one_confirmation_pins_the_publisher() {
        let fixture = Fixture::new();
        let provider = fixture.component_with_permissions(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
            vec![permission("storage.persist", "own-provider")],
        );
        let launch = fixture.component_with_permissions(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
            vec![permission("game.launch", "approved-game-session")],
        );
        let bundle = fixture.bundle(&[&provider, &launch]);

        let paths = DataPaths::at(fixture.dir.path().join("app-data"));
        let api = PluginApi::default();
        let state = PluginStateLayout::managed(&paths);
        api.set_state_layout(state.clone());
        let profiles = api.providers.profiles();
        profiles.update(Some("bundle-account"), true);
        let registry = PluginPackageRegistry::open(&paths).unwrap();
        let service = review_service(&api, &paths, &state, &registry);
        let profile = profiles.require().unwrap();

        // The fixture entrypoints are inert placeholder bytes, so a successful review also shows
        // that no component code ran to produce it.
        let review = service.bundle_import_review(bundle).await.unwrap();
        assert_eq!(review.bundle.id, "org.example.dolphin");
        assert_eq!(review.bundle.name, "Dolphin GameCube and Wii");
        assert_eq!(review.bundle.version, "0.1.0");
        assert_eq!(review.bundle.publisher_key_id, "dolphin-dev");
        assert_eq!(
            review.bundle.publisher_fingerprint,
            publisher_trust::key_fingerprint(&fixture.base64_key())
        );
        assert_eq!(review.trust, "untrusted");
        assert_eq!(review.pinned_fingerprint, None);
        assert_eq!(review.license.expressions, vec!["MIT".to_owned()]);
        assert_eq!(review.license.components.len(), 2);

        assert_eq!(review.components.len(), 2);
        let provider = review
            .components
            .iter()
            .find(|component| component.review.plugin_id == "org.example.dolphin")
            .expect("the provider is grouped by its own identity");
        assert_eq!(provider.kind, "provider");
        assert_eq!(
            provider.availability,
            BundleComponentAvailability::Available
        );
        assert_eq!(provider.review.version, "0.1.0");
        assert_eq!(provider.review.publisher_key_id, "dolphin-dev");
        assert_eq!(
            provider.review.publisher_fingerprint,
            review.bundle.publisher_fingerprint
        );
        assert_eq!(provider.review.permission_ids, vec!["storage.persist"]);
        assert_eq!(
            provider.review.requested_permissions[0].scope,
            "own-provider"
        );
        assert_eq!(provider.review.license, "MIT");
        assert_eq!(
            provider.review.license_files,
            vec!["licenses/LICENSE".to_owned()]
        );
        assert!(provider.review.native_access.is_empty());
        assert_eq!(provider.review.compatibility, "compatible");
        assert_eq!(
            provider.review.compatibility_details.state,
            CompatibilityState::Compatible
        );
        let launch = review
            .components
            .iter()
            .find(|component| component.review.plugin_id == "org.example.dolphin.launch")
            .expect("the extension is grouped by its own identity");
        assert_eq!(launch.kind, "extension");
        assert_eq!(launch.availability, BundleComponentAvailability::Available);
        assert_eq!(launch.review.permission_ids, vec!["game.launch"]);

        // Reviewing and confirming is not consent: no grant record exists for either component.
        assert!(state.open_grants(&profile).unwrap().identities().is_empty());

        // A mismatched confirmation pins nothing; the exact displayed fingerprint pins both
        // components in one decision.
        assert!(
            service
                .bundle_import_confirm_publisher(
                    &review.token,
                    &format!("SHA256:{}", "0".repeat(64))
                )
                .await
                .is_err()
        );
        let pinned = service
            .bundle_import_confirm_publisher(&review.token, &review.bundle.publisher_fingerprint)
            .await
            .unwrap();
        assert_eq!(pinned.trust, "trusted");
        assert_eq!(pinned.pinned_fingerprint, None);
        assert!(
            pinned
                .components
                .iter()
                .all(|component| component.review.trust == "trusted")
        );
        let trust = publisher_trust::PublisherTrustStore::open(state.trust_dir()).unwrap();
        for plugin_id in ["org.example.dolphin", "org.example.dolphin.launch"] {
            assert_eq!(
                trust.inspect(plugin_id, &fixture.base64_key()).unwrap(),
                publisher_trust::KeyStatus::Trusted {
                    fingerprint: review.bundle.publisher_fingerprint.clone()
                }
            );
        }
        assert!(state.open_grants(&profile).unwrap().identities().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn grouped_bundle_permissions_isolate_a_denial_and_a_stale_revision() {
        let fixture = Fixture::new();
        let provider = fixture.component_with_permissions(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
            vec![permission("storage.persist", "own-provider")],
        );
        let launch = fixture.component_with_permissions(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
            vec![permission("game.launch", "approved-game-session")],
        );
        let bundle = fixture.bundle(&[&provider, &launch]);

        let paths = DataPaths::at(fixture.dir.path().join("app-data"));
        let api = PluginApi::default();
        let state = PluginStateLayout::managed(&paths);
        api.set_state_layout(state.clone());
        let profiles = api.providers.profiles();
        profiles.update(Some("bundle-account"), true);
        let registry = PluginPackageRegistry::open(&paths).unwrap();
        let service = review_service(&api, &paths, &state, &registry);

        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let settings =
            SqliteSettingsStore::open(&fixture.dir.path().join("bundles.sqlite3")).unwrap();
        install_bundle_candidate(
            &bundle,
            report,
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await
        .unwrap();

        let review = service.bundle_import_review(bundle.clone()).await.unwrap();
        service
            .bundle_import_confirm_publisher(&review.token, &review.bundle.publisher_fingerprint)
            .await
            .unwrap();

        // The host registers only trusted active packages, exactly as it does at startup.
        let store = SqliteSettingsStore::new(db::open(Path::new(":memory:")).unwrap());
        let library: Arc<dyn gamenode_core::library::LibraryStore> =
            Arc::new(store.library_store());
        let endpoint = fixture.dir.path().join("plugins.sock");
        let host = HostedProviders::bundled(
            api.providers.clone(),
            Arc::new(Default::default()),
            Some(endpoint.clone()),
            paths.clone(),
            library,
            None,
        )
        .await
        .unwrap();
        let _server =
            PluginApiServer::start(&tokio::runtime::Handle::current(), &endpoint, api.clone())
                .unwrap();
        service.set_lifecycle(host.lifecycle());

        let provider_snapshot = service
            .permission_snapshot("org.example.dolphin".into())
            .await
            .unwrap();
        let launch_snapshot = service
            .permission_snapshot("org.example.dolphin.launch".into())
            .await
            .unwrap();
        for snapshot in [&provider_snapshot, &launch_snapshot] {
            assert_eq!(snapshot.grant_revision, 0);
            assert!(snapshot.effective_grants.is_empty());
            assert!(!snapshot.requested_permissions.is_empty());
        }

        // One grouped review: the provider is granted and the extension is denied. Denying one
        // component neither grants it nor disturbs its sibling's record.
        let first = service
            .apply_bundle_permissions(vec![
                submission(&provider_snapshot, provider_snapshot.grant_revision, true),
                submission(&launch_snapshot, launch_snapshot.grant_revision, false),
            ])
            .await
            .unwrap();
        assert_eq!(first.components.len(), 2);
        assert!(
            first
                .components
                .iter()
                .all(|component| component.error.is_none())
        );
        let provider_saved = first.components[0]
            .snapshot
            .as_ref()
            .expect("the provider decision was saved");
        let launch_saved = first.components[1]
            .snapshot
            .as_ref()
            .expect("the denied component keeps its own record");
        assert_eq!(
            provider_saved.effective_grants[0].state,
            PermissionState::Granted
        );
        assert_eq!(
            launch_saved.effective_grants[0].state,
            PermissionState::Denied
        );
        assert_eq!(provider_saved.grant_revision, 1);
        assert_eq!(launch_saved.grant_revision, 1);

        // A stale revision fails only the component that carries it. The sibling still saves and
        // the stale component keeps its previous grant and revision.
        let stale = service
            .apply_bundle_permissions(vec![
                submission(provider_saved, provider_saved.grant_revision - 1, false),
                submission(launch_saved, launch_saved.grant_revision, true),
            ])
            .await
            .unwrap();
        assert!(
            stale.components[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("grants changed")),
            "{:?}",
            stale.components[0]
        );
        assert!(stale.components[0].snapshot.is_none());
        let launch_updated = stale.components[1]
            .snapshot
            .as_ref()
            .expect("the sibling decision still applies");
        assert_eq!(
            launch_updated.effective_grants[0].state,
            PermissionState::Granted
        );
        assert_eq!(launch_updated.grant_revision, 2);

        let provider_after = service
            .permission_snapshot("org.example.dolphin".into())
            .await
            .unwrap();
        assert_eq!(provider_after.grant_revision, provider_saved.grant_revision);
        assert_eq!(
            provider_after.effective_grants[0].state,
            PermissionState::Granted
        );
        let launch_after = service
            .permission_snapshot("org.example.dolphin.launch".into())
            .await
            .unwrap();
        assert_eq!(launch_after.grant_revision, 2);
        assert_eq!(
            launch_after.effective_grants[0].state,
            PermissionState::Granted
        );
    }

    /// Records every component it is asked to stop; the stop itself is the no-op an idle component
    /// needs, so the test observes the removal order without starting a real process.
    #[derive(Default)]
    struct RecordingStopper {
        stopped: Mutex<Vec<String>>,
    }

    impl RecordingStopper {
        fn stopped(&self) -> Vec<String> {
            self.stopped.lock().unwrap().clone()
        }
    }

    impl BundleComponentStopper for RecordingStopper {
        fn stop_component<'a>(
            &'a self,
            plugin_id: &'a str,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            self.stopped.lock().unwrap().push(plugin_id.to_owned());
            Box::pin(std::future::ready(()))
        }
    }

    /// Records the durable-retention choice made for each removed component.
    #[derive(Default)]
    struct RecordingRetention {
        calls: Mutex<Vec<(String, DataRetention)>>,
    }

    impl RecordingRetention {
        fn calls(&self) -> Vec<(String, DataRetention)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl DurableRetentionSink for RecordingRetention {
        fn apply<'a>(
            &'a self,
            plugin_id: &'a str,
            retention: DataRetention,
        ) -> Pin<Box<dyn Future<Output = Result<DurableRetentionOutcome, String>> + Send + 'a>>
        {
            self.calls
                .lock()
                .unwrap()
                .push((plugin_id.to_owned(), retention));
            let outcome = DurableRetentionOutcome {
                plugin_id: plugin_id.to_owned(),
                data_retention: retention.as_str(),
                archived_records: u64::from(retention == DataRetention::Keep),
                removed_records: u64::from(retention == DataRetention::Delete),
                reclaimed_bytes: 0,
            };
            Box::pin(std::future::ready(Ok(outcome)))
        }
    }

    #[tokio::test]
    async fn a_bundle_removal_applies_one_retention_choice_to_every_removed_component() {
        let components = vec![
            RemovedBundleComponent {
                plugin_id: "org.example.dolphin".to_owned(),
                version: "0.1.0".to_owned(),
                stopped: true,
            },
            RemovedBundleComponent {
                plugin_id: "org.example.dolphin.launch".to_owned(),
                version: "0.2.0".to_owned(),
                stopped: false,
            },
        ];
        let sink = RecordingRetention::default();

        let kept = apply_bundle_durable_retention(&components, DataRetention::Keep, &sink).await;
        assert_eq!(kept.data_retention, "kept");
        assert_eq!(kept.retained.len(), 2);
        assert!(kept.failures.is_empty());
        assert!(
            kept.retained.iter().all(|outcome| {
                outcome.data_retention == "kept" && outcome.archived_records == 1
            })
        );
        assert_eq!(
            sink.calls(),
            [
                ("org.example.dolphin".to_owned(), DataRetention::Keep),
                ("org.example.dolphin.launch".to_owned(), DataRetention::Keep)
            ]
        );

        let deleted =
            apply_bundle_durable_retention(&components[..1], DataRetention::Delete, &sink).await;
        assert_eq!(deleted.data_retention, "deleted");
        assert_eq!(deleted.retained.len(), 1);
        assert_eq!(deleted.retained[0].removed_records, 1);
        assert_eq!(
            sink.calls().last(),
            Some(&("org.example.dolphin".to_owned(), DataRetention::Delete))
        );
    }

    #[tokio::test]
    async fn removing_a_bundle_stops_the_active_component_and_deletes_only_its_versions() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        // No Windows target matches the Linux test host, so this component is installed inactive.
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "windows",
            "msvc",
            "1.6.0",
        );
        // An unrelated standalone version of the same plugin id stays installed and active.
        let standalone_launch = fixture.component(
            "launch-standalone",
            "extension",
            "org.example.dolphin.launch",
            "0.1.9",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let key = fixture.base64_key();
        let (registry, settings) = install_target(&fixture);

        let staged = fixture.dir.path().join("standalone-launch");
        stage_package_archive(&standalone_launch, &staged, &key).unwrap();
        let standalone = registry
            .install_signed("org.example.dolphin.launch", "0.1.9", &staged, &key, true)
            .unwrap();

        let installed = install_bundle_candidate(
            &bundle,
            report,
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await
        .unwrap();

        // A grant record and a library entry exist before the uninstall and must survive it.
        let grants_dir = fixture.dir.path().join("account");
        let identity = PluginIdentity {
            plugin_id: "org.example.dolphin".to_owned(),
            publisher_fingerprint: "ab".repeat(32),
            profile_id: "fixture-profile".to_owned(),
        };
        let scope = GrantScope::Declared {
            value: "https://api.example.com".to_owned(),
        };
        let request = PermissionRequest {
            permission_id: gamenode_plugin_protocol::capability::NETWORK_FETCH.to_owned(),
            required: false,
            allowed_scopes: BTreeSet::from([scope.clone()]),
        };
        let decision = PermissionDecision {
            permission_id: gamenode_plugin_protocol::capability::NETWORK_FETCH.to_owned(),
            scope,
            granted: true,
        };
        PermissionGrantStore::open(&grants_dir)
            .unwrap()
            .replace_grants(identity.clone(), &[request], &[decision])
            .unwrap();
        let grants_before = PermissionGrantStore::open(&grants_dir).unwrap();
        assert!(!grants_before.effective_grants(&identity).is_empty());

        settings
            .library_store()
            .save_scan(
                "dolphin",
                &[Record {
                    id: "gamecube-1".to_owned(),
                    name: Some("Wind Waker".to_owned()),
                    install_dir: "/games/wind-waker".to_owned(),
                    fields: BTreeMap::new(),
                }],
                &BTreeMap::new(),
                1_700_000_000,
            )
            .unwrap();

        let stopper = RecordingStopper::default();
        let removed = remove_installed_bundle(
            "org.example.dolphin",
            registry.clone(),
            settings.bundle_store(),
            &stopper,
        )
        .await
        .unwrap();

        assert_eq!(removed.bundle, installed.bundle);
        assert_eq!(removed.components.len(), 2);
        let provider_removal = removed
            .components
            .iter()
            .find(|component| component.plugin_id == "org.example.dolphin")
            .unwrap();
        assert_eq!(provider_removal.version, "0.1.0");
        assert!(provider_removal.stopped);
        let launch_removal = removed
            .components
            .iter()
            .find(|component| component.plugin_id == "org.example.dolphin.launch")
            .unwrap();
        assert_eq!(launch_removal.version, "0.2.0");
        // The bundle's extension version was inactive, so there was no process to stop; the active
        // process belongs to the unrelated standalone version and is left alone.
        assert!(!launch_removal.stopped);
        assert_eq!(stopper.stopped(), vec!["org.example.dolphin"]);

        // The mapping is gone for both components.
        let store = settings.bundle_store();
        assert_eq!(store.bundle("org.example.dolphin").unwrap(), None);
        assert_eq!(
            store.bundle_for_plugin("org.example.dolphin").unwrap(),
            None
        );
        assert_eq!(
            store
                .bundle_for_plugin("org.example.dolphin.launch")
                .unwrap(),
            None
        );

        // The bundle's versions are gone; the standalone version of the same plugin id survives,
        // is still active and still verifies.
        assert!(registry.versions("org.example.dolphin").unwrap().is_empty());
        assert!(registry.active("org.example.dolphin").unwrap().is_none());
        assert_eq!(
            registry.versions("org.example.dolphin.launch").unwrap(),
            vec![standalone.clone()]
        );
        assert_eq!(
            registry.active("org.example.dolphin.launch").unwrap(),
            Some(standalone.clone())
        );
        assert_eq!(
            registry
                .verify_active_package("org.example.dolphin.launch", &key)
                .unwrap(),
            Some(standalone)
        );

        // Grants and library history are untouched.
        let grants_after = PermissionGrantStore::open(&grants_dir).unwrap();
        assert_eq!(
            grants_after.effective_grants(&identity),
            grants_before.effective_grants(&identity)
        );
        assert_eq!(
            grants_after.revision(&identity),
            grants_before.revision(&identity)
        );
        let library = settings.library_store().load_cache(None).unwrap();
        assert_eq!(library.entries.len(), 1);
        assert_eq!(library.entries[0].key, "dolphin:gamecube-1");
        assert!(library.entries[0].installed);
    }

    #[tokio::test]
    async fn a_partial_removal_keeps_only_the_failed_component_mapped() {
        let fixture = Fixture::new();
        let provider = fixture.component(
            "provider",
            "provider",
            "org.example.dolphin",
            "0.1.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let launch = fixture.component(
            "launch",
            "extension",
            "org.example.dolphin.launch",
            "0.2.0",
            "linux",
            "gnu",
            "1.6.0",
        );
        let bundle = fixture.bundle(&[&provider, &launch]);
        let report = inspect_bundle_candidate(&bundle, EmulationPolicy::RequireNative)
            .await
            .unwrap();
        let (registry, settings) = install_target(&fixture);
        let installed = install_bundle_candidate(
            &bundle,
            report,
            registry.clone(),
            settings.bundle_store(),
            EmulationPolicy::RequireNative,
            1_700_000_000,
        )
        .await
        .unwrap();

        // Damage the extension's versions directory so its recorded version can no longer be
        // deleted, while the provider stays removable.
        let launch_path = installed
            .components
            .iter()
            .find(|component| component.plugin_id == "org.example.dolphin.launch")
            .unwrap()
            .package_path
            .clone();
        let versions_dir = launch_path
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf();
        fs::remove_dir_all(&versions_dir).unwrap();
        fs::write(&versions_dir, b"not a directory").unwrap();

        let stopper = RecordingStopper::default();
        let result = remove_installed_bundle(
            "org.example.dolphin",
            registry.clone(),
            settings.bundle_store(),
            &stopper,
        )
        .await;
        let BundleRemovalError::PartialRemoval {
            bundle_id,
            removed,
            failures,
            ..
        } = result.unwrap_err()
        else {
            panic!("expected a partial removal");
        };
        assert_eq!(bundle_id, "org.example.dolphin");
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].plugin_id, "org.example.dolphin");
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].plugin_id, "org.example.dolphin.launch");
        assert_eq!(failures[0].version, "0.2.0");

        // The provider is deleted and no longer mapped.
        assert!(registry.versions("org.example.dolphin").unwrap().is_empty());
        // Only the failed component stays mapped: no row names the deleted provider.
        let store = settings.bundle_store();
        let remaining = store
            .bundle("org.example.dolphin")
            .unwrap()
            .expect("the partial mapping is kept for a retry");
        assert_eq!(remaining.components.len(), 1);
        assert_eq!(
            remaining.components[0].plugin_id,
            "org.example.dolphin.launch"
        );
        assert_eq!(
            store.bundle_for_plugin("org.example.dolphin").unwrap(),
            None
        );
        assert_eq!(
            store
                .bundle_for_plugin("org.example.dolphin.launch")
                .unwrap(),
            Some(remaining)
        );
    }
}
