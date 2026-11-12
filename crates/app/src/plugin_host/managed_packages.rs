//! Compatibility-gated package validation, installation, and activation.
//!
//! These lifecycle helpers accept only packages already staged by the bounded extractor. They
//! verify signatures before reading the manifest and never execute package contents.

use super::{
    compatibility::{
        CompatibilityState, CompatibilityVerdict, TargetCompatibilityAssessment,
        assess_manifest_targets,
    },
    package::Manifest,
};
use gamenode_platform::{
    plugin_environment::{EmulationPolicy, PluginEnvironment},
    plugin_packages::{IntegrityManifest, verify_package_directory},
    plugin_registry::{InstalledPluginVersion, PluginPackageRegistry},
};
use sha2::Digest;
use std::{fs, path::Path};
use thiserror::Error;

const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum PackageLifecycleError {
    #[error("package validation failed: {0}")]
    InvalidPackage(String),
    #[error("no target is compatible with the current execution environment")]
    NoCompatibleTarget(Vec<CompatibilityVerdict>),
    #[error("multiple package targets match the current execution environment")]
    AmbiguousTarget,
    #[error("installed plugin version was not found")]
    VersionNotFound,
    #[error("installed package identity or version does not match its registry record")]
    RegistryIdentityMismatch,
    #[error("plugin package registry operation failed: {0}")]
    Registry(String),
    #[error("execution environment detection failed: {0}")]
    Environment(String),
    #[error("package validation worker failed")]
    WorkerFailed,
}

#[derive(Clone, Debug)]
pub struct PackageCandidateReport {
    pub manifest: Manifest,
    pub environment: PluginEnvironment,
    pub targets: Vec<TargetCompatibilityAssessment>,
}

impl PackageCandidateReport {
    /// Return the unique compatible target. Incompatible or indeterminate candidates never
    /// select a substitute target implicitly.
    pub fn selected_target(&self) -> Result<&TargetCompatibilityAssessment, PackageLifecycleError> {
        let mut compatible = self
            .targets
            .iter()
            .filter(|target| target.verdict.state == CompatibilityState::Compatible);
        let Some(selected) = compatible.next() else {
            return Err(PackageLifecycleError::NoCompatibleTarget(
                self.targets
                    .iter()
                    .map(|target| target.verdict.clone())
                    .collect(),
            ));
        };
        if compatible.next().is_some() {
            return Err(PackageLifecycleError::AmbiguousTarget);
        }
        Ok(selected)
    }
}

#[derive(Clone)]
pub struct ManagedPackageLifecycle {
    registry: PluginPackageRegistry,
}

impl ManagedPackageLifecycle {
    pub fn new(registry: PluginPackageRegistry) -> Self {
        Self { registry }
    }

    /// Verify and assess a staged candidate for import or update selection. All file reads and
    /// JSON processing run on blocking workers; environment detection has its own worker too.
    pub async fn inspect_candidate(
        &self,
        package_root: impl AsRef<Path>,
        publisher_key: [u8; 32],
        emulation_policy: EmulationPolicy,
    ) -> Result<PackageCandidateReport, PackageLifecycleError> {
        let environment = gamenode_platform::plugin_environment::detect()
            .await
            .map_err(PackageLifecycleError::Environment)?;
        let package_root = package_root.as_ref().to_owned();
        let evaluation_environment = environment.clone();
        let (manifest, targets) = tokio::task::spawn_blocking(move || {
            let integrity = verify_package_directory(&package_root, &publisher_key)
                .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
            let manifest_bytes = read_signed_manifest(&package_root, &integrity)?;
            let manifest = Manifest::parse_json(&manifest_bytes)
                .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?
                .manifest()
                .clone();
            validate_manifest_payload(&package_root, &integrity, &manifest)?;
            let targets =
                assess_manifest_targets(&manifest_bytes, &evaluation_environment, emulation_policy)
                    .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
            Ok::<_, PackageLifecycleError>((manifest, targets))
        })
        .await
        .map_err(|_| PackageLifecycleError::WorkerFailed)??;
        Ok(PackageCandidateReport {
            manifest,
            environment,
            targets,
        })
    }

