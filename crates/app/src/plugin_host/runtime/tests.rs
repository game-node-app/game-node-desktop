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
