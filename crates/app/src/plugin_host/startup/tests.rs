//! Regression coverage for composing installed third-party provider packages into the library.
//!
//! The bug this covers: the desktop built `LibraryMachine` from the embedded official manifests
//! only, while an installed third-party provider lived in the package registry and was registered
//! with the runtime. These tests install real signed fixture packages into a temporary registry and
//! drive the same startup and post-import composition the binary uses, through the library scan
//! effects and a real SQLite library store.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine as _;
use gamenode_core::library::{
    Effect, Input, LastAccount, LibraryCache, LibraryMachine, LibraryStore, ScanOutcome,
};
use gamenode_platform::db::SqliteSettingsStore;
use gamenode_platform::paths::DataPaths;
use gamenode_platform::plugin_registry::PluginPackageRegistry;
use gamenode_plugin_protocol::models;
use gamenode_plugin_protocol::proto::{
    DiscoveryRecord, DiscoveryResult, OperationDisposition, OperationResult, ProviderOperation,
};
use tonic::Status;

use super::*;
use crate::plugin_host::provider::Completion;
use crate::plugin_host::publisher_trust;
use crate::plugin_host::runtime::{Observer, ProviderRuntime};
use crate::plugin_host::updates::{PluginImportInstallOutcome, PluginUpdateService};

const FIXTURE_PLUGIN: &str = "dev.example.third-party-provider";
const FIXTURE_PROVIDER: &str = "third-party-fixture";
const FIXTURE_VERSION: &str = "1.0.0";

/// Runs the repository package tool; the fixture key and every archive are real tool output.
fn package_tool(work: &Path, args: &[&str]) -> String {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/plugin-package.py");
    assert!(script.is_file(), "missing plugin package tool: {script:?}");
    let output = std::process::Command::new("python3")
        .arg(&script)
        .args(args)
        .current_dir(work)
        .output()
        .expect("python3 with the cryptography package must run the package fixture tooling");
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
    key_bytes: [u8; 32],
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
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
        let key_bytes: [u8; 32] = base64::engine::general_purpose::STANDARD
            .decode(&public_key)
            .unwrap()
            .try_into()
            .unwrap();
        Self {
            dir,
            key,
            public_key,
            key_bytes,
        }
    }

    fn fingerprint(&self) -> String {
        publisher_trust::key_fingerprint(&self.key_bytes)
    }

    /// A signed provider package. `provider_id` defaults to the fixture provider.
    fn provider(&self, name: &str, provider_id: Option<&str>, discovery: bool) -> Package {
        self.provider_as(
            FIXTURE_PLUGIN,
            name,
            provider_id.unwrap_or(FIXTURE_PROVIDER),
            discovery,
            "linux",
        )
    }

    fn provider_as(
        &self,
        plugin_id: &str,
        name: &str,
        provider_id: &str,
        discovery: bool,
        os: &str,
    ) -> Package {
        let mut permissions = vec![serde_json::json!({
            "id": "provider.context.read",
            "required": true,
            "scope": "own-provider",
            "reason": "Read the host-issued provider context."
        })];
        if discovery {
            permissions.push(serde_json::json!({
                "id": "library.discovery",
                "required": false,
                "scope": "own-provider",
                "reason": "Discover this provider's games."
            }));
        }
        self.package(
            plugin_id,
            name,
            "provider",
            Some(provider_id),
            permissions,
            os,
            "1.6.0",
        )
    }

    fn extension(&self, plugin_id: &str, name: &str) -> Package {
        self.package(
            plugin_id,
            name,
            "extension",
            None,
            vec![serde_json::json!({
                "id": "game.launch",
                "required": false,
                "scope": "own-provider",
                "reason": "Launch a game."
            })],
            "linux",
            "1.6.0",
        )
    }

    fn package(
        &self,
        plugin_id: &str,
        name: &str,
        kind: &str,
        provider_id: Option<&str>,
        permissions: Vec<serde_json::Value>,
        os: &str,
        api_min: &str,
    ) -> Package {
        let slug = name.to_ascii_lowercase().replace([' ', '/'], "-");
        let source = self.dir.path().join(format!("{slug}-source"));
        std::fs::create_dir_all(source.join("bin")).unwrap();
        std::fs::create_dir_all(source.join("licenses")).unwrap();
        std::fs::write(source.join("bin/plugin"), b"fixture executable").unwrap();
        std::fs::write(source.join("licenses/LICENSE"), b"fixture license").unwrap();
        let mut manifest = serde_json::json!({
            "schema": 1,
            "id": plugin_id,
            "kind": kind,
            "name": name,
            "version": FIXTURE_VERSION,
            "api": { "min": api_min, "max": "1.6.0" },
            "permissions": permissions,
            "targets": [{
                "os": os,
                "arch": "x86_64",
                "abi": "gnu",
                "entrypoint": "bin/plugin"
            }],
            "publisher": {
                "key_id": "fixture-dev",
                "public_key": self.public_key
            },
            "license": { "expression": "MIT", "files": ["licenses/LICENSE"] }
        });
        if let Some(provider_id) = provider_id {
            manifest["provider_id"] = serde_json::json!(provider_id);
        }
        std::fs::write(
            source.join("plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let archive = self.dir.path().join(format!("{slug}.gnplugin"));
        package_tool(
            self.dir.path(),
            &[
                "pack",
                "--source",
                source.to_str().unwrap(),
                "--output",
                archive.to_str().unwrap(),
                "--private-key",
                self.key.to_str().unwrap(),
            ],
        );
        // `pack` writes the signed metadata into the archive, not the source directory. The
        // reviewed import stages the extracted archive; do the same for direct registry installs.
        let staged = self.dir.path().join(format!("{slug}-staged"));
        extract_archive(&archive, &staged);
        Package { archive, staged }
    }
}

struct Package {
    archive: PathBuf,
    /// The archive contents, including `integrity.json` and `signature.ed25519`.
    staged: PathBuf,
}

fn extract_archive(archive: &Path, dest: &Path) {
    let file = std::fs::File::open(archive).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).unwrap();
        let name = entry
            .enclosed_name()
            .unwrap_or_else(|| panic!("unsafe archive path: {}", entry.name()));
        let path = dest.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).unwrap();
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut out = std::fs::File::create(&path).unwrap();
        std::io::copy(&mut entry, &mut out).unwrap();
    }
}

