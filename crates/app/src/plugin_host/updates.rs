//! Feed-driven updates for managed third-party packages.
//!
//! A selected feed release is downloaded to inert bytes, checked against the signed feed and
//! package integrity metadata, extracted with the local-import allowlist, then sent through the
//! ordinary compatibility and registry installation path. Calling `apply_approved_release`
//! represents the user's explicit approval; this module never starts plugin code.

use super::managed_packages::{ManagedPackageLifecycle, PackageLifecycleError};
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
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

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
}