    /// Install a compatible candidate. An incompatible update is rejected before the registry
    /// is changed, leaving the active version and all plugin data untouched.
    pub async fn install_candidate(
        &self,
        package_root: impl AsRef<Path>,
        publisher_key: [u8; 32],
        emulation_policy: EmulationPolicy,
        activate: bool,
    ) -> Result<InstalledPluginVersion, PackageLifecycleError> {
        let package_root = package_root.as_ref().to_owned();
        let report = self
            .inspect_candidate(&package_root, publisher_key, emulation_policy)
            .await?;
        report.selected_target()?;
        let plugin_id = report.manifest.id;
        let version = report.manifest.version;
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            registry
                .install_signed(
                    &plugin_id,
                    &version,
                    &package_root,
                    &publisher_key,
                    activate,
                )
                .map_err(|error| PackageLifecycleError::Registry(error.to_string()))
        })
        .await
        .map_err(|_| PackageLifecycleError::WorkerFailed)?
    }

    /// Revalidate a stored version against current environment facts before switching the active
    /// pointer. This does not execute the package and does not fall back to another version.
    pub async fn activate_version(
        &self,
        plugin_id: String,
        version: String,
        publisher_key: [u8; 32],
        emulation_policy: EmulationPolicy,
    ) -> Result<InstalledPluginVersion, PackageLifecycleError> {
        let registry = self.registry.clone();
        let lookup_id = plugin_id.clone();
        let lookup_version = version.clone();
        let installed = tokio::task::spawn_blocking(move || {
            registry
                .versions(&lookup_id)
                .map_err(|error| PackageLifecycleError::Registry(error.to_string()))?
                .into_iter()
                .find(|installed| installed.version == lookup_version)
                .ok_or(PackageLifecycleError::VersionNotFound)
        })
        .await
        .map_err(|_| PackageLifecycleError::WorkerFailed)??;
        let report = self
            .inspect_candidate(&installed.package_path, publisher_key, emulation_policy)
            .await?;
        if report.manifest.id != plugin_id || report.manifest.version != version {
            return Err(PackageLifecycleError::RegistryIdentityMismatch);
        }
        report.selected_target()?;
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            registry
                .set_active(&plugin_id, &version)
                .map_err(|error| PackageLifecycleError::Registry(error.to_string()))?;
            Ok::<_, PackageLifecycleError>(installed)
        })
        .await
        .map_err(|_| PackageLifecycleError::WorkerFailed)?
    }
}

fn read_signed_manifest(
    package_root: &Path,
    integrity: &IntegrityManifest,
) -> Result<Vec<u8>, PackageLifecycleError> {
    let manifest_path = package_root.join("plugin.json");
    let metadata = fs::symlink_metadata(&manifest_path)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        return Err(PackageLifecycleError::InvalidPackage(
            "plugin.json is not a bounded regular file".into(),
        ));
    }
    let bytes = fs::read(&manifest_path)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    let expected =
        integrity.files.0.get("plugin.json").ok_or_else(|| {
            PackageLifecycleError::InvalidPackage("plugin.json is unsigned".into())
        })?;
    if format!("{:x}", sha2::Sha256::digest(&bytes)) != *expected {
        return Err(PackageLifecycleError::InvalidPackage(
            "plugin.json changed during compatibility validation".into(),
        ));
    }
    Ok(bytes)
}

fn validate_manifest_payload(
    package_root: &Path,
    integrity: &IntegrityManifest,
    manifest: &Manifest,
) -> Result<(), PackageLifecycleError> {
    let mut referenced = manifest.license.files.clone();
    referenced.extend(manifest.license.notices.iter().cloned());
    referenced.extend(manifest.resources.values().cloned());
    referenced.extend(manifest.schemas.iter().map(|schema| schema.asset.clone()));
    referenced.extend(
        manifest
            .targets
            .iter()
            .map(|target| target.entrypoint.clone()),
    );
    for path in referenced {
        if !integrity.files.0.contains_key(&path) {
            return Err(PackageLifecycleError::InvalidPackage(format!(
                "manifest reference is missing from signed payload: {path}"
            )));
        }
    }
    if !manifest.schemas.is_empty() {
        let mut assets = std::collections::BTreeMap::new();
        for schema in &manifest.schemas {
            assets.insert(
                schema.asset.clone(),
                read_signed_payload(package_root, integrity, &schema.asset, 64 * 1024)?,
            );
        }
        manifest
            .validate_schema_assets(&assets)
            .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    }
    Ok(())
}

