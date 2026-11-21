//! Supervised runtime tests. The fixture child runs this same test binary with the explicit
//! plugin-process marker, so the host exercises the real process boundary.

use super::*;
use crate::plugin_host::{
    PluginApiServer,
    provider::{Registration, RegistrationKind, RegistrationOrigin},
};
use gamenode_plugin_protocol::capability as cap;
use std::sync::Arc;
use std::time::Duration;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

const MANIFEST: &str = r#"
id = "fixture"
name = "Fixture"
platforms = ["linux"]
source = "fixture"

[scan]
"#;

#[cfg(target_os = "linux")]
fn signed_provider_archive(root: &Path) -> PathBuf {
    use base64::Engine as _;
    use ring::signature::KeyPair as _;
    use sha2::Digest as _;
    use std::os::unix::fs::PermissionsExt;
    use zip::write::SimpleFileOptions;

    std::fs::create_dir_all(root).unwrap();
    let key = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[77; 32]).unwrap();
    let manifest = serde_json::to_vec(&serde_json::json!({
        "schema": 1,
        "id": "fixture.plugin",
        "kind": "provider",
        "provider_id": "fixture",
        "name": "Signed acceptance fixture",
        "version": "1.0.0",
        "api": {"min": "1.6.0", "max": "1.6.0"},
        "permissions": [{
            "id": cap::CATALOG,
            "required": false,
            "scope": "own provider catalog",
            "reason": "Submit the fixture catalog"
        }],
        "targets": [{
            "os": "linux",
            "arch": "x86_64",
            "abi": "gnu",
            "entrypoint": "plugin"
        }],
        "publisher": {
            "key_id": "acceptance-fixture-key",
            "public_key": base64::engine::general_purpose::STANDARD.encode(key.public_key().as_ref())
        },
        "license": {"expression": "MIT", "files": ["LICENSE"]}
    }))
    .unwrap();
    let executable = format!(
        "#!/bin/sh\nexport GAMENODE_TEST_PLUGIN_PROCESS=fixture\nexport GAMENODE_TEST_PLUGIN_MODE=provider-catalog\nexec '{}' --exact plugin_host::process::tests::fixture_process --nocapture\n",
        std::env::current_exe().unwrap().display()
    );
    let license = b"fixture license";
    let payloads = [
        ("plugin.json", manifest.as_slice()),
        ("plugin", executable.as_bytes()),
        ("LICENSE", license.as_slice()),
    ];
    let mut files = std::collections::BTreeMap::new();
    for (name, bytes) in payloads {
        std::fs::write(root.join(name), bytes).unwrap();
        files.insert(
            name.to_owned(),
            format!("{:x}", sha2::Sha256::digest(bytes)),
        );
    }
    std::fs::set_permissions(root.join("plugin"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let integrity = serde_json::to_vec(&serde_json::json!({"schema": 1, "files": files})).unwrap();
    let mut signed = gamenode_platform::plugin_packages::SIGNATURE_DOMAIN_PREFIX.to_vec();
    signed.extend_from_slice(&integrity);
    let signature = key.sign(&signed);

    let archive = root.with_extension("gnplugin");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    for (name, bytes) in [
        ("plugin.json", manifest.as_slice()),
        ("plugin", executable.as_bytes()),
        ("LICENSE", license.as_slice()),
        ("integrity.json", integrity.as_slice()),
        ("signature.ed25519", signature.as_ref()),
    ] {
        let options = if name == "plugin" {
            SimpleFileOptions::default().unix_permissions(0o755)
        } else {
            SimpleFileOptions::default()
        };
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    archive
}

async fn hosted_fixture() -> (HostedProviders, PluginApiServer, tempfile::TempDir) {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = directory.path().join("plugins.sock");
    let root = crate::plugin_host::PluginApi::default();
    let services = Arc::new(
        ProviderServices::new(
            ProviderContext {
                provider_id: "fixture".into(),
                platform: "linux".into(),
                manifest_toml: MANIFEST.into(),
                granted_capabilities: vec![cap::CONTEXT.into()],
                ..Default::default()
            },
            None,
            Arc::new(
                gamenode_platform::db::SqliteSettingsStore::new(
                    gamenode_platform::db::open(Path::new(":memory:")).unwrap(),
                )
                .library_store(),
            ),
            None,
            None,
        )
        .unwrap(),
    );
    root.providers
        .register(Registration {
            provider_id: "fixture".into(),
            plugin_id: "fixture.plugin".into(),
            declared_developer: "Fixture".into(),
            package_digest: vec![],
            origin: RegistrationOrigin::Official,
            kind: RegistrationKind::Provider,
            playtime: Default::default(),
            capabilities: vec![cap::CONTEXT.into()],
            shared_catalog: false,
            services: services.clone(),
        })
        .unwrap();
    let server =
        PluginApiServer::start(&tokio::runtime::Handle::current(), &endpoint, root.clone())
            .unwrap();
    let api = root.providers.clone();
    let processes = ProviderProcesses::new(
        api.clone(),
        endpoint,
        vec![ProcessConfig {
            provider_id: "fixture".into(),
            executable: std::env::current_exe().unwrap(),
            arguments: vec![
                "--exact".into(),
                "plugin_host::process::tests::fixture_process".into(),
                "--nocapture".into(),
            ],
            environment: vec![
                ("GAMENODE_TEST_PLUGIN_PROCESS".into(), "fixture".into()),
                ("GAMENODE_TEST_PLUGIN_MODE".into(), "normal".into()),
                (
                    "GAMENODE_TEST_PLUGIN_CRASH_MARKER".into(),
                    directory.path().join("crash-once").into_os_string(),
                ),
            ],
            startup_timeout: Duration::from_secs(10),
            manifest_capabilities: vec![cap::CONTEXT.into()],
            required_capabilities: vec![],
            blocked_capabilities: vec![],
            integrity: None,
        }],
    )
    .unwrap();
    let mut entries = std::collections::BTreeMap::new();
    entries.insert(
        "fixture".into(),
        Entry {
            services,
            initialized: tokio::sync::Mutex::new(None),
            packaged: true,
            initialize: true,
        },
    );
    let registry = gamenode_platform::plugin_registry::PluginPackageRegistry::at(
        directory.path().join("registry"),
    )
    .unwrap();
    let lifecycle = Arc::new(ThirdPartyPluginLifecycle::new(
        api.clone(),
        processes.clone(),
        PluginStateLayout::new(directory.path().join("state")),
        registry.clone(),
        ManagedPackageLifecycle::new(registry),
    ));
    let host = HostedProviders {
        lifecycle,
        intents: Arc::new(Default::default()),
        managed_plugins: vec![],
        api,
        processes,
        endpoint_available: true,
        entries,
        failures: Mutex::new(std::collections::BTreeMap::new()),
        observer: Mutex::new(None),
    };
    (host, server, directory)
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn imported_signed_provider_runs_from_registry_and_profile_fences_after_permission_revocation()
 {
    use crate::plugin_host::{
        PluginApi, grants::PermissionDecision, process::ProviderProcesses,
        profile::PluginStateLayout, runtime::ThirdPartyPluginLifecycle,
        updates::PluginUpdateService,
    };
    use gamenode_platform::{
        db::{self, SqliteSettingsStore},
        paths::DataPaths,
        plugin_registry::PluginPackageRegistry,
    };
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        directory.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let endpoint = directory.path().join("plugins.sock");
    let paths = DataPaths::at(directory.path().join("app-data"));
    let api = PluginApi::default();
    let state = PluginStateLayout::managed(&paths);
    api.set_state_layout(state.clone());
    api.providers.profiles().update(Some("account-one"), true);

    // Import uses the same signed archive and publisher-pin flow as the UI. A small preliminary
    // lifecycle gives that flow an authenticated active profile; the runtime below is rebuilt
    // from the imported active package, as it is on application startup.
    let registry = PluginPackageRegistry::open(&paths).unwrap();
    let preliminary_processes = ProviderProcesses::new_for_lifecycle(
        api.providers.clone(),
        endpoint.clone(),
        vec![],
        state.clone(),
    )
    .unwrap();
    let preliminary = Arc::new(ThirdPartyPluginLifecycle::new(
        api.providers.clone(),
        preliminary_processes,
        state,
        registry.clone(),
        crate::plugin_host::managed_packages::ManagedPackageLifecycle::new(registry.clone()),
    ));
    let importer = PluginUpdateService::open(&paths).unwrap();
    importer.set_lifecycle(preliminary);
    let archive = signed_provider_archive(&directory.path().join("package"));
    let review = importer.import_review(archive).await.unwrap();
    assert_eq!(review.package.trust, "untrusted");
    assert_eq!(review.package.compatibility, "compatible");
    importer
        .import_confirm_publisher(&review.token, &review.package.publisher_fingerprint)
        .await
        .unwrap();
    assert_eq!(
        importer.import_install(&review.token, false).await.unwrap(),
        "fixture.plugin"
    );
    let imported = registry.active("fixture.plugin").unwrap().unwrap();
    let entrypoint = imported.package_path.join("plugin");
    let metadata = std::fs::symlink_metadata(&entrypoint).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
    assert_eq!(
        metadata.permissions().mode() & 0o777,
        0o700,
        "only the signed target entrypoint receives executable permissions"
    );

    let store = SqliteSettingsStore::new(db::open(Path::new(":memory:")).unwrap());
    let library: Arc<dyn gamenode_core::library::LibraryStore> = Arc::new(store.library_store());
    let host = HostedProviders::bundled(
        api.providers.clone(),
        Arc::new(Default::default()),
        Some(endpoint.clone()),
        paths,
        library,
        None,
    )
    .await
    .unwrap();
    let server =
        PluginApiServer::start(&tokio::runtime::Handle::current(), &endpoint, api.clone()).unwrap();
    let plugin = host
        .managed_plugin_inventory()
        .into_iter()
        .find(|plugin| plugin.plugin_id == "fixture.plugin")
        .expect("the active signed provider is loaded from the registry");
    assert_eq!(plugin.status, "installed");
    assert_eq!(plugin.version, "1.0.0");

    let lifecycle = host.lifecycle();
    let snapshot = lifecycle
        .permission_snapshot("fixture.plugin".into())
        .await
        .unwrap();
    let scope = snapshot.requested_permissions[0]
        .allowed_scopes
        .iter()
        .next()
        .unwrap()
        .clone();
    let decision = |granted| PermissionDecision {
        permission_id: cap::CATALOG.into(),
        scope: scope.clone(),
        granted,
    };
    let activation = tokio::sync::Mutex::new(());
    let granted = lifecycle
        .replace_permissions(
            &activation,
            "fixture.plugin".into(),
            snapshot.profile_id.clone(),
            snapshot.publisher_fingerprint.clone(),
            snapshot.grant_revision,
            vec![decision(true)],
        )
        .await
        .unwrap();
    assert_eq!(
        granted.effective_grants[0].state,
        crate::plugin_host::grants::PermissionState::Granted
    );

    lifecycle.enable("fixture.plugin".into()).await.unwrap();
    host.execute("fixture", ProviderOperation::Initialize)
        .await
        .unwrap();
    let catalog = host
        .execute("fixture", ProviderOperation::SyncCatalog)
        .await
        .unwrap();
    let Some(Ok(owned)) = catalog.catalog.owned else {
        panic!("the signed child must submit a complete catalog over Exchange");
    };
    assert_eq!(owned.len(), 1);
    assert_eq!(owned[0].id, "fixture-game");

    let revoked = lifecycle
        .replace_permissions(
            &activation,
            "fixture.plugin".into(),
            granted.profile_id,
            granted.publisher_fingerprint,
            granted.grant_revision,
            vec![decision(false)],
        )
        .await
        .unwrap();
    assert_eq!(
        revoked.effective_grants[0].state,
        crate::plugin_host::grants::PermissionState::Denied
    );
    let persisted = lifecycle
        .permission_snapshot("fixture.plugin".into())
        .await
        .unwrap();
    assert_eq!(
        persisted.effective_grants[0].state,
        crate::plugin_host::grants::PermissionState::Denied,
        "the host persists the revoked catalog grant for this identity"
    );

    api.providers.profiles().update(Some("account-two"), true);
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.processes.process_id("fixture").await.is_some()
            || api.providers.connected("fixture")
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("profile fencing stops the imported provider process and session");
    host.shutdown().await;
    server.shutdown().await;
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn official_linux_providers_remain_composed_across_managed_registry_states() {
    use crate::plugin_host::{
        PluginApi, grants::PermissionDecision, process::ProviderProcesses,
        profile::PluginStateLayout, runtime::ThirdPartyPluginLifecycle,
        updates::PluginUpdateService,
    };
    use gamenode_platform::{
        db::{self, SqliteSettingsStore},
        paths::DataPaths,
        plugin_registry::PluginPackageRegistry,
    };
    use std::os::unix::fs::PermissionsExt;

    for state_name in ["empty", "invalid", "enabled", "disabled"] {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), PermissionsExt::from_mode(0o700)).unwrap();
        let endpoint = directory.path().join("plugins.sock");
        let paths = DataPaths::at(directory.path().join("app-data"));
        let api = PluginApi::default();
        let state = PluginStateLayout::managed(&paths);
        api.set_state_layout(state.clone());
        api.providers.profiles().update(Some("account-one"), true);
        let registry = PluginPackageRegistry::open(&paths).unwrap();

        match state_name {
            "invalid" => {
                // A complete registry record pointing at an invalid manifest is ignored by
                // runtime composition; it must not hide or replace an official provider.
                let source = directory.path().join("invalid-package");
                std::fs::create_dir_all(&source).unwrap();
                std::fs::write(source.join("plugin.json"), b"{").unwrap();
                registry
                    .install_verified("invalid.plugin", "1.0.0", &source, true)
                    .unwrap();
            }
            "enabled" | "disabled" => {
                let preliminary_processes = ProviderProcesses::new_for_lifecycle(
                    api.providers.clone(),
                    endpoint.clone(),
                    vec![],
                    state.clone(),
                )
                .unwrap();
                let preliminary = Arc::new(ThirdPartyPluginLifecycle::new(
                    api.providers.clone(),
                    preliminary_processes,
                    state.clone(),
                    registry.clone(),
                    crate::plugin_host::managed_packages::ManagedPackageLifecycle::new(
                        registry.clone(),
                    ),
                ));
                let importer = PluginUpdateService::open(&paths).unwrap();
                importer.set_lifecycle(preliminary);
                let archive = signed_provider_archive(&directory.path().join("package"));
                let review = importer.import_review(archive).await.unwrap();
                assert_eq!(review.package.compatibility, "compatible", "{state_name}");
                importer
                    .import_confirm_publisher(&review.token, &review.package.publisher_fingerprint)
                    .await
                    .unwrap();
                assert_eq!(
                    importer.import_install(&review.token, false).await.unwrap(),
                    "fixture.plugin",
                    "{state_name}"
                );
            }
            "empty" => {}
            _ => unreachable!(),
        }

        // Normal application startup has initialized the package trust store before composing
        // provider registrations; create the same host-owned state for every fixture variant.
        let _updates = PluginUpdateService::open(&paths).unwrap();
        let store = SqliteSettingsStore::new(db::open(Path::new(":memory:")).unwrap());
        let library: Arc<dyn gamenode_core::library::LibraryStore> =
            Arc::new(store.library_store());
        let host = HostedProviders::bundled(
            api.providers.clone(),
            Arc::new(Default::default()),
            Some(endpoint.clone()),
            paths,
            library,
            None,
        )
        .await
        .unwrap();
        let server =
            PluginApiServer::start(&tokio::runtime::Handle::current(), &endpoint, api.clone())
                .unwrap();

        // Official services remain registered and their static plugin-backed descriptors remain
        // identical, whether the managed registry is empty, malformed, enabled or disabled.
        for provider in ["steam", "packagekit"] {
            if provider == "packagekit" {
                assert_eq!(
                    gamenode_core::provider::Platform::current(),
                    Some(gamenode_core::provider::Platform::Linux)
                );
            }
            assert!(
                host.services(provider).is_some(),
                "{state_name}: {provider}"
            );
            assert_eq!(
                host.availability(provider),
                "missing",
                "{state_name}: {provider}"
            );
            let unavailable = host
                .execute(provider, ProviderOperation::Initialize)
                .await
                .expect_err("a missing official executable must not use an in-process substitute");
            assert_eq!(
                unavailable.code(),
                tonic::Code::Unavailable,
                "{state_name}: {provider}"
            );
            assert_eq!(
                host.availability(provider),
                "missing",
                "{state_name}: {provider}"
            );
            assert!(!host.connected(provider), "{state_name}: {provider}");
            assert_eq!(
                host.processes.process_id(provider).await,
                None,
                "{state_name}: {provider}"
            );
        }

        match state_name {
            "enabled" => {
                let lifecycle = host.lifecycle();
                let snapshot = lifecycle
                    .permission_snapshot("fixture.plugin".into())
                    .await
                    .unwrap();
                let scope = snapshot.requested_permissions[0]
                    .allowed_scopes
                    .iter()
                    .next()
                    .unwrap()
                    .clone();
                let activation = tokio::sync::Mutex::new(());
                lifecycle
                    .replace_permissions(
                        &activation,
                        "fixture.plugin".into(),
                        snapshot.profile_id,
                        snapshot.publisher_fingerprint,
                        snapshot.grant_revision,
                        vec![PermissionDecision {
                            permission_id: cap::CATALOG.into(),
                            scope,
                            granted: true,
                        }],
                    )
                    .await
                    .unwrap();
                lifecycle.enable("fixture.plugin".into()).await.unwrap();
                assert!(host.connected("fixture"));
                assert_eq!(host.availability("steam"), "missing");
                assert_eq!(host.availability("packagekit"), "missing");
            }
            "disabled" => {
                assert!(
                    !host
                        .lifecycle()
                        .enabled("fixture.plugin".into())
                        .await
                        .unwrap()
                );
                assert!(!host.connected("fixture"));
            }
            _ => assert!(host.managed_plugin_inventory().is_empty(), "{state_name}"),
        }

        host.shutdown().await;
        server.shutdown().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_provider_stays_installed_and_start_clears_obsolete_failures() {
    let (host, _server, _directory) = hosted_fixture().await;

    // Start normally: the supervised child establishes its session.
    host.execute("fixture", ProviderOperation::Initialize)
        .await
        .unwrap();
    assert_eq!(host.availability("fixture"), "installed");
    assert!(host.available("fixture"));
    assert!(host.connected("fixture"));

    // Stop is deliberate. The inventory row remains installed with a stopped session so Settings
    // can still offer Start.
    host.stop("fixture").await.unwrap();
    assert_eq!(host.availability("fixture"), "installed");
    assert!(!host.available("fixture"));
    assert!(!host.connected("fixture"));
    assert_eq!(
        (host.availability("fixture"), host.connected("fixture")),
        ("installed", false),
        "PluginsPanel sees a restartable installed plugin, not a failed one"
    );
    assert_eq!(host.processes.process_id("fixture").await, None);

    // A scan or other provider call is refused with the exact stable error. It must not record a
    // failure that would hide Start.
    for operation in [ProviderOperation::Discover, ProviderOperation::Initialize] {
        let error = host.execute("fixture", operation).await.unwrap_err();
        assert_eq!(error.code(), tonic::Code::Unavailable);
        assert_eq!(error.message(), "plugin stopped");
        assert_eq!(host.availability("fixture"), "installed");
        assert!(host.failures.lock().unwrap().is_empty());
    }

    // Even an obsolete crash marker does not survive a successful fresh session.
    host.failures
        .lock()
        .unwrap()
        .insert("fixture".into(), "provider_unavailable".into());
    assert_eq!(host.availability("fixture"), "installed");

    host.start("fixture").await.unwrap();
    assert!(host.failures.lock().unwrap().is_empty());
    assert_eq!(host.availability("fixture"), "installed");
    assert!(host.available("fixture"));
    assert!(host.connected("fixture"));
    assert!(host.processes.process_id("fixture").await.is_some());
    host.execute("fixture", ProviderOperation::Initialize)
        .await
        .unwrap();

    host.shutdown().await;
}

/// A minimal installed and registered third-party plugin beside a host-owned durable pool.
struct UninstallFixture {
    dir: tempfile::TempDir,
    root: crate::plugin_host::PluginApi,
    state: PluginStateLayout,
    registry: gamenode_platform::plugin_registry::PluginPackageRegistry,
    plugin_id: &'static str,
    fingerprint: String,
    owner: OwnerScope,
}

impl UninstallFixture {
    fn new(plugin_id: &'static str, provider_id: &'static str) -> Self {
        use crate::plugin_host::{PluginApi, provider::ExtensionServices};
        use gamenode_platform::plugin_shared_data::OwnerScope;

        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("app-data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let root = PluginApi {
            shared_pool: Arc::new(crate::plugin_host::shared_pool::DurableSharedPool::new(
                &data_dir,
            )),
            storage: Arc::new(crate::plugin_host::storage::PluginStorage::new(
                dir.path().join("plugin-data"),
            )),
            ..Default::default()
        };
        let state = PluginStateLayout::new(dir.path().join("state"));
        root.set_state_layout(state.clone());
        root.providers.profiles().update(Some("account-one"), true);
        let fingerprint = "ab".repeat(32);
        root.providers
            .register(Registration {
                provider_id: provider_id.into(),
                plugin_id: plugin_id.into(),
                declared_developer: "Example Developer".into(),
                package_digest: vec![7; 32],
                kind: RegistrationKind::Provider,
                origin: RegistrationOrigin::ThirdParty {
                    publisher_fingerprint: fingerprint.clone(),
                },
                capabilities: vec![cap::CONTEXT.into()],
                shared_catalog: false,
                playtime: Default::default(),
                services: Arc::new(ExtensionServices),
            })
            .unwrap();
        let epoch = root.providers.profiles().require().unwrap().epoch();
        let resolver = root
            .providers
            .profiles()
            .resolver(plugin_id, &fingerprint, None)
            .unwrap();
        let owner = OwnerScope::new(*resolver.storage_owner().key(), epoch).unwrap();
        let registry = gamenode_platform::plugin_registry::PluginPackageRegistry::at(
            dir.path().join("registry"),
        )
        .unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("plugin.json"), b"{}").unwrap();
        registry
            .install_verified(plugin_id, "1.0.0", &source, true)
            .unwrap();
        Self {
            dir,
            root,
            state,
            registry,
            plugin_id,
            fingerprint,
            owner,
        }
    }

    fn lifecycle(&self) -> ThirdPartyPluginLifecycle {
        let processes = ProviderProcesses::new_for_lifecycle(
            self.root.providers.clone(),
            self.dir.path().join("plugins.sock"),
            vec![],
            self.state.clone(),
        )
        .unwrap();
        let lifecycle = ThirdPartyPluginLifecycle::new(
            self.root.providers.clone(),
            processes,
            self.state.clone(),
            self.registry.clone(),
            ManagedPackageLifecycle::new(self.registry.clone()),
        );
        lifecycle.set_private_storage(self.root.storage.clone());
        lifecycle
    }

    fn reinstall(&self, version: &str) {
        use crate::plugin_host::provider::ExtensionServices;

        self.root
            .providers
            .register(Registration {
                provider_id: "producer".into(),
                plugin_id: self.plugin_id.into(),
                declared_developer: "Example Developer".into(),
                package_digest: vec![7; 32],
                kind: RegistrationKind::Provider,
                origin: RegistrationOrigin::ThirdParty {
                    publisher_fingerprint: self.fingerprint.clone(),
                },
                capabilities: vec![cap::CONTEXT.into()],
                shared_catalog: false,
                playtime: Default::default(),
                services: Arc::new(ExtensionServices),
            })
            .unwrap();
        let source = self.dir.path().join(format!("source-{version}"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("plugin.json"), b"{}").unwrap();
        self.registry
            .install_verified(self.plugin_id, version, &source, true)
            .unwrap();
    }

    fn profile_id(&self) -> String {
        self.root
            .providers
            .profiles()
            .require()
            .unwrap()
            .profile_id()
            .to_owned()
    }

    fn private_root(&self) -> PathBuf {
        self.dir.path().join("plugin-data")
    }

    fn private_dir(&self, plugin_id: &str, profile_id: &str) -> PathBuf {
        gamenode_platform::plugin_storage::namespace_dir(
            &self.private_root(),
            &self.fingerprint,
            plugin_id,
            profile_id,
        )
    }

    fn write_private_at(&self, plugin_id: &str, profile_id: &str, key: &str, value: &[u8]) {
        let mut store = gamenode_platform::plugin_storage::DurableStore::open(
            &self.private_dir(plugin_id, profile_id),
        )
        .unwrap();
        store
            .put(
                key,
                value,
                1,
                None,
                100,
                gamenode_platform::plugin_storage::QUOTA_BYTES,
            )
            .unwrap();
    }

    fn write_private(&self, plugin_id: &str, key: &str, value: &[u8]) {
        self.write_private_at(plugin_id, &self.profile_id(), key, value);
    }

    fn read_private_at(&self, plugin_id: &str, profile_id: &str, key: &str) -> Option<Vec<u8>> {
        let store = gamenode_platform::plugin_storage::DurableStore::open_existing(
            &self.private_dir(plugin_id, profile_id),
        )
        .unwrap()?;
        store.get(key, 100).unwrap().map(|stored| stored.value)
    }

    fn read_private(&self, plugin_id: &str, key: &str) -> Option<Vec<u8>> {
        self.read_private_at(plugin_id, &self.profile_id(), key)
    }

    async fn publish(&self, resource: &str, title: &str) -> String {
        use crate::plugin_host::shared_pool::DurableWrite;
        use gamenode_platform::plugin_shared_data::RecordProvenance;

        let record = self
            .root
            .shared_pool
            .persist(DurableWrite {
                owner: self.owner,
                dataset_id: "org.example.catalog".to_owned(),
                schema_id: "org.example.catalog.v1".to_owned(),
                schema_version: 1,
                fields_json: format!(r#"{{"title":"{title}"}}"#),
                opaque_payload: Vec::new(),
                resource_id: Some(resource.to_owned()),
                observed_at: 1_700_000_000,
                sequence: 1,
                source_observation_id: format!("observation-{resource}"),
                expires_at: None,
                now: 1_700_000_000,
                provenance: RecordProvenance {
                    source_plugin_id: self.plugin_id.to_owned(),
                    source_publisher_fingerprint: self.fingerprint.clone(),
                    source_profile_id: "account-one".to_owned(),
                    declared_developer: "Example Developer".to_owned(),
                    origin: "registry".to_owned(),
                    package_version: "1.0.0".to_owned(),
                    package_digest: vec![7; 32],
                },
            })
            .await
            .unwrap();
        record.record_id
    }
}

#[tokio::test]
async fn uninstall_keep_archives_only_the_plugins_durable_records_and_keeps_private_storage() {
    use crate::plugin_host::retention::DataRetention;

    let fixture = UninstallFixture::new("org.example.producer", "producer");
    let record_id = fixture.publish("disc-1", "Dolphin").await;
    let other_owner = OwnerScope::new([9; 32], 1).unwrap();
    fixture
        .root
        .shared_pool
        .persist(crate::plugin_host::shared_pool::DurableWrite {
            owner: other_owner,
            dataset_id: "org.example.catalog".to_owned(),
            schema_id: "org.example.catalog.v1".to_owned(),
            schema_version: 1,
            fields_json: r#"{"title":"Other"}"#.to_owned(),
            opaque_payload: Vec::new(),
            resource_id: Some("disc-2".to_owned()),
            observed_at: 1_700_000_000,
            sequence: 1,
            source_observation_id: "observation-other".to_owned(),
            expires_at: None,
            now: 1_700_000_000,
            provenance: gamenode_platform::plugin_shared_data::RecordProvenance {
                source_plugin_id: "org.example.other".to_owned(),
                source_publisher_fingerprint: "cd".repeat(32),
                source_profile_id: "account-one".to_owned(),
                declared_developer: "Other".to_owned(),
                origin: "registry".to_owned(),
                package_version: "1.0.0".to_owned(),
                package_digest: vec![8; 32],
            },
        })
        .await
        .unwrap();
    let private = fixture.dir.path().join("plugin-data/private.json");
    std::fs::create_dir_all(private.parent().unwrap()).unwrap();
    std::fs::write(&private, b"private").unwrap();

    let lifecycle = fixture.lifecycle();
    let outcome = lifecycle
        .remove_with_retention(fixture.plugin_id.to_owned(), DataRetention::Keep)
        .await
        .unwrap();
    assert_eq!(outcome.data_retention, "kept");
    assert_eq!(outcome.archived_records, 1);
    assert_eq!(outcome.removed_records, 0);

    let usage = fixture
        .root
        .shared_pool
        .usage(fixture.owner, 1_700_000_100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(usage.records, 0, "kept rows are inert");
    assert_eq!(usage.archived_records, 1);
    assert!(
        fixture
            .root
            .shared_pool
            .get_any(record_id, 1_700_000_100)
            .await
            .unwrap()
            .is_none()
    );
    let other = fixture
        .root
        .shared_pool
        .usage(other_owner, 1_700_000_100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(other.records, 1, "another owner is untouched");
    assert_eq!(other.archived_records, 0);
    assert_eq!(std::fs::read(&private).unwrap(), b"private");
}

#[tokio::test]
async fn reinstalling_requires_a_reviewed_reuse_before_archived_records_are_readable() {
    use crate::plugin_host::retention::DataRetention;

    let fixture = UninstallFixture::new("org.example.producer", "producer");
    let record_id = fixture.publish("disc-1", "Dolphin").await;
    let lifecycle = fixture.lifecycle();
    lifecycle
        .remove_with_retention(fixture.plugin_id.to_owned(), DataRetention::Keep)
        .await
        .unwrap();

    // Archived rows are invisible to every ordinary read path, including a fresh consumer list.
    assert!(
        fixture
            .root
            .shared_pool
            .list_dataset_any("org.example.catalog".to_owned(), 1_700_000_100)
            .await
            .unwrap()
            .0
            .is_empty()
    );
    assert!(
        fixture
            .root
            .shared_pool
            .get_any(record_id.clone(), 1_700_000_100)
            .await
            .unwrap()
            .is_none()
    );

    // A reinstall shares the owner key, but still needs the separate reviewed reuse decision.
    fixture.reinstall("1.0.1");
    let profile_id = fixture
        .root
        .providers
        .profiles()
        .require()
        .unwrap()
        .profile_id()
        .to_owned();
    let denied = lifecycle
        .reuse_archived_durable_data(fixture.plugin_id.to_owned(), &profile_id, false)
        .await
        .unwrap_err();
    assert_eq!(denied.code(), tonic::Code::FailedPrecondition);
    assert!(
        fixture
            .root
            .shared_pool
            .get_any(record_id.clone(), 1_700_000_110)
            .await
            .unwrap()
            .is_none(),
        "an unconfirmed reuse changes nothing"
    );

    let reused = lifecycle
        .reuse_archived_durable_data(fixture.plugin_id.to_owned(), &profile_id, true)
        .await
        .unwrap();
    assert_eq!(reused.archived_records, 1);
    let listed = fixture
        .root
        .shared_pool
        .list_dataset_any("org.example.catalog".to_owned(), 1_700_000_120)
        .await
        .unwrap()
        .0;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].record_id, record_id);
    assert_eq!(
        listed[0].fields.get("title"),
        Some(&serde_json::Value::String("Dolphin".to_owned()))
    );
}

#[tokio::test]
async fn uninstall_delete_removes_only_the_selected_plugins_durable_records() {
    use crate::plugin_host::retention::DataRetention;

    let fixture = UninstallFixture::new("org.example.producer", "producer");
    fixture.publish("disc-1", "Dolphin").await;
    let other_owner = OwnerScope::new([9; 32], 1).unwrap();
    fixture
        .root
        .shared_pool
        .persist(crate::plugin_host::shared_pool::DurableWrite {
            owner: other_owner,
            dataset_id: "org.example.catalog".to_owned(),
            schema_id: "org.example.catalog.v1".to_owned(),
            schema_version: 1,
            fields_json: r#"{"title":"Other"}"#.to_owned(),
            opaque_payload: Vec::new(),
            resource_id: Some("disc-2".to_owned()),
            observed_at: 1_700_000_000,
            sequence: 1,
            source_observation_id: "observation-other".to_owned(),
            expires_at: None,
            now: 1_700_000_000,
            provenance: gamenode_platform::plugin_shared_data::RecordProvenance {
                source_plugin_id: "org.example.other".to_owned(),
                source_publisher_fingerprint: "cd".repeat(32),
                source_profile_id: "account-one".to_owned(),
                declared_developer: "Other".to_owned(),
                origin: "registry".to_owned(),
                package_version: "1.0.0".to_owned(),
                package_digest: vec![8; 32],
            },
        })
        .await
        .unwrap();

    let lifecycle = fixture.lifecycle();
    let outcome = lifecycle
        .remove_with_retention(fixture.plugin_id.to_owned(), DataRetention::Delete)
        .await
        .unwrap();
    assert_eq!(outcome.data_retention, "deleted");
    assert_eq!(outcome.removed_records, 1);
    assert!(outcome.reclaimed_bytes > 0);
    let usage = fixture
        .root
        .shared_pool
        .usage(fixture.owner, 1_700_000_100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(usage.records, 0);
    assert_eq!(usage.archived_records, 0);
    let other = fixture
        .root
        .shared_pool
        .usage(other_owner, 1_700_000_100)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(other.records, 1, "another owner is untouched");
}

#[tokio::test]
async fn uninstall_keep_archives_private_entries_and_needs_a_reviewed_reuse() {
    use crate::plugin_host::retention::DataRetention;

    let fixture = UninstallFixture::new("org.example.producer", "producer");
    fixture.write_private(fixture.plugin_id, "kept", b"private value");
    fixture.write_private_at(fixture.plugin_id, "account-two", "kept", b"other profile");
    fixture.write_private("org.example.other", "kept", b"other plugin");
    let save = fixture.dir.path().join("saves/slot-1.sav");
    std::fs::create_dir_all(save.parent().unwrap()).unwrap();
    std::fs::write(&save, b"game save").unwrap();
    let backup = fixture.dir.path().join("backups/slot-1.zip");
    std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
    std::fs::write(&backup, b"backup archive").unwrap();

    let lifecycle = fixture.lifecycle();
    let outcome = lifecycle
        .remove_with_retention(fixture.plugin_id.to_owned(), DataRetention::Keep)
        .await
        .unwrap();
    assert_eq!(outcome.data_retention, "kept");
    assert_eq!(outcome.private_archived_records, 1);
    assert_eq!(outcome.private_removed_records, 0);
    assert!(fixture.read_private(fixture.plugin_id, "kept").is_none());
    assert_eq!(
        fixture.read_private("org.example.other", "kept").as_deref(),
        Some(&b"other plugin"[..])
    );
    assert_eq!(
        std::fs::read(
            fixture
                .private_dir(fixture.plugin_id, "account-two")
                .join("storage.sqlite3")
        )
        .is_ok(),
        true,
        "another profile is untouched"
    );

    // A reinstall shares the owner namespace but cannot read or replace the archived value.
    fixture.reinstall("1.0.1");
    let dir = fixture.private_dir(fixture.plugin_id, &fixture.profile_id());
    let mut store = gamenode_platform::plugin_storage::DurableStore::open(&dir).unwrap();
    assert_eq!(store.archived_usage().unwrap().records, 1);
    assert!(matches!(
        store.put(
            "kept",
            b"replacement",
            2,
            None,
            100,
            gamenode_platform::plugin_storage::QUOTA_BYTES
        ),
        Err(gamenode_platform::plugin_storage::PluginStorageError::Archived)
    ));
    drop(store);

    let profile_id = fixture
        .root
        .providers
        .profiles()
        .require()
        .unwrap()
        .profile_id()
        .to_owned();
    let unconfirmed = lifecycle
        .reuse_archived_private_data(fixture.plugin_id.to_owned(), &profile_id, false)
        .await
        .unwrap_err();
    assert_eq!(unconfirmed.code(), tonic::Code::FailedPrecondition);
    let wrong_profile = lifecycle
        .reuse_archived_private_data(fixture.plugin_id.to_owned(), "another-profile", true)
        .await
        .unwrap_err();
    assert_eq!(wrong_profile.code(), tonic::Code::FailedPrecondition);
    assert!(fixture.read_private(fixture.plugin_id, "kept").is_none());
    let reused = lifecycle
        .reuse_archived_private_data(fixture.plugin_id.to_owned(), &profile_id, true)
        .await
        .unwrap();
    assert_eq!(reused.restored_records, 1);
    assert_eq!(
        fixture.read_private(fixture.plugin_id, "kept").as_deref(),
        Some(&b"private value"[..])
    );

    assert_eq!(std::fs::read(&save).unwrap(), b"game save");
    assert_eq!(std::fs::read(&backup).unwrap(), b"backup archive");
}

#[tokio::test]
async fn uninstall_delete_removes_only_the_selected_private_namespace() {
    use crate::plugin_host::retention::DataRetention;

    let fixture = UninstallFixture::new("org.example.producer", "producer");
    fixture.write_private(fixture.plugin_id, "kept", b"private value");
    fixture.write_private_at(fixture.plugin_id, "account-two", "kept", b"other profile");
    fixture.write_private("org.example.other", "kept", b"other plugin");
    let other_owner = OwnerScope::new([9; 32], 1).unwrap();
    fixture
        .root
        .shared_pool
        .persist(crate::plugin_host::shared_pool::DurableWrite {
            owner: other_owner,
            dataset_id: "org.example.catalog".to_owned(),
            schema_id: "org.example.catalog.v1".to_owned(),
            schema_version: 1,
            fields_json: r#"{"title":"Other owner"}"#.to_owned(),
            opaque_payload: Vec::new(),
            resource_id: Some("disc-other".to_owned()),
            observed_at: 1_700_000_000,
            sequence: 1,
            source_observation_id: "observation-other".to_owned(),
            expires_at: None,
            now: 1_700_000_000,
            provenance: gamenode_platform::plugin_shared_data::RecordProvenance {
                source_plugin_id: "org.example.other".to_owned(),
                source_publisher_fingerprint: "cd".repeat(32),
                source_profile_id: "account-one".to_owned(),
                declared_developer: "Other".to_owned(),
                origin: "registry".to_owned(),
                package_version: "1.0.0".to_owned(),
                package_digest: vec![8; 32],
            },
        })
        .await
        .unwrap();
    let save = fixture.dir.path().join("saves/slot-1.sav");
    std::fs::create_dir_all(save.parent().unwrap()).unwrap();
    std::fs::write(&save, b"game save").unwrap();

    let lifecycle = fixture.lifecycle();
    let outcome = lifecycle
        .remove_with_retention(fixture.plugin_id.to_owned(), DataRetention::Delete)
        .await
        .unwrap();
    assert_eq!(outcome.data_retention, "deleted");
    assert_eq!(outcome.private_removed_records, 1);
    assert_eq!(
        outcome.private_reclaimed_bytes,
        gamenode_platform::plugin_storage::charge("kept", 13)
    );
    assert!(
        !fixture
            .private_dir(fixture.plugin_id, &fixture.profile_id())
            .exists()
    );
    assert!(
        fixture
            .private_dir(fixture.plugin_id, "account-two")
            .exists(),
        "another profile is untouched"
    );
    assert_eq!(
        fixture.read_private("org.example.other", "kept").as_deref(),
        Some(&b"other plugin"[..])
    );
    assert_eq!(std::fs::read(&save).unwrap(), b"game save");
    assert_eq!(
        fixture
            .root
            .shared_pool
            .usage(other_owner, 1_700_000_100)
            .await
            .unwrap()
            .unwrap()
            .records,
        1,
        "another owner's durable pool row is untouched"
    );
}