fn paths() -> (tempfile::TempDir, DataPaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path().join("app-data"));
    paths.ensure_data_dir().unwrap();
    crate::plugin_host::profile::PluginStateLayout::prepare_managed(&paths).unwrap();
    (dir, paths)
}

fn trust(fixture: &Fixture, paths: &DataPaths, plugin_id: &str) {
    let dir = crate::plugin_host::profile::PluginStateLayout::managed(paths).trust_dir();
    let mut store = publisher_trust::PublisherTrustStore::open(dir).unwrap();
    store
        .confirm_first_key(plugin_id, &fixture.key_bytes, &fixture.fingerprint())
        .unwrap();
}

/// Installs a signed provider package directly into the immutable registry, exactly as an
/// accepted import leaves it.
fn install_signed(fixture: &Fixture, paths: &DataPaths, plugin_id: &str, package: &Package) {
    let registry = PluginPackageRegistry::open(paths).unwrap();
    registry
        .install_signed_with_entrypoint(
            plugin_id,
            FIXTURE_VERSION,
            &package.staged,
            &fixture.key_bytes,
            "bin/plugin",
            true,
        )
        .unwrap();
}

/// Installs an arbitrary package directory without signature validation. Used only to model the
/// malformed and reserved-id entries a corrupt or legacy registry can contain; the reviewed
/// import flow refuses these before any registry write.
fn install_unverified(paths: &DataPaths, plugin_id: &str, package_root: &Path) {
    let registry = PluginPackageRegistry::open(paths).unwrap();
    registry
        .install_verified(plugin_id, FIXTURE_VERSION, package_root, true)
        .unwrap();
}

fn library_store(paths: &DataPaths) -> std::sync::Arc<dyn LibraryStore> {
    let settings = SqliteSettingsStore::open(&paths.database_file()).unwrap();
    std::sync::Arc::new(settings.library_store())
}

fn account() -> LastAccount {
    LastAccount {
        id: "fixture-account".to_owned(),
        name: "Fixture Account".to_owned(),
    }
}

