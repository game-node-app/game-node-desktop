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
    pub extension_schemas: Vec<gamenode_plugin_protocol::proto::ExtensionSchemaDescriptor>,
}

/// UI diagnostics for signed packages. Only the effective execution facts used
/// for review are returned, rather than the raw host or os-release metadata.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageCompatibilityReview {
    pub state: CompatibilityState,
    pub ambiguous: bool,
    pub os: Option<String>,
    pub release: Option<String>,
    pub product: Option<gamenode_platform::plugin_environment::OperatingSystemKind>,
    pub build: Option<u32>,
    pub architecture: Option<String>,
    pub abi: Option<String>,
    pub libc: Option<gamenode_platform::plugin_environment::LibcIdentity>,
    pub emulation: gamenode_platform::plugin_environment::EmulationStatus,
    pub targets: Vec<TargetCompatibilityAssessment>,
}

impl PackageCandidateReport {
    pub fn compatibility_review(&self) -> PackageCompatibilityReview {
        let selected = self.selected_target();
        let ambiguous = matches!(&selected, Err(PackageLifecycleError::AmbiguousTarget));
        let state = if selected.is_ok() {
            CompatibilityState::Compatible
        } else if ambiguous {
            CompatibilityState::Incompatible
        } else if self
            .targets
            .iter()
            .any(|target| target.verdict.state == CompatibilityState::Indeterminate)
        {
            CompatibilityState::Indeterminate
        } else {
            CompatibilityState::Incompatible
        };
        let userspace = self.environment.userspace.as_ref();
        PackageCompatibilityReview {
            state,
            ambiguous,
            os: userspace.map(|os| os.id.clone().unwrap_or_else(|| os.family.clone())),
            release: userspace.and_then(|os| os.version_id.clone()),
            product: userspace.and_then(|os| os.kind),
            build: userspace.and_then(|os| os.build),
            architecture: self.environment.process_architecture.clone(),
            abi: self.environment.process_abi.clone(),
            libc: self.environment.libc.clone(),
            emulation: self.environment.emulation.clone(),
            targets: self.targets.clone(),
        }
    }

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

/// Ordering boundary for host writes that change the active package, pinned publisher, or
/// runtime registration of a plugin. Acquire it inside the worker that owns the write.
pub(super) fn lock_publisher_identity() -> std::sync::MutexGuard<'static, ()> {
    static PUBLISHER_IDENTITY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    PUBLISHER_IDENTITY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        inspect_staged_package(package_root, publisher_key, emulation_policy).await
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
        let entrypoint = report.selected_target()?.target.entrypoint.clone();
        let plugin_id = report.manifest.id;
        let version = report.manifest.version;
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            let _publisher = activate.then(lock_publisher_identity);
            registry
                .install_signed_with_entrypoint(
                    &plugin_id,
                    &version,
                    &package_root,
                    &publisher_key,
                    &entrypoint,
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
            let _publisher = lock_publisher_identity();
            registry
                .set_active(&plugin_id, &version)
                .map_err(|error| PackageLifecycleError::Registry(error.to_string()))?;
            Ok::<_, PackageLifecycleError>(installed)
        })
        .await
        .map_err(|_| PackageLifecycleError::WorkerFailed)?
    }

    /// Removes every installed version after the caller has stopped processes and revoked
    /// sessions. Filesystem work stays on a blocking worker.
    pub async fn remove_plugin(&self, plugin_id: String) -> Result<(), PackageLifecycleError> {
        let registry = self.registry.clone();
        tokio::task::spawn_blocking(move || {
            let _publisher = lock_publisher_identity();
            registry
                .remove_plugin(&plugin_id)
                .map_err(|error| PackageLifecycleError::Registry(error.to_string()))
        })
        .await
        .map_err(|_| PackageLifecycleError::WorkerFailed)?
    }
}

/// Verify and assess one staged package directory without needing a registry. Bundle inspection
/// and standalone import share this entry point so neither can drift from the other. All file
/// reads and JSON processing run on blocking workers; environment detection has its own worker.
pub(super) async fn inspect_staged_package(
    package_root: impl AsRef<Path>,
    publisher_key: [u8; 32],
    emulation_policy: EmulationPolicy,
) -> Result<PackageCandidateReport, PackageLifecycleError> {
    let environment = gamenode_platform::plugin_environment::detect()
        .await
        .map_err(PackageLifecycleError::Environment)?;
    inspect_staged_package_in(package_root, publisher_key, emulation_policy, environment).await
}

