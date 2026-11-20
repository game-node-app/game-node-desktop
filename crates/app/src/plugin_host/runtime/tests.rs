//! Supervised runtime tests. The fixture child runs this same test binary with the explicit
//! plugin-process marker, so the host exercises the real process boundary.

use super::*;
use crate::plugin_host::{
    PluginApiServer,
    provider::{Registration, RegistrationKind, RegistrationOrigin},
};
use gamenode_plugin_protocol::capability as cap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const MANIFEST: &str = r#"
id = "fixture"
name = "Fixture"
platforms = ["linux"]
source = "fixture"

[scan]
"#;

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