/// Starts the library for one account and returns the initial scan effects.
fn start_machine(providers: Vec<Provider>) -> (LibraryMachine, Vec<Effect>) {
    let mut machine = LibraryMachine::new(providers, Default::default());
    let effects = machine.handle(Input::AccountChanged(Some(account())));
    let op = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadCache { op, .. } => Some(*op),
            _ => None,
        })
        .expect("the account load starts with a cache read");
    let effects = machine.handle(Input::Loaded {
        op,
        result: Ok(LibraryCache::default()),
    });
    (machine, effects)
}

/// A provider runtime whose answer is controlled per provider. Official providers scan empty;
/// the fixture provider reports the configured records or failure.
struct FixtureRuntime {
    provider: String,
    records: Mutex<Vec<DiscoveryRecord>>,
    failure: Mutex<Option<String>>,
    discoveries: Mutex<Vec<String>>,
}

impl FixtureRuntime {
    fn new(provider: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            records: Mutex::new(Vec::new()),
            failure: Mutex::new(None),
            discoveries: Mutex::new(Vec::new()),
        }
    }

    fn set_records(&self, records: Vec<DiscoveryRecord>) {
        *self.records.lock().unwrap() = records;
    }

    fn fail(&self, reason: &str) {
        *self.failure.lock().unwrap() = Some(reason.to_owned());
    }

    fn discoveries(&self) -> Vec<String> {
        self.discoveries.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ProviderRuntime for FixtureRuntime {
    fn accounts(&self) -> Vec<String> {
        Vec::new()
    }
    fn has_provider(&self, _provider: &str) -> bool {
        true
    }
    fn status(&self, _provider: &str) -> Option<models::AccountStatus> {
        None
    }
    fn available(&self, _provider: &str) -> bool {
        true
    }
    fn set_observer(&self, _observer: Observer) {}
    async fn shutdown(&self) {}
    async fn execute(
        &self,
        provider: &str,
        operation: ProviderOperation,
    ) -> Result<Completion, Status> {
        if operation != ProviderOperation::Discover {
            return Ok(Completion::fixture(
                OperationResult {
                    disposition: OperationDisposition::Completed as i32,
                    ..Default::default()
                },
                Default::default(),
            ));
        }
        if provider == self.provider {
            self.discoveries.lock().unwrap().push(provider.to_owned());
            if let Some(reason) = self.failure.lock().unwrap().clone() {
                return Err(Status::unavailable(reason));
            }
        }
        let records = if provider == self.provider {
            self.records.lock().unwrap().clone()
        } else {
            Vec::new()
        };
        Ok(Completion::fixture(
            OperationResult {
                disposition: OperationDisposition::Completed as i32,
                discovery: Some(DiscoveryResult {
                    provider_id: provider.to_owned(),
                    records,
                    ..Default::default()
                }),
                ..Default::default()
            },
            Default::default(),
        ))
    }
}

/// Drives the machine's scan effects through the runtime and persists the outcomes, mirroring the
/// engine's `LibEffect::RunScan` / `LibEffect::PersistScan` path without a network or account.
async fn drive(
    machine: &mut LibraryMachine,
    runtime: &dyn ProviderRuntime,
    store: &dyn LibraryStore,
    effects: Vec<Effect>,
) {
    let mut queue: VecDeque<Effect> = effects.into();
    while let Some(effect) = queue.pop_front() {
        match effect {
            Effect::RunScan { op, provider } => {
                let result = match runtime
                    .execute(&provider, ProviderOperation::Discover)
                    .await
                {
                    Ok(completion) => completion
                        .result
                        .discovery
                        .filter(|discovery| discovery.provider_id == provider)
                        .map(|discovery| {
                            gamenode_core::provider::record::finish_records(
                                &provider,
                                discovery
                                    .records
                                    .into_iter()
                                    .map(|record| gamenode_core::provider::RawRecord {
                                        id: Some(record.id),
                                        name: Some(record.name),
                                        install_dir: Some(record.install_dir),
                                        fields: record.fields.into_iter().collect(),
                                    })
                                    .collect(),
                                &[],
                            )
                        })
                        .ok_or_else(|| "provider discovery returned no observations".to_owned()),
                    Err(status) => Err(status.message().to_owned()),
                };
                let more = machine.handle(Input::ProviderScanned {
                    op,
                    provider,
                    result,
                    targets: BTreeMap::new(),
                    now: 100,
                });
                queue.extend(more);
            }
            Effect::PersistScan {
                provider,
                outcome,
                now,
            } => match outcome {
                ScanOutcome::Scanned { records, targets } => {
                    store.save_scan(&provider, &records, &targets, now).unwrap();
                }
                ScanOutcome::Failed(error) => store.save_scan_failure(&provider, &error).unwrap(),
            },
            _ => {}
        }
    }
}

fn row(paths: &DataPaths, provider: &str) -> Option<(Option<i64>, Option<String>)> {
    let conn = gamenode_platform::db::open(&paths.database_file()).unwrap();
    conn.query_row(
        "SELECT last_success_at, last_error FROM provider_scans WHERE provider = ?1",
        [provider],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .ok()
}

fn library_entry(paths: &DataPaths, key: &str) -> Option<(String, String, bool)> {
    let conn = gamenode_platform::db::open(&paths.database_file()).unwrap();
    conn.query_row(
        "SELECT provider, name, installed FROM library_entries WHERE key = ?1",
        [key],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .ok()
}

fn official_providers(platform: Platform) -> Vec<Provider> {
    crate::providers::load_bundled_with_plugins(platform, &crate::plugin_host::backends(platform))
        .providers
}

/// The regression the bug escaped: the binary's own startup composition must include an already
/// installed third-party provider, and the library scan must list its records.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_composition_loads_an_installed_provider_into_the_library_and_scans_it() {
    let (_dir, paths) = paths();
    let fixture = Fixture::new();
    let package = fixture.provider("Third Party Fixture", Some(FIXTURE_PROVIDER), true);
    install_signed(&fixture, &paths, FIXTURE_PLUGIN, &package);
    trust(&fixture, &paths, FIXTURE_PLUGIN);

    let composed = load_library_providers(Platform::Linux, &paths).await;
    assert!(
        composed.diagnostics.is_empty(),
        "{:?}",
        composed.diagnostics
    );
    let provider = composed
        .get(FIXTURE_PROVIDER)
        .expect("the installed provider joins the startup library list");
    assert_eq!(provider.name(), "Third Party Fixture");
    assert_eq!(provider.source(), "community");
    assert_eq!(provider.discovery_plugin(), Some(FIXTURE_PLUGIN));

    // The official providers are the same entries, in the same order, with their backends.
    let official = official_providers(Platform::Linux);
    assert_eq!(
        composed
            .providers
            .iter()
            .take(official.len())
            .map(|provider| provider.id())
            .collect::<Vec<_>>(),
        official
            .iter()
            .map(|provider| provider.id())
            .collect::<Vec<_>>()
    );
    assert_eq!(composed.providers.len(), official.len() + 1);

    let store = library_store(&paths);
    let (mut machine, effects) = start_machine(composed.providers);
    let runtime = FixtureRuntime::new(FIXTURE_PROVIDER);
    runtime.set_records(vec![DiscoveryRecord {
        id: "FIXTURE-1".to_owned(),
        name: "Fixture Game".to_owned(),
        install_dir: "/fixtures/Fixture Game".to_owned(),
        ..Default::default()
    }]);
    drive(&mut machine, &runtime, store.as_ref(), effects).await;

    assert_eq!(runtime.discoveries(), [FIXTURE_PROVIDER]);
    let entry = machine
        .entry(&format!("{FIXTURE_PROVIDER}:FIXTURE-1"))
        .expect("the scan added the provider's record");
    assert_eq!(entry.name.as_deref(), Some("Fixture Game"));
    assert!(entry.installed);
    assert_eq!(
        machine
            .scan_state(FIXTURE_PROVIDER)
            .and_then(|state| state.last_success_at),
        Some(100)
    );
    let (success, error) = row(&paths, FIXTURE_PROVIDER).expect("provider_scans has a row");
    assert_eq!(success, Some(100));
    assert_eq!(error, None);
    assert_eq!(
        library_entry(&paths, &format!("{FIXTURE_PROVIDER}:FIXTURE-1")),
        Some((FIXTURE_PROVIDER.to_owned(), "Fixture Game".to_owned(), true))
    );
    let view = machine.view().expect("the library view is available");
    assert!(
        view.providers
            .iter()
            .any(|provider| provider.id == FIXTURE_PROVIDER && provider.count == 1),
        "{:?}",
        view.providers
    );
}

/// The reviewed import path: install a signed package at runtime, add it to the existing library,
/// request a scan, and keep the previous machine and its records.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn importing_a_provider_at_runtime_adds_and_scans_it_without_a_new_machine() {
    let (_dir, paths) = paths();
    let fixture = Fixture::new();
    let package = fixture.provider("Imported Fixture", Some(FIXTURE_PROVIDER), true);
    trust(&fixture, &paths, FIXTURE_PLUGIN);

    // Start with the official providers only, exactly like a host whose registry is empty.
    let composed = load_library_providers(Platform::Linux, &paths).await;
    assert!(composed.get(FIXTURE_PROVIDER).is_none());
    let store = library_store(&paths);
    let (mut machine, effects) = start_machine(composed.providers);
    let runtime = FixtureRuntime::new(FIXTURE_PROVIDER);
    drive(&mut machine, &runtime, store.as_ref(), effects).await;
    assert!(
        machine
            .providers()
            .iter()
            .all(|p| p.id() != FIXTURE_PROVIDER)
    );

    // The reviewed import installs and pins the package. Registration/activation needs a composed
    // host, which this library-level test deliberately does not build.
    let service = import_service(&paths);
    let outcome = reviewed_import(&service, &fixture, &package).await;
    assert_eq!(outcome.plugin_id, FIXTURE_PLUGIN);

    // The app's post-import callback re-reads the installed package and adds it to the same
    // machine; no new LibraryMachine and no restart.
    let provider = service
        .library_provider(FIXTURE_PLUGIN)
        .await
        .unwrap()
        .expect("the imported provider composes");
    assert_eq!(provider.id(), FIXTURE_PROVIDER);
    machine.put_provider(provider);
    assert_eq!(
        machine
            .providers()
            .iter()
            .filter(|provider| provider.id() == FIXTURE_PROVIDER)
            .count(),
        1
    );

    runtime.set_records(vec![DiscoveryRecord {
        id: "IMPORTED-1".to_owned(),
        name: "Imported Game".to_owned(),
        install_dir: "/fixtures/Imported Game".to_owned(),
        ..Default::default()
    }]);
    let effects = machine.handle(Input::ScanRequested);
    drive(&mut machine, &runtime, store.as_ref(), effects).await;

    assert_eq!(runtime.discoveries(), [FIXTURE_PROVIDER]);
    assert!(
        machine
            .entry(&format!("{FIXTURE_PROVIDER}:IMPORTED-1"))
            .is_some_and(|entry| entry.installed)
    );
    let (success, error) = row(&paths, FIXTURE_PROVIDER).expect("provider_scans has a row");
    assert_eq!(success, Some(100));
    assert_eq!(error, None);
}

/// A failed scan after the provider joined the library keeps the previous records and stores the
/// provider error.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_scan_keeps_the_previous_records_and_reports_the_provider_error() {
    let (_dir, paths) = paths();
    let fixture = Fixture::new();
    let package = fixture.provider("Failing Fixture", Some(FIXTURE_PROVIDER), true);
    install_signed(&fixture, &paths, FIXTURE_PLUGIN, &package);
    trust(&fixture, &paths, FIXTURE_PLUGIN);

    let composed = load_library_providers(Platform::Linux, &paths).await;
    let store = library_store(&paths);
    let (mut machine, effects) = start_machine(composed.providers);
    let runtime = FixtureRuntime::new(FIXTURE_PROVIDER);
    runtime.set_records(vec![DiscoveryRecord {
        id: "KEPT-1".to_owned(),
        name: "Kept Game".to_owned(),
        install_dir: "/fixtures/Kept Game".to_owned(),
        ..Default::default()
    }]);
    drive(&mut machine, &runtime, store.as_ref(), effects).await;
    assert!(
        machine
            .entry(&format!("{FIXTURE_PROVIDER}:KEPT-1"))
            .is_some_and(|entry| entry.installed)
    );

    runtime.fail("the fixture collection is unreadable");
    let effects = machine.handle(Input::ScanRequested);
    drive(&mut machine, &runtime, store.as_ref(), effects).await;

    let entry = machine
        .entry(&format!("{FIXTURE_PROVIDER}:KEPT-1"))
        .expect("a failed scan never deletes a record");
    assert!(entry.installed, "the previous installed state stays");
    assert_eq!(
        machine
            .scan_state(FIXTURE_PROVIDER)
            .and_then(|state| state.last_error.as_deref()),
        Some("the fixture collection is unreadable")
    );
    let (success, error) = row(&paths, FIXTURE_PROVIDER).expect("provider_scans has a row");
    assert_eq!(success, Some(100), "the earlier success is retained");
    assert_eq!(
        error.as_deref(),
        Some("the fixture collection is unreadable")
    );
}