fn read_signed_payload(
    package_root: &Path,
    integrity: &IntegrityManifest,
    relative_path: &str,
    max_bytes: u64,
) -> Result<Vec<u8>, PackageLifecycleError> {
    let root = fs::canonicalize(package_root)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    let path = package_root.join(relative_path);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > max_bytes {
        return Err(PackageLifecycleError::InvalidPackage(format!(
            "signed payload is not a bounded regular file: {relative_path}"
        )));
    }
    let canonical = fs::canonicalize(&path)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    if !canonical.starts_with(root) {
        return Err(PackageLifecycleError::InvalidPackage(format!(
            "signed payload escapes package root: {relative_path}"
        )));
    }
    let bytes = fs::read(canonical)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    let expected = integrity
        .files
        .0
        .get(relative_path)
        .ok_or_else(|| PackageLifecycleError::InvalidPackage("payload is unsigned".into()))?;
    if format!("{:x}", sha2::Sha256::digest(&bytes)) != *expected {
        return Err(PackageLifecycleError::InvalidPackage(format!(
            "signed payload changed during validation: {relative_path}"
        )));
    }
    Ok(bytes)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use base64::Engine;
    use ring::signature::KeyPair;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn write_signed_package(
        root: &Path,
        version: &str,
        os: &str,
        key: &ring::signature::Ed25519KeyPair,
    ) {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("licenses")).unwrap();
        fs::write(root.join("bin/plugin"), b"fixture executable").unwrap();
        fs::write(root.join("licenses/LICENSE"), b"fixture license").unwrap();
        let abi = if os == "windows" { "msvc" } else { "gnu" };
        let manifest = serde_json::to_vec(&serde_json::json!({
            "schema": 1,
            "id": "org.example.lifecycle",
            "kind": "provider",
            "provider_id": "lifecycle-fixture",
            "name": "Lifecycle Fixture",
            "version": version,
            "api": {"min": "1.4.0", "max": "1.4.0"},
            "targets": [{"os": os, "arch": "x86_64", "abi": abi, "entrypoint": "bin/plugin"}],
            "publisher": {
                "key_id": "fixture-key",
                "public_key": base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref())
            },
            "license": {"expression": "MIT", "files": ["licenses/LICENSE"]}
        }))
        .unwrap();
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
        let mut signed = gamenode_platform::plugin_packages::SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(&integrity);
        fs::write(root.join("integrity.json"), integrity).unwrap();
        fs::write(root.join("signature.ed25519"), key.sign(&signed).as_ref()).unwrap();
    }

    #[tokio::test]
    async fn incompatible_update_and_activation_leave_previous_version_active() {
        let base = tempdir().unwrap();
        let registry = PluginPackageRegistry::at(base.path().join("registry")).unwrap();
        let lifecycle = ManagedPackageLifecycle::new(registry.clone());
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
        let public_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
        let v1 = base.path().join("v1");
        write_signed_package(&v1, "1.0.0", "linux", &key);
        let installed = lifecycle
            .install_candidate(&v1, public_key, EmulationPolicy::RequireNative, true)
            .await
            .unwrap();
        assert_eq!(installed.version, "1.0.0");

        let v2 = base.path().join("v2");
        write_signed_package(&v2, "2.0.0", "windows", &key);
        assert!(matches!(
            lifecycle
                .install_candidate(&v2, public_key, EmulationPolicy::RequireNative, true,)
                .await,
            Err(PackageLifecycleError::NoCompatibleTarget(_))
        ));
        assert_eq!(
            registry
                .active("org.example.lifecycle")
                .unwrap()
                .unwrap()
                .version,
            "1.0.0"
        );
        assert_eq!(registry.versions("org.example.lifecycle").unwrap().len(), 1);

        let v2 = base.path().join("v2");
        registry
            .install_signed("org.example.lifecycle", "2.0.0", &v2, &public_key, false)
            .unwrap();
        assert!(matches!(
            lifecycle
                .activate_version(
                    "org.example.lifecycle".into(),
                    "2.0.0".into(),
                    public_key,
                    EmulationPolicy::RequireNative,
                )
                .await,
            Err(PackageLifecycleError::NoCompatibleTarget(_))
        ));
        assert_eq!(
            registry
                .active("org.example.lifecycle")
                .unwrap()
                .unwrap()
                .version,
            "1.0.0"
        );
    }
}