/// The same verification with an execution environment the caller already detected. A bundle
/// install evaluates every component on one worker and must not re-detect the environment for
/// each one; the result is otherwise identical to [`inspect_staged_package`].
pub(super) async fn inspect_staged_package_in(
    package_root: impl AsRef<Path>,
    publisher_key: [u8; 32],
    emulation_policy: EmulationPolicy,
    environment: PluginEnvironment,
) -> Result<PackageCandidateReport, PackageLifecycleError> {
    let package_root = package_root.as_ref().to_owned();
    tokio::task::spawn_blocking(move || {
        inspect_staged_package_blocking(
            &package_root,
            publisher_key,
            emulation_policy,
            &environment,
        )
    })
    .await
    .map_err(|_| PackageLifecycleError::WorkerFailed)?
}

/// The blocking body shared by every staged inspection path. Callers own the environment and
/// must already be on a worker thread.
pub(super) fn inspect_staged_package_blocking(
    package_root: &Path,
    publisher_key: [u8; 32],
    emulation_policy: EmulationPolicy,
    environment: &PluginEnvironment,
) -> Result<PackageCandidateReport, PackageLifecycleError> {
    let integrity = verify_package_directory(package_root, &publisher_key)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    let manifest_bytes = read_signed_manifest(package_root, &integrity)?;
    let manifest = Manifest::parse_json(&manifest_bytes)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?
        .manifest()
        .clone();
    let extension_schemas = validate_manifest_payload(package_root, &integrity, &manifest)?;
    let targets = assess_manifest_targets(&manifest_bytes, environment, emulation_policy)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    Ok(PackageCandidateReport {
        manifest,
        environment: environment.clone(),
        targets,
        extension_schemas,
    })
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
) -> Result<Vec<gamenode_plugin_protocol::proto::ExtensionSchemaDescriptor>, PackageLifecycleError>
{
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
    let mut descriptors = Vec::new();
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
        for schema in &manifest.schemas {
            let bytes = assets.get(&schema.asset).ok_or_else(|| {
                PackageLifecycleError::InvalidPackage("schema asset disappeared".into())
            })?;
            descriptors.push(packaged_schema_descriptor(schema, bytes)?);
        }
    }
    gamenode_plugin_protocol::extensions::validate_schema_catalog(&descriptors).map_err(|_| {
        PackageLifecycleError::InvalidPackage("invalid extension schema catalog".into())
    })?;
    Ok(descriptors)
}

fn packaged_schema_descriptor(
    packaged: &super::package::PackagedSchema,
    bytes: &[u8],
) -> Result<gamenode_plugin_protocol::proto::ExtensionSchemaDescriptor, PackageLifecycleError> {
    use gamenode_plugin_protocol::proto::{
        ExtensionFieldClassification as Classification, ExtensionSchemaDescriptor,
        ExtensionSchemaField,
    };
    let schema: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    semver::Version::parse(&packaged.version)
        .map_err(|error| PackageLifecycleError::InvalidPackage(error.to_string()))?;
    // Full SemVer is encoded in the id so prereleases remain distinguishable; wire version 1
    // identifies this manifest-to-protocol descriptor mapping.
    let encoded_version = packaged
        .version
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let schema_id = format!("{}.v{encoded_version}", packaged.namespace);
    if schema_id.len() > 256 {
        return Err(PackageLifecycleError::InvalidPackage(
            "schema identity is too long".into(),
        ));
    }
    let mut fields = Vec::new();
    collect_schema_fields(&schema, "", &mut fields)?;
    let fields = fields
        .into_iter()
        .map(|(path, classification)| ExtensionSchemaField {
            path,
            classification: match classification {
                1 => Classification::Public as i32,
                2 => Classification::Sensitive as i32,
                _ => Classification::Private as i32,
            },
        })
        .collect();
    let descriptor = ExtensionSchemaDescriptor {
        namespace: packaged.namespace.clone(),
        schema_id,
        schema_version: 1,
        schema_digest: sha2::Sha256::digest(bytes).to_vec(),
        schema_json: bytes.to_vec(),
        fields,
        optional: true,
    };
    gamenode_plugin_protocol::extensions::validate_schema_descriptor(&descriptor).map_err(
        |_| {
            PackageLifecycleError::InvalidPackage(
                "packaged schema is outside the runtime schema subset".into(),
            )
        },
    )?;
    Ok(descriptor)
}