/// Untrusted, incompatible, extension-only, discovery-less, reserved and malformed packages are
/// skipped with a diagnostic; the official providers remain composed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ineligible_third_party_packages_are_skipped_and_official_providers_remain() {
    let (_dir, paths) = paths();
    let fixture = Fixture::new();

    // Untrusted: signed and installed, but no key was pinned.
    let untrusted = fixture.provider("Untrusted Candidate", Some("untrusted-candidate"), true);
    install_signed(&fixture, &paths, FIXTURE_PLUGIN, &untrusted);

    // Incompatible: the only target is a Windows executable on Linux.
    const INCOMPATIBLE: &str = "dev.example.incompatible-provider";
    let incompatible = fixture.provider_as(
        INCOMPATIBLE,
        "Incompatible Candidate",
        "incompatible-candidate",
        true,
        "windows",
    );
    install_signed(&fixture, &paths, INCOMPATIBLE, &incompatible);
    trust(&fixture, &paths, INCOMPATIBLE);

    // The remaining packages are unsigned registry entries, so each gets its own plugin id.
    const EXTENSION: &str = "dev.example.extension-only";
    let extension = fixture.extension(EXTENSION, "Extension Candidate");
    install_unverified(&paths, EXTENSION, &extension.staged);
    const NO_DISCOVERY: &str = "dev.example.no-discovery";
    let no_discovery = fixture.provider_as(
        NO_DISCOVERY,
        "No Discovery Candidate",
        "no-discovery-candidate",
        false,
        "linux",
    );
    install_unverified(&paths, NO_DISCOVERY, &no_discovery.staged);
    const RESERVED: &str = "dev.example.reserved";
    let reserved = fixture.provider_as(RESERVED, "Reserved Candidate", "steam", true, "linux");
    install_unverified(&paths, RESERVED, &reserved.staged);

    // A malformed manifest as found by a corrupt registry. It is not even valid JSON.
    let malformed = fixture.dir.path().join("malformed-source");
    std::fs::create_dir_all(&malformed).unwrap();
    std::fs::write(malformed.join("plugin.json"), b"{ not a manifest").unwrap();
    install_unverified(&paths, "dev.example.malformed", &malformed);

    let composed = load_library_providers(Platform::Linux, &paths).await;
    for unexpected in [
        "untrusted-candidate",
        "incompatible-candidate",
        "no-discovery-candidate",
    ] {
        assert!(
            composed.get(unexpected).is_none(),
            "`{unexpected}` was not added"
        );
    }
    assert!(!composed.diagnostics.is_empty());
    let diagnostics = composed.diagnostics.join("\n");
    for expected in [
        "untrusted publisher",
        "incompatible",
        "does not declare `library.discovery`",
        "is not a provider",
    ] {
        assert!(
            diagnostics.contains(expected),
            "missing `{expected}` in:\n{diagnostics}"
        );
    }
    // The reserved provider id is rejected by the signed-manifest validation itself, so the
    // diagnostic names that rule rather than a duplicate registration.
    assert!(diagnostics.contains("reserved"), "{diagnostics}");

    // Official providers and their order are untouched by every rejected package, and none of
    // the rejected packages appended anything (including the reserved `steam` provider id).
    let official = official_providers(Platform::Linux);
    assert_eq!(
        composed
            .providers
            .iter()
            .map(|provider| provider.id().to_owned())
            .collect::<Vec<_>>(),
        official
            .iter()
            .map(|provider| provider.id().to_owned())
            .collect::<Vec<_>>()
    );
    assert!(composed.get("steam").is_some());
    assert_eq!(composed.providers.len(), official.len());
}

