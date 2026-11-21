//! Per-component inspection of a verified bundle without executing package code.
//!
//! A bundle is only a distribution envelope. Every component is staged through the ordinary
//! bounded package extractor and evaluated with the same signature, manifest and compatibility
//! code a standalone import uses, so a component is never trusted merely because the bundle
//! verified.
//!
//! The two failure modes stay separate:
//!
//! - A structural failure (bad envelope, corrupt component archive, signature failure, invalid
//!   manifest) rejects the whole bundle: [`inspect_bundle_candidate`] returns an error and no
//!   component report.
//! - A component that is only incompatible with the current platform or API is reported as
//!   [`BundleComponentAvailability::Unavailable`] while compatible siblings remain available.
//!   Installed, that component is disabled; the bundle is not rejected.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use gamenode_platform::{
    plugin_bundles::{BundleError, stage_bundle_archive},
    plugin_environment::EmulationPolicy,
    plugin_packages::stage_package_archive,
};
use thiserror::Error;

use super::{
    managed_packages::{
        PackageCandidateReport, PackageCompatibilityReview, PackageLifecycleError,
        inspect_staged_package,
    },
    package::PluginKind,
    publisher_trust::key_fingerprint,
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

/// Verify a bundle archive, stage every component through the bounded package extractor and run
/// the ordinary candidate inspection on each one. No package code is executed and all file I/O
/// runs on blocking workers; the private staging directory is removed before this returns.
pub async fn inspect_bundle_candidate(
    archive_path: impl AsRef<Path>,
    emulation_policy: EmulationPolicy,
) -> Result<BundleCandidateReport, BundleInspectionError> {
    let archive_path = archive_path.as_ref().to_owned();
    let work = tokio::task::spawn_blocking(InspectionWorkDir::create)
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
        components.push(BundleComponentInspection {
            plugin_id: report.manifest.id.clone(),
            kind: component.kind.clone(),
            version: report.manifest.version.clone(),
            archive_path: component.path.clone(),
            availability: availability(&report),
            compatibility: report.compatibility_review(),
        });
    }

    Ok(BundleCandidateReport { bundle, components })
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

/// Private staging root for one inspection. It is created with owner-only permissions and
/// removed on drop, including when a component fails validation.
struct InspectionWorkDir {
    path: PathBuf,
}

impl InspectionWorkDir {
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
            "could not reserve bundle inspection staging",
        ))
    }
}

impl Drop for InspectionWorkDir {
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
    use crate::plugin_host::{compatibility::CompatibilityState, publisher_trust};
    use base64::Engine as _;
    use std::{
        io::{Read, Write},
        path::PathBuf,
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
}