fn collect_schema_fields(
    schema: &serde_json::Value,
    prefix: &str,
    output: &mut Vec<(String, u8)>,
) -> Result<u8, PackageLifecycleError> {
    let object = schema.as_object().ok_or_else(|| {
        PackageLifecycleError::InvalidPackage("schema node must be an object".into())
    })?;
    let own = match object
        .get("x-gamenode-classification")
        .and_then(serde_json::Value::as_str)
    {
        Some("public") => 1,
        Some("sensitive") => 2,
        Some("private") => 3,
        _ if prefix.is_empty() => 1,
        _ => {
            return Err(PackageLifecycleError::InvalidPackage(
                "schema field lacks a valid classification".into(),
            ));
        }
    };
    let mut maximum = own;
    if let Some(properties) = object
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        for (name, child) in properties {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}.{name}")
            };
            let class = collect_schema_fields(child, &path, output)?;
            maximum = maximum.max(class);
            output.push((path, class));
        }
    }
    if prefix.is_empty() {
        return Ok(maximum);
    }
    Ok(own.max(maximum))
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
pub(super) mod tests {
    use super::*;
    use base64::Engine;
    use ring::signature::KeyPair;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[test]
    fn signed_manifest_schema_maps_to_host_owned_optional_descriptor() {
        let bytes = br#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":{"display":{"type":"object","properties":{"label":{"type":"string","maxLength":80,"x-gamenode-classification":"public"},"notes":{"type":"string","maxLength":80,"x-gamenode-classification":"sensitive"}},"required":["label","notes"],"additionalProperties":false,"x-gamenode-classification":"public"}},"required":["display"],"additionalProperties":false}"#;
        let descriptor = packaged_schema_descriptor(
            &super::super::package::PackagedSchema {
                namespace: "org.example.catalog".into(),
                version: "1.2.3".into(),
                classification: super::super::package::DataClassification::Sensitive,
                asset: "schemas/catalog.json".into(),
            },
            bytes,
        )
        .unwrap();
        assert_eq!(descriptor.schema_id, "org.example.catalog.v1-2-3");
        assert_eq!(descriptor.schema_version, 1);
        assert!(descriptor.optional);
        assert!(descriptor.fields.iter().any(|field| {
            field.path == "display.label"
                && field.classification
                    == gamenode_plugin_protocol::proto::ExtensionFieldClassification::Public as i32
        }));
        assert!(descriptor.fields.iter().any(|field| {
            field.path == "display"
                && field.classification
                    == gamenode_plugin_protocol::proto::ExtensionFieldClassification::Sensitive
                        as i32
        }));
    }

    pub(in crate::plugin_host) fn write_signed_package(
        root: &Path,
        version: &str,
        os: &str,
        key: &ring::signature::Ed25519KeyPair,
    ) {
        write_signed_package_with_playtime(root, version, os, key, None);
    }

    fn write_signed_package_with_playtime(
        root: &Path,
        version: &str,
        os: &str,
        key: &ring::signature::Ed25519KeyPair,
        playtime: Option<serde_json::Value>,
    ) {
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("licenses")).unwrap();
        fs::write(root.join("bin/plugin"), b"fixture executable").unwrap();
        fs::write(root.join("licenses/LICENSE"), b"fixture license").unwrap();
        let abi = if os == "windows" { "msvc" } else { "gnu" };
        let mut manifest_value = serde_json::json!({
            "schema": 1,
            "id": "org.example.lifecycle",
            "kind": "provider",
            "provider_id": "lifecycle-fixture",
            "name": "Lifecycle Fixture",
            "version": version,
            "api": {"min": "1.6.0", "max": "1.6.0"},
            "targets": [{"os": os, "arch": "x86_64", "abi": abi, "entrypoint": "bin/plugin"}],
            "publisher": {
                "key_id": "fixture-key",
                "public_key": base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref())
            },
            "license": {"expression": "MIT", "files": ["licenses/LICENSE"]}
        });
        if let Some(playtime) = playtime {
            manifest_value["playtime"] = playtime;
        }
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
        let mut signed = gamenode_platform::plugin_packages::SIGNATURE_DOMAIN_PREFIX.to_vec();
        signed.extend_from_slice(&integrity);
        fs::write(root.join("integrity.json"), integrity).unwrap();
        fs::write(root.join("signature.ed25519"), key.sign(&signed).as_ref()).unwrap();
    }

    #[test]
    fn playtime_disclosure_is_part_of_the_verified_package_manifest() {
        let base = tempdir().unwrap();
        let package = base.path().join("signed-playtime");
        let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
        let public_key: [u8; 32] = key.public_key().as_ref().try_into().unwrap();
        write_signed_package_with_playtime(
            &package,
            "1.0.0",
            "linux",
            &key,
            Some(serde_json::json!({
                "identifies_gameplay": true,
                "identifies_pauses": true,
                "without_pause_detection": "disabled"
            })),
        );

        let integrity = verify_package_directory(&package, &public_key).unwrap();
        let manifest_bytes = read_signed_manifest(&package, &integrity).unwrap();
        let manifest = Manifest::parse_json(&manifest_bytes).unwrap();
        assert!(manifest.manifest().playtime.identifies_gameplay);
        assert!(manifest.manifest().playtime.identifies_pauses);

        let manifest_path = package.join("plugin.json");
        let mut edited: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        edited["playtime"]["identifies_pauses"] = serde_json::Value::Bool(false);
        fs::write(&manifest_path, serde_json::to_vec(&edited).unwrap()).unwrap();
        assert!(verify_package_directory(&package, &public_key).is_err());
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