/// Opens the update service with an active profile and the lifecycle the import path requires.
/// The lifecycle has no host composition, so runtime registration is reported as unavailable;
/// installation and reviewed grants still complete.
fn import_service(paths: &DataPaths) -> PluginUpdateService {
    let state = crate::plugin_host::profile::PluginStateLayout::managed(paths);
    let api = crate::plugin_host::PluginApi::default();
    api.set_state_layout(state.clone());
    api.providers
        .profiles()
        .update(Some("fixture-account"), true);
    let registry = PluginPackageRegistry::open(paths).unwrap();
    let lifecycle =
        std::sync::Arc::new(crate::plugin_host::runtime::ThirdPartyPluginLifecycle::new(
            api.providers.clone(),
            crate::plugin_host::process::ProviderProcesses::new_for_lifecycle(
                api.providers.clone(),
                paths.data_dir().join("plugins.sock"),
                vec![],
                state.clone(),
            )
            .unwrap(),
            state.clone(),
            registry.clone(),
            crate::plugin_host::managed_packages::ManagedPackageLifecycle::new(registry),
        ));
    let service = PluginUpdateService::open(paths).unwrap();
    service.set_lifecycle(lifecycle);
    service.set_profiles(api.providers.profiles());
    service
}

/// Drives the real reviewed standalone import; only the trust confirmation and installation are
/// used, and the runtime composition is intentionally absent.
async fn reviewed_import(
    service: &PluginUpdateService,
    fixture: &Fixture,
    package: &Package,
) -> PluginImportInstallOutcome {
    let review = service
        .import_review(package.archive.clone())
        .await
        .unwrap();
    assert_eq!(review.package.plugin_id, FIXTURE_PLUGIN);
    service
        .import_confirm_publisher(&review.token, &fixture.fingerprint())
        .await
        .unwrap();
    service
        .import_install(&review.token, false, &[])
        .await
        .unwrap()
}

/// The real signed Dolphin bundle end to end: the reviewed import registers the provider in the
/// already composed host, and the desktop's own library composition then lists and scans it.
/// Gated by `DOLPHIN_BUNDLE_PATH` because it needs the sibling repository's signed artifact.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_real_dolphin_bundle_joins_the_running_library_and_discovers_gzle01() {
    use std::os::unix::fs::PermissionsExt;

    const DOLPHIN_PLUGIN: &str = "dev.richmont.gamenode.dolphin-emu";
    const DOLPHIN_PROVIDER: &str = "dolphin-emu";
    const DOLPHIN_PUBLISHER_PUBLIC_KEY: &str = "9MJsveQ7SI656O7ykweL/Y+WN1pye9EJ9W+yaQZmidw=";

    let Some(artifact) = crate::plugin_host::bundles::dolphin_bundle_artifact() else {
        return;
    };
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let paths = DataPaths::at(directory.path().join("app-data"));
    paths.ensure_data_dir().unwrap();
    crate::plugin_host::profile::PluginStateLayout::prepare_managed(&paths).unwrap();

    let api = crate::plugin_host::PluginApi::default();
    api.set_state_layout(crate::plugin_host::profile::PluginStateLayout::managed(
        &paths,
    ));
    api.providers
        .profiles()
        .update(Some("dolphin-library-account"), true);
    let settings = SqliteSettingsStore::open(&paths.database_file()).unwrap();
    let registry = PluginPackageRegistry::open(&paths).unwrap();
    let endpoint = paths.data_dir().join("plugins.sock");
    let store = library_store(&paths);
    let service = PluginUpdateService::open(&paths).unwrap();
    let host = crate::plugin_host::runtime::HostedProviders::bundled(
        api.providers.clone(),
        std::sync::Arc::new(Default::default()),
        Some(endpoint.clone()),
        paths.clone(),
        store.clone(),
        None,
    )
    .await
    .unwrap();
    let _server = crate::plugin_host::PluginApiServer::start(
        &tokio::runtime::Handle::current(),
        &endpoint,
        api.clone(),
    )
    .unwrap();
    service.set_lifecycle(host.lifecycle());
    service.set_profiles(api.providers.profiles());
    service.set_bundle_store(settings.bundle_store());

    // Reviewed import into the already composed host: no restart and no second HostedProviders.
    let review = service
        .bundle_import_review(artifact.clone())
        .await
        .unwrap();
    service
        .bundle_import_confirm_publisher(&review.token, &review.bundle.publisher_fingerprint)
        .await
        .unwrap();
    let installed = service
        .bundle_import_install(&review.token, false, &[])
        .await
        .unwrap();
    assert_eq!(
        installed.bundle_id,
        "dev.richmont.gamenode.dolphin-emu.bundle"
    );
    assert!(
        installed
            .components
            .iter()
            .any(|component| component.kind == "provider" && component.active)
    );
    assert_eq!(
        registry
            .active(DOLPHIN_PLUGIN)
            .unwrap()
            .map(|installed| installed.version),
        Some("0.1.0".to_owned())
    );
    // The same host the desktop composed now has the provider; nothing is rebuilt for the import.
    assert!(host.has_provider(DOLPHIN_PROVIDER));

    // Grant the reviewed permissions and enable the provider in the active profile.
    let snapshot = host
        .lifecycle()
        .permission_snapshot(DOLPHIN_PLUGIN.into())
        .await
        .unwrap();
    let decisions = snapshot
        .requested_permissions
        .iter()
        .flat_map(|request| {
            request.allowed_scopes.iter().map(|scope| {
                crate::plugin_host::grants::PermissionDecision {
                    permission_id: request.permission_id.clone(),
                    scope: scope.clone(),
                    granted: true,
                }
            })
        })
        .collect::<Vec<_>>();
    let activation = tokio::sync::Mutex::new(());
    host.lifecycle()
        .replace_permissions(
            &activation,
            DOLPHIN_PLUGIN.into(),
            snapshot.profile_id.clone(),
            snapshot.publisher_fingerprint.clone(),
            snapshot.grant_revision,
            decisions,
        )
        .await
        .unwrap();
    host.lifecycle()
        .enable(DOLPHIN_PLUGIN.into())
        .await
        .unwrap();

    // The provider scanner reads a plain GameCube disc header from the temporary collection.
    let rom_root = directory.path().join("collection");
    std::fs::create_dir_all(&rom_root).unwrap();
    std::fs::write(
        rom_root.join("The Legend of Zelda.iso"),
        b"GZLE01\x00\x00\x00\x00",
    )
    .unwrap();
    let mut config = host.processes().config(DOLPHIN_PROVIDER).unwrap();
    config.environment.push((
        std::ffi::OsString::from("DOLPHIN_PLUGIN_ROOTS"),
        rom_root.into_os_string(),
    ));
    host.processes()
        .replace_config(DOLPHIN_PROVIDER, config)
        .await
        .unwrap();

    // The desktop's startup/post-import composition now lists the provider.
    let composed = load_library_providers(Platform::Linux, &paths).await;
    let provider = composed
        .get(DOLPHIN_PROVIDER)
        .expect("the real bundle joins the library provider list");
    assert_eq!(provider.discovery_plugin(), Some(DOLPHIN_PLUGIN));
    assert_eq!(provider.source(), "community");

    // A fresh machine built from that composition represents a startup with the bundle already
    // installed; the scan runs through the same still-live host.
    let (mut machine, effects) = start_machine(composed.providers);
    drive(&mut machine, host.as_ref(), store.as_ref(), effects).await;

    let entry = machine
        .entry(&format!("{DOLPHIN_PROVIDER}:GZLE01"))
        .expect("the real provider discovered GZLE01");
    assert_eq!(entry.name.as_deref(), Some("The Legend of Zelda"));
    assert!(entry.installed);
    assert_eq!(
        entry.fields.get("platform").map(String::as_str),
        Some("gamecube")
    );
    let (success, error) = row(&paths, DOLPHIN_PROVIDER).expect("provider_scans has a row");
    assert!(success.is_some());
    assert_eq!(error, None);
    assert_eq!(
        library_entry(&paths, &format!("{DOLPHIN_PROVIDER}:GZLE01")),
        Some((
            DOLPHIN_PROVIDER.to_owned(),
            "The Legend of Zelda".to_owned(),
            true,
        ))
    );
    let view = machine.view().expect("the library view is available");
    assert!(
        view.providers
            .iter()
            .any(|provider| provider.id == DOLPHIN_PROVIDER && provider.count == 1),
        "{:?}",
        view.providers
    );

    // The publisher key the bundle review showed is the pinned development key; no fixture or
    // substitute provider supplies the record.
    let publisher_key: [u8; 32] = base64::engine::general_purpose::STANDARD
        .decode(DOLPHIN_PUBLISHER_PUBLIC_KEY)
        .unwrap()
        .try_into()
        .unwrap();
    let pinned = publisher_trust::key_fingerprint(&publisher_key);
    assert_eq!(review.bundle.publisher_fingerprint, pinned);
}
